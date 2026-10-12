//! Fonts the host offers for download: the web build's self-hosted set of open-licence fonts.
//!
//! A browser has no font folders to scan, so the web site ships a pinned set of TTF files with a
//! `fonts/manifest.json` describing them. The shell hands the manifest to [`offer`]; the font list
//! then lists those families, and a family is fetched only once a workbook uses it ([`take_wanted`]).
//! Fetched faces are parse-checked and kept in memory ([`add_loaded`]); [`crate::fonts`] binds them
//! exactly like installed fonts. Aliases let a metric-compatible substitute stand in for a font we
//! can't ship (a workbook in Calibri renders in Carlito, bound under the name "Calibri").
//!
//! The pure parts (parsing, name resolution, choosing what to fetch) take their state as arguments;
//! the process-wide registry below only stores it, like the installed-font catalog.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock, RwLock};

use serde::Deserialize;

use crate::fonts::{Face, Source};

/// Weight of a face whose manifest entry gives none.
const REGULAR_WEIGHT: f32 = 400.0;

/// One downloadable face: a file relative to the manifest, and its weight and slant.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct OfferedFace {
    pub file: String,
    #[serde(default = "regular_weight")]
    pub weight: f32,
    #[serde(default)]
    pub italic: bool,
}

fn regular_weight() -> f32 {
    REGULAR_WEIGHT
}

/// One downloadable family, and the names it stands in for.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Offered {
    pub family: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub faces: Vec<OfferedFace>,
}

#[derive(Deserialize)]
struct Manifest {
    families: Vec<Offered>,
}

/// Parse a font manifest (`{"families":[{"family","aliases","faces":[{"file","weight","italic"}]}]}`).
/// Families without a name or without faces, and faces whose file is empty, absolute or climbs out
/// of the font folder, are dropped: the manifest comes over the network and is never trusted.
pub fn parse_manifest(text: &str) -> Result<Vec<Offered>, String> {
    let manifest: Manifest = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(manifest
        .families
        .into_iter()
        .filter_map(|mut f| {
            f.faces.retain(|face| safe_file(&face.file) && face.weight.is_finite());
            (!f.family.trim().is_empty() && !f.faces.is_empty()).then_some(f)
        })
        .collect())
}

/// A relative path inside the font folder: no scheme, no leading slash, no `..` component.
fn safe_file(file: &str) -> bool {
    !file.is_empty()
        && !file.starts_with('/')
        && !file.starts_with('\\')
        && !file.contains(':')
        && file.split(['/', '\\']).all(|part| !part.is_empty() && part != "..")
}

/// The offered family a workbook font name means: the family itself or one it stands in for,
/// ignoring ASCII case.
pub fn resolve<'a>(offered: &'a [Offered], name: &str) -> Option<&'a Offered> {
    offered
        .iter()
        .find(|o| o.family.eq_ignore_ascii_case(name))
        .or_else(|| offered.iter().find(|o| o.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))))
}

/// The offered families the `used` names need that are not `settled` (loaded or already
/// requested; lowercase family names), each once, in first-use order.
pub fn wanted(offered: &[Offered], used: &[String], settled: &HashSet<String>) -> Vec<Offered> {
    let mut out: Vec<Offered> = Vec::new();
    for name in used {
        let Some(o) = resolve(offered, name) else { continue };
        if settled.contains(&o.family.to_lowercase()) || out.iter().any(|w| w.family == o.family) {
            continue;
        }
        out.push(o.clone());
    }
    out
}

#[derive(Default)]
struct Registry {
    offered: Vec<Offered>,
    loaded: Vec<Face>,
    /// Lowercase family names that are loaded or requested: never fetched again.
    settled: HashSet<String>,
    generation: u64,
}

static REGISTRY: OnceLock<RwLock<Registry>> = OnceLock::new();

fn registry() -> &'static RwLock<Registry> {
    REGISTRY.get_or_init(|| RwLock::new(Registry::default()))
}

fn read<T>(f: impl FnOnce(&Registry) -> T) -> T {
    f(&registry().read().unwrap_or_else(|e| e.into_inner()))
}

fn write<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
    f(&mut registry().write().unwrap_or_else(|e| e.into_inner()))
}

/// Offer these families for download (replacing any earlier offer).
pub fn offer(families: Vec<Offered>) {
    write(|r| {
        r.offered = families;
        r.generation += 1;
    });
}

/// The families the `used` names need fetched, marked requested so each is fetched once.
pub fn take_wanted(used: &[String]) -> Vec<Offered> {
    write(|r| {
        let want = wanted(&r.offered, used, &r.settled);
        r.settled.extend(want.iter().map(|o| o.family.to_lowercase()));
        want
    })
}

/// Store fetched faces of `family` as `(bytes, weight, italic)`. Faces egui could not open are
/// dropped. Returns how many were kept; any kept face makes the next font rebuild bind the family.
pub fn add_loaded(family: &str, faces: Vec<(Vec<u8>, f32, bool)>) -> usize {
    let good: Vec<Face> = faces
        .into_iter()
        .filter(|(bytes, _, _)| crate::theme::font_parses(bytes, 0))
        .map(|(bytes, weight, italic)| Face { family: family.to_string(), source: Source::Bytes(Arc::from(bytes)), index: 0, weight, italic })
        .collect();
    let kept = good.len();
    if kept > 0 {
        write(|r| {
            r.settled.insert(family.to_lowercase());
            r.loaded.extend(good);
            r.generation += 1;
        });
    }
    kept
}

/// Changes whenever the offer or the loaded faces change, so the app knows to rebuild its fonts.
pub fn generation() -> u64 {
    read(|r| r.generation)
}

/// The offered family names, for the font list.
pub fn offered_families() -> Vec<String> {
    read(|r| r.offered.iter().map(|o| o.family.clone()).collect())
}

/// The loaded faces a workbook font name resolves to (its family or the family it is an alias of).
pub fn loaded_faces_for(name: &str) -> Vec<Face> {
    read(|r| {
        let family = resolve(&r.offered, name).map(|o| o.family.clone()).unwrap_or_else(|| name.to_string());
        r.loaded.iter().filter(|f| f.family.eq_ignore_ascii_case(&family)).cloned().collect()
    })
}

/// Forget every offer and loaded face (tests only).
#[cfg(test)]
pub(crate) fn reset() {
    write(|r| *r = Registry::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{"families":[
        {"family":"Carlito","aliases":["Calibri"],"faces":[
            {"file":"Carlito/Carlito-Regular.ttf"},
            {"file":"Carlito/Carlito-Bold.ttf","weight":700},
            {"file":"Carlito/Carlito-Italic.ttf","italic":true}]},
        {"family":"Liberation Sans","aliases":["Arial","Helvetica"],"faces":[{"file":"LiberationSans/LiberationSans-Regular.ttf"}]},
        {"family":"Lato","faces":[{"file":"Lato/Lato-Regular.ttf","weight":400}]}
    ]}"#;

    fn offered() -> Vec<Offered> {
        parse_manifest(MANIFEST).expect("manifest parses")
    }

    fn names(v: &[Offered]) -> Vec<&str> {
        v.iter().map(|o| o.family.as_str()).collect()
    }

    #[test]
    fn the_manifest_parses_with_defaults() {
        let o = offered();
        assert_eq!(names(&o), ["Carlito", "Liberation Sans", "Lato"]);
        assert_eq!(o[0].faces[0], OfferedFace { file: "Carlito/Carlito-Regular.ttf".into(), weight: 400.0, italic: false });
        assert_eq!(o[0].faces[1].weight, 700.0);
        assert!(o[0].faces[2].italic);
        assert_eq!(o[2].aliases, Vec::<String>::new());
    }

    #[test]
    fn a_bad_manifest_is_an_error_not_a_panic() {
        for text in ["", "not json", "[]", r#"{"families":7}"#, r#"{"families":[{"faces":[]}]}"#] {
            assert!(parse_manifest(text).is_err(), "{text:?} should be rejected");
        }
    }

    #[test]
    fn unsafe_files_and_empty_families_are_dropped() {
        let text = r#"{"families":[
            {"family":"Ok","faces":[{"file":"Ok/a.ttf"},{"file":"../etc/passwd"},{"file":"/abs.ttf"},
                {"file":"https://elsewhere.example/x.ttf"},{"file":"a//b.ttf"},{"file":""},{"file":"..\\x.ttf"}]},
            {"family":"AllBad","faces":[{"file":"../x.ttf"}]},
            {"family":"  ","faces":[{"file":"x.ttf"}]}
        ]}"#;
        let o = parse_manifest(text).expect("parses");
        assert_eq!(names(&o), ["Ok"]);
        assert_eq!(o[0].faces.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), ["Ok/a.ttf"]);
    }

    #[test]
    fn names_resolve_to_families_and_aliases_ignoring_case() {
        let o = offered();
        let cases = [
            ("Carlito", Some("Carlito")),
            ("calibri", Some("Carlito")),
            ("CALIBRI", Some("Carlito")),
            ("Arial", Some("Liberation Sans")),
            ("helvetica", Some("Liberation Sans")),
            ("Lato", Some("Lato")),
            ("Comic Sans MS", None),
            ("Calibri Light", None),
            ("", None),
        ];
        for (name, want) in cases {
            assert_eq!(resolve(&o, name).map(|f| f.family.as_str()), want, "{name:?}");
        }
    }

    #[test]
    fn wanted_fetches_each_needed_family_once_and_skips_settled_ones() {
        let o = offered();
        let used: Vec<String> = ["Calibri", "Carlito", "Comic Sans MS", "Lato", "calibri", "Arial"].iter().map(|s| s.to_string()).collect();
        assert_eq!(names(&wanted(&o, &used, &HashSet::new())), ["Carlito", "Lato", "Liberation Sans"]);
        let settled: HashSet<String> = ["lato".to_string()].into();
        assert_eq!(names(&wanted(&o, &used, &settled)), ["Carlito", "Liberation Sans"]);
        assert!(wanted(&o, &[], &HashSet::new()).is_empty());
        assert!(wanted(&[], &used, &HashSet::new()).is_empty());
    }

    #[test]
    fn the_registry_requests_once_and_keeps_only_parseable_faces() {
        let _g = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        offer(offered());
        let used = vec!["Calibri".to_string()];
        assert_eq!(names(&take_wanted(&used)), ["Carlito"]);
        assert!(take_wanted(&used).is_empty(), "requested once");
        let before = generation();
        let kept = add_loaded("Carlito", vec![(epaint_default_fonts::UBUNTU_LIGHT.to_vec(), 400.0, false), (b"junk".to_vec(), 700.0, false)]);
        assert_eq!(kept, 1, "the junk face is dropped");
        assert!(generation() > before);
        assert_eq!(loaded_faces_for("calibri").len(), 1, "an alias finds its family's faces");
        assert_eq!(loaded_faces_for("Carlito").len(), 1);
        assert!(loaded_faces_for("Lato").is_empty());
        assert_eq!(add_loaded("Lato", vec![(b"junk".to_vec(), 400.0, false)]), 0);
        reset();
    }
}
