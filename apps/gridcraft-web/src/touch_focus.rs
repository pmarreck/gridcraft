//! Keeps scrolling and pinching from moving the browser's keyboard focus.
//!
//! eframe 0.36.2 focuses the canvas on every `pointerup` and, on every `touchend`, blurs and
//! refocuses its hidden text input when that input has focus (a workaround that makes iOS show the
//! keyboard). While a cell is being edited, each scroll therefore closes and reopens the software
//! keyboard, and reopens it after the person dismissed it.
//!
//! [`install`] listens ahead of eframe. While the fingers scroll or pinch ([`Gesture`]), their
//! `pointerup` and `touchend` never reach eframe; the touch endings go to egui through
//! [`Endings`] instead, so the touch translator still sees every finger lift. A tap still reaches
//! eframe as before: it clicks inside the browser's handler, where Safari allows file pickers.

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_ui_egui::touch::TAP_SLOP;
use wasm_bindgen::JsValue;

/// Touch endings delivered to egui by GridCraft instead of eframe, for the next frame.
pub type Endings = Rc<RefCell<Vec<egui::Event>>>;

/// The most fingers followed at once; more are ignored.
const MAX_FINGERS: usize = 10;

/// Whether the fingers on the screen navigate (scroll or pinch) instead of tapping: a second
/// finger, or one that wandered further than a tap may ([`TAP_SLOP`], the touch translator's own
/// limit, so both agree on what is a tap). Positions are in egui points.
#[derive(Default)]
pub struct Gesture {
    /// Each finger's id and where it landed.
    fingers: Vec<(i32, egui::Pos2)>,
    navigating: bool,
}

impl Gesture {
    pub fn down(&mut self, id: i32, at: egui::Pos2) {
        if self.fingers.is_empty() {
            self.navigating = false;
        }
        if self.fingers.len() < MAX_FINGERS && !self.fingers.iter().any(|(f, _)| *f == id) {
            self.fingers.push((id, at));
        }
        self.navigating |= self.fingers.len() > 1;
    }

    pub fn moved(&mut self, id: i32, at: egui::Pos2) {
        self.navigating |= self.fingers.iter().any(|(f, start)| *f == id && start.distance(at) > TAP_SLOP);
    }

    /// A finger lifted or was cancelled. The verdict stands until the next gesture begins.
    pub fn up(&mut self, id: i32) {
        self.fingers.retain(|(f, _)| *f != id);
    }

    pub fn navigating(&self) -> bool {
        self.navigating
    }
}

/// The changed touches of `event`, at most [`MAX_FINGERS`].
fn changed(event: &web_sys::TouchEvent) -> Vec<web_sys::Touch> {
    let list = event.changed_touches();
    (0..list.length().min(MAX_FINGERS as u32)).filter_map(|i| list.item(i)).collect()
}

/// Where `touch` is on the canvas, in egui points (as eframe computes it).
fn position(canvas: &web_sys::HtmlCanvasElement, ctx: &egui::Context, touch: &web_sys::Touch) -> egui::Pos2 {
    let bounds = canvas.get_bounding_client_rect();
    let zoom = ctx.zoom_factor();
    egui::pos2(touch.client_x() as f32 - bounds.left() as f32, touch.client_y() as f32 - bounds.top() as f32) / zoom
}

/// Listen ahead of eframe's handlers (capture phase on the window) for the life of `runner`.
pub fn install(runner: &eframe::WebRunner, endings: Endings) -> Result<(), JsValue> {
    let Some(window) = web_sys::window() else { return Ok(()) };
    let gesture = Rc::new(RefCell::new(Gesture::default()));
    let options = web_sys::AddEventListenerOptions::new();
    options.set_capture(true);
    options.set_passive(false);
    for (name, landing) in [("touchstart", true), ("touchmove", false)] {
        let gesture = gesture.clone();
        runner.add_event_listener_ex(&window, name, &options, move |event: web_sys::TouchEvent, runner, _| {
            let mut gesture = gesture.borrow_mut();
            for touch in changed(&event) {
                let at = position(runner.canvas(), runner.egui_ctx(), &touch);
                if landing {
                    gesture.down(touch.identifier(), at);
                } else {
                    gesture.moved(touch.identifier(), at);
                }
            }
        })?;
    }
    let releases = gesture.clone();
    runner.add_event_listener_ex(&window, "pointerup", &options, move |event: web_sys::PointerEvent, _, _| {
        if event.pointer_type() == "touch" && releases.borrow().navigating() {
            event.stop_immediate_propagation();
        }
    })?;
    for (name, phase) in [("touchend", egui::TouchPhase::End), ("touchcancel", egui::TouchPhase::Cancel)] {
        let (gesture, endings) = (gesture.clone(), endings.clone());
        runner.add_event_listener_ex(&window, name, &options, move |event: web_sys::TouchEvent, runner, _| {
            let mut gesture = gesture.borrow_mut();
            let touches = changed(&event);
            for touch in &touches {
                gesture.up(touch.identifier());
            }
            if !gesture.navigating() {
                return;
            }
            endings.borrow_mut().extend(touches.iter().map(|touch| egui::Event::Touch {
                device_id: egui::TouchDeviceId(0),
                id: egui::TouchId::from(touch.identifier()),
                phase,
                pos: position(runner.canvas(), runner.egui_ctx(), touch),
                force: Some(touch.force()),
            }));
            event.prevent_default();
            event.stop_immediate_propagation();
            runner.egui_ctx().request_repaint();
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::{WebShell, launch};
    use gridcraft_engine::Session;
    use gridcraft_ui_egui::{Services, SheetApp};
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen_test::wasm_bindgen_test;

    const CANVAS: &str = "touch_focus_canvas";
    /// A phone-sized canvas, in CSS pixels.
    const WIDTH: f64 = 400.0;
    const HEIGHT: f64 = 600.0;
    /// A finger on the grid, below the ribbon and formula bar.
    const ON_GRID: (i32, i32) = (200, 450);
    /// A scroll drag's steps (one frame each) and distance.
    const STEPS: i32 = 6;
    const DRAG: i32 = 200;

    fn document() -> web_sys::Document {
        web_sys::window().unwrap().document().unwrap()
    }

    /// Wait for the browser's next animation frame, when eframe runs a frame of the app.
    async fn frame() {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap().request_animation_frame(&resolve).unwrap();
        });
        wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
    }

    async fn frames(n: usize) {
        for _ in 0..n {
            frame().await;
        }
    }

    /// GridCraft on a phone-sized canvas with the sales sample open, as the page starts it.
    async fn start() -> Page {
        let canvas: web_sys::HtmlCanvasElement = document().create_element("canvas").unwrap().dyn_into().unwrap();
        canvas.set_id(CANVAS);
        canvas.style().set_css_text(&format!("position:fixed;left:0;top:0;width:{WIDTH}px;height:{HEIGHT}px"));
        document().body().unwrap().append_child(&canvas).unwrap();
        let mut options = eframe::WebOptions::default();
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let ctx = Rc::new(RefCell::new(None));
        let app_ctx = ctx.clone();
        let runner = launch(canvas.clone(), options, move |cc, touch| {
            *app_ctx.borrow_mut() = Some(cc.egui_ctx.clone());
            let mut session = Session::new();
            session.execute("file.new", serde_json::json!({"sample": "sales"})).unwrap();
            WebShell::bare(SheetApp::new(session, Services::default()), touch)
        })
        .await
        .unwrap();
        frames(5).await;
        let ctx = ctx.borrow_mut().take().unwrap();
        Page { runner, canvas, ctx }
    }

    /// A running GridCraft, stopped and removed when dropped.
    struct Page {
        runner: eframe::WebRunner,
        canvas: web_sys::HtmlCanvasElement,
        ctx: egui::Context,
    }

    impl Page {
        fn with_app<T>(&self, f: impl FnOnce(&mut SheetApp) -> T) -> T {
            let out = f(self.runner.app_mut::<WebShell>().unwrap().app_mut());
            self.ctx.request_repaint();
            out
        }
    }

    impl Drop for Page {
        fn drop(&mut self) {
            self.runner.destroy();
            self.canvas.remove();
        }
    }

    fn active_tag() -> String {
        document().active_element().map(|e| e.tag_name()).unwrap_or_default()
    }

    /// Start editing the active cell, the way typing does, and wait for eframe to hand the keyboard
    /// focus to its text input.
    async fn edit(page: &Page) {
        page.canvas.focus().unwrap();
        page.with_app(|app| app.begin_edit(Some("12".into()), false));
        for _ in 0..20 {
            if active_tag() == "INPUT" {
                return;
            }
            frame().await;
        }
        panic!("editing never focused the text input (focus is on {:?})", active_tag());
    }

    /// Every focus and blur from now until the recorder is dropped.
    struct FocusLog {
        seen: Rc<RefCell<Vec<String>>>,
        listener: Closure<dyn FnMut(web_sys::Event)>,
    }

    impl FocusLog {
        fn start() -> Self {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let log = seen.clone();
            let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                let tag = event.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).map(|e| e.tag_name()).unwrap_or_default();
                log.borrow_mut().push(format!("{} {tag}", event.type_()));
            });
            let window = web_sys::window().unwrap();
            for name in ["focus", "blur"] {
                window.add_event_listener_with_callback_and_bool(name, listener.as_ref().unchecked_ref(), true).unwrap();
            }
            FocusLog { seen, listener }
        }

        fn events(&self) -> Vec<String> {
            self.seen.borrow().clone()
        }
    }

    impl Drop for FocusLog {
        fn drop(&mut self) {
            let window = web_sys::window().unwrap();
            for name in ["focus", "blur"] {
                window.remove_event_listener_with_callback_and_bool(name, self.listener.as_ref().unchecked_ref(), true).unwrap();
            }
        }
    }

    fn finger(canvas: &web_sys::HtmlCanvasElement, id: i32, (x, y): (i32, i32)) -> web_sys::Touch {
        let init = web_sys::TouchInit::new(id, canvas);
        init.set_client_x(x);
        init.set_client_y(y);
        web_sys::Touch::new(&init).unwrap()
    }

    /// Dispatch what a browser sends for fingers: the pointer event, then the touch event.
    fn send(canvas: &web_sys::HtmlCanvasElement, pointer: &str, touch: &str, down: &[web_sys::Touch], changed: &[web_sys::Touch]) {
        for t in changed {
            let init = web_sys::PointerEventInit::new();
            init.set_bubbles(true);
            init.set_cancelable(true);
            init.set_pointer_id(t.identifier());
            init.set_pointer_type("touch");
            init.set_is_primary(t.identifier() == changed[0].identifier());
            init.set_client_x(t.client_x());
            init.set_client_y(t.client_y());
            init.set_button(if pointer == "pointermove" { -1 } else { 0 });
            init.set_buttons(if pointer == "pointerup" { 0 } else { 1 });
            canvas.dispatch_event(&web_sys::PointerEvent::new_with_event_init_dict(pointer, &init).unwrap()).unwrap();
        }
        let list = |touches: &[web_sys::Touch]| touches.iter().cloned().map(wasm_bindgen::JsValue::from).collect::<js_sys::Array>();
        let init = web_sys::TouchEventInit::new();
        init.set_bubbles(true);
        init.set_cancelable(true);
        init.set_touches(&list(down));
        init.set_target_touches(&list(down));
        init.set_changed_touches(&list(changed));
        canvas.dispatch_event(&web_sys::TouchEvent::new_with_event_init_dict(touch, &init).unwrap()).unwrap();
    }

    /// A finger landing at `at` and lifting there.
    async fn tap(canvas: &web_sys::HtmlCanvasElement, at: (i32, i32)) {
        send(canvas, "pointerdown", "touchstart", &[finger(canvas, 8, at)], &[finger(canvas, 8, at)]);
        frame().await;
        send(canvas, "pointerup", "touchend", &[], &[finger(canvas, 8, at)]);
        frames(3).await;
    }

    /// One finger dragged `DRAG` pixels up from `from`, a frame per step.
    async fn scroll(canvas: &web_sys::HtmlCanvasElement, from: (i32, i32)) {
        let at = |step: i32| finger(canvas, 7, (from.0, from.1 - DRAG * step / STEPS));
        send(canvas, "pointerdown", "touchstart", &[at(0)], &[at(0)]);
        frame().await;
        for step in 1..=STEPS {
            send(canvas, "pointermove", "touchmove", &[at(step)], &[at(step)]);
            frame().await;
        }
        send(canvas, "pointerup", "touchend", &[], &[at(STEPS)]);
        frames(3).await;
    }

    #[wasm_bindgen_test]
    async fn scrolling_while_editing_leaves_the_keyboard_focus_alone() {
        let page = start().await;
        edit(&page).await;
        let before = page.with_app(|app| app.view().scroll);
        let focus = FocusLog::start();
        scroll(&page.canvas, ON_GRID).await;
        let (after, editing) = page.with_app(|app| (app.view().scroll, app.editor.is_some()));
        assert!((after.y - before.y).abs() > 50.0, "the drag scrolled the sheet ({before:?} to {after:?})");
        assert!(editing, "scrolling kept the cell edit open");
        assert_eq!(focus.events(), Vec::<String>::new(), "scrolling must not blur or refocus anything");
        assert_eq!(active_tag(), "INPUT", "the text input keeps the keyboard focus");
    }

    #[wasm_bindgen_test]
    async fn a_tap_still_reaches_eframe_and_selects_its_cell() {
        let page = start().await;
        let before = page.with_app(|app| app.session.active().map(|d| d.selection.active));
        tap(&page.canvas, ON_GRID).await;
        let after = page.with_app(|app| app.session.active().map(|d| d.selection.active));
        assert!(after.is_some() && after != before, "the tap selected the cell under it ({before:?} to {after:?})");
    }

    /// Whether each run of finger events ends up navigating. Positions are in points; ids 1 and 2.
    #[wasm_bindgen_test]
    fn fingers_navigate_once_they_wander_past_a_tap_or_a_second_lands() {
        enum Step {
            Down(i32, f32),
            Move(i32, f32),
            Up(i32),
        }
        use Step::*;
        let cases: &[(&str, &[Step], bool)] = &[
            ("a tap", &[Down(1, 0.0), Up(1)], false),
            ("a wobble within the slop", &[Down(1, 0.0), Move(1, TAP_SLOP / 2.0), Up(1)], false),
            ("exactly the slop", &[Down(1, 0.0), Move(1, TAP_SLOP), Up(1)], false),
            ("past the slop", &[Down(1, 0.0), Move(1, TAP_SLOP + 0.5), Up(1)], true),
            ("past the slop and back", &[Down(1, 0.0), Move(1, 3.0 * TAP_SLOP), Move(1, 0.0), Up(1)], true),
            ("a second finger", &[Down(1, 0.0), Down(2, 50.0), Up(2), Up(1)], true),
            ("a move from a finger never seen", &[Down(1, 0.0), Move(2, 3.0 * TAP_SLOP), Up(1)], false),
            ("a tap after a scroll", &[Down(1, 0.0), Move(1, 3.0 * TAP_SLOP), Up(1), Down(1, 0.0), Up(1)], false),
            ("a landing reported twice", &[Down(1, 0.0), Down(1, 0.0), Up(1)], false),
        ];
        let verdicts: Vec<(&str, bool)> = cases
            .iter()
            .map(|(name, steps, _)| {
                let mut gesture = Gesture::default();
                for step in *steps {
                    match *step {
                        Down(id, x) => gesture.down(id, egui::pos2(x, 0.0)),
                        Move(id, x) => gesture.moved(id, egui::pos2(x, 0.0)),
                        Up(id) => gesture.up(id),
                    }
                }
                (*name, gesture.navigating())
            })
            .collect();
        let expected: Vec<(&str, bool)> = cases.iter().map(|(name, _, navigating)| (*name, *navigating)).collect();
        assert_eq!(verdicts, expected);
    }
}
