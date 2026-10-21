//! One test owns this binary's process-global listener and collection runtime.
use meguri_engine::{OpSource, OpSpec};
use meguri_ffi::{
    CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    deck_tree,
    events::set_event_listener,
    lifecycle::{self, CollectionLifecycle},
    review_session,
};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, space::StatvfsSpace};
use std::{
    path::Path,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
const NOW: i64 = 1_894_701_600_000;
struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        NOW
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
fn finish(rx: &mpsc::Receiver<CoreEvent>, id: u64) -> (JobOutcome, Vec<CoreEvent>) {
    let mut seen = Vec::new();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).expect("open completion");
        let outcome = match &event.kind {
            CoreEventKind::JobFinished { id: got, outcome } if *got == id => Some(outcome.clone()),
            _ => None,
        };
        seen.push(event);
        if let Some(outcome) = outcome {
            return (outcome, seen);
        }
    }
}
fn location(root: &Path) -> CollectionLocation {
    let collection = root.join("collection");
    let backups = root.join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    CollectionLocation {
        collection_directory: collection.to_str().unwrap().into(),
        backups_directory: backups.to_str().unwrap().into(),
        app_group_snapshot: root.join("widget.snapshot").to_str().unwrap().into(),
    }
}
fn fixture(location: &CollectionLocation) {
    let clock = Arc::new(FixedClock::new(NOW, UtcOffset::UTC));
    meguri_engine::bootstrap::create(
        &Path::new(&location.collection_directory).join("collection.anki2"),
        clock.as_ref(),
        &SeededRandom::new(37),
    )
    .unwrap();
    let host = meguri_host::CoreHost::open(meguri_host::HostConfig::new(
        location.collection_directory.clone().into(),
        location.backups_directory.clone().into(),
        location.app_group_snapshot.clone().into(),
        clock,
        Arc::new(SeededRandom::new(37)),
        Arc::new(StatvfsSpace),
    ))
    .unwrap();
    let c = host.collection().unwrap();
    c.run(OpSpec::named("fixture policy", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let zoo = meguri_engine::decks::add(&c, "Zoo".into()).unwrap().value.id;
    let child = meguri_engine::decks::add(&c, "alpha::Child".into()).unwrap().value.id;
    let nt = {
        let reader = c.reader(ReaderRole::Query).unwrap();
        meguri_engine::notetypes::read::by_name(&*reader, "Basic")
            .unwrap()
            .unwrap()
            .id
    };
    for (index, deck) in [child, child, zoo].into_iter().enumerate() {
        meguri_engine::notes::add(
            &c,
            meguri_engine::notes::AddNote {
                notetype_id: nt,
                fields: vec![format!("front {index}"), "back".into()],
                tags: vec![],
                deck_id: deck,
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    }
    c.run(
        OpSpec::named("fixture description", OpSource::Maintenance),
        move |ctx| {
            let mut deck = ctx.scope().deck(zoo)?.unwrap();
            if let meguri_model::proto::deck::DeckKind::Normal(normal) = &mut deck.kind.kind {
                normal.description = "<b>safe</b><script>unsafe()</script>".into();
            }
            meguri_engine::decks::save_in(ctx.scope(), deck)?;
            Ok(())
        },
    )
    .unwrap();
    c.run(
        OpSpec::named("fixture imported sentinels", OpSource::Maintenance),
        move |ctx| {
            fn unknown<M: meguri_model::proto::BlobMessage>(
                b: &meguri_model::proto::ProtoBlob<M>,
            ) -> meguri_model::proto::ProtoBlob<M> {
                let mut bytes = b.to_bytes().into_owned();
                bytes.extend_from_slice(&[0xa0, 0x01, 0x07]);
                meguri_model::proto::ProtoBlob::decode(&bytes).unwrap()
            }
            let mut n = ctx.scope().notetype(nt)?.unwrap();
            n.config.latex_pre = "imported latex preamble".into();
            n.config.latex_svg = true;
            n.config = unknown(&n.config);
            n.fields[0].config = unknown(&n.fields[0].config);
            n.templates[0].config = unknown(&n.templates[0].config);
            ctx.scope().put_notetype(&n)?;
            for mut note in ctx.scope().notes_after(None, 100)? {
                note.flags = 64;
                note.data = "imported note data".into();
                ctx.scope().put(&note)?;
            }
            for mut card in ctx.scope().cards_after(None, 100)? {
                card.flags = 0xa0;
                ctx.scope().put(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    host.close().unwrap();
}
struct BlockingClock {
    first: AtomicBool,
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl HostClock for BlockingClock {
    fn now_ms(&self) -> i64 {
        if !self.first.swap(true, Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
        }
        NOW
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct InvalidOffsetClock;
impl HostClock for InvalidOffsetClock {
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        86_400
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
#[test]
fn exported_open_counts_review_close_reopen_upgrade_and_races() {
    let dir = tempfile::tempdir().unwrap();
    let location = location(dir.path());
    fixture(&location);
    let (tx, rx) = mpsc::channel();
    set_event_listener(Arc::new(Listener(tx))).unwrap();
    let open = || collection::start_open(location.clone(), Arc::new(Clock), Arc::new(Space), 37).unwrap();
    let first = open();
    let (outcome, mut events) = finish(&rx, first.id);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready { generation: 1 }
    );
    assert_eq!(lifecycle::wait_collection_available(0).unwrap(), 1);
    assert!(events.iter().any(|e| e.generation == 1
        && e.kind
            == CoreEventKind::Lifecycle {
                state: CollectionLifecycle::Ready { generation: 1 }
            }));
    let tree = deck_tree::deck_tree_with_counts().unwrap();
    assert_eq!(tree.generation, 1);
    assert_eq!(tree.next_change_at, 1_894_766_400);
    assert_eq!(
        tree.nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["alpha", "Zoo"]
    );
    assert_eq!(
        (
            tree.nodes[0].counts.new,
            tree.nodes[0].counts.learning,
            tree.nodes[0].counts.review
        ),
        (2, 0, 0)
    );
    assert_eq!(tree.nodes[0].children[0].name, "Child");
    assert_eq!(tree.nodes[0].children[0].counts.new, 2);
    assert_eq!(tree.nodes[1].counts.new, 1);
    assert_eq!(
        deck_tree::deck_description_html(tree.nodes[1].id).unwrap(),
        "<b>safe</b>"
    );
    assert_eq!(
        deck_tree::deck_study_state(tree.nodes[0].id).unwrap().queue_counts.new,
        2
    );
    let persisted = typed_surface(&rx, &mut events, tree.nodes[1].id);
    let review_session::ReviewStartResult::Started {
        session_id,
        generation,
        head,
        ..
    } = review_session::start_session(tree.nodes[0].id, None).unwrap()
    else {
        panic!("review must use the opened host")
    };
    assert_eq!(generation, 1);
    assert_eq!(head.counts.new, 2);
    assert!(!head.items.is_empty());
    review_session::end_session(session_id).unwrap();
    lifecycle::close_collection().unwrap();
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::NoCollection
    );
    assert!(deck_tree::deck_tree_with_counts().is_err());
    let second = open();
    let (outcome, more) = finish(&rx, second.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(deck_tree::deck_tree_with_counts().unwrap().generation, 2);
    assert_eq!(
        meguri_ffi::notes::read_note(persisted.0).unwrap().fields,
        ["persisted front", "back"]
    );
    assert_eq!(
        meguri_ffi::notetypes::read_notetype(persisted.1).unwrap().css,
        ".card { color: green; }"
    );
    assert_eq!(
        meguri_ffi::media::add_media("Surface.PNG".into(), vec![1, 2, 3]).unwrap(),
        "surface.png"
    );

    lifecycle::close_collection().unwrap();
    // Hold an actual foreign clock call. Both competing control operations
    // must refuse promptly, and neither may close this open's generation.
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let pending = collection::start_open(
        location.clone(),
        Arc::new(BlockingClock {
            first: AtomicBool::new(false),
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }),
        Arc::new(Space),
        37,
    )
    .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(lifecycle::close_collection(), Err(CoreError::Busy { .. })));
    assert!(matches!(
        collection::start_open(location.clone(), Arc::new(Clock), Arc::new(Space), 37),
        Err(CoreError::Busy { .. })
    ));
    pending.cancel.cancel().unwrap();
    release_tx.send(()).unwrap();
    let (outcome, more) = finish(&rx, pending.id);
    events.extend(more);
    assert!(
        matches!(outcome, JobOutcome::Failed { error: CoreError::Unavailable { reason } } if reason.contains("remains open"))
    );
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready { generation: 3 }
    );
    lifecycle::close_collection().unwrap();
    // A queued cancelled open releases ownership before JobFinished, even
    // when the registry skips its work closure entirely.
    let workers = meguri_engine::EngineConfig::default().job_workers;
    let started = Arc::new(Barrier::new(workers + 1));
    let release = Arc::new(Barrier::new(workers + 1));
    for _ in 0..workers {
        let started = started.clone();
        let release = release.clone();
        meguri_ffi::jobs::start("occupy pool", 0, move |_| {
            started.wait();
            release.wait();
            Ok(())
        })
        .unwrap();
    }
    started.wait();
    let queued = open();
    queued.cancel.cancel().unwrap();
    release.wait();
    let (outcome, more) = finish(&rx, queued.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::NoCollection
    );
    let next = open();
    let (outcome, more) = finish(&rx, next.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    lifecycle::close_collection().unwrap();
    // A bad synchronous host callback unwinds an open step. No live writer
    // or poisoned process reservation may escape to prevent a successor.
    let invalid = collection::start_open(location.clone(), Arc::new(InvalidOffsetClock), Arc::new(Space), 37).unwrap();
    let (outcome, more) = finish(&rx, invalid.id);
    events.extend(more);
    assert!(matches!(outcome, JobOutcome::Failed { .. }));
    let next = open();
    let (outcome, more) = finish(&rx, next.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    lifecycle::close_collection().unwrap();
    // Existing generated schema-11 fixture, copied with no newly invented input.
    let upgraded = tempfile::tempdir().unwrap();
    let upgrade = self::location(upgraded.path());
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-11.anki2"),
        Path::new(&upgrade.collection_directory).join("collection.anki2"),
    )
    .unwrap();
    let job = collection::start_open(upgrade, Arc::new(Clock), Arc::new(Space), 37).unwrap();
    let (outcome, more) = finish(&rx, job.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(events.iter().any(|e| matches!(
        e.kind,
        CoreEventKind::Lifecycle {
            state: CollectionLifecycle::Upgrading { from: 11, .. }
        }
    )));
    lifecycle::close_collection().unwrap();
    // An error before/inside open must release the reservation for a successor.
    let missing = tempfile::tempdir().unwrap();
    let missing_location = self::location(missing.path());
    let failed = collection::start_open(missing_location, Arc::new(Clock), Arc::new(Space), 37).unwrap();
    let (outcome, more) = finish(&rx, failed.id);
    events.extend(more);
    assert!(matches!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::Open {
                failure: lifecycle::CollectionOpenFailure::NotFound
            }
        }
    ));
    assert!(matches!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Failed {
            reason: lifecycle::CollectionFailedReason::Open {
                error: lifecycle::CollectionOpenFailure::NotFound
            }
        }
    ));
    let last = open();
    let (outcome, more) = finish(&rx, last.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    lifecycle::close_collection().unwrap();
    verify_imported_sentinels(&location, persisted.1);
    assert!(
        events.windows(2).all(|e| e[0].seq < e[1].seq),
        "one process dispatcher preserves order across reopen"
    );
}

// Runs within the single process-runtime test above, using only real exports.
fn typed_surface(rx: &mpsc::Receiver<CoreEvent>, events: &mut Vec<CoreEvent>, zoo: i64) -> (i64, i64) {
    use meguri_ffi::{decks, mutations::MutationSummary, notes, notetypes, suspend, tags, undo};
    use review_session::{ReviewUndoScope as Scope, ReviewUndoStatus as Status};
    let mut summaries: Vec<MutationSummary> = Vec::new();
    assert!(matches!(
        undo::peek_undo(Scope::Global).unwrap(),
        Status::Unavailable { .. }
    ));
    let no_undo = undo::start_undo(Scope::Global).unwrap();
    let (outcome, more) = finish(rx, no_undo.handle.id);
    events.extend(more);
    assert!(matches!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::UndoUnavailable { .. }
        }
    ));
    assert!(matches!(
        notes::read_note(-99),
        Err(CoreError::NoteNotFound { id: -99 })
    ));
    let deck = decks::add_deck("Surface".into()).unwrap();
    summaries.push(deck.summary.clone());
    assert_eq!(deck.summary.undo_name.as_deref(), Some("Add Deck"));
    assert_eq!(deck.summary.generation, 1);
    assert!(deck.summary.changes.decks.iter().any(|r| r.id == deck.id));
    let renamed = decks::rename_deck(deck.id, "Surface renamed".into()).unwrap();
    summaries.push(renamed.summary);
    assert_eq!(renamed.name, "Surface renamed");
    decks::add_deck("Surface renamed::Kid".into()).unwrap();
    assert!(matches!(
        decks::rename_deck(deck.id, "Surface renamed::Kid::Parent".into()),
        Err(CoreError::DeckRefused {
            refusal: meguri_ffi::error_types::DeckRefusal::UnderDescendant
        })
    ));
    let collapsed = decks::collapse_decks(vec![deck.id], decks::DeckCollapseScope::Browser, false).unwrap();
    summaries.push(collapsed);
    let nt = notetypes::notetype_names()
        .unwrap()
        .into_iter()
        .find(|n| n.name == "Basic")
        .unwrap()
        .id;
    let before = notetypes::read_notetype(nt).unwrap();
    let css = notetypes::set_notetype_css(nt, before.base.clone(), ".card { color: green; }".into()).unwrap();
    summaries.push(css.summary);
    assert!(matches!(
        notetypes::rename_notetype(nt, before.base, "stale".into()),
        Err(CoreError::Stale { .. })
    ));
    let fresh = notetypes::read_notetype(nt).unwrap();
    let clone = notetypes::clone_notetype(nt, Some("Surface copy".into())).unwrap();
    assert_ne!(clone.id, nt);
    assert_eq!(notetypes::read_notetype(clone.id).unwrap().css, fresh.css);
    let bad = notetypes::add_notetype(notetypes::AddNotetypeRequest {
        name: "invalid".into(),
        cloze: false,
        fields: vec![],
        templates: vec![],
        css: None,
    });
    assert!(matches!(bad, Err(CoreError::NotetypeRefused { .. })));
    let note = notes::add_note(notes::AddNoteRequest {
        notetype_id: nt,
        fields: vec!["surface front".into(), "back".into()],
        tags: vec!["Parent::Child".into()],
        deck_id: deck.id,
        guid: Some("surface-guid".into()),
        update_add_defaults: false,
    })
    .unwrap();
    summaries.push(note.summary.clone());
    assert_eq!(note.cards.len(), 1);
    let check = notes::check_note_fields(nt, vec!["surface front".into(), "back".into()], None).unwrap();
    assert_eq!(check.state, notes::FieldState::Duplicate);
    assert_eq!(check.duplicates, [note.id]);
    assert_eq!(
        notes::check_note_fields(nt, vec!["surface front".into(), "back".into()], Some(note.id))
            .unwrap()
            .state,
        notes::FieldState::Normal
    );
    let updated = notes::update_note(notes::UpdateNoteRequest {
        id: note.id,
        fields: vec!["persisted front".into(), "back".into()],
        tags: vec!["Parent::Child".into()],
    })
    .unwrap();
    summaries.push(updated.summary);
    assert!(updated.written);
    assert!(
        !notes::update_note(notes::UpdateNoteRequest {
            id: note.id,
            fields: vec!["persisted front".into(), "back".into()],
            tags: vec!["Parent::Child".into()]
        })
        .unwrap()
        .written
    );
    assert_eq!(tags::mark_notes(vec![note.id]).unwrap().count, 1);
    assert!(notes::read_note(note.id).unwrap().tags.contains(&"marked".into()));
    assert_eq!(tags::toggle_note_mark(note.id, vec![note.id]).unwrap().count, 1);
    assert!(!notes::read_note(note.id).unwrap().tags.contains(&"marked".into()));
    assert_eq!(tags::add_note_tags(vec![note.id], "extra".into()).unwrap().count, 1);
    let job = tags::start_rename_tag("extra".into(), "renamed".into()).unwrap();
    let (outcome, more) = finish(rx, job.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let tag_result = job.result.result().unwrap().unwrap();
    summaries.push(tag_result.summary);
    assert!(notes::read_note(note.id).unwrap().tags.contains(&"renamed".into()));
    let workers = meguri_engine::EngineConfig::default().job_workers;
    let entered = Arc::new(Barrier::new(workers + 1));
    let release = Arc::new(Barrier::new(workers + 1));
    for _ in 0..workers {
        let entered = entered.clone();
        let release = release.clone();
        meguri_ffi::jobs::start("hold bulk cancellation queue", 1, move |_| {
            entered.wait();
            release.wait();
            Ok(())
        })
        .unwrap();
    }
    entered.wait();
    let cancelled = tags::start_add_note_tags(vec![note.id], "must-not-appear".into()).unwrap();
    cancelled.handle.cancel.cancel().unwrap();
    release.wait();
    let (outcome, more) = finish(rx, cancelled.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert!(cancelled.result.result().unwrap().is_none());
    assert!(
        !notes::read_note(note.id)
            .unwrap()
            .tags
            .contains(&"must-not-appear".into())
    );
    // Flags declare long_write from the selection, and global undo/redo
    // retain that actual step data. No blocking undo runs on an FFI lane.
    let flag = tags::start_set_card_flag(vec![note.cards[0]; 2_001], 5).unwrap();
    let (outcome, more) = finish(rx, flag.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let result = flag.result.result().unwrap().unwrap();
    assert_eq!(result.count, 1);
    summaries.push(result.summary);
    let Status::Available { step } = undo::peek_undo(Scope::Global).unwrap() else {
        panic!("flag undo missing")
    };
    assert!(step.long_write.is_some());
    let restore = undo::start_undo(Scope::Global).unwrap();
    let (outcome, more) = finish(rx, restore.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let undo::UndoResult::Global { summary, step } = restore.result.result().unwrap().unwrap() else {
        panic!("global routed to session")
    };
    assert!(step.long_write.is_some());
    summaries.push(summary);
    assert!(matches!(
        undo::peek_redo(Scope::Global).unwrap(),
        Status::Available { .. }
    ));
    let redo = undo::start_redo(Scope::Global).unwrap();
    let (outcome, more) = finish(rx, redo.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(matches!(
        redo.result.result().unwrap(),
        Some(undo::UndoResult::Global { .. })
    ));
    assert!(matches!(
        undo::peek_undo(Scope::Session { session_id: u64::MAX }),
        Err(CoreError::InvalidArgument { .. })
    ));
    let review_session::ReviewStartResult::Started { session_id, head, .. } =
        review_session::start_session(zoo, None).unwrap()
    else {
        panic!("review start")
    };
    let card = &head.items[0];
    assert!(matches!(
        review_session::answer_session(
            session_id,
            card.card_id,
            review_session::ReviewRating::Easy,
            card.state_token.clone(),
            NOW,
            2_000
        )
        .unwrap(),
        review_session::ReviewAnswerOutcome::Accepted { .. }
    ));
    let scoped = undo::start_undo(Scope::Session { session_id }).unwrap();
    let (outcome, more) = finish(rx, scoped.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let Some(undo::UndoResult::Session { head }) = scoped.result.result().unwrap() else {
        panic!("session bypassed U43")
    };
    assert_eq!(head.items[0].card_id, card.card_id);
    let scoped_redo = undo::start_redo(Scope::Session { session_id }).unwrap();
    let (outcome, more) = finish(rx, scoped_redo.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(matches!(
        scoped_redo.result.result().unwrap(),
        Some(undo::UndoResult::Session { .. })
    ));
    // U32 must flush the actual session participant and retain its identity.
    suspend::flush_for_background().unwrap();
    assert_eq!(suspend::suspend(5_000).unwrap(), suspend::SuspendOutcome::Suspended);
    assert!(tags::mark_notes(vec![note.id]).is_err());
    let resumed = suspend::resume().unwrap();
    assert_eq!(resumed.active_session, Some(session_id));
    review_session::end_session(session_id).unwrap();
    let imported = notes::read_note(card.note_id).unwrap();
    notes::update_note(notes::UpdateNoteRequest {
        id: imported.id,
        fields: imported.fields,
        tags: vec!["imported-tag-edit".into()],
    })
    .unwrap();
    tags::mark_notes(vec![imported.id]).unwrap();
    tags::set_card_flag(vec![card.card_id], 3).unwrap();
    let deletion = decks::start_delete_decks(vec![deck.id]).unwrap();
    let (outcome, more) = finish(rx, deletion.handle.id);
    events.extend(more);
    assert!(matches!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::Unavailable { .. }
        }
    ));
    assert!(deletion.result.result().unwrap().is_none());
    assert_eq!(decks::deck_deletion_summary(vec![deck.id]).unwrap().cards, 1);
    assert_eq!(notes::read_note(note.id).unwrap().guid, "surface-guid");
    assert_eq!(
        meguri_ffi::media::add_media("Surface.PNG".into(), vec![1, 2, 3]).unwrap(),
        "surface.png"
    );
    // Drain a final real job, then compare exact exported metadata to the
    // process dispatcher's commit envelopes (not a racing stream last_seq).
    let empty = decks::add_deck("Empty Surface".into()).unwrap();
    let deletion = decks::start_delete_decks(vec![empty.id]).unwrap();
    let (outcome, more) = finish(rx, deletion.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(deletion.result.result().unwrap().unwrap().deleted.cards, 0);
    let addition = notes::start_add_notes(vec![notes::AddNoteRequest {
        notetype_id: nt,
        fields: vec!["delete me".into(), "back".into()],
        tags: vec![],
        deck_id: deck.id,
        guid: None,
        update_add_defaults: false,
    }])
    .unwrap();
    let (outcome, more) = finish(rx, addition.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let added = addition.result.result().unwrap().unwrap();
    assert_eq!(added.notes.len(), 1);
    assert_eq!(added.summary.undo_name.as_deref(), Some("Add Note"));
    let transient = &added.notes[0];
    let remove = notes::start_delete_notes(vec![transient.id]).unwrap();
    let (outcome, more) = finish(rx, remove.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let removed = remove.result.result().unwrap().unwrap();
    assert_eq!(removed.count, 1);
    assert!(removed.summary.is_some());
    let absent = notes::start_delete_notes(vec![transient.id]).unwrap();
    let (outcome, more) = finish(rx, absent.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let absent = absent.result.result().unwrap().unwrap();
    assert_eq!(absent.count, 0);
    assert!(absent.summary.is_none());
    for summary in summaries {
        let event = events.iter().find(|e| e.seq == summary.seq).expect("commit envelope");
        assert_eq!(event.generation, summary.generation);
        assert_eq!(
            event.kind,
            CoreEventKind::Changes {
                changes: summary.changes
            }
        );
    }
    (note.id, nt)
}
fn verify_imported_sentinels(location: &CollectionLocation, nt: i64) {
    let host = meguri_host::CoreHost::open(meguri_host::HostConfig::new(
        location.collection_directory.clone().into(),
        location.backups_directory.clone().into(),
        location.app_group_snapshot.clone().into(),
        Arc::new(FixedClock::new(NOW, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(37)),
        Arc::new(StatvfsSpace),
    ))
    .unwrap();
    let c = host.collection().unwrap();
    {
        let r = c.reader(ReaderRole::Query).unwrap();
        let n = r.notetype(meguri_model::ids::NotetypeId(nt)).unwrap().unwrap();
        assert_eq!(n.config.latex_pre, "imported latex preamble");
        assert!(n.config.latex_svg);
        assert_eq!(n.config.css, ".card { color: green; }");
        for fields in [
            n.config.unknown_fields(),
            n.fields[0].config.unknown_fields(),
            n.templates[0].config.unknown_fields(),
        ] {
            assert!(fields.iter().any(|f| f.number() == 20 && f.bytes() == [0xa0, 0x01, 7]));
        }
        let imported = r
            .notes_after(None, 100)
            .unwrap()
            .into_iter()
            .filter(|n| n.data == "imported note data")
            .collect::<Vec<_>>();
        assert_eq!(imported.len(), 3);
        assert!(imported.iter().any(|n| n.tags.contains("imported-tag-edit")));
        for n in &imported {
            assert_eq!(n.flags, 64);
            for card in r.cards_of_note(n.id).unwrap() {
                assert_eq!(card.flags & !7, 0xa0);
            }
        }
    }
    assert_eq!(
        host.media().unwrap().store().lookup("surface.png").unwrap().is_some(),
        true
    );
    host.close().unwrap();
}
