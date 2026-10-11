//! The browser build keeps workbooks in a library inside the browser: Save offers it beside a
//! download, Ctrl+S and AutoSave write to it, and File › Open lists its workbooks to open, rename
//! or delete.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::io::FileStore as _;
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::library::Library;
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

fn click(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(3);
}

fn page(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<&'static str> {
    h.state().backstage.as_ref().map(|b| b.page.id())
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
    assert!(!d.is_dirty());
    // From now on Ctrl+S and AutoSave write to the browser.
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "8"})).unwrap();
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    assert_eq!(a1(&lib.read("Book1.xlsx").unwrap()), Value::Number(8.0));
    h.state_mut().grid.autosave = true;
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "9"})).unwrap();
    h.run_steps(2);
    assert_eq!(a1(&lib.read("Book1.xlsx").unwrap()), Value::Number(9.0), "AutoSave stores the change");
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
