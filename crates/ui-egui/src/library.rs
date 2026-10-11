//! Workbooks kept inside the browser: a flat library of named files that the engine reads and
//! writes through [`gridcraft_engine::io::FileStore`], so Open, Save, Ctrl+S and AutoSave work
//! there as they do on a disk.
//!
//! Pure and synchronous. Every change is applied in memory at once and recorded in a journal,
//! which the host drains and persists (IndexedDB in the web build), then reports back. A write
//! is [`Written::Pending`] until the host confirms it, so the workbook stays unsaved until the
//! browser has really stored it.
//!
//! Several tabs can share one library. Each stored workbook has a revision that increases with
//! every write, and every change carries the revision it expects to replace, so the host can
//! refuse a write from a tab that has not seen a newer save (optimistic concurrency). A
//! workbook another tab changed is *stale* here until this tab reads it (opening or reloading
//! takes the newer version) or the user chooses to keep their own ([`Library::accept`]).

use std::cell::RefCell;
use std::collections::BTreeMap;

use gridcraft_engine::io::Written;
use gridcraft_engine::{EngineError, Result};

/// Longest name in bytes, as on common desktop file systems.
const MAX_NAME_BYTES: usize = 255;

/// One workbook in the library, as listed.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredWorkbook {
    pub name: String,
    pub size: usize,
    pub modified_ms: f64,
}

/// A workbook as persistent storage holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredRecord {
    pub name: String,
    pub bytes: Vec<u8>,
    pub modified_ms: f64,
    pub revision: u64,
}

/// A change the host must persist, in order, each as one atomic step that first checks the
/// stored revision: `expect: None` means the name must not be stored yet. A mismatch means
/// another tab wrote first; the host then reports [`Library::conflicted`].
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Put {
        ticket: u64,
        name: String,
        bytes: Vec<u8>,
        modified_ms: f64,
        expect: Option<u64>,
    },
    Delete {
        ticket: u64,
        name: String,
        expect: u64,
    },
    /// Store `bytes` under `to` (which must not exist) at revision 1 and delete `from`.
    Rename {
        ticket: u64,
        from: String,
        to: String,
        bytes: Vec<u8>,
        modified_ms: f64,
        expect: u64,
    },
}

impl Change {
    pub fn ticket(&self) -> u64 {
        match self {
            Change::Put { ticket, .. } | Change::Delete { ticket, .. } | Change::Rename { ticket, .. } => *ticket,
        }
    }

    /// The stored names the change touches; after a refusal the host reports what each holds.
    pub fn names(&self) -> Vec<&str> {
        match self {
            Change::Put { name, .. } | Change::Delete { name, .. } => vec![name],
            Change::Rename { from, to, .. } => vec![from, to],
        }
    }
}

/// What became of one [`Change`], as the host reports it ([`crate::SheetApp::library_settled`]).
/// Refusals carry what storage now holds under each of [`Change::names`] (`None`: nothing).
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Committed,
    /// Another tab had stored a different revision first; nothing was written.
    Conflict(Vec<(String, Option<StoredRecord>)>),
    /// Storage failed (quota, browser error); nothing was written.
    Failed(String, Vec<(String, Option<StoredRecord>)>),
}

struct Entry {
    bytes: Vec<u8>,
    modified_ms: f64,
    /// The stored revision as far as this tab knows, including its own writes still in flight.
    revision: u64,
    /// Another tab stored a version this tab hasn't taken.
    stale: bool,
}

#[derive(Default)]
struct State {
    files: BTreeMap<String, Entry>,
    journal: Vec<Change>,
    last_ticket: u64,
}

impl State {
    fn ticket(&mut self) -> u64 {
        self.last_ticket += 1;
        self.last_ticket
    }

    /// Makes the entry match storage (`None`: not stored).
    fn sync(&mut self, name: &str, record: Option<StoredRecord>, stale: bool) {
        match record {
            Some(r) => {
                self.files.insert(name.to_string(), Entry { bytes: r.bytes, modified_ms: r.modified_ms, revision: r.revision, stale });
            }
            None => {
                self.files.remove(name);
            }
        }
    }
}

/// The browser's workbook library: an in-memory mirror of persistent storage plus a journal of
/// changes not yet persisted (write-behind). `clock` stamps modification times in milliseconds.
pub struct Library {
    state: RefCell<State>,
    clock: Box<dyn Fn() -> f64>,
}

/// A workbook name the library accepts: one flat file name with no surrounding spaces, no path
/// separators or control characters, not `.`/`..`, at most [`MAX_NAME_BYTES`] bytes.
pub fn check_name(name: &str) -> std::result::Result<(), String> {
    if name.trim().is_empty() {
        return Err("A workbook needs a name.".into());
    }
    if name != name.trim() {
        return Err("A workbook name can't start or end with a space.".into());
    }
    if name == "." || name == ".." || name.contains(['/', '\\']) || name.chars().any(char::is_control) {
        return Err(format!("“{name}” can't be used as a workbook name."));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err("That workbook name is too long.".into());
    }
    Ok(())
}

fn missing(name: &str) -> String {
    format!("“{name}” isn't in this browser.")
}

impl Library {
    pub fn new(clock: impl Fn() -> f64 + 'static) -> Library {
        Library { state: RefCell::default(), clock: Box::new(clock) }
    }

    /// Loads workbooks read back from persistent storage; they are not journaled again. Records
    /// whose names the library would refuse are skipped.
    pub fn restore(&self, records: Vec<StoredRecord>) {
        let mut state = self.state.borrow_mut();
        for r in records {
            if check_name(&r.name).is_ok() {
                let name = r.name.clone();
                state.sync(&name, Some(r), false);
            }
        }
    }

    /// Every workbook, most recently modified first (ties by name).
    pub fn list(&self) -> Vec<StoredWorkbook> {
        let state = self.state.borrow();
        let mut list: Vec<StoredWorkbook> =
            state.files.iter().map(|(name, e)| StoredWorkbook { name: name.clone(), size: e.bytes.len(), modified_ms: e.modified_ms }).collect();
        list.sort_by(|a, b| b.modified_ms.total_cmp(&a.modified_ms).then_with(|| a.name.cmp(&b.name)));
        list
    }

    pub fn contains(&self, name: &str) -> bool {
        self.state.borrow().files.contains_key(name)
    }

    /// Another tab stored a version of `name` this tab hasn't taken: saving here would replace it.
    pub fn is_stale(&self, name: &str) -> bool {
        self.state.borrow().files.get(name).is_some_and(|e| e.stale)
    }

    /// The user keeps this tab's version of a stale workbook: the next save replaces the newer one.
    pub fn accept(&self, name: &str) {
        if let Some(e) = self.state.borrow_mut().files.get_mut(name) {
            e.stale = false;
        }
    }

    /// Another tab stored `record` under `name` (`None`: deleted it). A different revision makes
    /// the workbook stale here; the same one is our own write coming back.
    pub fn changed_elsewhere(&self, name: &str, record: Option<StoredRecord>) {
        let mut state = self.state.borrow_mut();
        let stale = match (&record, state.files.get(name)) {
            (Some(r), Some(e)) => e.stale || r.revision != e.revision,
            _ => false,
        };
        state.sync(name, record, stale);
    }

    /// The host refused a change because storage held another tab's version: take that version
    /// and mark the workbook stale (even if the revision numbers happen to match).
    pub fn conflicted(&self, name: &str, record: Option<StoredRecord>) {
        let stale = record.is_some();
        self.state.borrow_mut().sync(name, record, stale);
    }

    /// The host couldn't store a change (quota, storage error): match what storage still holds.
    pub fn write_failed(&self, name: &str, record: Option<StoredRecord>) {
        self.state.borrow_mut().sync(name, record, false);
    }

    /// Renames `from` to `to`, refusing to replace another workbook.
    pub fn rename(&self, from: &str, to: &str) -> std::result::Result<(), String> {
        check_name(to)?;
        let mut state = self.state.borrow_mut();
        if !state.files.contains_key(from) {
            return Err(missing(from));
        }
        if from == to {
            return Ok(());
        }
        if state.files.contains_key(to) {
            return Err(format!("A workbook named “{to}” already exists."));
        }
        let Some(old) = state.files.remove(from) else { return Err(missing(from)) };
        let modified_ms = (self.clock)();
        let ticket = state.ticket();
        state.journal.push(Change::Rename {
            ticket,
            from: from.to_string(),
            to: to.to_string(),
            bytes: old.bytes.clone(),
            modified_ms,
            expect: old.revision,
        });
        state.files.insert(to.to_string(), Entry { bytes: old.bytes, modified_ms, revision: 1, stale: false });
        Ok(())
    }

    pub fn remove(&self, name: &str) -> std::result::Result<(), String> {
        let mut state = self.state.borrow_mut();
        let Some(old) = state.files.remove(name) else { return Err(missing(name)) };
        let ticket = state.ticket();
        state.journal.push(Change::Delete { ticket, name: name.to_string(), expect: old.revision });
        Ok(())
    }

    /// The changes made since the last call, oldest first, for the host to persist.
    pub fn take_changes(&self) -> Vec<Change> {
        std::mem::take(&mut self.state.borrow_mut().journal)
    }
}

impl gridcraft_engine::io::FileStore for Library {
    /// Reading takes the stored version, so a workbook opened or reloaded is no longer stale.
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        let mut state = self.state.borrow_mut();
        let entry = state.files.get_mut(path).ok_or_else(|| EngineError::Other(missing(path)))?;
        entry.stale = false;
        Ok(entry.bytes.clone())
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<Written> {
        check_name(path).map_err(EngineError::Other)?;
        let mut state = self.state.borrow_mut();
        let expect = match state.files.get(path) {
            Some(e) if e.stale => {
                return Err(EngineError::Other(format!("“{path}” was changed in another tab, so it wasn't saved over.")));
            }
            Some(e) => Some(e.revision),
            None => None,
        };
        let modified_ms = (self.clock)();
        let ticket = state.ticket();
        state.journal.push(Change::Put { ticket, name: path.to_string(), bytes: bytes.to_vec(), modified_ms, expect });
        state.files.insert(path.to_string(), Entry { bytes: bytes.to_vec(), modified_ms, revision: expect.map_or(1, |r| r + 1), stale: false });
        Ok(Written::Pending(ticket))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::io::{FileStore as _, Written};

    fn library() -> Library {
        let now = std::cell::Cell::new(1000.0);
        Library::new(move || {
            now.set(now.get() + 1.0);
            now.get()
        })
    }

    fn record(name: &str, bytes: &[u8], revision: u64) -> StoredRecord {
        StoredRecord { name: name.into(), bytes: bytes.to_vec(), modified_ms: 5.0, revision }
    }

    #[test]
    fn names_are_classified_over_a_set() {
        let accepted = ["Budget.xlsx", "Q1 report.xlsx", "naïve résumé.xlsx", "数据.xlsx", ".hidden.xlsx", "a", "x.csv"];
        let long = "a".repeat(256);
        let rejected = [
            "",
            " ",
            " Budget.xlsx",
            "Budget.xlsx ",
            ".",
            "..",
            "a/b.xlsx",
            "/Budget.xlsx",
            "a\\b.xlsx",
            "tab\there.xlsx",
            "nul\0.xlsx",
            long.as_str(),
        ];
        for name in accepted {
            assert!(check_name(name).is_ok(), "{name:?} should be accepted");
        }
        for name in rejected {
            assert!(check_name(name).is_err(), "{name:?} should be rejected");
        }
        assert!(check_name(&"a".repeat(255)).is_ok(), "255 bytes is the limit");
    }

    #[test]
    fn writes_are_pending_tickets_that_expect_the_revision_they_replace() {
        let lib = library();
        assert_eq!(lib.write("A.xlsx", b"one").unwrap(), Written::Pending(1));
        assert_eq!(lib.write("A.xlsx", b"two").unwrap(), Written::Pending(2));
        assert_eq!(lib.write("B.xlsx", b"b").unwrap(), Written::Pending(3));
        assert_eq!(lib.read("A.xlsx").unwrap(), b"two");
        assert_eq!(lib.list().into_iter().map(|w| (w.name, w.size)).collect::<Vec<_>>(), [("B.xlsx".to_string(), 1), ("A.xlsx".to_string(), 3)]);
        assert_eq!(
            lib.take_changes(),
            [
                Change::Put { ticket: 1, name: "A.xlsx".into(), bytes: b"one".to_vec(), modified_ms: 1001.0, expect: None },
                Change::Put { ticket: 2, name: "A.xlsx".into(), bytes: b"two".to_vec(), modified_ms: 1002.0, expect: Some(1) },
                Change::Put { ticket: 3, name: "B.xlsx".into(), bytes: b"b".to_vec(), modified_ms: 1003.0, expect: None },
            ],
            "a new name expects nothing stored; later writes expect the revision before them"
        );
        assert!(lib.take_changes().is_empty(), "draining empties the journal");
    }

    #[test]
    fn restored_records_carry_their_revisions_and_unusable_names_are_skipped() {
        let lib = library();
        lib.restore(vec![record("Kept.xlsx", b"k", 7), record("bad/name", b"x", 1)]);
        assert_eq!(lib.list().into_iter().map(|w| w.name).collect::<Vec<_>>(), ["Kept.xlsx"]);
        assert!(lib.take_changes().is_empty(), "restoring writes nothing back");
        lib.write("Kept.xlsx", b"k2").unwrap();
        assert!(matches!(lib.take_changes().as_slice(), [Change::Put { expect: Some(7), .. }]));
    }

    #[test]
    fn bad_names_and_missing_files_are_errors_that_change_nothing() {
        let lib = library();
        assert!(lib.write("a/b.xlsx", b"x").is_err());
        assert!(lib.read("Missing.xlsx").is_err());
        assert!(lib.remove("Missing.xlsx").is_err());
        assert!(lib.rename("Missing.xlsx", "Other.xlsx").is_err());
        assert!(lib.list().is_empty());
        assert!(lib.take_changes().is_empty());
    }

    #[test]
    fn rename_is_one_change_that_never_overwrites() {
        let lib = library();
        lib.restore(vec![record("A.xlsx", b"a", 4), record("B.xlsx", b"b", 1)]);
        assert!(lib.rename("A.xlsx", "B.xlsx").is_err(), "renaming onto another workbook is refused");
        assert!(lib.rename("A.xlsx", "bad/name").is_err());
        lib.rename("A.xlsx", "A.xlsx").unwrap();
        assert!(lib.take_changes().is_empty(), "renaming to the same name is a no-op; refusals change nothing");
        lib.rename("A.xlsx", "C.xlsx").unwrap();
        assert!(lib.read("A.xlsx").is_err());
        assert_eq!(lib.read("C.xlsx").unwrap(), b"a");
        assert!(
            matches!(lib.take_changes().as_slice(), [Change::Rename { from, to, bytes, expect: 4, .. }] if from == "A.xlsx" && to == "C.xlsx" && bytes == b"a")
        );
        lib.write("C.xlsx", b"c").unwrap();
        assert!(matches!(lib.take_changes().as_slice(), [Change::Put { expect: Some(1), .. }]), "the renamed copy starts at revision 1");
    }

    #[test]
    fn remove_deletes_the_revision_it_saw() {
        let lib = library();
        lib.restore(vec![record("A.xlsx", b"a", 3)]);
        lib.remove("A.xlsx").unwrap();
        assert!(lib.list().is_empty());
        assert!(matches!(lib.take_changes().as_slice(), [Change::Delete { name, expect: 3, .. }] if name == "A.xlsx"));
    }

    #[test]
    fn a_change_from_another_tab_makes_the_workbook_stale_until_it_is_read_or_accepted() {
        let lib = library();
        lib.restore(vec![record("A.xlsx", b"a", 1)]);
        lib.changed_elsewhere("A.xlsx", Some(record("A.xlsx", b"theirs", 1)));
        assert!(!lib.is_stale("A.xlsx"), "the same revision is not a change (an echo of our own write)");
        lib.changed_elsewhere("A.xlsx", Some(record("A.xlsx", b"theirs", 2)));
        assert!(lib.is_stale("A.xlsx"));
        let err = lib.write("A.xlsx", b"mine").unwrap_err().to_string();
        assert!(err.contains("another tab"), "a stale workbook is never written over: {err}");
        assert!(lib.take_changes().is_empty());
        assert_eq!(lib.read("A.xlsx").unwrap(), b"theirs", "reading (opening, reloading) takes the newer version");
        assert!(!lib.is_stale("A.xlsx"));
        lib.changed_elsewhere("A.xlsx", Some(record("A.xlsx", b"theirs again", 3)));
        lib.accept("A.xlsx");
        lib.write("A.xlsx", b"mine").unwrap();
        assert!(matches!(lib.take_changes().as_slice(), [Change::Put { expect: Some(3), .. }]), "keeping mine replaces the newest revision");
        lib.changed_elsewhere("New.xlsx", Some(record("New.xlsx", b"n", 1)));
        assert!(!lib.is_stale("New.xlsx") && lib.contains("New.xlsx"), "a workbook new to this tab is simply listed");
        lib.changed_elsewhere("New.xlsx", None);
        assert!(!lib.contains("New.xlsx"), "a deletion elsewhere removes it from the list");
    }

    #[test]
    fn a_conflicted_write_takes_the_stored_version_and_marks_it_stale() {
        let lib = library();
        lib.write("A.xlsx", b"mine").unwrap();
        lib.take_changes();
        lib.conflicted("A.xlsx", Some(record("A.xlsx", b"theirs", 1)));
        assert!(lib.is_stale("A.xlsx"), "same revision number, but someone else's write: stale anyway");
        assert_eq!(lib.list()[0].size, 6);
    }

    #[test]
    fn a_failed_write_resyncs_with_storage_without_blaming_another_tab() {
        let lib = library();
        lib.restore(vec![record("A.xlsx", b"old", 2)]);
        lib.write("A.xlsx", b"new").unwrap();
        lib.take_changes();
        lib.write_failed("A.xlsx", Some(record("A.xlsx", b"old", 2)));
        assert!(!lib.is_stale("A.xlsx"));
        assert_eq!(lib.read("A.xlsx").unwrap(), b"old", "the list shows what storage really holds");
        lib.write("A.xlsx", b"retry").unwrap();
        assert!(matches!(lib.take_changes().as_slice(), [Change::Put { expect: Some(2), .. }]), "a retry expects the stored revision");
        lib.remove("A.xlsx").unwrap();
        lib.take_changes();
        lib.write_failed("A.xlsx", Some(record("A.xlsx", b"old", 2)));
        assert!(lib.contains("A.xlsx"), "a delete that failed brings the workbook back to the list");
    }
}
