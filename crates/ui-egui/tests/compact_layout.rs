//! On a phone-width window the layout compacts: every ribbon tab stays reachable by swiping the
//! tab row, the status bar gives its height to the grid, and the workbook title never covers the
//! title bar's buttons. Every dialog fits the screen, scrolling when it is taller.

use egui::accesskit::Role;
use egui::{Event, Pos2, TouchDeviceId, TouchId, TouchPhase, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const PHONE: egui::Vec2 = vec2(390.0, 844.0);
const DESKTOP: egui::Vec2 = vec2(1440.0, 900.0);

fn harness(size: egui::Vec2) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).build_ui_state(
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

/// One frame of touch input, through the app's raw-input hook as the shells do.
fn touch(h: &mut Harness, phase: TouchPhase, pos: Pos2) {
    let mut raw = std::mem::take(h.input_mut());
    raw.events.push(Event::Touch { device_id: TouchDeviceId(1), id: TouchId(0), phase, pos, force: None });
    h.state_mut().raw_input_hook(&mut raw);
    *h.input_mut() = raw;
    h.run_steps(1);
}

/// A frame with no new input, through the hook (a tap's release arrives in the frame after the lift).
fn idle(h: &mut Harness) {
    let mut raw = std::mem::take(h.input_mut());
    h.state_mut().raw_input_hook(&mut raw);
    *h.input_mut() = raw;
    h.run_steps(1);
}

fn tab_rect(h: &Harness, tab: &str) -> egui::Rect {
    h.get_by_role_and_label(Role::Button, tab).rect()
}

#[test]
fn every_ribbon_tab_can_be_swiped_into_view_and_tapped_on_a_phone() {
    let mut h = harness(PHONE);
    assert!(tab_rect(&h, "View").left() > PHONE.x, "View starts off-screen: {:?}", tab_rect(&h, "View"));
    // Swipe the tab row left, slowly enough that no flick follows.
    let y = tab_rect(&h, "Home").center().y;
    let from = Pos2::new(PHONE.x - 20.0, y);
    touch(&mut h, TouchPhase::Start, from);
    for i in 1..=30 {
        touch(&mut h, TouchPhase::Move, from - vec2(20.0 * i as f32, 0.0));
    }
    for _ in 0..20 {
        touch(&mut h, TouchPhase::Move, from - vec2(600.0, 0.0));
    }
    touch(&mut h, TouchPhase::End, from - vec2(600.0, 0.0));
    h.run_steps(2);
    let view = tab_rect(&h, "View");
    assert!(view.left() >= 0.0 && view.right() <= PHONE.x, "View is on screen after the swipe: {view:?}");
    touch(&mut h, TouchPhase::Start, view.center());
    touch(&mut h, TouchPhase::End, view.center());
    idle(&mut h);
    assert_eq!(h.state().ui.ribbon_tab, "View");
}

#[test]
fn a_phone_hides_the_status_bar_and_gives_the_grid_its_height() {
    let bottom = |size| harness(size).state().grid.rect.unwrap().bottom();
    let (phone, wide) = (bottom(PHONE), bottom(vec2(800.0, PHONE.y)));
    assert!(phone > wide + 20.0, "the grid reaches lower on a phone ({phone}) than on a wide window ({wide})");
}

#[test]
fn the_title_never_covers_a_title_bar_button() {
    for size in [PHONE, vec2(600.0, 844.0), DESKTOP] {
        let h = harness(size);
        let title = h.get_by_role_and_label(Role::Label, "Book1").rect();
        assert!(title.left() >= 0.0 && title.right() <= size.x, "{size:?}: title on screen: {title:?}");
        for button in h.query_all_by_role(Role::Button) {
            let r = button.rect();
            if r.top() < title.bottom() && r.bottom() > title.top() {
                assert!(!r.intersects(title.shrink(0.5)), "{size:?}: the title {title:?} covers a button at {r:?}");
            }
        }
    }
}

/// Every dialog that opens a window, by the name `SheetApp::open_dialog` takes.
const DIALOGS: &[&str] = &[
    "insertCells",
    "insertPaste",
    "deleteCells",
    "rowHeight",
    "columnWidth",
    "defaultWidth",
    "renameSheet",
    "moveSheet",
    "protectSheet",
    "allowEditRanges",
    "protectWorkbook",
    "unprotectWorkbook",
    "unprotectSheet",
    "unhideSheet",
    "pasteSpecial",
    "series",
    "comment",
    "insertLink",
    "sparkline",
    "headerFooter",
    "pageSetup",
    "textToColumns",
    "removeDuplicates",
    "goalSeek",
    "forecastSheet",
    "analysisToolPak",
    "solver",
    "solverResults",
    "subtotal",
    "zoom",
    "defineName",
    "dataValidation",
    "cfQuick",
    "formatCells",
    "insertFunction",
    "find",
    "goTo",
    "pivotCalculatedField",
    "goToSpecial",
    "sort",
    "nameManager",
    "manageRules",
    "commandSearch",
    "agents",
    "about",
    "recommendedCharts",
    "selectData",
    "journal",
    "macros",
    "statistics",
    "accessibility",
    "errorChecking",
    "evaluateFormula",
    "spelling",
    "chartTitle",
    "insertPicture",
    "pickList",
    "saveCopy",
    "saveChanges",
    "libraryConflict",
    "start",
    "openLink",
    "openPassword",
];

/// Opens dialog `name` on a fresh window of `size` holding a little data and a chart (the chart
/// dialogs need both), and returns its window's rect, or None when no window opened.
fn open_dialog(size: egui::Vec2, name: &str) -> (Harness, Option<egui::Rect>) {
    let mut h = harness(size);
    let session = &mut h.state_mut().session;
    let values = serde_json::json!([["Name", "Qty"], ["a", 1], ["b", 2]]);
    session.execute("range.setValues", serde_json::json!({"range": "A1", "values": values})).unwrap();
    session.execute("selection.set", serde_json::json!({"range": "A1:B3"})).unwrap();
    let chart = session.execute("insert.chart", serde_json::json!({"type": "column", "at": "D3"})).unwrap();
    h.state_mut().open_dialog(name, serde_json::json!({"chart": chart["chart"]}));
    h.run_steps(10);
    let id = h.state().dialog.as_ref().map(|d| egui::Id::new("dialog").with(&d.name));
    let rect = id.and_then(|id| egui::AreaState::load(&h.ctx, id)).map(|s| s.rect());
    (h, rect)
}

#[test]
fn every_dialog_fits_on_a_phone_and_a_desktop_screen() {
    for size in [PHONE, DESKTOP] {
        let screen = egui::Rect::from_min_size(Pos2::ZERO, size);
        let misfits: Vec<String> = DIALOGS
            .iter()
            .filter_map(|&name| match open_dialog(size, name).1 {
                Some(r) if screen.shrink(1.0).contains_rect(r) => None,
                r => Some(format!("{name}: {r:?}")),
            })
            .collect();
        assert!(misfits.is_empty(), "dialogs not inside a {size:?} screen:\n{}", misfits.join("\n"));
    }
}

#[test]
fn a_dialog_ends_just_below_its_ok_button() {
    let (h, window) = open_dialog(DESKTOP, "insertCells");
    let (window, ok) = (window.unwrap(), h.get_by_role_and_label(Role::Button, "   OK   ").rect());
    assert!(window.bottom() - ok.bottom() < 24.0, "OK at {ok:?} sits well above the window's bottom edge {window:?}");
}

/// One frame of wheel input with the pointer at `at`.
fn wheel(h: &mut Harness, at: Pos2, delta: egui::Vec2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(1);
    h.input_mut().events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta,
        phase: TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(20);
}

#[test]
fn a_vertical_wheel_over_an_overflowing_ribbon_pans_it_sideways() {
    // Home is wider than 700 points, so its ribbon overflows.
    let size = vec2(700.0, 844.0);
    let mut h = harness(size);
    let paste = |h: &Harness| h.get_by_role_and_label(Role::Button, "Paste").rect();
    let before = paste(&h);
    let over_ribbon = Pos2::new(size.x / 2.0, tab_rect(&h, "Home").bottom() + 40.0);
    wheel(&mut h, over_ribbon, vec2(0.0, -10.0));
    let after = paste(&h);
    assert!(after.left() < before.left() - 100.0, "wheeling down pans the ribbon left: {before:?} -> {after:?}");
}
