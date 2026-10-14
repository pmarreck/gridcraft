//! The browser build keeps workbooks in a library inside the browser: Save offers it beside a
//! download, Ctrl+S and AutoSave write to it, and File › Open lists its workbooks to open, rename
//! or delete.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::io::FileStore as _;
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::library::{Change, Library, Outcome, StoredRecord};
use gridcraft_ui_egui::{Services, SheetApp};

type Downloads = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

fn library() -> Rc<Library> {
    let now = std::cell::Cell::new(0.0);
    Rc::new(Library::new(move || {
        now.set(now.get() + 1.0);
        now.get()
    }))
}

fn harness(lib: &Rc<Library>) -> (egui_kittest::Harness<'static, SheetApp>, Downloads) {
    let downloads = Downloads::default();
    let capture = downloads.clone();
    let mut session = Session::new();
    session.new_workbook();
    let services = Services {
        download: Some(Box::new(move |name, bytes| capture.lock().unwrap().push((name.to_string(), bytes.to_vec())))),
        library: Some(lib.clone()),
        ..Default::default()
    };
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    (h, downloads)
}

fn choose(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    if h.state().backstage.is_none() {
        h.get_by_label("File").click();
        h.run_steps(3);
    }
    click(h, label);
}

/// Click the button named `label` drawn last: a dialog's Save over the title bar's.
fn click(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    use egui_kittest::kittest::Queryable as _;
    let button = h.query_all_by_role_and_label(egui::accesskit::Role::Button, label).last();
    button.unwrap_or_else(|| panic!("no {label} button")).click();
    h.run_steps(3);
}

fn page(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<&'static str> {
    h.state().backstage.as_ref().map(|b| b.page.id())
}

/// The browser stores every pending change.
fn persist(h: &mut egui_kittest::Harness<'static, SheetApp>, lib: &Library) {
    for change in lib.take_changes() {
        h.state_mut().library_settled(&change, Outcome::Committed);
    }
    h.run_steps(2);
}

/// An xlsx workbook whose A1 holds `n`.
fn xlsx(n: f64) -> Vec<u8> {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", serde_json::json!({"cell": "A1", "input": n.to_string()})).unwrap();
    let r = s.execute("file.saveBytes", serde_json::json!({"format": "xlsx"})).unwrap();
    gridcraft_engine::io::base64_decode(r["base64"].as_str().unwrap()).unwrap()
}

fn active_a1(h: &egui_kittest::Harness<'static, SheetApp>) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0))
}

/// A library holding `Shared.xlsx` (A1 = 1) at revision 1, opened in a fresh harness.
fn shared() -> (Rc<Library>, egui_kittest::Harness<'static, SheetApp>) {
    let lib = library();
    lib.restore(vec![StoredRecord { name: "Shared.xlsx".into(), bytes: xlsx(1.0), modified_ms: 1.0, revision: 1 }]);
    let (mut h, _) = harness(&lib);
    h.state_mut().open_path("Shared.xlsx");
    h.run_steps(2);
    (lib, h)
}

fn theirs(n: f64, revision: u64) -> Option<StoredRecord> {
    Some(StoredRecord { name: "Shared.xlsx".into(), bytes: xlsx(n), modified_ms: 2.0, revision })
}

fn dialog<'a>(h: &'a egui_kittest::Harness<'static, SheetApp>) -> Option<&'a str> {
    h.state().dialog.as_ref().map(|d| d.name())
}

fn a1(bytes: &[u8]) -> Value {
    let (wb, _) = gridcraft_engine::io::open_bytes("x.xlsx", bytes).unwrap();
    wb.active().unwrap().value(CellRef::new(0, 0))
}

fn type_into_focus(h: &mut egui_kittest::Harness<'static, SheetApp>, text: &str) {
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::Text(text.into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}

#[test]
fn saving_a_new_workbook_offers_the_browser_or_a_download() {
    let lib = library();
    let (mut h, downloads) = harness(&lib);
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "7"})).unwrap();
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(page(&h), Some("saveAs"), "Save on a new workbook asks where, instead of downloading at once");
    assert!(downloads.lock().unwrap().is_empty());
    click(&mut h, "Save in this browser");
    assert!(h.state().backstage.is_none(), "saving returns to the sheet");
    assert_eq!(a1(&lib.read("Book1.xlsx").unwrap()), Value::Number(7.0));
    let d = h.state().session.active().unwrap();
    assert_eq!(d.path.as_deref(), Some("Book1.xlsx"));
    assert!(d.is_dirty(), "unsaved until the browser confirms the write");
    persist(&mut h, &lib);
    assert!(!h.state().session.active().unwrap().is_dirty(), "confirmed: saved");
    // From now on Ctrl+S and AutoSave write to the browser.
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "8"})).unwrap();
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    assert_eq!(a1(&lib.read("Book1.xlsx").unwrap()), Value::Number(8.0));
    persist(&mut h, &lib);
    h.state_mut().grid.autosave = true;
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "9"})).unwrap();
    h.run_steps(2);
    assert_eq!(a1(&lib.read("Book1.xlsx").unwrap()), Value::Number(9.0), "AutoSave stores the change");
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "10"})).unwrap();
    h.state_mut().grid.last_autosave = 0.0;
    h.run_steps(2);
    assert_eq!(a1(&lib.read("Book1.xlsx").unwrap()), Value::Number(9.0), "AutoSave waits while a write is still pending");
    persist(&mut h, &lib);
    h.state_mut().grid.last_autosave = 0.0;
    h.run_steps(2);
    assert_eq!(a1(&lib.read("Book1.xlsx").unwrap()), Value::Number(10.0), "and saves the newer edit once it lands");
    // The download is still on offer from Save As.
    choose(&mut h, "Save As");
    click(&mut h, "Download (.xlsx)");
    assert_eq!(downloads.lock().unwrap().len(), 1);
}

#[test]
fn save_as_takes_a_name_and_asks_before_replacing_another_workbook() {
    let lib = library();
    lib.write("Taken.xlsx", b"not a workbook").unwrap();
    let (mut h, _) = harness(&lib);
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "1"})).unwrap();
    choose(&mut h, "Save As");
    h.get_by_label("Workbook name").click();
    h.run_steps(2);
    type_into_focus(&mut h, "Quarterly");
    assert_eq!(a1(&lib.read("Quarterly.xlsx").unwrap()), Value::Number(1.0), "Enter saves, and .xlsx is added");
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some("Quarterly.xlsx"));
    choose(&mut h, "Save As");
    h.get_by_label("Workbook name").click();
    h.run_steps(2);
    type_into_focus(&mut h, "Taken.xlsx");
    assert_eq!(lib.read("Taken.xlsx").unwrap(), b"not a workbook", "another workbook is never replaced without asking");
    click(&mut h, "Replace “Taken.xlsx”");
    assert_eq!(a1(&lib.read("Taken.xlsx").unwrap()), Value::Number(1.0));
}

#[test]
fn file_open_lists_the_library_to_open_rename_and_delete() {
    let lib = library();
    for (name, n) in [("First.xlsx", "1"), ("Second.xlsx", "2")] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", serde_json::json!({"cell": "A1", "input": n})).unwrap();
        let r = s.execute("file.saveBytes", serde_json::json!({"format": "xlsx"})).unwrap();
        lib.write(name, &gridcraft_engine::io::base64_decode(r["base64"].as_str().unwrap()).unwrap()).unwrap();
    }
    let (mut h, _) = harness(&lib);
    choose(&mut h, "Open");
    assert!(h.query_by_label("Import from this computer").is_some(), "files from the computer are imported from File › Open");
    click(&mut h, "First.xlsx");
    assert!(h.state().backstage.is_none());
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some("First.xlsx"));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(1.0));

    choose(&mut h, "Open");
    assert!(h.query_by_label("Rename First.xlsx").is_none(), "an open workbook can't be renamed or deleted");
    assert!(h.query_by_label("Delete First.xlsx").is_none());
    click(&mut h, "Rename Second.xlsx");
    type_into_focus(&mut h, "Renamed.xlsx");
    assert!(lib.read("Second.xlsx").is_err());
    assert_eq!(a1(&lib.read("Renamed.xlsx").unwrap()), Value::Number(2.0));

    click(&mut h, "Delete Renamed.xlsx");
    assert!(lib.contains("Renamed.xlsx"), "Delete asks first");
    click(&mut h, "Delete “Renamed.xlsx” permanently");
    assert!(!lib.contains("Renamed.xlsx"));
    assert_eq!(page(&h), Some("open"));
}

#[test]
fn a_write_the_browser_refuses_keeps_the_workbook_unsaved_and_says_why() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    let changes = lib.take_changes();
    assert_eq!(changes.len(), 1);
    h.state_mut().library_settled(&changes[0], Outcome::Failed("QuotaExceededError".into(), vec![("Shared.xlsx".into(), theirs(1.0, 1))]));
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().is_dirty(), "a refused write leaves the workbook unsaved");
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("QuotaExceededError")));
    assert!(!lib.is_stale("Shared.xlsx"), "a failure is not another tab's change");
    assert_eq!(dialog(&h), None);
}

#[test]
fn a_newer_version_from_another_tab_reloads_a_workbook_without_unsaved_changes() {
    let (lib, mut h) = shared();
    lib.changed_elsewhere("Shared.xlsx", theirs(5.0, 2));
    h.run_steps(2);
    assert_eq!(active_a1(&h), Value::Number(5.0), "the newer version replaces the unchanged one");
    assert!(!h.state().session.active().unwrap().is_dirty());
    assert!(!lib.is_stale("Shared.xlsx"));
    assert!(h.state().toast.as_ref().is_some_and(|(t, _)| t.contains("another tab")), "the reload is announced");
    assert_eq!(dialog(&h), None);
}

#[test]
fn with_unsaved_changes_keep_mine_replaces_the_newer_version() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    lib.changed_elsewhere("Shared.xlsx", theirs(5.0, 2));
    h.run_steps(2);
    assert_eq!(active_a1(&h), Value::Number(2.0), "unsaved work is never replaced without asking");
    assert_eq!(dialog(&h), Some("libraryConflict"));
    assert!(h.state_mut().run("file.save", serde_json::json!({})).is_err(), "saving over the newer version is refused until the user decides");
    click(&mut h, "Keep Mine");
    assert_eq!(dialog(&h), None);
    let changes = lib.take_changes();
    assert!(matches!(changes.as_slice(), [Change::Put { expect: Some(2), .. }]), "mine replaces revision 2: {changes:?}");
    assert!(h.state().session.active().unwrap().is_dirty());
    h.state_mut().library_settled(&changes[0], Outcome::Committed);
    assert!(!h.state().session.active().unwrap().is_dirty());
}

#[test]
fn with_unsaved_changes_use_theirs_discards_mine() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    lib.changed_elsewhere("Shared.xlsx", theirs(5.0, 2));
    h.run_steps(2);
    click(&mut h, "Use Theirs");
    assert_eq!(dialog(&h), None);
    assert_eq!(active_a1(&h), Value::Number(5.0));
    assert!(!h.state().session.active().unwrap().is_dirty());
    assert!(lib.take_changes().is_empty());
}

#[test]
fn with_unsaved_changes_save_a_copy_keeps_both() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    lib.changed_elsewhere("Shared.xlsx", theirs(5.0, 2));
    h.run_steps(2);
    click(&mut h, "Save a Copy…");
    assert_eq!(dialog(&h), None);
    assert_eq!(page(&h), Some("saveAs"));
    h.get_by_label("Workbook name").click();
    h.run_steps(2);
    type_into_focus(&mut h, "Mine");
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some("Mine.xlsx"));
    assert_eq!(a1(&lib.read("Mine.xlsx").unwrap()), Value::Number(2.0));
    assert_eq!(a1(&lib.read("Shared.xlsx").unwrap()), Value::Number(5.0), "the other tab's version is untouched");
    h.run_steps(3);
    assert_eq!(dialog(&h), None, "the copy is a different workbook: no further prompt");
}

#[test]
fn cancel_leaves_the_conflict_for_the_next_save_without_asking_again() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    lib.changed_elsewhere("Shared.xlsx", theirs(5.0, 2));
    h.run_steps(2);
    click(&mut h, "Cancel");
    h.run_steps(3);
    assert_eq!(dialog(&h), None, "dismissed: not asked again every frame");
    assert!(lib.is_stale("Shared.xlsx"));
    assert_eq!(active_a1(&h), Value::Number(2.0));
}

#[test]
fn a_save_storage_refuses_because_another_tab_wrote_first_asks_too() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    let changes = lib.take_changes();
    h.state_mut().library_settled(&changes[0], Outcome::Conflict(vec![("Shared.xlsx".into(), theirs(5.0, 2))]));
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().is_dirty());
    assert_eq!(dialog(&h), Some("libraryConflict"));
    assert_eq!(active_a1(&h), Value::Number(2.0));
}

#[test]
fn closing_with_save_finishes_once_the_browser_confirms_the_write() {
    let (lib, mut h) = shared();
    h.state_mut().session.new_workbook();
    h.state_mut().session.set_active(0);
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    h.state_mut().run("file.close", serde_json::json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(dialog(&h), Some("saveChanges"));
    click(&mut h, "Save");
    assert_eq!(h.state().session.documents().len(), 2, "not closed while the write is pending: nothing is lost if it fails");
    let changes = lib.take_changes();
    h.state_mut().library_settled(&changes[0], Outcome::Committed);
    h.run_steps(2);
    assert_eq!(h.state().session.documents().len(), 1, "closed once the browser stored it");
    assert!(h.state().session.documents().iter().all(|d| d.path.as_deref() != Some("Shared.xlsx")));
    assert_eq!(a1(&lib.read("Shared.xlsx").unwrap()), Value::Number(2.0));
}

#[test]
fn closing_with_save_stays_open_if_the_write_fails() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    h.state_mut().run("file.close", serde_json::json!({})).unwrap();
    h.run_steps(2);
    click(&mut h, "Save");
    let changes = lib.take_changes();
    h.state_mut().library_settled(&changes[0], Outcome::Failed("QuotaExceededError".into(), vec![("Shared.xlsx".into(), theirs(1.0, 1))]));
    h.run_steps(2);
    assert_eq!(h.state().session.documents().len(), 1);
    assert!(h.state().session.active().unwrap().is_dirty());
    assert_eq!(active_a1(&h), Value::Number(2.0), "the unsaved work is still open");
    // A later successful save must not close it: the user didn't ask again.
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    let changes = lib.take_changes();
    h.state_mut().library_settled(&changes[0], Outcome::Committed);
    h.run_steps(2);
    assert_eq!(h.state().session.documents().len(), 1);
}

#[test]
fn unsaved_work_is_reported_so_the_browser_can_warn_before_the_tab_closes() {
    let (lib, mut h) = shared();
    assert!(!h.state().has_unsaved_work());
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    assert!(h.state().has_unsaved_work(), "an edit");
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    assert!(h.state().has_unsaved_work(), "a write still in flight");
    persist(&mut h, &lib);
    assert!(!h.state().has_unsaved_work());
    h.input_mut().events.push(egui::Event::Text("9".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    assert!(h.state().has_unsaved_work(), "a cell being typed");
}

#[test]
fn a_download_is_a_copy_that_never_marks_the_workbook_saved() {
    let (lib, mut h) = shared();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "2"})).unwrap();
    choose(&mut h, "Save As");
    click(&mut h, "Download (.xlsx)");
    let d = h.state().session.active().unwrap();
    assert!(d.is_dirty(), "the download is a copy; the browser library still has the old version");
    assert_eq!(d.path.as_deref(), Some("Shared.xlsx"), "the workbook keeps its place in the library");
    assert!(lib.take_changes().is_empty());
    // A scripted save with an explicit path goes through the library, pending until confirmed.
    let r = h.state_mut().run("file.save", serde_json::json!({"path": "Scripted.xlsx"})).unwrap();
    assert_eq!(r["pending"], true);
    assert!(h.state().session.active().unwrap().is_dirty());
    persist(&mut h, &lib);
    assert!(!h.state().session.active().unwrap().is_dirty());
    assert_eq!(a1(&lib.read("Scripted.xlsx").unwrap()), Value::Number(2.0));
}
