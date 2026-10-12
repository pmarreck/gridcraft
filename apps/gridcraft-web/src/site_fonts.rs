//! Fonts served next to the page (`fonts/manifest.json` and the files it lists), fetched only when
//! a workbook uses them. What to fetch and how fetched faces bind is decided in
//! [`gridcraft_ui_egui::font_offer`]; this module only does the HTTP requests.

use gridcraft_ui_egui::font_offer::{self, Offered};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::JsFuture;

/// The font folder, relative to the page so the site works under any subpath.
const FONT_DIR: &str = "fonts/";
const MANIFEST: &str = "manifest.json";

/// GET `url` (relative to the page) and return the body, or why it failed.
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response =
        JsFuture::from(window.fetch_with_str(url)).await.map_err(|e| format!("{e:?}"))?.dyn_into().map_err(|e| format!("{e:?}"))?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body = JsFuture::from(response.array_buffer().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    Ok(js_sys::Uint8Array::new(&body).to_vec())
}

/// Offer the fonts this site serves. A site without fonts (or an unreadable manifest) offers none,
/// and workbooks keep the built-in fonts.
pub fn offer(ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let text = match fetch_bytes(&format!("{FONT_DIR}{MANIFEST}")).await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                log::info!("this site serves no extra fonts ({e})");
                return;
            }
        };
        match font_offer::parse_manifest(&text) {
            Ok(families) => {
                font_offer::offer(families);
                ctx.request_repaint();
            }
            Err(e) => log::warn!("the font manifest is unreadable: {e}"),
        }
    });
}

/// Fetch every face of one offered family, then hand the faces to the font registry. A face that
/// fails to download is skipped; the family's other faces still bind.
pub fn fetch(family: Offered, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let mut faces = Vec::new();
        for face in &family.faces {
            match fetch_bytes(&format!("{FONT_DIR}{}", face.file)).await {
                Ok(bytes) => faces.push((bytes, face.weight, face.italic)),
                Err(e) => log::warn!("couldn't fetch font {}: {e}", face.file),
            }
        }
        if font_offer::add_loaded(&family.family, faces) == 0 {
            log::warn!("no usable face of {} arrived; its cells keep the built-in font", family.family);
        }
        ctx.request_repaint();
    });
}
