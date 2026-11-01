//! Test-only exports (KTD18, R1967), compiled only with the `test-hooks`
//! feature: Debug and Profile builds, never Release or AppStore. CoreBridge
//! never names them; the MeguriCore Swift tests call them through the
//! generated bindings. Like every export they exist on every slice.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use meguri_engine::{EventKind, Generation};

use crate::cancel::CancelToken;
use crate::error::CoreError;
use crate::events::{self, CoreEventKind};
use crate::jobs::{self, JobHandle, JobOutcome};
use crate::{callback, changes, runtime};

#[path = "test_hooks_cloud_asset_reads.rs"]
mod cloud_asset_read_fixtures;

/// Codec-authored remote pa over the synthetic adapter fixture's real row and
/// binding. This supplies bytes only; fetched/policy ports must issue the work.
#[derive(Clone, uniffi::Record)]
pub struct CloudAssetReadRemoteFixture {
    pub record: crate::sync::CloudRecord,
    pub payload: Vec<u8>,
    pub note_id: i64,
    pub fields: Vec<u8>,
}

#[uniffi::export]
pub fn test_cloud_asset_read_remote_fixture(ordinal: u32) -> Result<CloudAssetReadRemoteFixture, CoreError> {
    callback::enter("test_cloud_asset_read_remote_fixture");
    cloud_asset_read_fixtures::remote(ordinal)
}

#[derive(Clone, uniffi::Record)]
pub struct CloudAssetReadFixtureObservation {
    pub retry_count: Option<u32>,
    pub owned_count: u64,
    pub row_fields: Option<Vec<u8>>,
    pub wal_receipts: u64,
    pub changed_by_incoming: bool,
}

#[uniffi::export]
pub fn test_cloud_asset_read_fixture_observation(
    name: String,
    note_id: i64,
) -> Result<CloudAssetReadFixtureObservation, CoreError> {
    callback::enter("test_cloud_asset_read_fixture_observation");
    cloud_asset_read_fixtures::observe(&name, note_id)
}

/// Fail the next actual admission-accounting transaction after native enqueue.
#[uniffi::export]
pub fn test_cloud_asset_read_fail_next_accounting() -> Result<(), CoreError> {
    callback::enter("test_cloud_asset_read_fail_next_accounting");
    meguri_sync::cloud::testing::arm_tracking_fault(
        "cloud.tracking.commit.asset_read",
        meguri_sync::cloud::testing::TrackingFault::Io,
    )
    .map_err(crate::sync::cloud_error)
}

/// Returns `error`, so Swift can check how each error crosses the bridge.
#[uniffi::export]
pub fn test_return_error(error: CoreError) -> Result<(), CoreError> {
    callback::enter("test_return_error");
    Err(error)
}

/// Panics with `message`; UniFFI catches it and Swift gets an internal error.
#[uniffi::export]
pub fn test_panic(message: String) -> Result<(), CoreError> {
    callback::enter("test_panic");
    panic!("{message}");
}

/// Sleeps in 1 ms steps until `token` is cancelled (`Err(Cancelled)`) or
/// `max_iterations` steps have passed (`Ok(steps)`).
#[uniffi::export]
pub fn test_cancellable_loop(token: Arc<CancelToken>, max_iterations: u64) -> Result<u64, CoreError> {
    callback::enter("test_cancellable_loop");
    for _ in 0..max_iterations {
        token.check()?;
        thread::sleep(Duration::from_millis(1));
    }
    Ok(max_iterations)
}

/// Publishes `per_producer` `JobFinished` events (ids `(producer + 1) *
/// 1_000_000 + index`, above any real job id a test reaches; outcome
/// `Succeeded`) from each of `producers` threads named
/// `meguri-test-producer-<n>`, and returns once all are queued.
#[uniffi::export]
pub fn test_emit_events(producers: u32, per_producer: u32) -> Result<(), CoreError> {
    callback::enter("test_emit_events");
    let threads: Vec<_> = (0..producers)
        .map(|producer| {
            thread::Builder::new()
                .name(format!("meguri-test-producer-{producer}"))
                .spawn(move || -> Result<(), CoreError> {
                    for index in 0..per_producer {
                        let id = (u64::from(producer) + 1) * 1_000_000 + u64::from(index);
                        events::publish(
                            0,
                            None,
                            CoreEventKind::JobFinished {
                                id,
                                outcome: JobOutcome::Succeeded,
                            },
                        )?;
                    }
                    Ok(())
                })
                .map_err(|e| CoreError::Internal {
                    message: format!("cannot start a producer: {e}"),
                })
        })
        .collect::<Result<_, _>>()?;
    for producer in threads {
        producer.join().map_err(|p| CoreError::from_panic(p.as_ref()))??;
    }
    Ok(())
}

/// Publishes one `Changes` event with `changes::sample()`'s change set
/// (every list filled; its values are listed there), as the engine's
/// executor would, at generation 0, and returns its `seq`.
#[uniffi::export]
pub fn test_emit_changes() -> Result<u64, CoreError> {
    callback::enter("test_emit_changes");
    let stream = runtime::engine()?.stream();
    Ok(stream
        .publish(Generation(0), None, EventKind::Changes(Arc::new(changes::sample())))
        .0)
}

/// Starts a job that reports progress `reports_per_second` times a second for
/// `duration_ms` (or until cancelled), then ends with `fail_with` or success.
#[uniffi::export]
pub fn test_start_job(
    duration_ms: u64,
    reports_per_second: u32,
    fail_with: Option<CoreError>,
) -> Result<JobHandle, CoreError> {
    callback::enter("test_start_job");
    let step = Duration::from_secs(1) / reports_per_second.max(1);
    let steps =
        u64::try_from(Duration::from_millis(duration_ms).as_nanos() / step.as_nanos().max(1)).map_err(|_| {
            CoreError::InvalidArgument {
                message: "duration too long".into(),
            }
        })?;
    // Batch high-rate test reports to avoid per-report sub-ms sleep rounding in the VM.
    // Keep nominal pacing (including the final sleep); production throttling is unchanged.
    let batch_size = (Duration::from_millis(5).as_nanos() / step.as_nanos().max(1)).clamp(1, 64) as u32;
    jobs::start("test", 0, move |ctx| {
        let mut done = 0;
        while done < steps {
            let batch_len = (steps - done).min(u64::from(batch_size)) as u32;
            for _ in 0..batch_len {
                ctx.cancel.check()?;
                ctx.progress.report(done, Some(steps));
                done += 1;
            }
            thread::sleep(step * batch_len);
        }
        fail_with.map_or(Ok(()), Err)
    })
}

/// Synthetic empty collection for exported-service tests; no production opener.
#[uniffi::export]
pub fn test_bootstrap_collection(location: crate::collection::CollectionLocation) -> Result<(), CoreError> {
    callback::enter("test_bootstrap_collection");
    let clock = meguri_model::clock::FixedClock::new(1_894_701_600_000, meguri_model::clock::UtcOffset::UTC);
    meguri_engine::bootstrap::create(
        &std::path::Path::new(&location.collection_directory).join("collection.anki2"),
        &clock,
        &meguri_model::rng::SeededRandom::new(37),
    )
    .map_err(CoreError::from)
}

/// Rust writer-gate fixtures use the runtime owned by the real exports.
/// This helper is not a UniFFI export and adds no generated measurement API.
pub fn collection_for_testing() -> Result<meguri_engine::Collection, CoreError> {
    runtime::host()?.collection().map_err(Into::into)
}

/// Rust integration fixtures share the exports' engine, slot and maintenance
/// drain. Not a UniFFI export; never opens a second process engine.
pub fn engine_for_testing() -> Result<meguri_engine::Engine, CoreError> {
    runtime::engine().cloned()
}

// Swift cannot arm Rust function-pointer faults. This narrow KTD18 adapter
// exposes the existing restore boundaries, never a replacement result or event.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum MaintenanceTestBoundary {
    BeforeRename,
    AfterActivation,
}
#[derive(Default)]
struct MaintenanceGateState {
    reached: bool,
    released: bool,
}
#[derive(uniffi::Object)]
pub struct MaintenanceTestGate {
    state: std::sync::Mutex<MaintenanceGateState>,
    changed: std::sync::Condvar,
    point: &'static str,
    cleared: std::sync::atomic::AtomicBool,
}
static MAINTENANCE_GATE: std::sync::Mutex<Option<std::sync::Weak<MaintenanceTestGate>>> = std::sync::Mutex::new(None);
fn maintenance_boundary() {
    let gate = MAINTENANCE_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(std::sync::Weak::upgrade);
    if let Some(gate) = gate {
        let mut state = gate.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.reached = true;
        gate.changed.notify_all();
        drop(
            gate.changed
                .wait_timeout_while(state, Duration::from_secs(20), |s| !s.released)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}
#[uniffi::export]
pub fn test_arm_maintenance_boundary(
    boundary: MaintenanceTestBoundary,
    fail_reevaluation: bool,
) -> Result<Arc<MaintenanceTestGate>, CoreError> {
    callback::enter("test_arm_maintenance_boundary");
    use meguri_model::faultpoint::{self, Fault};
    let mut active = MAINTENANCE_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.as_ref().and_then(std::sync::Weak::upgrade).is_some() {
        return Err(CoreError::Busy {
            running_op: "maintenance fault fixture".into(),
        });
    }
    let point = match boundary {
        MaintenanceTestBoundary::BeforeRename => "replace.after_check",
        MaintenanceTestBoundary::AfterActivation => "replace.after_dir_sync",
    };
    let gate = Arc::new(MaintenanceTestGate {
        state: std::sync::Mutex::new(MaintenanceGateState::default()),
        changed: std::sync::Condvar::new(),
        point,
        cleared: std::sync::atomic::AtomicBool::new(false),
    });
    *active = Some(Arc::downgrade(&gate));
    drop(active);
    faultpoint::arm(point, Fault::Callback(maintenance_boundary));
    if fail_reevaluation {
        faultpoint::arm(
            "restore.before_maintenance_reevaluation",
            Fault::IoError(std::io::ErrorKind::Other),
        );
    }
    Ok(gate)
}
#[uniffi::export]
impl MaintenanceTestGate {
    /// The test waits on the query lane; release/cancel remain available on control.
    pub fn wait_until_reached(&self) -> Result<(), CoreError> {
        callback::enter("MaintenanceTestGate.wait_until_reached");
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(20), |s| !s.reached)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.reached {
            Ok(())
        } else {
            Err(CoreError::Internal {
                message: "maintenance boundary was not reached".into(),
            })
        }
    }
    pub fn release(&self) -> Result<(), CoreError> {
        callback::enter("MaintenanceTestGate.release");
        self.unblock();
        Ok(())
    }
    /// Call after drain so the injected reevaluation error is not cleared early.
    pub fn reset(&self) -> Result<(), CoreError> {
        callback::enter("MaintenanceTestGate.reset");
        self.clear();
        Ok(())
    }
}
impl MaintenanceTestGate {
    fn unblock(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .released = true;
        self.changed.notify_all();
    }
    fn clear(&self) {
        self.unblock();
        if self.cleared.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        meguri_model::faultpoint::disarm(self.point);
        meguri_model::faultpoint::disarm("restore.before_maintenance_reevaluation");
        MAINTENANCE_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
impl Drop for MaintenanceTestGate {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Read the real notes b-tree location and flush WAL before the caller closes
/// publicly and damages the on-disk page. This supplies layout, never a verdict.
#[uniffi::export]
pub fn test_maintenance_notes_page_offset() -> Result<u64, CoreError> {
    callback::enter("test_maintenance_notes_page_offset");
    use meguri_storage::{ReaderRole, raw::SqlValue};
    let collection = runtime::host()?.collection()?;
    let reader = collection.reader(ReaderRole::Query)?;
    let integer = |sql: &str| -> Result<i64, CoreError> {
        let rows = reader.execute_raw(sql, &[])?;
        match rows.rows.first().and_then(|row| row.first()) {
            Some(SqlValue::Integer(value)) => Ok(*value),
            _ => Err(CoreError::Internal {
                message: "expected SQLite page metadata".into(),
            }),
        }
    };
    let root = integer("SELECT rootpage FROM sqlite_master WHERE name='notes'")?;
    let size = integer("PRAGMA page_size")?;
    drop(reader);
    let offset = root
        .checked_sub(1)
        .filter(|page| *page > 0)
        .and_then(|page| page.checked_mul(size))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| CoreError::Internal {
            message: "invalid notes page location".into(),
        })?;
    collection
        .submit_maintenance(
            meguri_engine::executor::catalog::ANALYZE,
            Duration::from_secs(1),
            |writer| Ok(writer.execute_raw("PRAGMA wal_checkpoint(TRUNCATE)", &[])?),
        )
        .wait()?;
    Ok(offset)
}

/// Synthetic policy fixtures must declare their publication binding before the
/// first Host open, just like cloud_policy_tests::unbound_fixture. Production
/// start_open deliberately keeps HostConfig's LocalOnly default.
#[uniffi::export]
pub fn test_cloud_start_native_policy_open(
    location: crate::collection::CollectionLocation,
    clock: Arc<dyn crate::collection::HostClock>,
    disk_space: Arc<dyn crate::collection::DiskSpaceProvider>,
    random_seed: u64,
) -> Result<JobHandle, CoreError> {
    callback::enter("test_cloud_start_native_policy_open");
    use crate::collection::{ForeignClock, ForeignSpace};
    use meguri_model::rng::SeededRandom;
    use meguri_sync::cloud::Environment;

    let (inputs, key_bytes) = crate::sync::platform_inputs()
        .ok_or_else(|| crate::sync::cloud_error("native policy fixture requires its synthetic platform"))?;
    if inputs.scope.container.as_str() != "synthetic-u114" || inputs.scope.environment != Environment::Development {
        return Err(CoreError::InvalidArgument {
            message: "native policy fixture requires synthetic-u114 development".into(),
        });
    }
    let reservation = runtime::reserve_open()?;
    let mut config = meguri_host::HostConfig::new(
        location.collection_directory.into(),
        location.backups_directory.into(),
        location.app_group_snapshot.into(),
        Arc::new(ForeignClock(clock)),
        Arc::new(SeededRandom::new(random_seed)),
        Arc::new(ForeignSpace(disk_space)),
    );
    config.manual_transfer_directory = location.manual_transfer_directory.into();
    config.publication_binding = native_policy_fixture_binding();
    config.cloud = Some(inputs);
    config.cloud_key_bytes = key_bytes;
    jobs::start(
        "open native policy fixture",
        runtime::engine()?.generation().0,
        move |job| {
            job.cancel.check()?;
            reservation.open(config)?;
            if job.cancel.is_cancelled()? {
                return Err(CoreError::Unavailable {
                    reason: "open completed after cancellation; collection remains open; close it explicitly".into(),
                });
            }
            Ok(())
        },
    )
}

fn native_policy_fixture_binding() -> meguri_engine::device_state::AccountBinding {
    meguri_engine::device_state::AccountBinding::Connected {
        account: "synthetic-account-A".into(),
        configuration: "synthetic-u114".into(),
        environment: "development".into(),
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyExchangeObservation {
    pub policy_exchange: u64,
    pub mapped_runtime_id: Option<u64>,
    pub captures: std::collections::HashMap<u64, u64>,
    pub completed_ids: Vec<u64>,
    pub active_id: Option<u64>,
}

/// Reads the original RunExchange's existing Driver mapping and Runtime IDs.
/// Never captures, completes, resets or infers an exchange from a count.
#[uniffi::export]
pub fn test_cloud_policy_exchange_observation(
    command: Arc<crate::cloud_policy::CloudPolicyCommand>,
) -> Result<CloudPolicyExchangeObservation, CoreError> {
    callback::enter("test_cloud_policy_exchange_observation");
    let observed = command.exchange_observation_for_testing()?;
    Ok(CloudPolicyExchangeObservation {
        policy_exchange: observed.policy_exchange,
        mapped_runtime_id: observed.captures.get(&observed.policy_exchange).copied(),
        captures: observed.captures.into_iter().collect(),
        completed_ids: observed.completed_ids,
        active_id: observed.active_id,
    })
}

/// Read-only setup assertions over the real publication sidecar and runtime
/// projection. This grants no authority and never repairs an invalid fixture.
#[uniffi::export]
pub fn test_cloud_require_native_policy_baseline() -> Result<(), CoreError> {
    callback::enter("test_cloud_require_native_policy_baseline");
    use meguri_engine::{LifecycleState, device_state::DeviceState};
    use meguri_sync::cloud::{
        BindingState,
        policy::{Fence, Lifecycle, Scan},
    };

    let host = runtime::host()?;
    let collection = host.collection()?;
    if !matches!(host.engine().state(), LifecycleState::Ready { .. }) {
        return Err(crate::sync::cloud_error("native policy fixture must be Ready"));
    }
    let publication = DeviceState::for_collection(collection.path()).publication()?;
    if !publication.is_some_and(|p| p.binding == native_policy_fixture_binding()) {
        return Err(crate::sync::cloud_error(
            "native policy fixture opening binding does not match",
        ));
    }
    let core = host
        .cloud_runtime()
        .ok_or_else(|| crate::sync::cloud_error("native policy fixture has no cloud runtime"))?;
    let projection = core
        .policy_projection(Lifecycle::Ready, collection.generation().0)
        .map_err(crate::sync::cloud_error)?;
    let facts = &projection.facts;
    if facts.fence != Fence::Clear || facts.summary.scan != Scan::None {
        return Err(crate::sync::cloud_error(format!(
            "native policy fixture requires Clear fence and no scan: {:?}, {:?}",
            facts.fence, facts.summary.scan
        )));
    }
    if !facts.binding.as_ref().is_some_and(|b| {
        b.verified
            && b.usable
            && b.state == BindingState::Active
            && b.account.0.as_slice() == b"synthetic-account-A"
            && b.id.0.as_slice() == projection.backend_guard.binding.as_slice()
    }) {
        return Err(crate::sync::cloud_error(
            "native policy fixture requires its matching verified active binding",
        ));
    }
    if projection.engine.is_some() || projection.tracking_stopped || !projection.root_confirmed {
        return Err(crate::sync::cloud_error(
            "native policy fixture requires confirmed baseline before SDK start",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudAdapterBaseline {
    pub record_names: Vec<String>,
    pub deck_ids: Vec<i64>,
    pub review_deck_id: i64,
    pub review_card_id: i64,
    pub review_record_name: String,
    pub collection_zone: String,
    pub incoming_record: crate::sync::CloudRecord,
    pub root_record: crate::sync::CloudRecord,
}
#[uniffi::export]
pub fn test_cloud_prepare_adapter_baseline(
    entity_count: u32,
    local_revision: u64,
    saved_engine_state: Option<Vec<u8>>,
    fail_next_inbox_commit: bool,
    fail_next_state_commit: bool,
) -> Result<CloudAdapterBaseline, CoreError> {
    callback::enter("test_cloud_prepare_adapter_baseline");
    use meguri_model::{
        card::{CardQueue, CardType},
        ids::*,
        revlog::{RevlogEntry, RevlogKind},
        usn::Usn,
    };
    use meguri_storage::{ConfigValue, ReadRows, ReaderRole};
    use meguri_sync::cloud::{entity_codec as codec, workflow::Saved};
    if local_revision != 3 || entity_count == 0 || entity_count > 400 {
        return Err(CoreError::InvalidArgument {
            message: "unsupported adapter baseline size or revision".into(),
        });
    }
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| crate::sync::cloud_error("cloud capture was not composed"))?;
    let collection = host.collection()?;
    let (clock, random) = host.cloud_fixture_sources();
    collection.run(
        meguri_engine::OpSpec::named("adapter baseline admission", meguri_engine::OpSource::Maintenance),
        |ctx| {
            ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
            ctx.scope()
                .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
            Ok(())
        },
    )?;
    let mut decks = Vec::new();
    for i in 0..entity_count {
        decks.push(
            meguri_engine::decks::add(&collection, format!("Synthetic cloud deck {i}"))?
                .value
                .id,
        );
    }
    let review_deck = meguri_engine::decks::add(&collection, "Synthetic cloud review".into())?
        .value
        .id;
    let nt = meguri_engine::notetypes::add(
        &collection,
        meguri_engine::notetypes::NewNotetype {
            name: "Synthetic cloud cards".into(),
            kind: meguri_engine::notetypes::NotetypeKind::Standard,
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![meguri_engine::notetypes::NewTemplate {
                name: "Card".into(),
                front: "{{Front}}".into(),
                back: "{{Back}}".into(),
                ..Default::default()
            }],
            css: None,
        },
    )?
    .value
    .id;
    let note = meguri_engine::notes::add(
        &collection,
        meguri_engine::notes::AddNote {
            notetype_id: nt,
            fields: vec!["Synthetic question".into(), "Synthetic answer".into()],
            tags: vec![],
            deck_id: review_deck,
            guid: None,
            update_add_defaults: false,
        },
        meguri_engine::OpSource::UserUndoable,
    )?
    .value;
    let card = collection.reader(ReaderRole::Query)?.cards_of_note(note.id)?[0].id;
    let prior_review_ms = clock.now_ms() - 86_400_000;
    collection.run(
        meguri_engine::OpSpec::named("adapter baseline due review", meguri_engine::OpSource::Maintenance),
        move |ctx| {
            let mut row = ctx
                .scope()
                .card(card)?
                .ok_or_else(|| meguri_engine::EngineError::Internal {
                    message: "synthetic card absent".into(),
                })?;
            row.card_type = CardType::Review;
            row.queue = CardQueue::Review;
            row.due = -10_000;
            row.interval = 1;
            row.reps = 1;
            row.ease_factor = 2500;
            ctx.scope().put_card_forced(&row)?;
            ctx.scope().insert_revlog(&RevlogEntry {
                id: RevlogId(prior_review_ms),
                card_id: card,
                usn: Usn(-1),
                button: 3,
                interval: 1,
                last_interval: 0,
                ease_factor: 2500,
                answer_millis: 100,
                kind: RevlogKind::Review,
            })?;
            Ok(())
        },
    )?;
    let media = host.media()?;
    media
        .store()
        .register_full(|_, _| true)
        .map_err(crate::sync::cloud_error)?;
    let (_, root, incoming) = core
        .prepare_adapter_baseline_for_testing(&collection, clock.as_ref(), random.as_ref(), saved_engine_state)
        .map_err(crate::sync::cloud_error)?;
    // Two actual commits put every selected deck at revision 3. Capture is
    // already attached to the host's writer; no revisions or marks are patched.
    for (i, id) in decks.iter().enumerate() {
        meguri_engine::decks::rename(&collection, *id, format!("Synthetic second {i}"))?;
        meguri_engine::decks::rename(&collection, *id, format!("Synthetic third {i}"))?;
    }
    core.reconcile_binding(&collection, &media, std::time::Duration::from_secs(1))
        .map_err(crate::sync::cloud_error)?;
    core.accept_root_fetch(
        &core.instance().map_err(crate::sync::cloud_error)?,
        Some(&root),
        true,
        1,
    )
    .map_err(crate::sync::cloud_error)?;
    let names = decks
        .iter()
        .map(|id| {
            codec::entity_name(&[114; 32], codec::Kind::Deck, &codec::id_key(id.0)).map_err(crate::sync::cloud_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let review_name =
        codec::entity_name(&[114; 32], codec::Kind::Card, &codec::id_key(card.0)).map_err(crate::sync::cloud_error)?;
    core.arm_adapter_faults_for_testing(fail_next_inbox_commit, fail_next_state_commit);
    let collection_zone = meguri_sync::cloud::adapter::collection_zone(meguri_sync::cloud::EpochId([114; 16]));
    let record = |r: Saved, record_type: &str, zone: &str| crate::sync::CloudRecord {
        name: r.name,
        envelope: r.envelope,
        system_fields: Some(r.system_fields),
        record_type: Some(record_type.into()),
        zone: Some(zone.into()),
        ..Default::default()
    };
    Ok(CloudAdapterBaseline {
        record_names: names,
        deck_ids: decks.into_iter().map(|id| id.0).collect(),
        review_deck_id: review_deck.0,
        review_card_id: card.0,
        review_record_name: review_name,
        collection_zone: collection_zone.clone(),
        incoming_record: record(incoming, "MGEntity", &collection_zone),
        root_record: record(root, "MGActive", "meguri-root"),
    })
}

/// Interrupt the next real incoming writer after its durable intent.
#[uniffi::export]
pub fn test_cloud_interrupt_next_incoming(cancel: bool) -> Result<(), CoreError> {
    callback::enter("test_cloud_interrupt_next_incoming");
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| crate::sync::cloud_error("cloud capture unavailable"))?;
    core.interrupt_next_incoming_for_testing(if cancel {
        meguri_sync::cloud::testing::ApplyInterruption::Cancel
    } else {
        meguri_sync::cloud::testing::ApplyInterruption::Constraint
    });
    Ok(())
}

/// Valid remote note with an absent notetype: the real apply parks it Reference.
#[uniffi::export]
pub fn test_cloud_reference_wait_record(erev: u64) -> Result<crate::sync::CloudRecord, CoreError> {
    callback::enter("test_cloud_reference_wait_record");
    use meguri_sync::cloud::{entity_codec as codec, row_payload};
    if !(2..=3).contains(&erev) {
        return Err(CoreError::InvalidArgument {
            message: "unsupported synthetic revision".into(),
        });
    }
    let payload = codec::Payload::Note(codec::Note {
        id: 114_502,
        mid: 114_999,
        guid: b"synthetic-waiting".to_vec(),
        flds: erev.to_string().into_bytes(),
        ..Default::default()
    });
    let key = row_payload::payload_key(&payload).map_err(crate::sync::cloud_error)?;
    Ok(crate::sync::CloudRecord {
        name: codec::entity_name(&[114; 32], codec::Kind::Note, &key).map_err(crate::sync::cloud_error)?,
        envelope: codec::Entity::build(
            codec::EntityFields {
                kind: codec::Kind::Note,
                key,
                origin: [51; 16],
                erev,
                generation: None,
                tombstone: false,
                deleted_at_ms: 0,
                writer: [61; 16],
                base_sha256: None,
            },
            &payload.encode().map_err(crate::sync::cloud_error)?,
        )
        .map_err(crate::sync::cloud_error)?
        .encode()
        .map_err(crate::sync::cloud_error)?,
        system_fields: Some(erev.to_be_bytes().to_vec()),
        ..Default::default()
    })
}

/// Observe the actual answered card and history entry through one Query snapshot.
#[uniffi::export]
pub fn test_physical_review_rows(
    card_id: i64,
    revlog_id: i64,
) -> Result<crate::physical_review_rows::PhysicalReviewRows, CoreError> {
    callback::enter("test_physical_review_rows");
    crate::physical_review_rows::observe(&runtime::host()?.collection()?, card_id, revlog_id)
}

/// Real synthetic retained rows only. Production still issues/guards every pass.
#[uniffi::export]
pub fn test_cloud_zone_deletion_fixture(sdk: String) -> Result<Vec<String>, CoreError> {
    callback::enter("test_cloud_zone_deletion_fixture");
    let host = runtime::host()?;
    host.cloud_runtime()
        .ok_or_else(|| crate::sync::cloud_error("cloud fixture unavailable"))?
        .prepare_zone_deletion_fixture_for_testing(&sdk)
        .map_err(crate::sync::cloud_error)
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudZoneDeletionFixtureRow {
    pub zone: String,
    pub due: bool,
}
#[uniffi::export]
pub fn test_cloud_zone_deletion_rows() -> Result<Vec<CloudZoneDeletionFixtureRow>, CoreError> {
    callback::enter("test_cloud_zone_deletion_rows");
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| crate::sync::cloud_error("cloud fixture unavailable"))?;
    Ok(core
        .publication_control_for_testing()
        .map_err(crate::sync::cloud_error)?
        .zones
        .into_iter()
        .map(|(zone, row)| CloudZoneDeletionFixtureRow {
            zone,
            due: row.deletion_due,
        })
        .collect())
}
