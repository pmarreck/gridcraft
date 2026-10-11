//! IndexedDB persistence for the browser workbook library ([`gridcraft_ui_egui::library`]).
//!
//! The library is the source of truth while the page runs; this module loads it once at startup
//! and applies its journal behind it. Each [`Change`] is one read-write transaction that first
//! reads the stored revision and writes only if it is the one the change expects (optimistic
//! concurrency between tabs); otherwise the transaction is aborted and the change reported as a
//! conflict. Transactions commit in the order they were created, and the shell creates them one
//! at a time, so changes land in the order they were made. A change counts as stored only when
//! its transaction completes, not when a request succeeds.
//!
//! Records are `{bytes: Uint8Array, modified: number, revision: number}` keyed by name. A record
//! without a revision (written before revisions existed) reads as revision 0.

use gridcraft_ui_egui::library::{Change, StoredRecord};
use js_sys::{Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbObjectStore, IdbRequest, IdbTransaction, IdbTransactionMode};

/// The database the app uses; tests open their own.
pub const DB_NAME: &str = "gridcraft";
const DB_VERSION: u32 = 1;
const STORE: &str = "workbooks";
/// Tabs announce stored changes to each other on this channel (one name per message).
pub const CHANNEL: &str = "gridcraft-library";

/// What became of one change.
#[derive(Debug, PartialEq)]
pub enum Applied {
    Committed,
    /// Storage held a different revision; nothing was written.
    Conflict,
}

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

/// Opens (creating on first use) the library database `name`.
pub async fn open(name: &str) -> Result<IdbDatabase, String> {
    let factory = web_sys::window().ok_or("no window")?.indexed_db().map_err(|e| describe(&e))?.ok_or("IndexedDB is unavailable")?;
    let request = factory.open_with_u32(name, DB_VERSION).map_err(|e| describe(&e))?;
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

fn decode(name: String, value: &JsValue) -> Option<StoredRecord> {
    let bytes = Reflect::get(value, &"bytes".into()).ok()?.dyn_into::<Uint8Array>().ok()?.to_vec();
    let modified_ms = Reflect::get(value, &"modified".into()).ok().and_then(|m| m.as_f64()).unwrap_or(0.0);
    let revision = Reflect::get(value, &"revision".into()).ok().and_then(|r| r.as_f64()).map_or(0, |r| r as u64);
    Some(StoredRecord { name, bytes, modified_ms, revision })
}

fn encode(bytes: &[u8], modified_ms: f64, revision: u64) -> Result<JsValue, String> {
    let value = Object::new();
    Reflect::set(&value, &"bytes".into(), &Uint8Array::from(bytes)).map_err(|e| describe(&e))?;
    Reflect::set(&value, &"modified".into(), &JsValue::from_f64(modified_ms)).map_err(|e| describe(&e))?;
    Reflect::set(&value, &"revision".into(), &JsValue::from_f64(revision as f64)).map_err(|e| describe(&e))?;
    Ok(value.into())
}

/// Every stored workbook. Records that don't have the expected shape are skipped.
pub async fn load_all(db: &IdbDatabase) -> Result<Vec<StoredRecord>, String> {
    let tx = db.transaction_with_str(STORE).map_err(|e| describe(&e))?;
    let store = tx.object_store(STORE).map_err(|e| describe(&e))?;
    // Both lists come back in key order, so they pair up.
    let keys = settled(&store.get_all_keys().map_err(|e| describe(&e))?).await.map_err(|e| describe(&e))?;
    let values = settled(&store.get_all().map_err(|e| describe(&e))?).await.map_err(|e| describe(&e))?;
    let (keys, values): (js_sys::Array, js_sys::Array) = (keys.unchecked_into(), values.unchecked_into());
    Ok(keys.iter().zip(values.iter()).filter_map(|(key, value)| decode(key.as_string()?, &value)).collect())
}

/// What storage holds under `name` now (`None`: nothing).
pub async fn read(db: &IdbDatabase, name: &str) -> Result<Option<StoredRecord>, String> {
    let tx = db.transaction_with_str(STORE).map_err(|e| describe(&e))?;
    let store = tx.object_store(STORE).map_err(|e| describe(&e))?;
    let value = settled(&store.get(&JsValue::from_str(name)).map_err(|e| describe(&e))?).await.map_err(|e| describe(&e))?;
    Ok(if value.is_undefined() { None } else { decode(name.to_string(), &value) })
}

/// The stored revision of `name` inside `store`'s transaction (`None`: not stored).
async fn revision(store: &IdbObjectStore, name: &str) -> Result<Option<u64>, String> {
    let value = settled(&store.get(&JsValue::from_str(name)).map_err(|e| describe(&e))?).await.map_err(|e| describe(&e))?;
    Ok(if value.is_undefined() { None } else { Some(decode(name.to_string(), &value).map_or(0, |r| r.revision)) })
}

/// Applies one change in one transaction, writing only if storage holds the revisions the change
/// expects, and waits until the transaction commits.
pub async fn apply(db: &IdbDatabase, change: &Change) -> Result<Applied, String> {
    let tx = db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite).map_err(|e| describe(&e))?;
    let store = tx.object_store(STORE).map_err(|e| describe(&e))?;
    let done = committed(&tx);
    let expected = match change {
        Change::Put { name, expect, .. } => revision(&store, name).await? == *expect,
        Change::Delete { name, expect, .. } => revision(&store, name).await? == Some(*expect),
        Change::Rename { from, to, expect, .. } => revision(&store, from).await? == Some(*expect) && revision(&store, to).await?.is_none(),
    };
    if !expected {
        tx.abort().ok();
        // An aborted transaction rejects `done`; here that is the conflict, not a failure.
        let _ = done.await;
        return Ok(Applied::Conflict);
    }
    let queued = match change {
        Change::Put { name, bytes, modified_ms, expect, .. } => {
            store.put_with_key(&encode(bytes, *modified_ms, expect.map_or(1, |r| r + 1))?, &JsValue::from_str(name)).map(|_| ())
        }
        Change::Delete { name, .. } => store.delete(&JsValue::from_str(name)).map(|_| ()),
        Change::Rename { from, to, bytes, modified_ms, .. } => store
            .put_with_key(&encode(bytes, *modified_ms, 1)?, &JsValue::from_str(to))
            .and_then(|_| store.delete(&JsValue::from_str(from)))
            .map(|_| ()),
    };
    if let Err(e) = queued {
        tx.abort().ok();
        let _ = done.await;
        return Err(describe(&e));
    }
    done.await.map(|_| Applied::Committed).map_err(|e| describe(&e))
}

#[cfg(test)]
mod tests {
    //! These run in a real browser: `wasm-bindgen-test-runner` with headless Chromium (the
    //! flake's `checks.web-storage`). Each test uses its own database.

    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    async fn fresh(test: &str) -> IdbDatabase {
        let name = format!("gridcraft-test-{test}");
        if let Some(factory) = web_sys::window().and_then(|w| w.indexed_db().ok().flatten())
            && let Ok(request) = factory.delete_database(&name)
        {
            let _ = settled(&request).await;
        }
        open(&name).await.unwrap()
    }

    fn put(ticket: u64, name: &str, bytes: &[u8], expect: Option<u64>) -> Change {
        Change::Put { ticket, name: name.into(), bytes: bytes.to_vec(), modified_ms: ticket as f64, expect }
    }

    async fn stored(db: &IdbDatabase, name: &str) -> Option<(Vec<u8>, u64)> {
        read(db, name).await.unwrap().map(|r| (r.bytes, r.revision))
    }

    #[wasm_bindgen_test]
    async fn writes_store_bytes_with_increasing_revisions() {
        let db = fresh("revisions").await;
        assert_eq!(apply(&db, &put(1, "A.xlsx", b"one", None)).await, Ok(Applied::Committed));
        assert_eq!(apply(&db, &put(2, "A.xlsx", b"two", Some(1))).await, Ok(Applied::Committed));
        assert_eq!(stored(&db, "A.xlsx").await, Some((b"two".to_vec(), 2)));
        let all = load_all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].name.as_str(), all[0].modified_ms), ("A.xlsx", 2.0));
    }

    #[wasm_bindgen_test]
    async fn a_write_expecting_an_older_revision_is_refused_and_changes_nothing() {
        let db = fresh("stale").await;
        apply(&db, &put(1, "A.xlsx", b"one", None)).await.unwrap();
        apply(&db, &put(2, "A.xlsx", b"two", Some(1))).await.unwrap();
        assert_eq!(apply(&db, &put(3, "A.xlsx", b"stale", Some(1))).await, Ok(Applied::Conflict));
        assert_eq!(apply(&db, &put(4, "A.xlsx", b"new name?", None)).await, Ok(Applied::Conflict), "a name already stored is not new");
        assert_eq!(stored(&db, "A.xlsx").await, Some((b"two".to_vec(), 2)));
    }

    #[wasm_bindgen_test]
    async fn two_connections_behave_like_two_tabs() {
        let one = fresh("tabs").await;
        let two = open("gridcraft-test-tabs").await.unwrap();
        apply(&one, &put(1, "A.xlsx", b"base", None)).await.unwrap();
        assert_eq!(apply(&two, &put(1, "A.xlsx", b"tab two", Some(1))).await, Ok(Applied::Committed));
        assert_eq!(apply(&one, &put(2, "A.xlsx", b"tab one, unaware", Some(1))).await, Ok(Applied::Conflict));
        assert_eq!(stored(&one, "A.xlsx").await, Some((b"tab two".to_vec(), 2)));
    }

    #[wasm_bindgen_test]
    async fn delete_and_rename_check_revisions_and_never_overwrite() {
        let db = fresh("rename").await;
        apply(&db, &put(1, "A.xlsx", b"a", None)).await.unwrap();
        apply(&db, &put(2, "B.xlsx", b"b", None)).await.unwrap();
        let rename =
            |to: &str, expect| Change::Rename { ticket: 3, from: "A.xlsx".into(), to: to.into(), bytes: b"a".to_vec(), modified_ms: 3.0, expect };
        assert_eq!(apply(&db, &rename("B.xlsx", 1)).await, Ok(Applied::Conflict), "renaming onto a stored workbook");
        assert_eq!(apply(&db, &rename("C.xlsx", 9)).await, Ok(Applied::Conflict), "renaming a revision that moved on");
        assert_eq!(stored(&db, "B.xlsx").await, Some((b"b".to_vec(), 1)));
        assert_eq!(apply(&db, &rename("C.xlsx", 1)).await, Ok(Applied::Committed));
        assert_eq!(stored(&db, "A.xlsx").await, None);
        assert_eq!(stored(&db, "C.xlsx").await, Some((b"a".to_vec(), 1)));
        assert_eq!(apply(&db, &Change::Delete { ticket: 4, name: "C.xlsx".into(), expect: 7 }).await, Ok(Applied::Conflict));
        assert_eq!(apply(&db, &Change::Delete { ticket: 5, name: "C.xlsx".into(), expect: 1 }).await, Ok(Applied::Committed));
        assert_eq!(stored(&db, "C.xlsx").await, None);
    }

    #[wasm_bindgen_test]
    async fn stored_workbooks_survive_closing_and_reopening_the_database() {
        let db = fresh("reopen").await;
        apply(&db, &put(1, "Kept.xlsx", b"kept", None)).await.unwrap();
        db.close();
        let again = open("gridcraft-test-reopen").await.unwrap();
        assert_eq!(stored(&again, "Kept.xlsx").await, Some((b"kept".to_vec(), 1)));
    }
}
