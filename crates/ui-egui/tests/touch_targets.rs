//! After a finger touches the screen, every control in the ribbon is at least 44 points square,
//! the smallest target a fingertip hits reliably (Apple's Human Interface Guidelines). The
//! three-row stacks of small buttons become one row, and the ribbon scrolls sideways as on a
//! phone. A mouse or trackpad brings the compact ribbon back.

use egui::accesskit::{Action, Role};
use egui::{Event, Pos2, TouchDeviceId, TouchId, TouchPhase, vec2};
use egui_kittest::kittest::{By, NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// The smallest target a fingertip hits reliably.
const FINGER: f32 = 44.0;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    // Wide enough that every tab's controls are on screen without scrolling.
    let mut h = Harness::builder().with_size(vec2(3600.0, 900.0)).build_ui_state(
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

/// One frame of raw input through the app's hook, as the shells deliver it.
fn frame(h: &mut Harness, events: Vec<Event>) {
    let mut raw = std::mem::take(h.input_mut());
    raw.events.extend(events);
    h.state_mut().raw_input_hook(&mut raw);
    *h.input_mut() = raw;
    h.run_steps(1);
}

fn finger(phase: TouchPhase, pos: Pos2) -> Event {
    Event::Touch { device_id: TouchDeviceId(1), id: TouchId(0), phase, pos, force: None }
}

/// Tap an empty cell far below the ribbon.
fn tap(h: &mut Harness) {
    let at = Pos2::new(900.0, 700.0);
    frame(h, vec![finger(TouchPhase::Start, at)]);
    frame(h, vec![finger(TouchPhase::End, at)]);
    for _ in 0..3 {
        frame(h, vec![]);
    }
}

/// The ribbon tab named `tab`: the lowest button with that name (the title bar has a Home too).
fn tab_rect(h: &Harness, tab: &str) -> egui::Rect {
    let rects = h.query_all_by_role_and_label(Role::Button, tab).map(|n| n.rect());
    rects.max_by(|a, b| a.top().total_cmp(&b.top())).unwrap_or_else(|| panic!("no {tab} tab"))
}

/// Clickable controls in the ribbon card smaller than a fingertip, by name and size. Text labels
/// are not controls (egui makes them clickable only to select their text).
fn small_targets(h: &Harness) -> Vec<String> {
    let tabs = tab_rect(h, "Home");
    let (top, bottom) = (f64::from(tabs.bottom()), f64::from(tabs.bottom() + 92.0));
    h.query_all(By::new().predicate(move |node| {
        node.bounding_box().is_some_and(|b| b.y0 >= top && b.y1 <= bottom) && node.data().supports_action(Action::Click) && node.role() != Role::Label
    }))
    .filter(|node| node.rect().width() < FINGER - 0.5 || node.rect().height() < FINGER - 0.5)
    .map(|node| {
        let name = node.accesskit_node().label().or_else(|| node.accesskit_node().value()).unwrap_or_default();
        format!("{name:?} {:.0}×{:.0}", node.rect().width(), node.rect().height())
    })
    .collect()
}

#[test]
fn after_a_tap_every_ribbon_control_is_finger_sized() {
    let mut small = Vec::new();
    for tab in gridcraft_ui_egui::ribbon::TABS {
        let mut h = harness();
        tap(&mut h);
        h.state_mut().ui.ribbon_tab = (*tab).to_string();
        frame(&mut h, vec![]);
        frame(&mut h, vec![]);
        small.extend(small_targets(&h).into_iter().map(|c| format!("{tab}: {c}")));
    }
    assert!(small.is_empty(), "{} ribbon controls are smaller than {FINGER} points:\n{}", small.len(), small.join("\n"));
}

#[test]
fn a_mouse_keeps_and_brings_back_the_compact_ribbon() {
    let mut h = harness();
    let cut = |h: &Harness| h.get_by_role_and_label(Role::Button, "Cut").rect();
    assert!(cut(&h).height() < 30.0, "with a mouse, Cut stays small: {:?}", cut(&h));
    tap(&mut h);
    assert!(cut(&h).height() >= FINGER - 0.5, "after a tap, Cut is finger-sized: {:?}", cut(&h));
    frame(&mut h, vec![Event::PointerMoved(Pos2::new(900.0, 700.0))]);
    frame(&mut h, vec![]);
    assert!(cut(&h).height() < 30.0, "a hovering mouse brings back the compact ribbon: {:?}", cut(&h));
}

/// The title bar and formula bar have fixed heights; growing their buttons there would overlap
/// them, so only the ribbon grows.
#[test]
fn the_title_bar_and_formula_bar_keep_their_size_after_a_tap() {
    let mut h = harness();
    let sizes = |h: &Harness| ["Undo", "Redo", "Save"].map(|name| h.get_by_role_and_label(Role::Button, name).rect().size());
    let before = sizes(&h);
    tap(&mut h);
    assert_eq!(sizes(&h), before);
}
