//! IndexedDB persistence for the browser workbook library ([`gridcraft_ui_egui::library`]).
//!
//! The library is the source of truth while the page runs; this module loads it once at startup
//! and writes its journal behind it. Each batch of changes is one read-write transaction, and
//! IndexedDB runs overlapping read-write transactions in the order they were created, so changes
//! land in the order they were made.

use gridcraft_ui_egui::library::Change;
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};

const DB_NAME: &str = "gridcraft";
const DB_VERSION: u32 = 1;
const STORE: &str = "workbooks";

fn describe(e: &JsValue) -> String {
    Reflect::get(e, &"message".into()).ok().and_then(|m| m.as_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| format!("{e:?}"))
}

/// Resolves with the request's result once it succeeds, or rejects with its error.
fn settled(request: &IdbRequest) -> JsFuture {
    let request = request.clone();
    let promise = Promise::new(&mut |resolve: Function, reject: Function| {
        let done = request.clone();
        let failed = request.clone();
        let on_success = Closure::once_into_js(move || {
            let _ = resolve.call1(&JsValue::NULL, &done.result().unwrap_or(JsValue::UNDEFINED));
        });
        let on_error = Closure::once_into_js(move || {
            let error = failed.error().ok().flatten().map_or(JsValue::from_str("request failed"), JsValue::from);
            let _ = reject.call1(&JsValue::NULL, &error);
        });
        request.set_onsuccess(Some(on_success.unchecked_ref()));
        request.set_onerror(Some(on_error.unchecked_ref()));
    });
    JsFuture::from(promise)
}

/// Resolves when the transaction commits; rejects if it fails or aborts (for example, when the
/// browser's storage quota is exceeded).
fn committed(tx: &IdbTransaction) -> JsFuture {
    let tx = tx.clone();
    let promise = Promise::new(&mut |resolve: Function, reject: Function| {
        let failed = tx.clone();
        let on_complete = Closure::once_into_js(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let reject_with_error = move || {
            let error = failed.error().map_or(JsValue::from_str("transaction aborted"), JsValue::from);
            let _ = reject.call1(&JsValue::NULL, &error);
        };
        let reject_on_abort = reject_with_error.clone();
        tx.set_oncomplete(Some(on_complete.unchecked_ref()));
        tx.set_onerror(Some(Closure::once_into_js(reject_with_error).unchecked_ref()));
        tx.set_onabort(Some(Closure::once_into_js(reject_on_abort).unchecked_ref()));
    });
    JsFuture::from(promise)
}

/// Opens (creating on first use) the database that holds the library.
pub async fn open() -> Result<IdbDatabase, String> {
    let factory = web_sys::window().ok_or("no window")?.indexed_db().map_err(|e| describe(&e))?.ok_or("IndexedDB is unavailable")?;
    let request = factory.open_with_u32(DB_NAME, DB_VERSION).map_err(|e| describe(&e))?;
    let upgrading = request.clone();
    let on_upgrade = Closure::once_into_js(move || {
        if let Ok(db) = upgrading.result().and_then(|r| r.dyn_into::<IdbDatabase>())
            && !db.object_store_names().contains(STORE)
            && let Err(e) = db.create_object_store(STORE)
        {
            log::error!("couldn't create the workbook store: {}", describe(&e));
        }
    });
    request.set_onupgradeneeded(Some(on_upgrade.unchecked_ref()));
    let db = settled(&request).await.map_err(|e| describe(&e))?;
    db.dyn_into::<IdbDatabase>().map_err(|_| "IndexedDB returned something other than a database".to_string())
}

/// Every stored workbook as (name, bytes, modified time in ms). Records that don't have the
/// expected shape are skipped.
pub async fn load_all(db: &IdbDatabase) -> Result<Vec<(String, Vec<u8>, f64)>, String> {
    let tx = db.transaction_with_str(STORE).map_err(|e| describe(&e))?;
    let store = tx.object_store(STORE).map_err(|e| describe(&e))?;
    // Both lists come back in key order, so they pair up.
    let keys = settled(&store.get_all_keys().map_err(|e| describe(&e))?).await.map_err(|e| describe(&e))?;
    let values = settled(&store.get_all().map_err(|e| describe(&e))?).await.map_err(|e| describe(&e))?;
    let (keys, values): (Array, Array) = (keys.unchecked_into(), values.unchecked_into());
    let mut out = Vec::new();
    for (key, value) in keys.iter().zip(values.iter()) {
        let Some(name) = key.as_string() else { continue };
        let Some(bytes) = Reflect::get(&value, &"bytes".into()).ok().and_then(|b| b.dyn_into::<Uint8Array>().ok()) else { continue };
        let modified = Reflect::get(&value, &"modified".into()).ok().and_then(|m| m.as_f64()).unwrap_or(0.0);
        out.push((name, bytes.to_vec(), modified));
    }
    Ok(out)
}

fn record(bytes: &[u8], modified_ms: f64) -> Result<Object, String> {
    let value = Object::new();
    Reflect::set(&value, &"bytes".into(), &Uint8Array::from(bytes)).map_err(|e| describe(&e))?;
    Reflect::set(&value, &"modified".into(), &JsValue::from_f64(modified_ms)).map_err(|e| describe(&e))?;
    Ok(value)
}

/// Writes `changes` in one transaction and waits for it to commit.
pub async fn persist(db: &IdbDatabase, changes: Vec<Change>) -> Result<(), String> {
    let tx = db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite).map_err(|e| describe(&e))?;
    let store = tx.object_store(STORE).map_err(|e| describe(&e))?;
    for change in &changes {
        let queued = match change {
            Change::Put { name, bytes, modified_ms } => store.put_with_key(&record(bytes, *modified_ms)?.into(), &JsValue::from_str(name)),
            Change::Delete { name } => store.delete(&JsValue::from_str(name)),
        };
        if let Err(e) = queued {
            tx.abort().ok();
            return Err(describe(&e));
        }
    }
    committed(&tx).await.map(|_| ()).map_err(|e| describe(&e))
}
