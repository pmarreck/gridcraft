//! On a phone-width window the layout compacts: every ribbon tab stays reachable by swiping the
//! tab row, the status bar gives its height to the grid, and the workbook title never covers the
//! title bar's buttons.

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
