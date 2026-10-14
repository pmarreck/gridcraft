//! Touch gestures, translated into the mouse, wheel and zoom input the rest of the UI already
//! understands.
//!
//! egui turns the first finger into a mouse pointer, so out of the box a one-finger drag selects a
//! range on a phone instead of scrolling. [`TouchTranslator`] rewrites each frame's raw input
//! before egui sees it: the emulated pointer events are dropped and the touches become
//! - a tap: a primary click where the finger lifted;
//! - a tap followed at once by a press nearby: a held primary button, so dragging selects a
//!   rectangle and lifting without moving is a double-click (edit the cell);
//! - a one-finger drag: wheel scrolling that follows the finger, with momentum after a flick;
//! - two fingers: wheel scrolling by the midpoint and zoom by the change in their spread;
//! - a long press: a secondary click (the context menu).
//!
//! Every widget therefore reacts to touch through its existing mouse code. Input with no touches
//! passes through untouched, so mouse and trackpad behaviour is unchanged. Pure: the caller
//! supplies the clock.

use std::collections::VecDeque;

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2};

/// How far (points) a finger may wander and still tap or long-press.
pub const TAP_SLOP: f32 = 10.0;
/// A press this soon after a tap, and this close to it, holds the button (tap-then-drag).
/// Matches egui's double-click window, so lifting without moving is a double-click.
pub const DOUBLE_TAP_SECONDS: f64 = 0.3;
pub const DOUBLE_TAP_DISTANCE: f32 = 30.0;
/// How long a finger must rest, unmoved, to open the context menu.
pub const LONG_PRESS_SECONDS: f64 = 0.5;
/// The recent stretch of movement a flick's speed is measured over.
const VELOCITY_WINDOW: f64 = 0.1;
/// Slower lifts (points per second) just stop.
const FLING_MIN_SPEED: f32 = 200.0;
/// Momentum decays by e every this many seconds, and stops below the stop speed.
const MOMENTUM_TIME_CONSTANT: f64 = 0.325;
const MOMENTUM_STOP_SPEED: f32 = 20.0;
/// The longest frame gap one momentum step covers, so a stalled tab does not jump.
const MAX_STEP: f64 = 0.1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Mode {
    #[default]
    Idle,
    /// One finger down; not yet a tap, a scroll or a long press.
    Pending { start: Pos2, since: f64 },
    /// The primary button is held where the finger is (tap-then-drag).
    Held { at: Pos2 },
    /// Scrolling from `at` (the finger, or two fingers' midpoint, `spread` apart). The pointer
    /// stays at `origin`, where the gesture began, so the wheel keeps going to the view the finger
    /// first touched even when the finger leaves it.
    Scroll { at: Pos2, spread: Option<f32>, origin: Pos2 },
    /// The gesture already did its job (a long press, or stopping a flick): ignore it until lifted.
    Spent,
}

#[derive(Clone, Copy, Debug)]
struct Momentum {
    velocity: Vec2,
    at: Pos2,
    last: f64,
}

/// The translator's state between frames.
#[derive(Default)]
pub struct TouchTranslator {
    fingers: Vec<(u64, Pos2)>,
    mode: Mode,
    /// Where and when the last tap lifted.
    last_tap: Option<(Pos2, f64)>,
    /// Events for the next frame (a click's release).
    queued: Vec<Event>,
    /// Recent scroll steps, for a flick's speed.
    samples: VecDeque<(f64, Vec2)>,
    momentum: Option<Momentum>,
    /// See [`TouchTranslator::coarse`].
    coarse: bool,
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
}

fn wheel(delta: Vec2, phase: TouchPhase) -> Event {
    Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase, modifiers: Modifiers::NONE }
}

impl TouchTranslator {
    /// Whether the person is pointing with a finger (a coarse pointer, in CSS terms), so controls
    /// should be finger-sized.
    pub fn coarse(&self) -> bool {
        self.coarse
    }

    /// Rewrite one frame's `events` at time `now` (seconds). Returns how soon another frame is
    /// needed (for a pending long press, momentum or a queued release), or `None`.
    pub fn translate(&mut self, now: f64, events: &mut Vec<Event>) -> Option<f64> {
        let touching = !self.fingers.is_empty() || events.iter().any(|e| matches!(e, Event::Touch { .. }));
        // Only a mouse or trackpad hovers or wheels with no finger down.
        if touching {
            self.coarse = true;
        } else if events.iter().any(|e| matches!(e, Event::PointerMoved(_) | Event::MouseWheel { .. })) {
            self.coarse = false;
        }
        if !touching && self.queued.is_empty() && self.momentum.is_none() && self.mode == Mode::Idle {
            return None;
        }
        let input = std::mem::take(events);
        let mut out = std::mem::take(&mut self.queued);
        let caught = self.step_momentum(now, &input, &mut out);
        let mut scrolled = false;
        for event in input {
            match event {
                Event::Touch { id, phase, pos, .. } => {
                    scrolled |= self.touch(now, id.0, phase, pos, caught, &mut out);
                }
                // The browser reports the lift as a pointer release before the touch ends, and
                // eframe runs that frame inside the browser's handler, the only time Safari lets a
                // page open a file picker. A tap clicks there, so its button may open one.
                Event::PointerButton { button: PointerButton::Primary, pressed: false, pos, .. }
                    if self.fingers.len() == 1 && matches!(self.mode, Mode::Pending { .. }) =>
                {
                    self.tap(now, pos, &mut out);
                    self.queued.clear();
                    out.push(button(pos, PointerButton::Primary, false));
                    self.mode = Mode::Spent;
                }
                // The pointer egui emulates from the first finger: the touches say it better.
                Event::PointerButton { button: PointerButton::Primary, .. } | Event::PointerMoved(_) | Event::PointerGone if touching => {}
                other => out.push(other),
            }
        }
        if scrolled {
            self.scroll(now, &mut out);
        }
        if let Mode::Pending { start, since } = self.mode
            && now - since >= LONG_PRESS_SECONDS
        {
            out.push(Event::PointerMoved(start));
            out.push(button(start, PointerButton::Secondary, true));
            self.queued.push(button(start, PointerButton::Secondary, false));
            self.mode = Mode::Spent;
        }
        *events = out;
        match self.mode {
            _ if !self.queued.is_empty() || self.momentum.is_some() => Some(0.0),
            Mode::Pending { since, .. } => Some((since + LONG_PRESS_SECONDS - now).max(0.0)),
            _ => None,
        }
    }

    /// Advance a flick's momentum, or stop it when a finger lands (returning true).
    fn step_momentum(&mut self, now: f64, input: &[Event], out: &mut Vec<Event>) -> bool {
        let Some(m) = self.momentum.as_mut() else { return false };
        let landed = input.iter().any(|e| matches!(e, Event::Touch { phase: TouchPhase::Start, .. }));
        let dt = (now - m.last).clamp(0.0, MAX_STEP);
        m.velocity *= (-dt / MOMENTUM_TIME_CONSTANT).exp() as f32;
        m.last = now;
        if landed || m.velocity.length() < MOMENTUM_STOP_SPEED {
            out.push(wheel(Vec2::ZERO, TouchPhase::End));
            out.push(Event::PointerGone);
            self.momentum = None;
            return landed;
        }
        out.push(Event::PointerMoved(m.at));
        out.push(wheel(m.velocity * dt as f32, TouchPhase::Move));
        false
    }

    /// Apply one touch event; returns true when the scroll reference should be re-read.
    fn touch(&mut self, now: f64, id: u64, phase: TouchPhase, pos: Pos2, caught: bool, out: &mut Vec<Event>) -> bool {
        match phase {
            TouchPhase::Start => {
                self.fingers.retain(|(f, _)| *f != id);
                self.fingers.push((id, pos));
                match self.fingers.len() {
                    1 => self.first_finger(now, pos, caught, out),
                    2 => self.second_finger(out),
                    _ => {}
                }
                false
            }
            TouchPhase::Move => {
                if let Some(f) = self.fingers.iter_mut().find(|(f, _)| *f == id) {
                    f.1 = pos;
                }
                match self.mode {
                    Mode::Pending { start, .. } if start.distance(pos) > TAP_SLOP => {
                        out.push(Event::PointerMoved(start));
                        out.push(wheel(Vec2::ZERO, TouchPhase::Start));
                        out.push(wheel(pos - start, TouchPhase::Move));
                        self.samples.clear();
                        self.samples.push_back((now, pos - start));
                        self.mode = Mode::Scroll { at: pos, spread: None, origin: start };
                        false
                    }
                    Mode::Held { .. } => {
                        out.push(Event::PointerMoved(pos));
                        self.mode = Mode::Held { at: pos };
                        false
                    }
                    Mode::Scroll { .. } => true,
                    _ => false,
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                self.fingers.retain(|(f, _)| *f != id);
                self.lift(now, pos, phase == TouchPhase::Cancel, out);
                false
            }
        }
    }

    fn first_finger(&mut self, now: f64, pos: Pos2, caught: bool, out: &mut Vec<Event>) {
        let follows_tap = self.last_tap.is_some_and(|(at, t)| now - t <= DOUBLE_TAP_SECONDS && at.distance(pos) <= DOUBLE_TAP_DISTANCE);
        self.mode = if caught {
            Mode::Spent
        } else if follows_tap {
            self.last_tap = None;
            out.push(Event::PointerMoved(pos));
            out.push(button(pos, PointerButton::Primary, true));
            Mode::Held { at: pos }
        } else {
            Mode::Pending { start: pos, since: now }
        };
    }

    fn second_finger(&mut self, out: &mut Vec<Event>) {
        if let Mode::Held { at } = self.mode {
            out.push(button(at, PointerButton::Primary, false));
        }
        let (mid, spread) = self.reference();
        let origin = if let Mode::Scroll { origin, .. } = self.mode {
            origin
        } else {
            out.push(Event::PointerMoved(mid));
            out.push(wheel(Vec2::ZERO, TouchPhase::Start));
            mid
        };
        self.samples.clear();
        self.mode = Mode::Scroll { at: mid, spread, origin };
    }

    /// A tap at `pos`: press now and release on the next frame (unless the caller releases now).
    fn tap(&mut self, now: f64, pos: Pos2, out: &mut Vec<Event>) {
        out.push(Event::PointerMoved(pos));
        out.push(button(pos, PointerButton::Primary, true));
        self.queued.push(button(pos, PointerButton::Primary, false));
        self.queued.push(Event::PointerGone);
        self.last_tap = Some((pos, now));
    }

    fn lift(&mut self, now: f64, pos: Pos2, cancel: bool, out: &mut Vec<Event>) {
        match self.mode {
            Mode::Pending { .. } if cancel => {
                out.push(Event::PointerGone);
                self.mode = Mode::Idle;
            }
            Mode::Pending { .. } => {
                self.tap(now, pos, out);
                self.mode = Mode::Idle;
            }
            Mode::Held { .. } => {
                out.push(button(pos, PointerButton::Primary, false));
                out.push(Event::PointerGone);
                self.mode = Mode::Idle;
            }
            Mode::Scroll { origin, .. } if !self.fingers.is_empty() => {
                // From two fingers to one: carry on from the remaining finger, without a jump.
                let (at, spread) = self.reference();
                self.mode = Mode::Scroll { at, spread, origin };
            }
            Mode::Scroll { origin, .. } => {
                out.push(wheel(Vec2::ZERO, TouchPhase::End));
                out.push(Event::PointerGone);
                let velocity = if cancel { Vec2::ZERO } else { self.flick_velocity(now) };
                if velocity.length() >= FLING_MIN_SPEED {
                    self.momentum = Some(Momentum { velocity, at: origin, last: now });
                }
                self.mode = Mode::Idle;
            }
            Mode::Spent if self.fingers.is_empty() => {
                out.push(Event::PointerGone);
                self.mode = Mode::Idle;
            }
            Mode::Spent | Mode::Idle => {}
        }
    }

    /// Scroll (and zoom) by how far the reference point (and spread) moved since last time.
    fn scroll(&mut self, now: f64, out: &mut Vec<Event>) {
        let Mode::Scroll { at, spread, origin } = self.mode else { return };
        let (now_at, now_spread) = self.reference();
        out.push(Event::PointerMoved(origin));
        out.push(wheel(now_at - at, TouchPhase::Move));
        if let (Some(before), Some(after)) = (spread, now_spread)
            && before > 0.0
            && after != before
        {
            out.push(Event::Zoom(after / before));
        }
        self.samples.push_back((now, now_at - at));
        self.mode = Mode::Scroll { at: now_at, spread: now_spread, origin };
    }

    /// The point scrolling follows: the finger, or the first two fingers' midpoint and spread.
    fn reference(&self) -> (Pos2, Option<f32>) {
        match self.fingers.as_slice() {
            [(_, a), (_, b), ..] => (a.lerp(*b, 0.5), Some(a.distance(*b))),
            [(_, a)] => (*a, None),
            [] => (Pos2::ZERO, None),
        }
    }

    /// Speed of the last stretch of scrolling (points per second), from the steps after the oldest
    /// one inside the window, over the time they span.
    fn flick_velocity(&mut self, now: f64) -> Vec2 {
        while self.samples.front().is_some_and(|(t, _)| now - t > VELOCITY_WINDOW) {
            self.samples.pop_front();
        }
        let (Some((first, _)), Some((last, _))) = (self.samples.front(), self.samples.back()) else { return Vec2::ZERO };
        let span = (last - first) as f32;
        if span <= 0.0 {
            return Vec2::ZERO;
        }
        self.samples.iter().skip(1).fold(Vec2::ZERO, |sum, (_, d)| sum + *d) / span
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{TouchDeviceId, TouchId, pos2, vec2};

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Event {
        Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos: pos2(x, y), force: None }
    }

    /// What eframe's web runner sends for a first finger: an emulated pointer event, then the touch.
    fn down(id: u64, x: f32, y: f32) -> Vec<Event> {
        let mut v = Vec::new();
        if id == 0 {
            v.push(Event::PointerButton { pos: pos2(x, y), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        }
        v.push(touch(id, TouchPhase::Start, x, y));
        v
    }

    fn moved(id: u64, x: f32, y: f32) -> Vec<Event> {
        let mut v = Vec::new();
        if id == 0 {
            v.push(Event::PointerMoved(pos2(x, y)));
        }
        v.push(touch(id, TouchPhase::Move, x, y));
        v
    }

    fn up(id: u64, x: f32, y: f32) -> Vec<Event> {
        let mut v = Vec::new();
        if id == 0 {
            v.push(Event::PointerButton { pos: pos2(x, y), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            v.push(Event::PointerGone);
        }
        v.push(touch(id, TouchPhase::End, x, y));
        v
    }

    /// A compact, comparable description of the translated events.
    fn describe(events: &[Event]) -> Vec<String> {
        events
            .iter()
            .map(|e| match e {
                Event::PointerMoved(p) => format!("move {} {}", p.x, p.y),
                Event::PointerButton { pos, button, pressed, .. } => {
                    format!("{} {:?} {} {}", if *pressed { "press" } else { "release" }, button, pos.x, pos.y)
                }
                Event::PointerGone => "gone".into(),
                Event::MouseWheel { delta, phase, unit: MouseWheelUnit::Point, .. } => format!("wheel {phase:?} {} {}", delta.x, delta.y),
                Event::Zoom(f) => format!("zoom {f}"),
                Event::Touch { .. } => "touch".into(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    /// Feed one frame and describe what egui would see.
    fn frame(t: &mut TouchTranslator, now: f64, mut events: Vec<Event>) -> Vec<String> {
        t.translate(now, &mut events);
        describe(&events)
    }

    #[test]
    fn input_without_touches_passes_through_unchanged() {
        let mut t = TouchTranslator::default();
        let mouse = vec![
            Event::PointerMoved(pos2(5.0, 6.0)),
            Event::PointerButton { pos: pos2(5.0, 6.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, -1.0), phase: TouchPhase::Move, modifiers: Modifiers::NONE },
            Event::Zoom(1.1),
        ];
        let mut events = mouse.clone();
        assert_eq!(t.translate(0.0, &mut events), None);
        assert_eq!(events, mouse);
    }

    /// A finger makes the pointer coarse (the ribbon grows finger-sized targets); a mouse or
    /// trackpad, which hovers and wheels with no finger down, makes it fine again. Each case is
    /// a run of frames and the verdict after the last.
    #[test]
    fn the_pointer_is_coarse_after_a_finger_and_fine_after_a_mouse() {
        let hover = || vec![Event::PointerMoved(pos2(5.0, 6.0))];
        let scroll = || vec![wheel(vec2(0.0, -10.0), TouchPhase::Move)];
        let typing = || vec![Event::Text("a".into())];
        let tap = || vec![down(0, 100.0, 50.0), up(0, 100.0, 50.0)];
        let cases: Vec<(&str, Vec<Vec<Event>>, bool)> = vec![
            ("no input yet", vec![], false),
            ("a hover", vec![hover()], false),
            ("a wheel", vec![scroll()], false),
            ("a finger down", vec![down(0, 100.0, 50.0)], true),
            ("a tap", tap(), true),
            ("a drag, whose emulated pointer moves", vec![down(0, 100.0, 50.0), moved(0, 100.0, 90.0)], true),
            ("a tap, then typing", [tap(), vec![typing()]].concat(), true),
            ("a tap, then a hover", [tap(), vec![hover()]].concat(), false),
            ("a tap, then a wheel", [tap(), vec![scroll()]].concat(), false),
            ("a hover, then a tap", [vec![hover()], tap()].concat(), true),
            ("a hover and a finger in one frame", vec![[hover(), down(0, 100.0, 50.0)].concat()], true),
        ];
        let wrong: Vec<_> = cases
            .into_iter()
            .filter_map(|(name, frames, coarse)| {
                let mut t = TouchTranslator::default();
                for (i, mut events) in frames.into_iter().enumerate() {
                    t.translate(i as f64 * 0.05, &mut events);
                }
                (t.coarse() != coarse).then(|| format!("{name}: expected coarse = {coarse}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_tap_is_a_click_where_the_finger_lifted() {
        let mut t = TouchTranslator::default();
        assert_eq!(frame(&mut t, 0.00, down(0, 100.0, 50.0)), Vec::<String>::new(), "nothing happens until we know it is a tap");
        assert_eq!(frame(&mut t, 0.05, moved(0, 103.0, 52.0)), Vec::<String>::new(), "a wobble within the slop is still a tap");
        assert_eq!(
            frame(&mut t, 0.10, up(0, 103.0, 52.0)),
            ["move 103 52", "press Primary 103 52", "release Primary 103 52", "gone"],
            "the pointer release clicks at once, while the browser is still handling the tap"
        );
        assert_eq!(frame(&mut t, 0.12, vec![]), Vec::<String>::new());
    }

    #[test]
    fn a_lift_without_a_pointer_release_clicks_over_two_frames() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 50.0));
        assert_eq!(frame(&mut t, 0.10, vec![touch(0, TouchPhase::End, 103.0, 52.0)]), ["move 103 52", "press Primary 103 52"]);
        assert_eq!(frame(&mut t, 0.12, vec![]), ["release Primary 103 52", "gone"], "the release comes a frame later so egui sees a click");
        assert_eq!(frame(&mut t, 0.14, vec![]), Vec::<String>::new());
    }

    #[test]
    fn a_one_finger_drag_scrolls_with_the_finger_and_never_presses() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 300.0));
        // Past the slop: the whole travel so far scrolls, so the content stays under the finger. The
        // pointer stays where the finger landed, so the wheel keeps reaching that view.
        assert_eq!(frame(&mut t, 0.02, moved(0, 100.0, 280.0)), ["move 100 300", "wheel Start 0 0", "wheel Move 0 -20"]);
        assert_eq!(frame(&mut t, 0.04, moved(0, 104.0, 250.0)), ["move 100 300", "wheel Move 4 -30"]);
        // A slow lift ends the scroll without momentum.
        assert_eq!(frame(&mut t, 1.00, moved(0, 104.0, 250.0)), ["move 100 300", "wheel Move 0 0"]);
        assert_eq!(frame(&mut t, 1.30, up(0, 104.0, 250.0)), ["wheel End 0 0", "gone"]);
        assert_eq!(frame(&mut t, 1.32, vec![]), Vec::<String>::new());
    }

    #[test]
    fn a_flick_keeps_scrolling_and_slows_to_a_stop() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 400.0));
        for i in 1..=5 {
            frame(&mut t, 0.016 * i as f64, moved(0, 100.0, 400.0 - 30.0 * i as f32));
        }
        let lifted = 0.016 * 5.0;
        frame(&mut t, lifted, up(0, 100.0, 250.0));
        let mut now = lifted;
        let mut steps = Vec::new();
        for _ in 0..1000 {
            now += 0.016;
            let mut events = Vec::new();
            let again = t.translate(now, &mut events);
            steps.extend(events.iter().filter_map(|e| match e {
                Event::MouseWheel { delta, phase: TouchPhase::Move, .. } => Some(delta.y),
                _ => None,
            }));
            if again.is_none() {
                break;
            }
        }
        assert!(steps.len() > 5, "momentum lasts a while: {steps:?}");
        assert!(steps.iter().all(|d| *d < 0.0), "and keeps the flick's direction: {steps:?}");
        assert!(steps.windows(2).all(|w| w[1].abs() <= w[0].abs() + 1e-3), "slowing down: {steps:?}");
        assert!(steps.len() < 1000, "and stops");
    }

    #[test]
    fn touching_again_stops_momentum() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 400.0));
        for i in 1..=5 {
            frame(&mut t, 0.016 * i as f64, moved(0, 100.0, 400.0 - 30.0 * i as f32));
        }
        frame(&mut t, 0.08, up(0, 100.0, 250.0));
        assert!(frame(&mut t, 0.096, vec![]).iter().any(|e| e.starts_with("wheel Move")));
        assert_eq!(frame(&mut t, 0.112, down(0, 100.0, 250.0)), ["wheel End 0 0", "gone"], "a finger stops a flick");
        assert_eq!(frame(&mut t, 0.20, up(0, 100.0, 250.0)), ["gone"], "lifting the finger that stopped a flick does not click either");
    }

    #[test]
    fn a_tap_then_a_drag_selects_a_rectangle() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 50.0));
        frame(&mut t, 0.08, up(0, 100.0, 50.0));
        frame(&mut t, 0.10, vec![]);
        // Pressing again nearby, soon after, holds the button down at once.
        assert_eq!(frame(&mut t, 0.25, down(0, 104.0, 48.0)), ["move 104 48", "press Primary 104 48"]);
        assert_eq!(frame(&mut t, 0.30, moved(0, 200.0, 150.0)), ["move 200 150"]);
        assert_eq!(frame(&mut t, 0.40, up(0, 200.0, 150.0)), ["release Primary 200 150", "gone"]);
    }

    #[test]
    fn a_double_tap_is_two_clicks_in_quick_succession() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 50.0));
        frame(&mut t, 0.06, up(0, 100.0, 50.0));
        frame(&mut t, 0.08, vec![]);
        assert_eq!(frame(&mut t, 0.16, down(0, 101.0, 51.0)), ["move 101 51", "press Primary 101 51"]);
        assert_eq!(frame(&mut t, 0.22, up(0, 101.0, 51.0)), ["release Primary 101 51", "gone"]);
    }

    #[test]
    fn a_second_tap_too_late_or_too_far_is_a_new_tap() {
        for (when, x) in [(0.80, 100.0), (0.20, 300.0)] {
            let mut t = TouchTranslator::default();
            frame(&mut t, 0.00, down(0, 100.0, 50.0));
            frame(&mut t, 0.06, up(0, 100.0, 50.0));
            frame(&mut t, 0.08, vec![]);
            assert_eq!(frame(&mut t, when, down(0, x, 50.0)), Vec::<String>::new(), "at {when}s, x {x}: undecided");
            assert_eq!(
                frame(&mut t, when + 0.05, moved(0, x, 120.0)),
                [format!("move {x} 50"), "wheel Start 0 0".into(), "wheel Move 0 70".into()],
                "so a drag scrolls"
            );
        }
    }

    #[test]
    fn a_long_press_is_a_secondary_click_and_lifting_does_nothing_more() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 50.0));
        let wait = t.translate(0.10, &mut Vec::new()).expect("a pending long press wants a frame");
        assert!(wait > 0.0 && wait <= LONG_PRESS_SECONDS, "{wait}");
        assert_eq!(frame(&mut t, LONG_PRESS_SECONDS, vec![]), ["move 100 50", "press Secondary 100 50"]);
        assert_eq!(frame(&mut t, LONG_PRESS_SECONDS + 0.02, vec![]), ["release Secondary 100 50"]);
        assert_eq!(frame(&mut t, 0.9, moved(0, 140.0, 90.0)), Vec::<String>::new(), "no scrolling after the menu opened");
        assert_eq!(frame(&mut t, 1.0, up(0, 140.0, 90.0)), ["gone"]);
    }

    #[test]
    fn two_fingers_scroll_by_their_midpoint_and_zoom_by_their_spread() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 100.0));
        assert_eq!(frame(&mut t, 0.02, down(1, 200.0, 100.0)), ["move 150 100", "wheel Start 0 0"], "no tap, no click");
        // Spread from 100 to 200 apart around a midpoint that moves 10 down.
        let mut both = moved(0, 50.0, 110.0);
        both.extend(moved(1, 250.0, 110.0));
        assert_eq!(frame(&mut t, 0.04, both), ["move 150 100", "wheel Move 0 10", "zoom 2"]);
        // One finger lifts: the other keeps scrolling, without jumping to its own position.
        assert_eq!(frame(&mut t, 0.06, up(1, 250.0, 110.0)), Vec::<String>::new());
        assert_eq!(frame(&mut t, 0.08, moved(0, 50.0, 130.0)), ["move 150 100", "wheel Move 0 20"]);
        assert_eq!(frame(&mut t, 1.00, up(0, 50.0, 130.0)), ["wheel End 0 0", "gone"]);
        assert_eq!(frame(&mut t, 1.02, vec![]), Vec::<String>::new(), "and never a click");
    }

    #[test]
    fn a_second_finger_during_a_tap_drag_releases_the_button_first() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 50.0));
        frame(&mut t, 0.06, up(0, 100.0, 50.0));
        frame(&mut t, 0.08, vec![]);
        frame(&mut t, 0.16, down(0, 100.0, 50.0));
        frame(&mut t, 0.20, moved(0, 150.0, 90.0));
        assert_eq!(frame(&mut t, 0.24, down(1, 250.0, 90.0)), ["release Primary 150 90", "move 200 90", "wheel Start 0 0"]);
    }

    #[test]
    fn a_cancelled_touch_neither_clicks_nor_flings() {
        let mut t = TouchTranslator::default();
        frame(&mut t, 0.00, down(0, 100.0, 50.0));
        assert_eq!(frame(&mut t, 0.05, vec![touch(0, TouchPhase::Cancel, 100.0, 50.0)]), ["gone"]);
        assert_eq!(t.translate(0.07, &mut Vec::new()), None);
        frame(&mut t, 1.00, down(0, 100.0, 400.0));
        for i in 1..=5 {
            frame(&mut t, 1.0 + 0.016 * i as f64, moved(0, 100.0, 400.0 - 30.0 * i as f32));
        }
        assert_eq!(frame(&mut t, 1.09, vec![touch(0, TouchPhase::Cancel, 100.0, 250.0)]), ["wheel End 0 0", "gone"]);
        assert_eq!(t.translate(1.11, &mut Vec::new()), None, "no momentum");
    }
}
