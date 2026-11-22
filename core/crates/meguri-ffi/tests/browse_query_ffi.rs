//! U264a's Browse query model and reads through their real exports on an
//! opened collection: T7's reading and T4's run text, T8's commit and
//! incomplete text, a sidebar deck value, the suggestions, the filter
//! counts as a job, the column sets, the counts of a selection, and Find
//! Duplicates as a job.
use meguri_ffi::{
    CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    browse_query::*,
    browse_search::*,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    decks::add_deck,
    notes::{AddNoteRequest, add_note},
    notetypes::{AddNotetypeRequest, NewNotetypeTemplate, add_notetype},
    sched_ops::{SchedOperation, SchedTargets, start_sched_op},
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        1_894_701_600_000
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct Space;
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        DiskCapacity::Available { bytes: u64::MAX }
    }
}
struct Listener(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Listener {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}
struct Searches(mpsc::Sender<BrowseSearchEvent>);
impl BrowseSearchListener for Searches {
    fn on_event(&self, event: BrowseSearchEvent) {
        let _ = self.0.send(event);
    }
}

fn finish(rx: &mpsc::Receiver<CoreEvent>, id: u64) -> JobOutcome {
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        if let CoreEventKind::JobFinished { id: got, outcome } = event.kind
            && got == id
        {
            return outcome;
        }
    }
}

fn open(dir: &tempfile::TempDir) -> mpsc::Receiver<CoreEvent> {
    open_with(dir, |_| {})
}

/// Opens a new collection after `prepare` has changed its file.
fn open_with(dir: &tempfile::TempDir, prepare: impl FnOnce(&std::path::Path)) -> mpsc::Receiver<CoreEvent> {
    let collection = dir.path().join("collection");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    meguri_engine::bootstrap::create(
        &collection.join("collection.anki2"),
        &meguri_model::clock::FixedClock::new(1_894_701_600_000, meguri_model::clock::UtcOffset::UTC),
        &meguri_model::rng::SeededRandom::new(7),
    )
    .unwrap();
    prepare(&collection.join("collection.anki2"));
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    let handle = collection::start_open(
        CollectionLocation {
            manual_transfer_directory: dir.path().join("ManualTransfers").to_string_lossy().into_owned(),
            collection_directory: collection.to_str().unwrap().into(),
            backups_directory: backups.to_str().unwrap().into(),
            app_group_snapshot: dir.path().join("widget.snapshot").to_str().unwrap().into(),
        },
        Arc::new(Clock),
        Arc::new(Space),
        7,
    )
    .unwrap();
    assert_eq!(finish(&rx, handle.id), JobOutcome::Succeeded);
    rx
}

fn template(name: &str, front: &str) -> NewNotetypeTemplate {
    NewNotetypeTemplate {
        name: name.into(),
        front: front.into(),
        back: "{{FrontSide}}".into(),
        browser_front: String::new(),
        browser_back: String::new(),
    }
}

fn labels() -> FilterLabels {
    let s = |text: &str| text.to_owned();
    FilterLabels {
        current_deck: s("Current Deck"),
        untagged: s("Untagged"),
        new: s("New"),
        learning: s("Learning"),
        review: s("Review"),
        suspended: s("Suspended"),
        buried: s("Buried"),
        leech: s("Leech"),
        no_flag: s("No Flag"),
        any_flag: s("Any Flag"),
        today: s("Today"),
        overdue: s("Overdue"),
        next_7_days: s("Next 7 Days"),
        last_7_days: s("Last 7 Days"),
        last_30_days: s("Last 30 Days"),
        due_today: s("Due Today"),
        added_in_last_7_days: s("Added in Last 7 Days"),
        leeches: s("Leeches"),
    }
}

fn tag(name: &str) -> FilterValue {
    FilterValue::Tag { name: name.into() }
}

fn suspended() -> FilterValue {
    FilterValue::State {
        state: FilterState::Suspended,
    }
}

#[test]
fn the_query_model_suggestions_counts_columns_and_selection_counts_run_through_their_exports() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let notetype = add_notetype(AddNotetypeRequest {
        name: "Duo".into(),
        cloze: false,
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![template("Forward", "{{Front}}"), template("Reverse", "{{Back}}")],
        css: Some(String::new()),
    })
    .unwrap()
    .id;
    let spanish = add_deck("Lang::Spanish".into()).unwrap().id;
    let mut cards = Vec::new();
    let mut notes = Vec::new();
    for (fields, tags, deck) in [(["alpha", "one"], "alpha", spanish), (["beta", "two"], "beta", 1)] {
        let added = add_note(AddNoteRequest {
            notetype_id: notetype,
            fields: fields.iter().map(|f| (*f).to_owned()).collect(),
            tags: vec![tags.into()],
            deck_id: deck,
            guid: None,
            update_add_defaults: false,
        })
        .unwrap();
        let mut own = added.cards;
        own.sort_unstable();
        cards.extend(own);
        notes.push(added.id);
    }
    let suspend = start_sched_op(
        SchedOperation::Suspend {
            targets: SchedTargets::Cards { ids: vec![cards[0]] },
        },
        None,
    )
    .unwrap();
    assert_eq!(finish(&core, suspend.handle.id), JobOutcome::Succeeded);

    // T7, T4: Anki text reads into tokens, and their run text reads back.
    let read = filter_read_text("deck:\"Lang::Spanish\" -is:suspended tag:alpha".into()).unwrap();
    let Some(FilterToken::Value { values, negated: false }) = read.tokens.first() else {
        panic!("{read:?}")
    };
    let [FilterValue::Deck { id, native, .. }] = values.as_slice() else {
        panic!("{values:?}")
    };
    assert_eq!((*id, native.as_str()), (spanish, "Lang\u{1f}Spanish"));
    assert_eq!(
        read.tokens[1..],
        [
            FilterToken::Value {
                values: vec![suspended()],
                negated: true,
            },
            FilterToken::Value {
                values: vec![tag("alpha")],
                negated: false,
            },
        ]
    );
    assert_eq!(read.text, "");
    let run = filter_run_text(read.clone()).unwrap();
    assert_eq!(filter_read_text(run.clone()).unwrap(), read);
    let tags = FilterQuery {
        tokens: vec![FilterToken::Value {
            values: vec![tag("alpha"), tag("beta")],
            negated: true,
        }],
        text: String::new(),
    };
    assert_eq!(filter_run_text(tags).unwrap(), "-(tag:alpha OR tag:beta)");
    assert!(matches!(
        filter_run_text(FilterQuery {
            tokens: vec![FilterToken::Value {
                values: Vec::new(),
                negated: false,
            }],
            text: String::new(),
        }),
        Err(meguri_ffi::CoreError::InvalidArgument { .. })
    ));

    // T8: a commit turns syntax into tokens; D5's incomplete text.
    let committed = filter_commit(FilterQuery {
        tokens: Vec::new(),
        text: "tag:beta word".into(),
    })
    .unwrap();
    assert_eq!(
        committed,
        FilterQuery {
            tokens: vec![FilterToken::Value {
                values: vec![tag("beta")],
                negated: false,
            }],
            text: "word".into(),
        }
    );
    assert!(filter_text_incomplete("deck:".into(), 5).unwrap());
    assert!(!filter_text_incomplete("word".into(), 4).unwrap());
    assert!(filter_text_incomplete("word".into(), 9).is_err());

    // The sidebar's deck value holds its tree.
    let Some(FilterValue::Deck { id, tree, .. }) = filter_deck_value(spanish).unwrap() else {
        panic!("no deck value")
    };
    assert_eq!((id, tree), (spanish, vec![spanish]));
    assert_eq!(filter_deck_value(-5).unwrap(), None);

    // T12: the opening list starts with the built-ins; a word lists
    // "Contains" first, then the tag it matches, with a key.
    let opening = filter_suggestions(FilterQuery::default(), 0, None, Vec::new(), labels()).unwrap();
    assert_eq!(
        opening[0].row,
        FilterSuggestionRow::BuiltIn {
            search: FilterBuiltIn::DueToday
        }
    );
    let word = FilterQuery {
        tokens: Vec::new(),
        text: "alp".into(),
    };
    let rows = filter_suggestions(word, 3, None, Vec::new(), labels()).unwrap();
    assert_eq!(rows[0].row, FilterSuggestionRow::Contains { word: "alp".into() });
    let alpha = rows
        .iter()
        .find(|row| row.row == FilterSuggestionRow::Value { value: tag("alpha") })
        .expect("the tag alpha is suggested");
    assert_eq!(alpha.section, Some(FilterSection::Tags));
    assert!(alpha.key.is_some());
    assert!(matches!(
        rows.last().map(|row| &row.row),
        Some(FilterSuggestionRow::SaveSearch)
    ));

    // T12.4: counts as a job, one entry per item in order.
    let counted = start_filter_counts(
        BrowseMode::Cards,
        vec![
            FilterCountItem::Value { value: tag("alpha") },
            FilterCountItem::Value { value: suspended() },
            FilterCountItem::Saved {
                text: "tag:beta".into(),
            },
            FilterCountItem::Query { query: read },
        ],
    )
    .unwrap();
    assert_eq!(finish(&core, counted.handle.id), JobOutcome::Succeeded);
    assert_eq!(counted.result.result().unwrap(), Some(vec![Some(2), Some(1), Some(2), Some(1)]));

    // V2.2 to V2.4: the default standard set, a change, Use Standard.
    let shown = browser_column_set(None).unwrap();
    assert_eq!(shown.set, ColumnSetId::Standard);
    assert_eq!(shown.cards, ["noteFld", "template", "deck", "cardDue", "cardMod"]);
    assert_eq!(shown.notes, ["noteFld", "note", "template", "noteTags", "noteCrt"]);
    let changed = change_browser_column_set(
        ColumnSetId::Standard,
        BrowseMode::Cards,
        ColumnSetChange::Show { key: "cardEase".into() },
    )
    .unwrap();
    assert!(changed.written);
    assert_eq!(changed.summary.undo_name, None);
    assert_eq!(browser_column_set(Some(notetype)).unwrap().cards.last().map(String::as_str), Some("cardEase"));
    assert!(matches!(
        change_browser_column_set(
            ColumnSetId::Standard,
            BrowseMode::Cards,
            ColumnSetChange::SortBy { key: "question".into() }
        ),
        Err(meguri_ffi::CoreError::InvalidArgument { .. })
    ));
    let own = browser_column_set(Some(notetype)).unwrap();
    assert_eq!((own.set, own.own), (ColumnSetId::Notetype { id: notetype }, false));
    assert!(!use_standard_browser_columns(notetype).unwrap().written);

    // V7, V8.1: the counts of a selection of every card.
    let (tx, rx) = mpsc::channel();
    let session = open_search_session(Arc::new(Searches(tx))).unwrap();
    let seq = session
        .start_search(
            String::new(),
            BrowseMode::Cards,
            BrowseSort {
                key: "noteFld".into(),
                direction: BrowseSortDirection::Ascending,
            },
            None,
        )
        .unwrap();
    let handle = loop {
        match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
            BrowseSearchEvent::Complete { seq: done, handle, .. } if done == seq => break handle,
            BrowseSearchEvent::Failed { error, .. } => panic!("{error:?}"),
            _ => {}
        }
    };
    let every = session
        .selection(handle, 0, vec![BrowseIndexRange { start: 0, end: 4 }])
        .unwrap();
    let job = start_browse_selection_counts(Arc::clone(&every)).unwrap();
    assert_eq!(finish(&core, job.handle.id), JobOutcome::Succeeded);
    let mut notetypes = vec![notetype];
    notetypes.sort_unstable();
    assert_eq!(
        job.result.result().unwrap(),
        Some(BrowseSelectionCounts {
            cards: 4,
            notes: 2,
            suspended: 1,
            buried: 0,
            decks: 2,
            deck: None,
            notetypes,
        })
    );
    let one = session
        .selection(handle, 0, vec![BrowseIndexRange { start: 0, end: 2 }])
        .unwrap();
    let job = start_browse_selection_counts(one).unwrap();
    assert_eq!(finish(&core, job.handle.id), JobOutcome::Succeeded);
    let counts = job.result.result().unwrap().unwrap();
    assert_eq!((counts.cards, counts.notes, counts.decks, counts.deck), (2, 1, 1, Some(spanish)));
    // A refusal of the selection is the result's error.
    let stale = session
        .selection(handle, 7, vec![BrowseIndexRange { start: 0, end: 1 }])
        .unwrap();
    let job = start_browse_selection_counts(stale).unwrap();
    assert_eq!(finish(&core, job.handle.id), JobOutcome::Succeeded);
    assert_eq!(
        job.result.result(),
        Err(meguri_ffi::CoreError::Browse {
            failure: BrowseFailure::StaleRevision { current: 0 }
        })
    );

    // V10: a third note whose Front differs from the first only in case,
    // markup and spaces. All Notes finds the pair; the handle's rows hold
    // only the first two notes, so All Found finds none.
    let third = add_note(AddNoteRequest {
        notetype_id: notetype,
        fields: vec!["<i>ALPHA</i> ".into(), "three".into()],
        tags: Vec::new(),
        deck_id: 1,
        guid: None,
        update_add_defaults: false,
    })
    .unwrap()
    .id;
    let job = start_find_duplicates("Front".into(), None).unwrap();
    assert_eq!(finish(&core, job.handle.id), JobOutcome::Succeeded);
    let mut pair = vec![notes[0], third];
    pair.sort_unstable();
    assert_eq!(
        job.result.result().unwrap(),
        Some(FindDuplicatesResult {
            groups: vec![pair.clone()],
            note_ids: pair,
            considered: 3,
        })
    );
    let job = start_find_duplicates("Front".into(), Some(Arc::clone(&every))).unwrap();
    assert_eq!(finish(&core, job.handle.id), JobOutcome::Succeeded);
    assert_eq!(
        job.result.result().unwrap(),
        Some(FindDuplicatesResult {
            groups: Vec::new(),
            note_ids: Vec::new(),
            considered: 2,
        })
    );
    let stale = session
        .selection(handle, 7, vec![BrowseIndexRange { start: 0, end: 1 }])
        .unwrap();
    let job = start_find_duplicates("Front".into(), Some(stale)).unwrap();
    assert_eq!(finish(&core, job.handle.id), JobOutcome::Succeeded);
    assert_eq!(
        job.result.result(),
        Err(meguri_ffi::CoreError::Browse {
            failure: BrowseFailure::StaleRevision { current: 0 }
        })
    );
    session.close().unwrap();
}

/// T14 and K6.1: a new collection opens Browse on the whole collection, in
/// Cards.
#[test]
fn browse_opening_of_a_new_collection_is_the_whole_collection_in_cards() {
    let dir = tempfile::tempdir().unwrap();
    let _core = open(&dir);
    assert_eq!(
        browse_opening().unwrap(),
        BrowseOpening {
            default_search: None,
            notes_mode: false
        }
    );
}

/// T14 and K6.4: a set default search comes back as stored, untrimmed, and
/// the notes mode as stored (K6.1).
#[test]
fn browse_opening_reads_a_set_default_search_and_the_notes_mode() {
    let dir = tempfile::tempdir().unwrap();
    let _core = open_with(&dir, |path| {
        let db = meguri_storage::raw::PlainDatabase::open(path).unwrap();
        for (key, value) in [("defaultSearchText", "\"  deck:x \""), ("browserTableShowNotesMode", "true")] {
            db.execute(
                "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) VALUES (?, 0, 0, ?)",
                &[
                    meguri_storage::raw::RawValue::text(key),
                    meguri_storage::raw::RawValue::Blob(value.as_bytes().to_vec()),
                ],
            )
            .unwrap();
        }
        db.close().unwrap();
    });
    assert_eq!(
        browse_opening().unwrap(),
        BrowseOpening {
            default_search: Some("  deck:x ".into()),
            notes_mode: true
        }
    );
}

/// V2.1 and K6.1: the Cards / Notes control writes the mode, which the next
/// opening reads; writing the stored value again writes nothing.
#[test]
fn the_cards_notes_control_writes_the_notes_mode() {
    let dir = tempfile::tempdir().unwrap();
    let _core = open(&dir);
    assert!(set_browser_notes_mode(true).unwrap().written);
    assert!(browse_opening().unwrap().notes_mode);
    assert!(!set_browser_notes_mode(true).unwrap().written);
    assert!(set_browser_notes_mode(false).unwrap().written);
    assert!(!browse_opening().unwrap().notes_mode);
}

/// T13 and K7.2: Save Search… stores the run text under its name, which the
/// saved searches' value list then shows; saving the same text again writes
/// nothing, another text replaces it, and an empty name is refused.
#[test]
fn save_search_stores_the_run_text_under_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let _core = open(&dir);
    let saved = || {
        filter_suggestions(FilterQuery::default(), 0, Some(FilterSection::SavedSearches), Vec::new(), labels())
            .unwrap()
            .into_iter()
            .filter_map(|row| match row.row {
                FilterSuggestionRow::SavedSearch { name, text } => Some((name, text)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(save_browser_search("Velt".into(), "tag:velt is:due".into()).unwrap().written);
    assert_eq!(saved(), vec![("Velt".to_owned(), "tag:velt is:due".to_owned())]);
    assert!(!save_browser_search("Velt".into(), "tag:velt is:due".into()).unwrap().written);
    assert!(save_browser_search("Velt".into(), "is:new".into()).unwrap().written);
    assert_eq!(saved(), vec![("Velt".to_owned(), "is:new".to_owned())]);
    assert!(save_browser_search(String::new(), "is:new".into()).is_err());
}
