//! Every control in the ribbon is announced by name: screen readers read it, and tests and agents
//! find it by the same words the tooltip shows.

use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::{By, NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = Harness::builder().with_size(vec2(2400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// The controls inside the ribbon card (below the tab row) that have no name, described by role
/// and position.
fn unnamed_controls(h: &Harness) -> Vec<String> {
    // The tab is the lower "Home": the title bar above has a Home button too.
    let tabs = h.query_all_by_role_and_label(Role::Button, "Home").map(|n| n.rect()).max_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
    let (top, bottom) = (tabs.bottom(), tabs.bottom() + 92.0);
    h.query_all(By::new().predicate(move |node| {
        let Some(b) = node.bounding_box() else { return false };
        let inside = b.y0 >= f64::from(top) && b.y1 <= f64::from(bottom);
        let name = node.label().or_else(|| node.value()).unwrap_or_default();
        inside && node.data().supports_action(egui::accesskit::Action::Click) && name.trim().is_empty()
    }))
    .map(|node| format!("{:?} at {:?}", node.accesskit_node().role(), node.rect()))
    .collect()
}

#[test]
fn every_ribbon_control_has_a_name() {
    let mut missing = Vec::new();
    for tab in gridcraft_ui_egui::ribbon::TABS {
        let mut h = harness();
        h.state_mut().ui.ribbon_tab = (*tab).to_string();
        h.run_steps(2);
        missing.extend(unnamed_controls(&h).into_iter().map(|c| format!("{tab}: {c}")));
    }
    assert!(missing.is_empty(), "{} ribbon controls have no name:\n{}", missing.len(), missing.join("\n"));
}

#[test]
fn a_ribbon_button_is_named_by_its_tooltip() {
    let h = harness();
    for name in ["Cut", "Copy", "Bold", "Paste"] {
        assert_eq!(h.query_all_by_label(name).count(), 1, "{name}");
    }
}
