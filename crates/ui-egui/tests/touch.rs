//! Touch input on the grid: one finger scrolls, a tap selects, a tap then a drag selects a
//! rectangle, a double-tap edits, two fingers pinch-zoom, and a long press opens the context menu.
//! Each test sends the events eframe's web runner produces for a finger (an emulated pointer
//! event alongside the touch) through the app's raw-input hook, as the real shells do.

use egui::{Event, Modifiers, PointerButton, Pos2, TouchDeviceId, TouchId, TouchPhase};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Gestures are timed, so frames run at a real display's pace (kittest's default step is 0.25 s).
const FRAME: f32 = 1.0 / 60.0;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "C3", "input": "123"})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(FRAME).build_ui_state(
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

fn point(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
}

/// Run one frame with `events`, passing them through the app's raw-input hook first.
fn frame(h: &mut Harness, events: Vec<Event>) {
    let mut raw = std::mem::take(h.input_mut());
    raw.events.extend(events);
    h.state_mut().raw_input_hook(&mut raw);
    *h.input_mut() = raw;
    h.run_steps(1);
}

/// Run `n` frames with no new input (time passes by the harness's step).
fn idle(h: &mut Harness, n: usize) {
    for _ in 0..n {
        frame(h, vec![]);
    }
}

fn touch(id: u64, phase: TouchPhase, pos: Pos2) -> Event {
    Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos, force: None }
}

fn down(id: u64, pos: Pos2) -> Vec<Event> {
    let mut v = Vec::new();
    if id == 0 {
        v.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    }
    v.push(touch(id, TouchPhase::Start, pos));
    v
}

fn moved(id: u64, pos: Pos2) -> Vec<Event> {
    let mut v = Vec::new();
    if id == 0 {
        v.push(Event::PointerMoved(pos));
    }
    v.push(touch(id, TouchPhase::Move, pos));
    v
}

fn up(id: u64, pos: Pos2) -> Vec<Event> {
    let mut v = Vec::new();
    if id == 0 {
        v.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        v.push(Event::PointerGone);
    }
    v.push(touch(id, TouchPhase::End, pos));
    v
}

fn selection(h: &Harness) -> RangeRef {
    h.state().session.active().unwrap().selection.current()
}

fn range(a1: &str) -> RangeRef {
    RangeRef::parse(a1).unwrap()
}

fn tap(h: &mut Harness, at: Pos2) {
    frame(h, down(0, at));
    frame(h, up(0, at));
    frame(h, vec![]);
}

#[test]
fn a_one_finger_drag_scrolls_the_sheet_without_selecting() {
    let mut h = harness();
    let from = point(&h, "E10");
    // Drag up and left by steps, slowly enough that no flick follows.
    for i in 0..=10 {
        frame(&mut h, if i == 0 { down(0, from) } else { moved(0, from - egui::vec2(15.0, 30.0) * i as f32) });
    }
    idle(&mut h, 10);
    let end = from - egui::vec2(150.0, 300.0);
    frame(&mut h, moved(0, end));
    frame(&mut h, up(0, end));
    idle(&mut h, 3);
    let now = point(&h, "E10");
    assert!((now - end).length() < 0.5, "the cell stayed under the finger: it is at {now:?}, the finger lifted at {end:?}");
    assert_eq!(selection(&h), range("A1"), "and nothing was selected");
}

#[test]
fn a_tap_selects_the_cell() {
    let mut h = harness();
    let at = point(&h, "C3");
    tap(&mut h, at);
    assert_eq!(selection(&h), range("C3"));
    assert!(h.state().editor.is_none());
}

#[test]
fn a_tap_then_a_drag_selects_a_rectangle() {
    let mut h = harness();
    let (from, to) = (point(&h, "B2"), point(&h, "D6"));
    tap(&mut h, from);
    frame(&mut h, down(0, from));
    for i in 1..=8 {
        frame(&mut h, moved(0, from.lerp(to, i as f32 / 8.0)));
    }
    frame(&mut h, up(0, to));
    idle(&mut h, 2);
    assert_eq!(selection(&h), range("B2:D6"));
    assert_eq!(h.state().view().scroll, egui::Vec2::ZERO, "no scrolling");
}

#[test]
fn a_double_tap_edits_the_cell() {
    let mut h = harness();
    let at = point(&h, "C3");
    tap(&mut h, at);
    tap(&mut h, at);
    assert_eq!(selection(&h), range("C3"));
    assert!(h.state().editor.is_some(), "the in-cell editor opened");
}

#[test]
fn two_fingers_pinch_to_zoom() {
    let mut h = harness();
    let zoom = |h: &Harness| h.state().session.active().unwrap().wb.active().unwrap().zoom;
    assert_eq!(zoom(&h), 100);
    let c = point(&h, "D8");
    frame(&mut h, down(0, c - egui::vec2(50.0, 0.0)));
    frame(&mut h, down(1, c + egui::vec2(50.0, 0.0)));
    for i in 1..=5 {
        let half = 50.0 + 10.0 * i as f32;
        let mut both = moved(0, c - egui::vec2(half, 0.0));
        both.extend(moved(1, c + egui::vec2(half, 0.0)));
        frame(&mut h, both);
    }
    frame(&mut h, up(1, c + egui::vec2(100.0, 0.0)));
    frame(&mut h, up(0, c - egui::vec2(100.0, 0.0)));
    idle(&mut h, 2);
    assert_eq!(zoom(&h), 200, "spreading twice as wide doubles the zoom");
    assert_eq!(selection(&h), range("A1"), "and selects nothing");
}

/// A slow pinch changes the zoom by less than half a percent a frame at low zooms; rounding each
/// frame's zoom to a whole percent would throw that away, and the zoom would never move.
#[test]
fn a_slow_pinch_at_a_low_zoom_still_zooms() {
    let mut h = harness();
    let zoom = |h: &Harness| h.state().session.active().unwrap().wb.active().unwrap().zoom;
    h.state_mut().session.execute("view.zoom", json!({"percent": 10})).unwrap();
    h.run_steps(2);
    assert_eq!(zoom(&h), 10);
    let c = point(&h, "D8");
    frame(&mut h, down(0, c - egui::vec2(50.0, 0.0)));
    frame(&mut h, down(1, c + egui::vec2(50.0, 0.0)));
    for i in 1..=50 {
        let half = 50.0 + i as f32;
        let mut both = moved(0, c - egui::vec2(half, 0.0));
        both.extend(moved(1, c + egui::vec2(half, 0.0)));
        frame(&mut h, both);
    }
    frame(&mut h, up(1, c + egui::vec2(100.0, 0.0)));
    frame(&mut h, up(0, c - egui::vec2(100.0, 0.0)));
    idle(&mut h, 2);
    assert_eq!(zoom(&h), 20, "spreading twice as wide doubles the zoom, however slowly");
}

#[test]
fn a_long_press_opens_the_context_menu_on_that_cell() {
    let mut h = harness();
    let at = point(&h, "C3");
    frame(&mut h, down(0, at));
    // Well past the long-press time at the harness's step.
    idle(&mut h, 60);
    assert!(h.state().grid.context_menu.is_some(), "the context menu opened");
    assert_eq!(selection(&h), range("C3"), "for the pressed cell");
    frame(&mut h, up(0, at));
    idle(&mut h, 2);
    assert!(h.state().grid.context_menu.is_some(), "lifting leaves it open");
}

/// Safari opens a file picker (and starts downloads) only while it is handling the tap itself.
/// eframe's web runner runs a frame inside `pointerup`, before `touchend` arrives; a tap must click
/// in that frame, so a button that opens the picker does so while the browser still allows it.
#[test]
fn a_tap_clicks_in_the_frame_the_browser_reports_the_lift() {
    let mut h = harness();
    let view = h.get_by_role_and_label(egui::accesskit::Role::Button, "View").rect().center();
    frame(&mut h, down(0, view));
    // `pointerup`: eframe runs this frame at once, inside the browser's event handler.
    frame(&mut h, vec![Event::PointerButton { pos: view, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    assert_eq!(h.state().ui.ribbon_tab, "View", "the tap clicked while the browser was handling it");
    // `touchend` follows in a later frame; the tap does not click twice.
    frame(&mut h, vec![touch(0, TouchPhase::End, view)]);
    idle(&mut h, 3);
    assert_eq!(h.state().ui.ribbon_tab, "View");
    assert!(!h.state().ui.ribbon_collapsed, "a second click on the active tab would collapse the ribbon");
}
