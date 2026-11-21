//! U267's Note Types reads through their real exports on an opened
//! collection: the ordered list, then each read as a job with its result,
//! an unknown note type's `NotFound`, and a cancelled job.
use meguri_ffi::{
    CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    notes::{AddNoteRequest, add_note},
    notetype_reads::*,
    notetype_structure::{
        NotetypeConfirmedLosses, NotetypeDraftChange, NotetypeStructureDraft, StructureDraftField,
        StructureDraftTemplate,
    },
    notetypes::{AddNotetypeRequest, NewNotetypeTemplate, NotetypeRecord, add_notetype, read_notetype},
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

fn note(id: i64, fields: &[&str]) {
    add_note(AddNoteRequest {
        notetype_id: id,
        fields: fields.iter().map(|f| (*f).to_owned()).collect(),
        tags: Vec::new(),
        deck_id: 1,
        guid: None,
        update_add_defaults: false,
    })
    .unwrap();
}

/// The unchanged draft of a note type as read.
fn draft(record: &NotetypeRecord) -> NotetypeStructureDraft {
    NotetypeStructureDraft {
        notetype_id: record.id,
        base: record.base.clone(),
        name: record.name.clone(),
        css: record.css.clone(),
        fields: record
            .fields
            .iter()
            .map(|f| StructureDraftField {
                origin: Some(f.ord),
                name: f.name.clone(),
                sticky: f.sticky,
                rtl: f.rtl,
                font_name: f.font_name.clone(),
                font_size: f.font_size,
                description: f.description.clone(),
                plain_text: f.plain_text,
                collapsed: f.collapsed,
                exclude_from_search: f.exclude_from_search,
            })
            .collect(),
        templates: record
            .templates
            .iter()
            .map(|t| StructureDraftTemplate {
                origin: Some(t.ord),
                name: t.name.clone(),
                front: t.front.clone(),
                back: t.back.clone(),
                browser_front: t.q_format_browser.clone(),
                browser_back: t.a_format_browser.clone(),
                target_deck_id: t.target_deck_id,
                browser_font_name: t.browser_font_name.clone(),
                browser_font_size: t.browser_font_size,
            })
            .collect(),
        sort_field: None,
        skip_checks: true,
        confirmed: NotetypeConfirmedLosses::default(),
    }
}

#[test]
fn the_note_types_reads_run_as_jobs_on_the_open_collection() {
    let dir = tempfile::tempdir().unwrap();
    let rx = open(&dir);
    let id = add_notetype(AddNotetypeRequest {
        name: "Trio".into(),
        cloze: false,
        fields: vec!["Front".into(), "Back".into(), "Extra".into()],
        templates: vec![template("Forward", "{{Front}}"), template("Reverse", "{{Back}}")],
        css: Some(String::new()),
    })
    .unwrap()
    .id;
    note(id, &["a", "b", "x"]);
    note(id, &["c", "<br>", ""]);

    let names: Vec<String> = notetype_list().unwrap().into_iter().map(|n| n.name).collect();
    assert!(names.iter().any(|n| n == "Trio"));
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| meguri_text::fold::compare_folded(a, b));
    assert_eq!(names, sorted);

    let counts = start_notetype_note_counts().unwrap();
    assert_eq!(finish(&rx, counts.handle.id), JobOutcome::Succeeded);
    let counts = counts.result.result().unwrap().unwrap();
    assert!(counts.contains(&NotetypeNoteCount { id, notes: 2 }));
    assert_eq!(counts.len(), names.len());

    let fields = start_notetype_field_content_counts(id).unwrap();
    assert_eq!(finish(&rx, fields.handle.id), JobOutcome::Succeeded);
    assert_eq!(
        fields.result.result().unwrap(),
        Some(NotetypeFieldContentCounts {
            notes: 2,
            fields: vec![2, 1, 1],
        })
    );
    let unknown = start_notetype_field_content_counts(9_999).unwrap();
    assert!(matches!(
        finish(&rx, unknown.handle.id),
        JobOutcome::Failed {
            error: CoreError::NotFound { .. }
        }
    ));
    assert_eq!(unknown.result.result().unwrap(), None);

    let record = read_notetype(id).unwrap();
    let mut removing = draft(&record);
    removing.fields.pop();
    removing.templates.pop();
    let summary = start_notetype_draft_summary(removing).unwrap();
    assert_eq!(finish(&rx, summary.handle.id), JobOutcome::Succeeded);
    let summary = summary.result.result().unwrap().unwrap();
    assert!(summary.loses_content);
    assert_eq!(
        summary.changes,
        [
            NotetypeDraftChange::FieldRemoved {
                ord: 2,
                name: "Extra".into(),
                notes_with_content: 1,
            },
            NotetypeDraftChange::TemplateRemoved {
                ord: 1,
                name: "Reverse".into(),
                cards_removed: 1,
                notes_without_cards: 0,
            },
        ]
    );

    let blank = start_notetype_notes_without_card(draft(&record)).unwrap();
    assert_eq!(finish(&rx, blank.handle.id), JobOutcome::Succeeded);
    let blank = blank.result.result().unwrap().unwrap();
    assert_eq!(blank.len(), 2);
    assert!(blank[0].note_ids.is_empty() && blank[0].field.is_none());
    assert_eq!((blank[1].note_ids.len(), blank[1].field), (1, Some(1)));

    let removal = start_notetype_removal_counts(id).unwrap();
    assert_eq!(finish(&rx, removal.handle.id), JobOutcome::Succeeded);
    assert_eq!(removal.result.result().unwrap(), Some(NotetypeRemovalCounts { notes: 2, cards: 3 }));

    let cancelled = start_notetype_removal_counts(id).unwrap();
    cancelled.handle.cancel.cancel().unwrap();
    let outcome = finish(&rx, cancelled.handle.id);
    assert!(
        matches!(outcome, JobOutcome::Cancelled | JobOutcome::Succeeded),
        "{outcome:?}"
    );

    assert_eq!(notetype_near_field_name("extra".into(), vec!["Front".into(), "Extra".into()]).unwrap(), Some(1));
    meguri_ffi::lifecycle::close_collection().unwrap();
}
