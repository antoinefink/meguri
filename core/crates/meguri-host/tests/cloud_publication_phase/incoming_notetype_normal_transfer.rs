//! S2.5 accepted normal-transfer aggregates are not ordinal authority.
//! Public synthetic rows, the host-owned writer and its registered Runtime only.
use super::*;
use cloud::runtime::{Incomplete, RuntimeError};
use meguri_engine::{OpResult, OpSpec};
use meguri_model::{changes::OpSource, ids::NoteId, notetype::Notetype};
use meguri_storage::{Cell, RowImage, StampMode};

const TIMEOUT: Duration = Duration::from_secs(30);

fn incoming_spec() -> OpSpec {
    OpSpec::new(meguri_engine::executor::catalog::SYNC_INCOMING_MERGE, OpSource::SyncIncoming)
        .stamp(StampMode::CallerSupplied)
}

#[derive(Clone, Copy)]
enum Replacement { Metadata, TemplateRename, TemplateSwap }

fn replacement(host: &CoreHost, id: NotetypeId, change: Replacement) -> Notetype {
    let mut aggregate = host.collection().unwrap().reader(ReaderRole::Query).unwrap()
        .notetype(id).unwrap().unwrap();
    assert_eq!(aggregate.templates.len(), 2);
    assert!(aggregate.fields.iter().all(|f| f.config.id.is_none()));
    assert!(aggregate.templates.iter().all(|t| t.config.id.is_none()));
    match change {
        Replacement::Metadata => {
            aggregate.name = "Normal-transfer metadata".into();
            aggregate.config.css = ".card { color: inherit; }".into();
        }
        Replacement::TemplateRename => aggregate.templates[0].name = "Incoming question".into(),
        Replacement::TemplateSwap => {
            let first = aggregate.templates[0].name.clone();
            aggregate.templates[0].name = aggregate.templates[1].name.clone();
            aggregate.templates[1].name = first;
        }
    }
    aggregate.mtime.0 += 1;
    aggregate
}

fn accept(host: &CoreHost, aggregate: Notetype, changed_note: Option<NoteId>) -> OpResult<()> {
    accept_with_other(host, aggregate, changed_note, None)
}

fn accept_with_other(host: &CoreHost, aggregate: Notetype, changed_note: Option<NoteId>,
    other: Option<NotetypeId>,
) -> OpResult<()> {
    let id = aggregate.id;
    let result = host.collection().unwrap().run(incoming_spec(), move |ctx| {
        if let Some(other) = other {
            // Preexisting and published: this is a valid native plain save,
            // not a newly created type or a report supplied for the target.
            let mut draft = meguri_engine::notetypes::Draft::of(ctx.scope().notetype(other)?.unwrap());
            draft.config.css = ".card { text-align: end; }".into();
            assert!(meguri_engine::notetypes::save_in(ctx.scope(), &draft, Default::default())?.written);
        }
        ctx.put_accepted_sync_notetype(&aggregate)?;
        if let Some(id) = changed_note {
            // This is a real content change, with mid and all card ordinals
            // unchanged. The loss seam otherwise would not drop this callback.
            change_note_content(ctx.scope(), id)?;
        }
        let journal = ctx.scope().journal().unwrap();
        assert!(journal.identity_layout(id.0).is_none(), "the accepted target has no identity authority");
        if let Some(other) = other {
            let report = journal.structure_report().expect("unrelated native save authors its own report");
            assert!(report.reasons.is_empty() && report.removed.is_empty() && report.type_changes.is_empty());
            assert_eq!(report.layouts.len(), 1);
            let layout = journal.identity_layout(other.0).expect("only the unrelated type has identity authority");
            assert_eq!(layout.field_map, vec![0, 1]);
            assert_eq!(layout.template_map, vec![0, 1]);
            assert!(!report.layouts.contains_key(&id.0));
        } else {
            assert!(journal.structure_report().is_none(), "no maps, reasons or generation input");
        }
        Ok(())
    }).expect("the exact finally accepted SyncIncoming aggregate must COMMIT");
    assert_eq!(result.changes.source, OpSource::SyncIncoming);
    assert!(result.changes.notetypes.iter().any(|n| n.id == id));
    result
}

fn change_note_content(scope: &mut meguri_storage::WriteScope<'_>, id: NoteId) -> Result<(), meguri_engine::EngineError> {
    let note = scope.note(id)?.unwrap();
    let mut fields = note.field_values().map(str::to_owned).collect::<Vec<_>>();
    fields[0].push_str(" — incoming content");
    meguri_engine::notes::update_in(scope, &meguri_engine::notes::UpdateNote {
        id, fields, tags: note.tags.split_whitespace().map(str::to_owned).collect(),
    })?;
    Ok(())
}

fn collection_rows(host: &CoreHost) -> Vec<(Table, Vec<RowImage>)> {
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    Table::ALL.into_iter().map(|table| {
        let mut rows = Vec::new();
        let mut after = None;
        loop {
            let page = snapshot.row_page(table, after.as_ref()).unwrap();
            if page.is_empty() { break; }
            let last = page.last().unwrap();
            after = Some(match (table.key_len(), &last[0]) {
                (2, Cell::Integer(first)) => RowKey::Pair(*first, cloud::row_payload::int(last, 1).unwrap()),
                (1, Cell::Integer(id)) => RowKey::Id(*id),
                (1, Cell::Text(key)) => RowKey::Text(key.clone()),
                _ => panic!("synthetic collection key"),
            });
            rows.extend(page);
        }
        (table, rows)
    }).collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Retained {
    entities: Vec<EntityFacts>,
    inbox: Vec<cloud::StoredInboxEntry>,
    intents: Vec<cloud::incoming::ApplyIntent>,
    change_counter: u64,
}
fn retained(host: &CoreHost, binding: &cloud::Binding) -> Retained {
    let runtime = host.cloud_runtime().unwrap();
    let mut entities = Vec::new();
    for kind in cloud::row_payload::KINDS {
        let mut after = None;
        loop {
            let page = runtime.entity_facts_page(kind, after.as_deref()).unwrap();
            if page.is_empty() { break; }
            after = Some(page.last().unwrap().key.clone());
            entities.extend(page);
        }
    }
    Retained {
        entities,
        inbox: runtime.inbox_entries(binding.id).unwrap(),
        intents: runtime.apply_intents(binding.id).unwrap(),
        change_counter: runtime.status().unwrap().change_counter,
    }
}

fn cover_current_open(host: &CoreHost, through: meguri_engine::Seq) {
    let collection = host.collection().unwrap();
    let (identity, seq) = {
        let reader = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = reader.snapshot().unwrap();
        (snapshot.wal_identity(), snapshot.seq())
    };
    assert!(seq >= through);
    assert_eq!(identity.generation(), collection.generation());
    collection.flush_wal(TIMEOUT).unwrap();
    let meguri_engine::WalCoverage::Covered(receipt) = collection.wait_wal_covered(identity, seq, TIMEOUT).unwrap()
        else { panic!("successful current-open WAL coverage is required before validation") };
    assert_eq!(receipt.identity(), identity);
    assert!(receipt.through() >= seq);
}

fn assert_current_open_covered(host: &CoreHost, seq: meguri_engine::Seq) {
    let collection = host.collection().unwrap();
    let identity = collection.reader(ReaderRole::Cloud).unwrap().snapshot().unwrap().wal_identity();
    let meguri_engine::WalCoverage::Covered(receipt) = collection.wait_wal_covered(identity, seq, Duration::ZERO).unwrap()
        else { panic!("this boundary requires an existing current-open WAL receipt") };
    assert_eq!(receipt.identity(), identity);
    assert!(receipt.through() >= seq);
}

fn assert_reconciled_source(host: &CoreHost, source: &EntityFacts) -> EntityFacts {
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let reader = collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = reader.snapshot().unwrap();
    let stored = runtime.stored_state_for_testing().unwrap();
    let status = runtime.status().unwrap();
    assert!(status.scan_completed_this_open);
    assert!(!status.scan_required && !status.scan_running && !status.tracking_stopped);
    assert!(runtime.row_classification_details().unwrap().0);
    let cursor = &stored.coverage.as_ref().unwrap().cursor;
    assert_eq!((cursor.seq, cursor.generation), (snapshot.seq().0, snapshot.generation().0));
    let identity = snapshot.wal_identity();
    let meguri_engine::WalCoverage::Covered(receipt) = collection.wait_wal_covered(identity, snapshot.seq(), Duration::ZERO).unwrap()
        else { panic!("reconciled source requires an existing current-open WAL receipt") };
    assert_eq!(receipt.identity(), identity);
    assert!(receipt.through() >= snapshot.seq());
    // Equal payloads retain every fact except provenance refreshed by the scan.
    let mut expected = source.clone();
    expected.observer_open = stored.open_counter;
    expected.observed_seq = cursor.seq;
    expected.observed_generation = cursor.generation;
    assert_eq!(runtime.entity_facts(&source.name).unwrap().unwrap(), expected);
    expected
}

fn assert_written(host: &CoreHost, expected: &Notetype) {
    let stored = host.collection().unwrap().reader(ReaderRole::Query).unwrap()
        .notetype(expected.id).unwrap().unwrap();
    // Digest covers all native head/child content, including opaque configs,
    // ordinal keys and template stamps; head stamps are compared separately.
    assert_eq!(meguri_engine::notetypes::AggregateDigest::of(&stored),
        meguri_engine::notetypes::AggregateDigest::of(expected));
    assert_eq!(stored.mtime, expected.mtime);
    assert_eq!(stored.usn, expected.usn);
}

fn assert_unproved(error: RuntimeError, id: NotetypeId) {
    assert!(matches!(error, RuntimeError::Tracking(cloud::Error::ValidatedUnprovedLayout { notetype })
        if notetype == id.0));
}

fn validate_choice(host: &CoreHost, service: &mut Service<SeededRandom>, binding: &cloud::Binding,
    source: &EntityFacts, sdk: &str, before: &Retained,
) {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let rows = collection_rows(host);
    let book = bookkeeping(host);
    let runs = runtime.incoming_runs();
    let id = NotetypeId(nt(source).id);
    for _ in 0..2 {
        // Reconcile must try the retained and incoming authority against the
        // actual rows; a generic error is never accepted as this finding.
        assert_unproved(runtime.reconcile_binding(&collection, &host.media().unwrap(), TIMEOUT).unwrap_err(), id);
        runtime.prepare_binding(&collection, &host.media().unwrap(), TIMEOUT).unwrap();
        assert!(matches!(host.engine().state(), meguri_engine::LifecycleState::Ready { .. }));
        assert!(!runtime.status().unwrap().tracking_stopped);
        assert!(runtime.status().unwrap().scan_required);
        let projection = runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap();
        assert_eq!(projection.facts.fence, cloud::policy::Fence::ChooseCollection);
        assert!(matches!(runtime.require_collection_sending(), Err(RuntimeError::Incomplete(Incomplete::StructuralLayout))));
        assert!(runtime.validate_sdk(sdk).is_err());
        assert!(runtime.register_sdk(true).is_err());
        let batch = runtime.build_collection_batch(&runtime.instance().unwrap(), &collection,
            &BTreeSet::from([source.name.clone()])).unwrap();
        assert_eq!(batch.id, 0);
        assert!(batch.saves.is_empty());
        assert!(matches!(runtime.drain_incoming(&collection, TIMEOUT),
            Err(RuntimeError::Incomplete(Incomplete::StructuralLayout))));
        let preview = actual_preview(host, service, &scope());
        assert_eq!(preview.decision, ConnectDecision::ChooseCollection);
        assert_eq!(preview.active_epoch, Some(binding.facts.epoch));
        assert!(preview.resume_binding.is_none());
        assert!(!preview.unsent_counts_exact);
        runtime.cancel_connection_preview_id(preview.id).unwrap();
        assert_eq!(retained(host, binding), *before, "source/base/ACK, marks, flights and incoming ownership survive");
        assert_eq!(runtime.entity_facts(&source.name).unwrap().as_ref(), Some(source));
        assert_eq!(collection_rows(host), rows);
        assert_eq!(bookkeeping(host), book);
        assert_eq!(runtime.incoming_runs(), runs);
    }
}

// Leave real pending and handed work on the published type. These images must
// survive the incoming replacement; zero/empty bookkeeping would miss resets.
fn hand_pending_type(host: &CoreHost) -> EntityFacts {
    let source = source_type(host);
    let collection = host.collection().unwrap();
    let mut draft = meguri_engine::notetypes::Draft::of(collection.reader(ReaderRole::Query).unwrap()
        .notetype(NotetypeId(nt(&source).id)).unwrap().unwrap());
    draft.config.css = ".card { text-align: start; }".into();
    meguri_engine::notetypes::save(&collection, draft, Default::default()).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    runtime.require_collection_sending().unwrap();
    let batch = runtime.build_collection_batch(&runtime.instance().unwrap(), &collection,
        &BTreeSet::from([source.name.clone()])).unwrap();
    assert_eq!(batch.saves.len(), 1);
    let pending = runtime.entity_facts(&source.name).unwrap().unwrap();
    assert!(pending.mark.is_some() && !pending.in_flight.is_empty());
    assert_eq!(pending.base_payload.as_ref(), Some(&source.payload));
    assert_eq!(pending.acked_payload, source.acked_payload);
    pending
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Observation { Live, JournalFailure, WalFailure, WholeCallbackLoss, Reopen }

fn unproved_case(observation: Observation, change: Replacement) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated_legacy_reversed(&dir);
    let host = open_prior_icloud_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(481)).unwrap();
    let other = if matches!(observation, Observation::JournalFailure | Observation::WalFailure) {
        let collection = host.collection().unwrap();
        let note = collection.reader(ReaderRole::Query).unwrap().row_page(Table::Notes, None).unwrap().remove(0);
        let target = NotetypeId(cloud::row_payload::int(&note, 2).unwrap());
        let added = meguri_engine::notetypes::clone_notetype(&collection, target,
            Some("Unrelated native save".into())).unwrap().value.id;
        // changes::prepare orders ids in a BTreeMap. observed_layout returns
        // None for the target, then reads this type's journal before-image;
        // only AFTER that entire prepass does ordinary projection refuse target.
        assert!(target.0 < added.0);
        Some(added)
    } else { None };
    let binding = publish_actual_collection(&host, &mut service);
    let source = hand_pending_type(&host);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let (sdk, _, _) = runtime.register_sdk(true).unwrap();
    runtime.validate_sdk(&sdk).unwrap();
    let mut incoming = nt(&source);
    incoming.name = b"Queued cloud metadata".to_vec();
    let saved = save_server(&mut service, &binding, &Payload::Notetype(incoming), source.origin, None);
    runtime.receive_collection_records(&sdk, &[saved]).unwrap();
    let before = retained(&host, &binding);
    assert_eq!(before.inbox.len(), 1);
    let book = bookkeeping(&host);
    let id = NotetypeId(nt(&source).id);
    let aggregate = replacement(&host, id, change);
    let expected = aggregate.clone();
    let note = collection.reader(ReaderRole::Query).unwrap().row_page(Table::Notes, None).unwrap().remove(0);
    let note = NoteId(cloud::row_payload::int(&note, 0).unwrap());
    collection.pause_automatic_wal_flush_for_testing(true);
    match observation {
        Observation::JournalFailure => runtime.fail_next_capture_journal_reads_for_testing(),
        Observation::WalFailure => collection.fail_wal_sync_after_next_commit_for_testing(),
        Observation::WholeCallbackLoss => runtime.arm_capture_loss_for_testing(note.0).unwrap(),
        _ => {},
    }
    let committed = accept_with_other(&host, aggregate,
        (observation == Observation::WholeCallbackLoss).then_some(note), other);
    if let Some(other) = other {
        assert_eq!(committed.changes.notetypes.iter().map(|n| n.id).collect::<Vec<_>>(), vec![id, other]);
        let stored = collection.reader(ReaderRole::Query).unwrap().notetype(other).unwrap().unwrap();
        assert_eq!(stored.config.css, ".card { text-align: end; }");
    }
    assert_written(&host, &expected);
    let proof = runtime.capture_proof(committed.seq, committed.generation).expect("real registered observer ran");
    assert!(proof.attempted && !proof.persisted);
    assert_eq!(retained(&host, &binding), before);
    assert_eq!(bookkeeping(&host), book);
    let committed_rows = collection_rows(&host);
    if matches!(observation, Observation::JournalFailure | Observation::WalFailure) {
        // Assert the original generic failure FIRST. Only a later successful
        // current-open validation is allowed to establish layout ambiguity.
        assert!(runtime.status().unwrap().tracking_stopped, "real unrelated-report capture must reach the armed fault");
        if observation == Observation::JournalFailure {
            assert_current_open_covered(&host, committed.seq);
        } else {
            let identity = collection.reader(ReaderRole::Cloud).unwrap().snapshot().unwrap().wal_identity();
            assert!(!matches!(collection.wait_wal_covered(identity, committed.seq, Duration::ZERO).unwrap(),
                meguri_engine::WalCoverage::Covered(_)), "failed native authority flush supplied no receipt");
        }
        assert_ne!(runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap()
            .facts.fence, cloud::policy::Fence::ChooseCollection);
        assert!(matches!(host.begin_cloud_connection_preview(), Err(RuntimeError::Tracking(cloud::Error::Database))));
        assert!(runtime.require_collection_sending().is_err());
        assert!(runtime.register_sdk(true).is_err());
        assert_eq!(retained(&host, &binding), before);
    } else if observation == Observation::Live {
        // A report-free refusal need not flush while automatic flushing is
        // paused. Explicit coverage below precedes subsequent validation.
        assert_eq!(runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap()
            .facts.fence, cloud::policy::Fence::ChooseCollection);
    } else if observation == Observation::WholeCallbackLoss {
        assert_ne!(runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap()
            .facts.fence, cloud::policy::Fence::ChooseCollection);
        assert!(committed.changes.notes.iter().any(|n| n.id == note));
        let changed = collection.reader(ReaderRole::Query).unwrap().note(note).unwrap().unwrap();
        assert!(changed.field_values().next().unwrap().ends_with(" — incoming content"));
        assert_eq!(changed.notetype_id, id);
    }
    assert_eq!(collection_rows(&host), committed_rows, "the original error/refusal preserves all committed rows");
    cover_current_open(&host, committed.seq);
    collection.pause_automatic_wal_flush_for_testing(false);
    let rows = collection_rows(&host);
    if observation != Observation::Reopen {
        validate_choice(&host, &mut service, &binding, &source, &sdk, &before);
    }
    host.close().unwrap(); drop(collection); drop(host);
    let host = open_prior_icloud_host(&dir, keys());
    prove_reopened_binding(&host, &binding);
    let collection = host.collection().unwrap();
    cover_current_open(&host, collection.last_seq());
    assert_eq!(collection_rows(&host), rows);
    assert_eq!(bookkeeping(&host), book);
    validate_choice(&host, &mut service, &binding, &source, &sdk, &before);
    host.close().unwrap();
}

#[test]
fn accepted_sync_template_swap_live_refuses_without_inventing_authority() {
    unproved_case(Observation::Live, Replacement::TemplateSwap);
}
#[test]
fn accepted_sync_template_rename_capture_error_precedes_validated_choice() {
    unproved_case(Observation::JournalFailure, Replacement::TemplateRename);
}
#[test]
fn accepted_sync_template_swap_wal_error_precedes_validated_choice() {
    unproved_case(Observation::WalFailure, Replacement::TemplateSwap);
}
#[test]
fn accepted_sync_template_swap_whole_callback_loss_reconciles_without_map() {
    unproved_case(Observation::WholeCallbackLoss, Replacement::TemplateSwap);
}
#[test]
fn accepted_sync_template_swap_fresh_reopen_cannot_recover_unknown_map() {
    unproved_case(Observation::Reopen, Replacement::TemplateSwap);
}

#[test]
fn accepted_sync_metadata_without_identity_authority_refuses_and_preserves_work() {
    // Matching names and unchanged arities supply no origin map for changed
    // head metadata. The same refusal/preservation gates apply as for a swap.
    unproved_case(Observation::Live, Replacement::Metadata);
}

fn prove_current_send_gate(host: &CoreHost, service: &mut Service<SeededRandom>,
    binding: &cloud::Binding, phase: &str,
) -> String {
    let runtime = host.cloud_runtime().unwrap();
    let status = runtime.status().unwrap();
    assert_eq!(status.binding.as_ref(), Some(binding), "{phase}: retained binding fence");
    assert!(status.identity_verified && status.stored_usable, "{phase}: current-open account/file authority");
    assert!(!status.tracking_stopped && !status.scan_required && status.scan_completed_this_open,
        "{phase}: reconciliation must precede SDK/root admission");
    match runtime.require_collection_sending() {
        Ok(()) => {}, // Publication/retained current-open proof already authorizes this gate.
        Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending)) => {
            runtime.start_sdk_instance(&runtime.instance().unwrap())
                .unwrap_or_else(|e| panic!("{phase}: root caller preconditions: {e:?}"));
            eprintln!("{phase}: fetching current-open Active because the send gate requires root confirmation");
            confirm_current_service_active(host, service);
        }
        Err(error) => panic!("{phase}: unexpected send gate before root fetch: {error:?}"),
    }
    runtime.require_collection_sending().unwrap_or_else(|e| panic!("{phase}: send gate: {e:?}"));
    let (sdk, _, _) = runtime.register_sdk(true).unwrap_or_else(|e| panic!("{phase}: SDK registration: {e:?}"));
    runtime.validate_sdk(&sdk).unwrap_or_else(|e| panic!("{phase}: current-open SDK: {e:?}"));
    sdk
}

#[test]
fn accepted_sync_exact_admitted_layout_live_covers_wal_before_admission() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated_legacy_reversed(&dir);
    let host = open_prior_icloud_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(482)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let source = source_type(&host);
    let runtime = host.cloud_runtime().unwrap();
    let original = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = &original.payload else { panic!("synthetic note") };
    let id = NoteId(note.id);
    let collection = host.collection().unwrap();
    let aggregate = collection.reader(ReaderRole::Query).unwrap()
        .notetype(NotetypeId(nt(&source).id)).unwrap().unwrap();
    let expected = aggregate.clone();
    let book = bookkeeping(&host);
    collection.pause_automatic_wal_flush_for_testing(true);
    let result = collection.run(incoming_spec(), move |ctx| {
        ctx.put_accepted_sync_notetype(&aggregate)?;
        change_note_content(ctx.scope(), id)?;
        assert!(ctx.scope().journal().unwrap().structure_report().is_none());
        assert!(ctx.scope().journal().unwrap().identity_layout(aggregate.id.0).is_none());
        Ok(())
    }).unwrap();
    assert_eq!(result.changes.source, OpSource::SyncIncoming);
    assert!(result.changes.notetypes.is_empty());
    assert!(result.changes.notes.iter().any(|n| n.id == id));
    assert_written(&host, &expected);
    let proof = runtime.capture_proof(result.seq, result.generation).unwrap();
    assert!(proof.attempted && proof.persisted);
    let captured = runtime.entity_facts(&original.name).unwrap().unwrap();
    assert_ne!(captured.payload, original.payload);
    assert_eq!(captured.payload, row(&host, &original.payload, &nt(&source)));
    assert_eq!(captured.origin, original.origin);
    assert_eq!(captured.base_payload.as_ref(), Some(&original.payload));
    assert_eq!(captured.acked_payload, original.acked_payload);
    assert_eq!(captured.acked_rev, original.acked_rev);
    assert!(captured.mark.is_some());
    assert_eq!(runtime.entity_facts(&source.name).unwrap().unwrap(), source);
    // Capture of this ordinary note edit need not flush. With automatic
    // flushing paused, the batch builder must wait for a real receipt before
    // handing any flight out; explicit current-open coverage then admits it.
    let sdk = prove_current_send_gate(&host, &mut service, &binding, "live exact admission");
    let before_handoff = retained(&host, &binding);
    let blocked = runtime.build_collection_batch(&sdk, &collection,
        &BTreeSet::from([original.name.clone()])).unwrap();
    assert_eq!(blocked.id, 0);
    assert!(blocked.saves.is_empty());
    assert_eq!(retained(&host, &binding), before_handoff);
    cover_current_open(&host, result.seq);
    assert_current_open_covered(&host, result.seq);
    let batch = runtime.build_collection_batch(&sdk, &collection,
        &BTreeSet::from([original.name.clone()])).unwrap();
    assert_ne!(batch.id, 0);
    assert_eq!(batch.saves.len(), 1);
    assert_eq!(bookkeeping(&host), book);
    collection.pause_automatic_wal_flush_for_testing(false);
    host.close().unwrap();
}

#[test]
fn accepted_sync_exact_admitted_layout_survives_whole_callback_loss_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated_legacy_reversed(&dir);
    let host = open_prior_icloud_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(485)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let source = source_type(&host);
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let original = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = &original.payload else { panic!("synthetic note") };
    let id = NoteId(note.id);
    let aggregate = collection.reader(ReaderRole::Query).unwrap()
        .notetype(NotetypeId(nt(&source).id)).unwrap().unwrap();
    let expected = aggregate.clone();
    runtime.arm_capture_loss_for_testing(id.0).unwrap();
    collection.pause_automatic_wal_flush_for_testing(true);
    let committed = collection.run(incoming_spec(), move |ctx| {
        // This exact current aggregate is already admitted. Unlike a name
        // swap, it requires no unknown ordinal transition from the old image.
        ctx.put_accepted_sync_notetype(&aggregate)?;
        change_note_content(ctx.scope(), id)?;
        assert!(ctx.scope().journal().unwrap().structure_report().is_none());
        assert!(ctx.scope().journal().unwrap().identity_layout(aggregate.id.0).is_none());
        Ok(())
    }).unwrap();
    assert_eq!(committed.changes.source, OpSource::SyncIncoming);
    assert!(committed.changes.notetypes.is_empty());
    assert!(committed.changes.notes.iter().any(|n| n.id == id));
    let proof = runtime.capture_proof(committed.seq, committed.generation).unwrap();
    assert!(proof.attempted && !proof.persisted);
    assert_written(&host, &expected);
    assert_eq!(runtime.entity_facts(&original.name).unwrap().unwrap(), original);
    cover_current_open(&host, committed.seq);
    collection.pause_automatic_wal_flush_for_testing(false);
    runtime.reconcile_binding(&collection, &host.media().unwrap(), TIMEOUT).unwrap();
    assert_ne!(runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap()
        .facts.fence, cloud::policy::Fence::ChooseCollection);
    prove_current_send_gate(&host, &mut service, &binding, "exact recovery after whole callback loss");
    let recovered = runtime.entity_facts(&original.name).unwrap().unwrap();
    assert_ne!(recovered.payload, original.payload);
    assert_eq!(recovered.origin, original.origin);
    assert_eq!(recovered.base_payload.as_ref(), Some(&original.payload));
    assert_eq!(recovered.acked_payload, original.acked_payload);
    assert_eq!(recovered.acked_rev, original.acked_rev);
    assert_reconciled_source(&host, &source);
    let rows = collection_rows(&host);
    host.close().unwrap(); drop(collection); drop(host);
    let host = open_prior_icloud_host(&dir, keys());
    prove_reopened_binding(&host, &binding);
    cover_current_open(&host, host.collection().unwrap().last_seq());
    host.prepare_cloud_runtime(TIMEOUT).unwrap();
    prove_current_send_gate(&host, &mut service, &binding, "exact recovery after fresh reopen");
    let runtime = host.cloud_runtime().unwrap();
    assert_eq!(collection_rows(&host), rows);
    assert_eq!(runtime.entity_facts(&source.name).unwrap().unwrap().payload, source.payload);
    let reopened = runtime.entity_facts(&original.name).unwrap().unwrap();
    assert_eq!(reopened.payload, recovered.payload);
    assert_eq!(reopened.origin, recovered.origin);
    assert_eq!(reopened.base_payload, recovered.base_payload);
    assert_eq!(reopened.acked_payload, recovered.acked_payload);
    assert_eq!(reopened.acked_rev, recovered.acked_rev);
    assert_eq!(reopened.mark, recovered.mark);
    assert_eq!(reopened.in_flight, recovered.in_flight);
    host.close().unwrap();
}

#[test]
fn accepted_sync_incoming_intent_worker() {
    use meguri_model::faultpoint::{self, Fault};
    let Some(root) = std::env::var_os("MEGURI_NORMAL_AGGREGATE_INTENT_ROOT") else { return; };
    let root = std::path::PathBuf::from(root);
    let dir = root.join("device");
    generated_legacy_reversed(&dir);
    let host = open_prior_icloud_host(&dir, keys());
    let mut service = Service::open(&root, Config::default(), SeededRandom::new(483)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let source = hand_pending_type(&host);
    let mut incoming = nt(&source);
    incoming.name = b"Intent-owned cloud metadata".to_vec();
    let saved = save_server(&mut service, &binding, &Payload::Notetype(incoming), source.origin, None);
    let runtime = host.cloud_runtime().unwrap();
    runtime.receive_collection_records(&start(&host), &[saved]).unwrap();
    // This aborts before collection COMMIT. It creates an actual validated
    // ordinary intent, without replacing the aggregate through CloudIncoming.
    faultpoint::arm("cloud.apply.after_intent", Fault::Abort);
    runtime.drain_incoming(&host.collection().unwrap(), TIMEOUT).unwrap();
    panic!("real incoming intent boundary was not reached");
}

#[test]
fn accepted_sync_refusal_preserves_actual_intent_inbox_base_ack_and_flights() {
    use std::os::unix::process::ExitStatusExt;
    let temp = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "incoming_notetype::normal_transfer::accepted_sync_incoming_intent_worker", "--nocapture"])
        .env("MEGURI_NORMAL_AGGREGATE_INTENT_ROOT", temp.path()).status().unwrap();
    assert_eq!(status.signal(), Some(6));
    let dir = temp.path().join("device");
    let host = open_prior_icloud_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    prove_reopened_binding(&host, &binding);
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(483)).unwrap();
    let source = source_type(&host);
    assert!(source.mark.is_some() && !source.in_flight.is_empty() && source.base_payload.is_some());
    let before = retained(&host, &binding);
    assert_eq!(before.intents.len(), 1);
    let intent = &before.intents[0];
    assert_eq!(intent.binding, binding.id);
    assert!(intent.result_seq.is_none() && intent.media.is_none());
    assert_eq!(intent.keys.len(), 1);
    assert_eq!(intent.keys[0].name, source.name);
    // Ordinary incoming keys retain the exact before digest, not a redundant
    // before-body. The final after-body and both digests certify this change.
    let native = current_type(&host, &nt(&source));
    assert_eq!(native, source.payload);
    let before_digest = source.payload.row_digest().unwrap();
    assert_eq!(before_digest, native.row_digest().unwrap());
    assert_eq!(intent.keys[0].before_digest, Some(before_digest));
    let after = intent.keys[0].after_payload.as_ref().expect("validated final after-payload");
    assert_ne!(after, &source.payload);
    assert!(!after.encode().unwrap().is_empty());
    assert_eq!(intent.keys[0].after_digest, Some(after.row_digest().unwrap()));
    assert_ne!(intent.keys[0].after_digest, intent.keys[0].before_digest);
    assert_eq!(intent.inbox_ids, before.inbox.iter().map(|e| e.id).collect::<Vec<_>>());
    let id = NotetypeId(nt(&source).id);
    let expected = replacement(&host, id, Replacement::TemplateRename);
    let committed = accept(&host, expected.clone(), None);
    assert_written(&host, &expected);
    assert_eq!(retained(&host, &binding), before);
    cover_current_open(&host, committed.seq);
    validate_choice(&host, &mut service, &binding, &source, &runtime.instance().unwrap(), &before);
    let rows = collection_rows(&host);
    host.close().unwrap(); drop(host);
    let host = open_prior_icloud_host(&dir, keys());
    prove_reopened_binding(&host, &binding);
    cover_current_open(&host, host.collection().unwrap().last_seq());
    validate_choice(&host, &mut service, &binding, &source,
        &host.cloud_runtime().unwrap().instance().unwrap(), &before);
    assert_eq!(collection_rows(&host), rows);
    host.close().unwrap();
}

#[test]
fn accepted_sync_callback_loss_account_error_is_not_validated_ambiguity() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated_legacy_reversed(&dir);
    let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
        cloud::OpaqueBytes(scope().account.into_bytes())))));
    let host = open_prior_icloud_host_with(&dir, keys(), account.clone(), scope());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(484)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let source = source_type(&host);
    let runtime = host.cloud_runtime().unwrap();
    let before = retained(&host, &binding);
    let collection = host.collection().unwrap();
    let Payload::Note(note) = &before.entities.iter().find(|e| e.kind == codec::Kind::Note).unwrap().payload
        else { panic!("synthetic note") };
    runtime.arm_capture_loss_for_testing(note.id).unwrap();
    // Lose a content callback while the aggregate still has exact admitted
    // authority. Recovery must clear the generic capture failure legitimately
    // before begin_connection_preview can reach its account provider.
    let aggregate = collection.reader(ReaderRole::Query).unwrap()
        .notetype(NotetypeId(nt(&source).id)).unwrap().unwrap();
    let expected = aggregate.clone();
    let id = NoteId(note.id);
    let result = collection.run(incoming_spec(), move |ctx| {
        ctx.put_accepted_sync_notetype(&aggregate)?;
        change_note_content(ctx.scope(), id)?;
        assert!(ctx.scope().journal().unwrap().structure_report().is_none());
        assert!(ctx.scope().journal().unwrap().identity_layout(aggregate.id.0).is_none());
        Ok(())
    }).unwrap();
    assert!(result.changes.notes.iter().any(|n| n.id == id));
    assert!(result.changes.notetypes.is_empty());
    let proof = runtime.capture_proof(result.seq, result.generation).unwrap();
    assert!(proof.attempted && !proof.persisted);
    assert_written(&host, &expected);
    assert!(runtime.status().unwrap().tracking_stopped);
    assert_eq!(retained(&host, &binding), before);
    let lost_rows = collection_rows(&host);
    let book = bookkeeping(&host);
    assert!(matches!(host.begin_cloud_connection_preview(), Err(RuntimeError::Tracking(cloud::Error::Database))));
    cover_current_open(&host, result.seq);
    runtime.reconcile_binding(&collection, &host.media().unwrap(), TIMEOUT).unwrap();
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert_eq!(collection_rows(&host), lost_rows);
    assert_eq!(bookkeeping(&host), book);
    let source = assert_reconciled_source(&host, &source);
    let recovered = retained(&host, &binding);
    let original_note = before.entities.iter().find(|e| e.kind == codec::Kind::Note &&
        matches!(&e.payload, Payload::Note(n) if n.id == id.0)).unwrap();
    let recovered_note = runtime.entity_facts(&original_note.name).unwrap().unwrap();
    assert_ne!(recovered_note.payload, original_note.payload);
    assert_eq!(recovered_note.origin, original_note.origin);
    assert_eq!(recovered_note.base_payload.as_ref(), Some(&original_note.payload));
    assert_eq!(recovered_note.acked_payload, original_note.acked_payload);
    assert_eq!(recovered_note.acked_rev, original_note.acked_rev);
    *account.0.lock().unwrap() = IdentityStatus::TemporarilyUnavailable;
    assert!(matches!(host.begin_cloud_connection_preview(),
        Err(RuntimeError::Publication(PublicationError::AccountUnavailable))));
    assert_ne!(runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap()
        .facts.fence, cloud::policy::Fence::ChooseCollection);
    assert_eq!(retained(&host, &binding), recovered);
    assert_eq!(collection_rows(&host), lost_rows);
    assert_eq!(bookkeeping(&host), book);
    // The provider error classifies no map. After restoring it, make a separate
    // unknown-map replacement and validate that specific committed aggregate.
    *account.0.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes()));
    let expected = replacement(&host, NotetypeId(nt(&source).id), Replacement::TemplateSwap);
    let result = accept(&host, expected.clone(), None);
    assert_written(&host, &expected);
    assert_eq!(retained(&host, &binding), recovered);
    cover_current_open(&host, result.seq);
    let rows = collection_rows(&host);
    validate_choice(&host, &mut service, &binding, &source, &runtime.instance().unwrap(), &recovered);
    assert_eq!(collection_rows(&host), rows);
    host.close().unwrap();
}
