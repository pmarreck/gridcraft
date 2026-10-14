//! Scrolling the cell being edited out of view keeps asking for text input. egui's text field
//! stops asking once it is clipped away, and eframe's web runner then takes the focus from its
//! hidden text input, which closes the software keyboard mid-edit (and reopens it on the way
//! back); desktop builds turn the IME off.

use egui::{Event, MouseWheelUnit, Pos2, TouchDeviceId, TouchId, TouchPhase, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A finger on the grid of a phone-sized screen, and how far a drag moves it: far enough to carry
/// A1, the cell being edited, out of view.
const ON_GRID: Pos2 = Pos2::new(200.0, 450.0);
const DRAG: f32 = 200.0;
const STEPS: usize = 6;

fn editing() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = Harness::builder().with_size(vec2(400.0, 600.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().begin_edit(Some("12".into()), false);
    h.run_steps(2);
    assert!(text_input(&h).is_some(), "editing asks for text input");
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

fn text_input(h: &Harness) -> Option<egui::output::IMEOutput> {
    h.output().platform_output.ime
}

/// Runs `frames` and returns the scroll it caused and, for each frame that didn't ask for text
/// input inside the screen, its index.
fn scroll(mut h: Harness, frames: Vec<Vec<Event>>) -> (f32, Vec<usize>) {
    let screen = egui::Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0));
    let before = h.state().view().scroll.y;
    let mut dropped = Vec::new();
    for (i, events) in frames.into_iter().chain(std::iter::repeat_n(vec![], 3)).enumerate() {
        frame(&mut h, events);
        if !text_input(&h).is_some_and(|ime| screen.contains_rect(ime.rect)) {
            dropped.push(i);
        }
    }
    assert!(h.state().editor.is_some(), "the edit stays open");
    (h.state().view().scroll.y - before, dropped)
}

#[test]
fn the_wheel_scrolling_the_edited_cell_away_keeps_the_text_input() {
    let wheel = |dy| Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, dy), phase: TouchPhase::Move, modifiers: Default::default() };
    let mut frames = vec![vec![Event::PointerMoved(ON_GRID)]];
    frames.extend((0..STEPS).map(|_| vec![wheel(-DRAG / STEPS as f32)]));
    let (scrolled, dropped) = scroll(editing(), frames);
    assert!(scrolled > DRAG / 2.0, "the wheel scrolled the sheet ({scrolled})");
    assert_eq!(dropped, Vec::<usize>::new(), "frames that dropped the text input");
}

#[test]
fn a_finger_scrolling_the_edited_cell_away_keeps_the_text_input() {
    let finger = |phase, pos| Event::Touch { device_id: TouchDeviceId(1), id: TouchId(0), phase, pos, force: None };
    let at = |step: usize| ON_GRID - vec2(0.0, DRAG * step as f32 / STEPS as f32);
    let mut frames = vec![vec![finger(TouchPhase::Start, at(0))]];
    frames.extend((1..=STEPS).map(|step| vec![finger(TouchPhase::Move, at(step))]));
    frames.push(vec![finger(TouchPhase::End, at(STEPS))]);
    let (scrolled, dropped) = scroll(editing(), frames);
    assert!(scrolled > DRAG / 2.0, "the finger scrolled the sheet ({scrolled})");
    assert_eq!(dropped, Vec::<usize>::new(), "frames that dropped the text input");
}
