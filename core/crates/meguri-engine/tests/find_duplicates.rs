//! Find Duplicates (`specs/browser/workspace.md` V10, V18's "V10's groups
//! and key rule"): the key strips HTML as `dupe:` does (media names kept,
//! character references decoded), collapses and trims White_Space and folds
//! case; a field name matches exactly in every note type that has it;
//! empty keys are left out; groups hold two or more notes. All Found takes
//! the notes of a handle's rows, in Cards and Notes mode; the refusals of a
//! selection, a cancel and a halt; nothing is written.

// A selection is a list of index ranges; one range is the usual one.
#![allow(clippy::single_range_in_vec_init)]

mod common;

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use common::Harness;
use common::tagging::raw;
use meguri_engine::browse::search::{
    BrowseError, Duplicates, DuplicatesTarget, HandleId, SearchEvent, SearchListener, SearchSession, SortDirection,
    SortSpec, find_duplicates,
};
use meguri_engine::{CancelToken, EngineError, Progress, ProgressListener, ProgressSink, notetypes};
use meguri_model::ids::{NoteId, NotetypeId};
use meguri_search::SearchMode;
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::ReaderRole;

const WAIT: Duration = Duration::from_secs(30);

struct Events(Mutex<mpsc::Sender<SearchEvent>>);

impl SearchListener for Events {
    fn event(&self, event: SearchEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

/// Three note types: Zorvan (Glif, Trand), Plimet (Brov, Glif, Esk) and
/// Quobel (glif, Back), whose field differs from Glif only in case.
struct Fx {
    h: Harness,
    zorvan: NotetypeId,
    plimet: NotetypeId,
    quobel: NotetypeId,
    session: SearchSession,
    events: mpsc::Receiver<SearchEvent>,
}

fn kind(h: &Harness, name: &str, fields: &[&str]) -> NotetypeId {
    notetypes::add(
        &h.collection,
        notetypes::NewNotetype {
            name: name.to_owned(),
            kind: notetypes::NotetypeKind::Standard,
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            templates: vec![
                notetypes::NewTemplate {
                    name: "One".to_owned(),
                    front: format!("{{{{{}}}}}", fields[0]),
                    back: String::new(),
                    ..Default::default()
                },
                notetypes::NewTemplate {
                    name: "Two".to_owned(),
                    front: format!("{{{{{}}}}}", fields[1]),
                    back: String::new(),
                    ..Default::default()
                },
            ],
            css: Some(String::new()),
        },
    )
    .unwrap()
    .value
    .id
}

fn fixture() -> Fx {
    let h = common::fresh();
    let zorvan = kind(&h, "Zorvan", &["Glif", "Trand"]);
    let plimet = kind(&h, "Plimet", &["Brov", "Glif", "Esk"]);
    let quobel = kind(&h, "Quobel", &["glif", "Back"]);
    let (sender, events) = mpsc::channel();
    let session = h.engine.open_search_session(Arc::new(Events(Mutex::new(sender))));
    Fx {
        h,
        zorvan,
        plimet,
        quobel,
        session,
        events,
    }
}

impl Fx {
    fn add(&self, kind: NotetypeId, fields: &[&str]) -> NoteId {
        common::notes::add(&self.h, common::notes::input(kind, fields)).id
    }

    fn all(&self, field: &str) -> Duplicates {
        find_duplicates(
            DuplicatesTarget::AllNotes(&self.h.engine),
            field,
            &CancelToken::new(),
            &ProgressSink::new(),
        )
        .unwrap()
    }

    /// Runs a search and returns its complete handle with its count.
    fn search(&self, text: &str, mode: SearchMode) -> (HandleId, u64) {
        let key = match mode {
            SearchMode::Cards => "cardDue",
            SearchMode::Notes => "noteCrt",
        };
        let seq = self
            .session
            .start_search(text, mode, SortSpec::new(key, SortDirection::Ascending), None);
        loop {
            let event = self.events.recv_timeout(WAIT).expect("a search event");
            if event.seq() != seq || !event.is_terminal() {
                continue;
            }
            match event {
                SearchEvent::Complete { handle, count, .. } => return (handle, count),
                other => panic!("{text:?}: {other:?}"),
            }
        }
    }

    fn found(&self, handle: HandleId, count: u64, field: &str) -> Result<Duplicates, BrowseError> {
        let selection = self.session.selection(handle, 0, vec![0..count]);
        find_duplicates(
            DuplicatesTarget::Found(&selection),
            field,
            &CancelToken::new(),
            &ProgressSink::new(),
        )
    }

    fn dump(&self) -> Vec<RawRows> {
        let reader = self.h.collection.reader(ReaderRole::Query).unwrap();
        [
            "SELECT id, mid, mod, usn, flds, sfld, csum FROM notes ORDER BY id",
            "SELECT id, queue, mod, usn FROM cards ORDER BY id",
            "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config ORDER BY KEY",
            "SELECT mod, scm, usn, ls FROM col",
        ]
        .into_iter()
        .map(|sql| reader.execute_raw(sql, &[]).unwrap())
        .collect()
    }
}

#[test]
fn the_key_rule_groups_notes_across_note_types_and_leaves_empty_keys_out() {
    let fx = fixture();
    // Letter case, markup, White_Space and a non-breaking space.
    let zub = [
        fx.add(fx.zorvan, &["Zub", "a"]),
        fx.add(fx.zorvan, &["<b>zub</b>", "b"]),
        fx.add(fx.plimet, &["x", "  ZUB\u{a0}", "y"]),
    ];
    // Not zub: the decoded spaces stay between the letters.
    fx.add(fx.zorvan, &["z&nbsp;u&nbsp;b", "b"]);
    // Character references decode by H3; a run of White_Space is one space.
    let pim = [
        fx.add(fx.zorvan, &["Pim &amp; Vok", "a"]),
        fx.add(fx.plimet, &["", "pim\t\n &   vok", ""]),
    ];
    // Media names stay: an image of the same file, and the bare name.
    let image = [
        fx.add(fx.zorvan, &["<img src=\"Fep.png\">", "a"]),
        fx.add(fx.zorvan, &["fep.PNG", "b"]),
    ];
    // Alone, empty, or only markup and White_Space: no group.
    fx.add(fx.zorvan, &["wem", "Zub"]);
    fx.add(fx.zorvan, &["", "x"]);
    fx.add(fx.plimet, &["zub", "<br> &nbsp; ", "x"]);
    fx.add(fx.zorvan, &["  ", "y"]);
    // The field named `glif` is another field.
    fx.add(fx.quobel, &["zub", "z"]);
    // A note whose stored fields stop before Glif's position reads it empty,
    // and one whose `flds` is not text is left out.
    let short = fx.add(fx.plimet, &["ra", "zub", "ti"]);
    let blob = fx.add(fx.plimet, &["ra", "zub", "ti"]);
    raw(
        &fx.h.collection,
        "UPDATE notes SET flds = 'zub' WHERE id = ?1",
        &[SqlValue::Integer(short.0)],
    );
    raw(
        &fx.h.collection,
        "UPDATE notes SET flds = CAST('ra\u{1f}zub' AS BLOB) WHERE id = ?1",
        &[SqlValue::Integer(blob.0)],
    );

    // `flds` stored as an integer reads as its text.
    let numbers = [fx.add(fx.zorvan, &["ra", "x"]), fx.add(fx.zorvan, &["ti", "x"])];
    for note in numbers {
        raw(
            &fx.h.collection,
            "UPDATE notes SET flds = 7 WHERE id = ?1",
            &[SqlValue::Integer(note.0)],
        );
    }

    let found = fx.all("Glif");
    let mut expected = vec![zub.to_vec(), pim.to_vec(), image.to_vec(), numbers.to_vec()];
    expected.sort_by_key(|group| group[0]);
    assert_eq!(found.groups, expected);
    // Every note of Zorvan and Plimet was read; Quobel's were not.
    assert_eq!(found.considered, 16);
    let mut ids: Vec<NoteId> = expected.into_iter().flatten().collect();
    ids.sort_unstable();
    assert_eq!(found.note_ids(), ids);

    // Letter case counts in the field name; one no note type has finds
    // nothing.
    let quobel = fx.all("glif");
    assert!(quobel.groups.is_empty());
    assert_eq!(quobel.considered, 1);
    assert_eq!(fx.all("Nolm"), Duplicates::default());
    assert_eq!(fx.all(""), Duplicates::default());
}

#[test]
fn all_found_takes_the_notes_of_a_handles_rows_in_both_modes() {
    let fx = fixture();
    let a = fx.add(fx.zorvan, &["nask", "ub"]);
    let b = fx.add(fx.zorvan, &["Nask", "ub"]);
    let c = fx.add(fx.zorvan, &["nask", "od"]);
    let d = fx.add(fx.zorvan, &["yalt", "ub"]);
    let e = fx.add(fx.zorvan, &["YALT", "od"]);

    // Every note: two groups.
    assert_eq!(fx.all("Glif").groups, vec![vec![a, b, c], vec![d, e]]);

    // The notes Trand:ub finds: each has two cards in Cards mode, and a
    // note counts once.
    for mode in [SearchMode::Cards, SearchMode::Notes] {
        let (handle, count) = fx.search("Trand:ub", mode);
        assert_eq!(count, if mode == SearchMode::Cards { 6 } else { 3 });
        let found = fx.found(handle, count, "Glif").unwrap();
        assert_eq!(found.groups, vec![vec![a, b]], "{mode:?}");
        assert_eq!(found.considered, 3, "{mode:?}");
    }

    // A selection of some rows only: the notes of those rows.
    let (handle, _) = fx.search("Trand:ub", SearchMode::Notes);
    let selection = fx.session.selection(handle, 0, vec![0..1]);
    let one = find_duplicates(DuplicatesTarget::Found(&selection), "Glif", &CancelToken::new(), &ProgressSink::new())
        .unwrap();
    assert_eq!((one.groups.len(), one.considered), (0, 1));

    // No results: nothing found, nothing read.
    let (empty, count) = fx.search("Trand:nix", SearchMode::Notes);
    assert_eq!(count, 0);
    assert_eq!(fx.found(empty, 0, "Glif").unwrap(), Duplicates::default());

    // A note deleted since the search is not read.
    let (handle, count) = fx.search("Trand:ub", SearchMode::Notes);
    meguri_engine::notes::delete(&fx.h.collection, vec![b], meguri_model::changes::OpSource::UserUndoable).unwrap();
    let after = fx.found(handle, count, "Glif").unwrap();
    assert_eq!((after.groups.len(), after.considered), (0, 2));
}

/// V10: a note with a negative id, which
/// only a damaged collection holds, is left out of the groups and the note
/// ids, so the Duplicates token can always be built; a group it leaves with
/// one note disappears. It is still read, so it counts as considered.
#[test]
fn a_note_with_a_negative_id_is_left_out_of_every_group() {
    let fx = fixture();
    let lone = fx.add(fx.zorvan, &["ko", "ub"]);
    let damaged_lone = fx.add(fx.zorvan, &["KO", "ub"]);
    let pair = [fx.add(fx.zorvan, &["lu", "ub"]), fx.add(fx.zorvan, &["Lu", "ub"])];
    let damaged_third = fx.add(fx.zorvan, &["lu ", "ub"]);
    let mut negative = Vec::new();
    for note in [damaged_lone, damaged_third] {
        let id = SqlValue::Integer(-note.0);
        let old = SqlValue::Integer(note.0);
        raw(&fx.h.collection, "UPDATE notes SET id = ?1 WHERE id = ?2", &[id.clone(), old.clone()]);
        raw(&fx.h.collection, "UPDATE cards SET nid = ?1 WHERE nid = ?2", &[id, old]);
        negative.push(NoteId(-note.0));
    }

    let found = fx.all("Glif");
    assert_eq!(found.groups, vec![pair.to_vec()]);
    assert_eq!(found.note_ids(), pair.to_vec());
    assert!(!found.note_ids().contains(&lone));
    assert_eq!(found.considered, 5);

    // All Found: the handle's rows hold the damaged notes too.
    for mode in [SearchMode::Cards, SearchMode::Notes] {
        let (handle, count) = fx.search("Trand:ub", mode);
        let found = fx.found(handle, count, "Glif").unwrap();
        assert_eq!(found.groups, vec![pair.to_vec()], "{mode:?}");
        assert!(negative.iter().all(|id| !found.note_ids().contains(id)), "{mode:?}");
        assert_eq!(found.considered, 5, "{mode:?}");
    }
}

#[test]
fn a_selection_is_refused_as_resolve_selection_refuses_it_and_cancel_and_halt_stop_it() {
    let fx = fixture();
    for word in ["a", "A", "b"] {
        fx.add(fx.zorvan, &[word, "m"]);
    }
    let (handle, count) = fx.search("", SearchMode::Notes);
    assert_eq!(count, 3);
    let run = |revision: u64, ranges: Vec<std::ops::Range<u64>>, cancel: &CancelToken| {
        let selection = fx.session.selection(handle, revision, ranges);
        find_duplicates(DuplicatesTarget::Found(&selection), "Glif", cancel, &ProgressSink::new())
    };
    let token = CancelToken::new();
    assert_eq!(run(4, vec![0..3], &token), Err(BrowseError::StaleRevision { current: 0 }));
    assert_eq!(run(0, vec![0..4], &token), Err(BrowseError::OutOfRange));
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert_eq!(run(0, vec![0..3], &cancelled), Err(BrowseError::Engine(EngineError::Cancelled)));
    let all = find_duplicates(DuplicatesTarget::AllNotes(&fx.h.engine), "Glif", &cancelled, &ProgressSink::new());
    assert_eq!(all, Err(BrowseError::Engine(EngineError::Cancelled)));

    // A replacement, a suspend or a close halts searches: refused.
    let refused = SearchSession::while_searches_stopped_for_testing(&fx.h.engine, || run(0, vec![0..3], &token));
    assert!(
        matches!(refused, Err(BrowseError::Engine(EngineError::CollectionUnavailable { .. }))),
        "{refused:?}"
    );

    // A released handle is unknown.
    let session = fx.h.engine.open_search_session(Arc::new(Events(Mutex::new(mpsc::channel().0))));
    let selection = session.selection(handle, 0, vec![0..3]);
    assert_eq!(
        find_duplicates(DuplicatesTarget::Found(&selection), "Glif", &token, &ProgressSink::new()),
        Err(BrowseError::UnknownHandle)
    );
    assert_eq!(run(0, vec![0..3], &token).unwrap().groups.len(), 1);
}

struct Reports(Mutex<Vec<(u64, Option<u64>)>>);

impl ProgressListener for Reports {
    fn on_progress(&self, progress: &Progress) {
        self.0.lock().unwrap().push((progress.completed, progress.total));
    }
}

#[test]
fn it_reports_progress_and_writes_nothing() {
    let fx = fixture();
    for index in 0..30 {
        fx.add(fx.zorvan, &[&format!("q{}", index % 10), "m"]);
    }
    fx.add(fx.quobel, &["q1", "m"]);
    let before = fx.dump();
    let reports = Arc::new(Reports(Mutex::new(Vec::new())));
    let progress = ProgressSink::new();
    progress.set_listener(reports.clone());
    let found = find_duplicates(DuplicatesTarget::AllNotes(&fx.h.engine), "Glif", &CancelToken::new(), &progress).unwrap();
    assert_eq!(found.groups.len(), 10);
    assert!(found.groups.iter().all(|group| group.len() == 3));
    assert_eq!(found.considered, 30);
    // The first report gives the total: the notes of the note types with
    // the field.
    assert_eq!(reports.0.lock().unwrap().first(), Some(&(0, Some(30))));
    assert_eq!(fx.dump(), before);
}

/// A progress clock that moves one second at every reading, so no report
/// is held back.
fn fake_now() -> Duration {
    static NOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    Duration::from_secs(NOW.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
}

/// Cancels `token` at the first report, which comes before the notes are
/// read.
struct CancelAtFirst(Arc<CancelToken>, Mutex<Vec<(u64, Option<u64>)>>);

impl ProgressListener for CancelAtFirst {
    fn on_progress(&self, progress: &Progress) {
        self.1.lock().unwrap().push((progress.completed, progress.total));
        self.0.cancel();
    }
}

#[test]
fn the_last_report_is_the_total_for_both_targets() {
    let fx = fixture();
    for index in 0..12 {
        fx.add(fx.zorvan, &[&format!("q{}", index % 4), "m"]);
    }
    fx.add(fx.quobel, &["q1", "m"]);
    let (handle, count) = fx.search("", SearchMode::Cards);
    assert_eq!(count, 26);
    let selection = fx.session.selection(handle, 0, vec![0..count]);
    for (target, total) in [
        (DuplicatesTarget::AllNotes(&fx.h.engine), 12),
        // The listed notes, the Quobel note among them.
        (DuplicatesTarget::Found(&selection), 13),
    ] {
        let reports = Arc::new(Reports(Mutex::new(Vec::new())));
        let progress = ProgressSink::with_clock(fake_now);
        progress.set_listener(reports.clone());
        let found = find_duplicates(target, "Glif", &CancelToken::new(), &progress).unwrap();
        assert_eq!((found.groups.len(), found.considered), (4, 12));
        let reports = reports.0.lock().unwrap();
        assert_eq!(reports.first(), Some(&(0, Some(total))), "{target:?}");
        assert_eq!(reports.last(), Some(&(total, Some(total))), "{target:?}");
    }
}

#[test]
fn a_cancel_at_the_first_progress_report_stops_both_targets() {
    let fx = fixture();
    for index in 0..30 {
        fx.add(fx.zorvan, &[&format!("q{}", index % 3), "m"]);
    }
    let (handle, count) = fx.search("", SearchMode::Notes);
    let selection = fx.session.selection(handle, 0, vec![0..count]);
    for target in [DuplicatesTarget::AllNotes(&fx.h.engine), DuplicatesTarget::Found(&selection)] {
        let token = CancelToken::new();
        let listener = Arc::new(CancelAtFirst(Arc::clone(&token), Mutex::new(Vec::new())));
        let progress = ProgressSink::with_clock(fake_now);
        progress.set_listener(listener.clone());
        assert_eq!(
            find_duplicates(target, "Glif", &token, &progress),
            Err(BrowseError::Engine(EngineError::Cancelled)),
            "{target:?}"
        );
        // Cancelled at the first report: nothing more was read.
        assert_eq!(*listener.1.lock().unwrap(), [(0, Some(30))], "{target:?}");
    }
    // The reader it held is free again.
    assert_eq!(fx.all("Glif").groups.len(), 3);
}

#[test]
fn a_note_type_with_the_lowest_id_is_read() {
    let fx = fixture();
    let pair = [fx.add(fx.zorvan, &["zub", "a"]), fx.add(fx.zorvan, &["Zub", "b"])];
    for sql in [
        "UPDATE notetypes SET id = ?1 WHERE id = ?2",
        "UPDATE fields SET ntid = ?1 WHERE ntid = ?2",
        "UPDATE templates SET ntid = ?1 WHERE ntid = ?2",
        "UPDATE notes SET mid = ?1 WHERE mid = ?2",
    ] {
        raw(
            &fx.h.collection,
            sql,
            &[SqlValue::Integer(i64::MIN), SqlValue::Integer(fx.zorvan.0)],
        );
    }
    let found = fx.all("Glif");
    assert_eq!((found.groups, found.considered), (vec![pair.to_vec()], 2));
}
