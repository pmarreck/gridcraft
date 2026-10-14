//! The browser file picker: a hidden `<input type=file>` clicked while the browser is still
//! handling the tap or click that asked for it. Safari opens a picker only then (eframe runs that
//! frame inside the browser's `pointerup` handler), so it must not wait for an async task the way
//! `rfd` does.

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_ui_egui::Inbox;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

/// The largest file the picker reads: 2 GiB, Excel's own limit for a workbook file.
pub(crate) const MAX_FILE_BYTES: f64 = 2_147_483_648.0;

/// Messages for the person about files the picker couldn't deliver, shown on the next frame.
pub(crate) type Failures = Rc<RefCell<Vec<String>>>;

thread_local! {
    /// The picker waiting for an answer. Holding it here, instead of `forget`ting its handlers,
    /// lets the page drop the input and both handlers once either fires, or when a new pick
    /// replaces a picker the browser never answered (older browsers send no `cancel`).
    static OPEN: RefCell<Option<Picker>> = const { RefCell::new(None) };
}

struct Picker {
    input: web_sys::HtmlInputElement,
    _on_change: Closure<dyn FnMut()>,
    on_cancel: Closure<dyn FnMut()>,
}

impl Picker {
    /// Take the input out of the page and unhook its handlers, so nothing can call them again.
    fn detach(&self) {
        self.input.set_onchange(None);
        let _ = self.input.remove_event_listener_with_callback("cancel", self.on_cancel.as_ref().unchecked_ref());
        self.input.remove();
    }
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.detach();
    }
}

/// Show the browser's file picker and deliver the chosen file to `inbox`; a file larger than
/// `limit` bytes, or one that can't be read, becomes a message in `failures` instead.
pub(crate) fn pick(accept: &str, limit: f64, inbox: Inbox, failures: Failures, ctx: egui::Context) {
    // Not running inside the old picker's handlers, so it can go right away.
    drop(OPEN.with(|open| open.borrow_mut().take()));
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let Some(body) = document.body() else { return };
    let Some(input) = document.create_element("input").ok().and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok()) else {
        return;
    };
    input.set_type("file");
    input.set_accept(accept);
    let _ = input.style().set_property("display", "none");
    // Safari delivers `change` only to an input in the document.
    if body.append_child(&input).is_err() {
        return;
    }
    let on_change = {
        let input = input.clone();
        Closure::<dyn FnMut()>::new(move || {
            let file = input.files().and_then(|files| files.get(0));
            release();
            let Some(file) = file else { return };
            let (inbox, failures, ctx) = (inbox.clone(), failures.clone(), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                match read(&file, limit).await {
                    Ok(bytes) => inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((file.name(), bytes)),
                    Err(message) => failures.borrow_mut().push(message),
                }
                ctx.request_repaint();
            });
        })
    };
    let on_cancel = Closure::<dyn FnMut()>::new(release);
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    let _ = input.add_event_listener_with_callback("cancel", on_cancel.as_ref().unchecked_ref());
    input.click();
    OPEN.with(|open| *open.borrow_mut() = Some(Picker { input, _on_change: on_change, on_cancel }));
}

/// Detach the open picker now and drop it once the handler that called this has returned: a
/// closure must not be dropped while it runs.
fn release() {
    let picker = OPEN.with(|open| open.borrow_mut().take());
    if let Some(picker) = &picker {
        picker.detach();
    }
    wasm_bindgen_futures::spawn_local(async move { drop(picker) });
}

/// Whether a picker is waiting for the person to choose or cancel.
#[cfg(test)]
pub(crate) fn is_open() -> bool {
    OPEN.with(|open| open.borrow().is_some())
}

/// Read `file` into memory unless it's larger than `limit` bytes. The copy reserves its memory
/// fallibly, so a file the page can't hold is a message rather than an aborted page.
async fn read(file: &web_sys::File, limit: f64) -> Result<Vec<u8>, String> {
    let name = file.name();
    let bytes = file.size();
    if bytes > limit {
        return Err(format!("“{name}” is {}. GridCraft in the browser opens files up to {}.", size(bytes), size(limit)));
    }
    let buffer = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await.map_err(|e| {
        let reason = e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())).unwrap_or_else(|| format!("{e:?}"));
        format!("GridCraft couldn't read “{name}”: {reason}")
    })?;
    let array = js_sys::Uint8Array::new(&buffer);
    let too_big = || format!("There isn't enough memory to open “{name}” ({}).", size(bytes));
    let len = usize::try_from(array.length()).map_err(|_| too_big())?;
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| too_big())?;
    out.resize(len, 0);
    array.copy_to(&mut out);
    Ok(out)
}

/// `bytes` in binary units, the way file managers show sizes.
fn size(bytes: f64) -> String {
    if bytes < 1024.0 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes / 1024.0;
    let mut unit = "KB";
    for next in ["MB", "GB", "TB"] {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    let tenths = (value * 10.0).round() / 10.0;
    if tenths.fract() == 0.0 { format!("{tenths:.0} {unit}") } else { format!("{tenths:.1} {unit}") }
}

#[cfg(test)]
mod tests {
    //! These run in headless Chromium with the storage tests (the flake's `checks.web-storage`).
    //! A test chooses a file by handing the input a `DataTransfer`'s file list, as a drop would.

    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    fn document() -> web_sys::Document {
        web_sys::window().and_then(|w| w.document()).unwrap()
    }

    fn file_inputs() -> u32 {
        document().query_selector_all("input[type=file]").unwrap().length()
    }

    fn file_input() -> web_sys::HtmlInputElement {
        document().query_selector("input[type=file]").unwrap().unwrap().dyn_into().unwrap()
    }

    /// Yield to the browser once: pending events, promises and spawned futures run.
    async fn yield_once() {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap().set_timeout_with_callback(&resolve).unwrap();
        });
        wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
    }

    /// Yield until `done` holds; a bounded number of turns, so a broken picker fails the test
    /// instead of hanging it.
    async fn settle(done: impl Fn() -> bool) -> bool {
        for _ in 0..100 {
            if done() {
                return true;
            }
            yield_once().await;
        }
        done()
    }

    fn choose(name: &str, bytes: &[u8]) {
        let input = file_input();
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
        let file = web_sys::File::new_with_u8_array_sequence(&parts, name).unwrap();
        let transfer = web_sys::DataTransfer::new().unwrap();
        transfer.items().add_with_file(&file).unwrap();
        input.set_files(Some(&transfer.files().unwrap()));
        input.dispatch_event(&web_sys::Event::new("change").unwrap()).unwrap();
    }

    fn open(limit: f64) -> (Inbox, Failures) {
        let (inbox, failures) = (Inbox::default(), Failures::default());
        pick(".xlsx", limit, inbox.clone(), failures.clone(), egui::Context::default());
        (inbox, failures)
    }

    #[wasm_bindgen_test]
    async fn a_cancelled_pick_leaves_nothing_behind() {
        open(MAX_FILE_BYTES);
        assert!(is_open());
        assert_eq!(file_inputs(), 1);
        file_input().dispatch_event(&web_sys::Event::new("cancel").unwrap()).unwrap();
        assert!(settle(|| !is_open()).await, "the picker was never released");
        assert_eq!(file_inputs(), 0);
    }

    #[wasm_bindgen_test]
    async fn a_chosen_file_arrives_and_the_picker_is_released() {
        let (inbox, failures) = open(MAX_FILE_BYTES);
        choose("Book.xlsx", b"PK\x03\x04");
        assert!(settle(|| !inbox.lock().unwrap().is_empty()).await, "the file never arrived");
        assert_eq!(*inbox.lock().unwrap(), vec![("Book.xlsx".to_string(), b"PK\x03\x04".to_vec())]);
        assert!(failures.borrow().is_empty());
        assert!(settle(|| !is_open()).await, "the picker was never released");
        assert_eq!(file_inputs(), 0);
    }

    #[wasm_bindgen_test]
    async fn a_file_over_the_limit_is_refused_with_a_message() {
        let (inbox, failures) = open(3.0);
        choose("Huge.xlsx", b"four");
        assert!(settle(|| !failures.borrow().is_empty()).await, "no message about the file");
        let message = failures.borrow()[0].clone();
        assert!(message.contains("Huge.xlsx"), "{message}");
        assert!(inbox.lock().unwrap().is_empty());
        assert!(settle(|| !is_open()).await, "the picker was never released");
    }

    #[wasm_bindgen_test]
    async fn a_file_at_the_limit_is_read() {
        let (inbox, failures) = open(4.0);
        choose("Exact.xlsx", b"four");
        assert!(settle(|| !inbox.lock().unwrap().is_empty()).await, "the file never arrived");
        assert!(failures.borrow().is_empty());
        assert!(settle(|| !is_open()).await);
    }

    #[wasm_bindgen_test]
    async fn a_new_pick_replaces_one_the_browser_never_answered() {
        open(MAX_FILE_BYTES);
        open(MAX_FILE_BYTES);
        assert_eq!(file_inputs(), 1, "the abandoned input is still in the page");
        file_input().dispatch_event(&web_sys::Event::new("cancel").unwrap()).unwrap();
        assert!(settle(|| !is_open()).await);
        assert_eq!(file_inputs(), 0);
    }

    #[wasm_bindgen_test]
    fn sizes_read_in_familiar_units() {
        assert_eq!(size(512.0), "512 bytes");
        assert_eq!(size(1536.0), "1.5 KB");
        assert_eq!(size(5_242_880.0), "5 MB");
        assert_eq!(size(MAX_FILE_BYTES), "2 GB");
    }
}
