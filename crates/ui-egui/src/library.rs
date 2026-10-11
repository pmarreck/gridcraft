//! Workbooks kept inside the browser: a flat library of named files that the engine reads and
//! writes through [`gridcraft_engine::io::FileStore`], so Open, Save, Ctrl+S and AutoSave work
//! there as they do on a disk. Pure and synchronous: every change is applied in memory at once
//! and recorded in a journal, which the host drains and persists (IndexedDB in the web build).

use std::cell::RefCell;
use std::collections::BTreeMap;

use gridcraft_engine::{EngineError, Result};

/// Longest name in bytes, as on common desktop file systems.
const MAX_NAME_BYTES: usize = 255;

/// One workbook in the library, as listed.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredWorkbook {
    pub name: String,
    pub size: usize,
    pub modified_ms: f64,
}

/// A change the host must persist, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Put { name: String, bytes: Vec<u8>, modified_ms: f64 },
    Delete { name: String },
}

#[derive(Default)]
struct State {
    files: BTreeMap<String, (Vec<u8>, f64)>,
    journal: Vec<Change>,
}

/// The browser's workbook library: an in-memory mirror of persistent storage plus a journal of
/// changes not yet persisted (write-behind). `clock` stamps modification times in milliseconds.
pub struct Library {
    state: RefCell<State>,
    clock: Box<dyn Fn() -> f64>,
}

/// A workbook name the library accepts: one flat file name with no surrounding spaces, no path
/// separators or control characters, not `.`/`..`, at most [`MAX_NAME_BYTES`] bytes.
pub fn check_name(name: &str) -> std::result::Result<(), String> {
    if name.trim().is_empty() {
        return Err("A workbook needs a name.".into());
    }
    if name != name.trim() {
        return Err("A workbook name can't start or end with a space.".into());
    }
    if name == "." || name == ".." || name.contains(['/', '\\']) || name.chars().any(char::is_control) {
        return Err(format!("“{name}” can't be used as a workbook name."));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err("That workbook name is too long.".into());
    }
    Ok(())
}

fn engine_error(message: String) -> EngineError {
    EngineError::Other(message)
}

impl Library {
    pub fn new(clock: impl Fn() -> f64 + 'static) -> Library {
        Library { state: RefCell::default(), clock: Box::new(clock) }
    }

    /// Loads workbooks read back from persistent storage; they are not journaled again. Entries
    /// whose names the library would refuse are skipped.
    pub fn restore(&self, entries: Vec<(String, Vec<u8>, f64)>) {
        let mut state = self.state.borrow_mut();
        for (name, bytes, modified_ms) in entries {
            if check_name(&name).is_ok() {
                state.files.insert(name, (bytes, modified_ms));
            }
        }
    }

    /// Every workbook, most recently modified first (ties by name).
    pub fn list(&self) -> Vec<StoredWorkbook> {
        let state = self.state.borrow();
        let mut list: Vec<StoredWorkbook> = state
            .files
            .iter()
            .map(|(name, (bytes, modified_ms))| StoredWorkbook { name: name.clone(), size: bytes.len(), modified_ms: *modified_ms })
            .collect();
        list.sort_by(|a, b| b.modified_ms.total_cmp(&a.modified_ms).then_with(|| a.name.cmp(&b.name)));
        list
    }

    pub fn contains(&self, name: &str) -> bool {
        self.state.borrow().files.contains_key(name)
    }

    /// Renames `from` to `to`. Refuses to replace another workbook. Journals the new copy before
    /// the deletion, so a host that persists only part of the journal loses nothing.
    pub fn rename(&self, from: &str, to: &str) -> std::result::Result<(), String> {
        check_name(to)?;
        let mut state = self.state.borrow_mut();
        if !state.files.contains_key(from) {
            return Err(format!("“{from}” isn't in this browser."));
        }
        if from == to {
            return Ok(());
        }
        if state.files.contains_key(to) {
            return Err(format!("A workbook named “{to}” already exists."));
        }
        let Some((bytes, _)) = state.files.remove(from) else { return Err(format!("“{from}” isn't in this browser.")) };
        let modified_ms = (self.clock)();
        state.journal.push(Change::Put { name: to.to_string(), bytes: bytes.clone(), modified_ms });
        state.journal.push(Change::Delete { name: from.to_string() });
        state.files.insert(to.to_string(), (bytes, modified_ms));
        Ok(())
    }

    pub fn remove(&self, name: &str) -> std::result::Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.files.remove(name).is_none() {
            return Err(format!("“{name}” isn't in this browser."));
        }
        state.journal.push(Change::Delete { name: name.to_string() });
        Ok(())
    }

    /// The changes made since the last call, oldest first, for the host to persist.
    pub fn take_changes(&self) -> Vec<Change> {
        std::mem::take(&mut self.state.borrow_mut().journal)
    }
}

impl gridcraft_engine::io::FileStore for Library {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.state.borrow().files.get(path).map(|(bytes, _)| bytes.clone()).ok_or_else(|| engine_error(format!("“{path}” isn't in this browser.")))
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        check_name(path).map_err(engine_error)?;
        let modified_ms = (self.clock)();
        let mut state = self.state.borrow_mut();
        state.journal.push(Change::Put { name: path.to_string(), bytes: bytes.to_vec(), modified_ms });
        state.files.insert(path.to_string(), (bytes.to_vec(), modified_ms));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::io::FileStore as _;

    fn library() -> Library {
        let now = std::cell::Cell::new(1000.0);
        Library::new(move || {
            now.set(now.get() + 1.0);
            now.get()
        })
    }

    #[test]
    fn names_are_classified_over_a_set() {
        let accepted = ["Budget.xlsx", "Q1 report.xlsx", "naïve résumé.xlsx", "数据.xlsx", ".hidden.xlsx", "a", "x.csv"];
        let long = "a".repeat(256);
        let rejected = [
            "",
            " ",
            " Budget.xlsx",
            "Budget.xlsx ",
            ".",
            "..",
            "a/b.xlsx",
            "/Budget.xlsx",
            "a\\b.xlsx",
            "tab\there.xlsx",
            "nul\0.xlsx",
            long.as_str(),
        ];
        for name in accepted {
            assert!(check_name(name).is_ok(), "{name:?} should be accepted");
        }
        for name in rejected {
            assert!(check_name(name).is_err(), "{name:?} should be rejected");
        }
        assert!(check_name(&"a".repeat(255)).is_ok(), "255 bytes is the limit");
    }

    #[test]
    fn writes_are_readable_listed_newest_first_and_journaled() {
        let lib = library();
        lib.write("Old.xlsx", b"one").unwrap();
        lib.write("New.xlsx", b"two").unwrap();
        assert_eq!(lib.read("Old.xlsx").unwrap(), b"one");
        let names: Vec<_> = lib.list().into_iter().map(|w| (w.name, w.size)).collect();
        assert_eq!(names, [("New.xlsx".to_string(), 3), ("Old.xlsx".to_string(), 3)]);
        assert_eq!(
            lib.take_changes(),
            [
                Change::Put { name: "Old.xlsx".into(), bytes: b"one".to_vec(), modified_ms: 1001.0 },
                Change::Put { name: "New.xlsx".into(), bytes: b"two".to_vec(), modified_ms: 1002.0 },
            ]
        );
        assert!(lib.take_changes().is_empty(), "draining empties the journal");
        lib.write("Old.xlsx", b"three").unwrap();
        assert_eq!(lib.list()[0].name, "Old.xlsx", "an overwrite is the newest");
        assert_eq!(lib.read("Old.xlsx").unwrap(), b"three");
    }

    #[test]
    fn bad_names_and_missing_files_are_errors_that_change_nothing() {
        let lib = library();
        assert!(lib.write("a/b.xlsx", b"x").is_err());
        assert!(lib.read("Missing.xlsx").is_err());
        assert!(lib.remove("Missing.xlsx").is_err());
        assert!(lib.rename("Missing.xlsx", "Other.xlsx").is_err());
        assert!(lib.list().is_empty());
        assert!(lib.take_changes().is_empty());
    }

    #[test]
    fn rename_moves_the_bytes_and_never_overwrites() {
        let lib = library();
        lib.write("A.xlsx", b"a").unwrap();
        lib.write("B.xlsx", b"b").unwrap();
        lib.take_changes();
        assert!(lib.rename("A.xlsx", "B.xlsx").is_err(), "renaming onto another workbook is refused");
        assert!(lib.rename("A.xlsx", "bad/name").is_err());
        assert!(lib.take_changes().is_empty());
        lib.rename("A.xlsx", "A.xlsx").unwrap();
        assert!(lib.take_changes().is_empty(), "renaming to the same name is a no-op");
        lib.rename("A.xlsx", "C.xlsx").unwrap();
        assert!(lib.read("A.xlsx").is_err());
        assert_eq!(lib.read("C.xlsx").unwrap(), b"a");
        // The new copy is stored before the old one is deleted, so an interrupted rename keeps both.
        let changes = lib.take_changes();
        assert!(
            matches!(changes.as_slice(), [Change::Put { name, bytes, .. }, Change::Delete { name: old }] if name == "C.xlsx" && bytes == b"a" && old == "A.xlsx")
        );
    }

    #[test]
    fn remove_deletes_and_journals() {
        let lib = library();
        lib.write("A.xlsx", b"a").unwrap();
        lib.take_changes();
        lib.remove("A.xlsx").unwrap();
        assert!(lib.list().is_empty());
        assert_eq!(lib.take_changes(), [Change::Delete { name: "A.xlsx".into() }]);
    }

    #[test]
    fn restored_files_are_listed_but_not_written_back() {
        let lib = library();
        lib.restore(vec![("Kept.xlsx".into(), b"k".to_vec(), 5.0), ("bad/name".into(), b"x".to_vec(), 6.0)]);
        assert_eq!(lib.list().into_iter().map(|w| w.name).collect::<Vec<_>>(), ["Kept.xlsx"], "unusable stored names are skipped");
        assert_eq!(lib.read("Kept.xlsx").unwrap(), b"k");
        assert!(lib.take_changes().is_empty());
    }
}
