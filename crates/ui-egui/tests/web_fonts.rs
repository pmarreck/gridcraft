//! The web build fetches fonts after the workbook is open: a fetched font must bind on the next
//! frames, without waiting for an edit, and render cells set in a name it stands in for.

use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp, font_offer, fonts};

/// Names no machine has installed, so only the offered font can bind them.
const FAMILY: &str = "GridCraft Test Sans";
const ALIAS: &str = "GridCraft Fictional Office Font";

#[test]
fn a_font_fetched_after_opening_binds_without_an_edit() {
    // The installed-font scan must be finished before the app binds anything (as on the web,
    // where the catalog is empty and complete at once).
    fonts::catalog();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", serde_json::json!({"cell": "A1", "input": "Hello"})).expect("cell.set");
    session.execute("home.fontName", serde_json::json!({"range": "A1", "name": ALIAS})).expect("home.fontName");
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1000.0, 700.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Services::default()),
    );
    h.run_steps(4);
    let role = fonts::role_key(ALIAS, false, false);
    assert!(!fonts::is_bound(&role), "nothing to bind before the font arrives");

    let manifest = format!(r#"{{"families":[{{"family":"{FAMILY}","aliases":["{ALIAS}"],"faces":[{{"file":"t/a.ttf"}}]}}]}}"#);
    font_offer::offer(font_offer::parse_manifest(&manifest).expect("manifest"));
    let want = font_offer::take_wanted(&h.state().used_fonts());
    assert_eq!(want.iter().map(|o| o.family.as_str()).collect::<Vec<_>>(), [FAMILY], "the workbook asks for its font");
    assert_eq!(font_offer::add_loaded(FAMILY, vec![(epaint_default_fonts::UBUNTU_LIGHT.to_vec(), 400.0, false)]), 1);
    h.run_steps(4);
    assert!(fonts::is_bound(&role), "the fetched font is bound under the workbook's name");
}
