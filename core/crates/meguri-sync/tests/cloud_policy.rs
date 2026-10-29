//! P21 tables exercise production policy; Q11 adapter proofs remain separate.
use meguri_sync::cloud::{BindingId, BindingState, OpaqueBytes, policy::*};

fn ready() -> State {
    State {
        settings: DeviceSettings {
            enabled: true,
            paused: false,
            automatic: true,
            media: false,
        },
        account: Account {
            status: AccountStatus::Available,
            identity: Some(OpaqueBytes(vec![1])),
        },
        facts: CoreFacts {
            lifecycle: Lifecycle::Ready,
            generation: 7,
            binding: Some(BindingView {
                id: BindingId([1; 16]),
                state: BindingState::Active,
                account: OpaqueBytes(vec![1]),
                usable: true,
                verified: true,
                publish_step: None,
            }),
            summary: Summary {
                collection_known: true,
                attention_known: true,
                scan: Scan::None,
                ..Default::default()
            },
            ..Default::default()
        },
        reachable: true,
        engine: Some(Engine {
            instance: "sdk1".into(),
            binding: BindingId([1; 16]),
            stopping: false,
        }),
        ..Default::default()
    }
}
fn run(state: State, event: Event) -> (State, Vec<Action>) {
    step(
        state,
        Input {
            now_ms: 100_000,
            facts: None,
            event,
        },
    )
}
fn kinds(actions: &[Action]) -> Vec<ActionKind> {
    actions.iter().map(|a| a.kind.clone()).collect()
}

#[test]
fn local_cloud_and_root_drain_have_different_update_authority() {
    let mut state = ready();
    state.facts.summary.sendable = 2;
    state.facts.summary.parked = 1;
    let (state, actions) = run(state, Event::TrackingChanged(ChangeCause::Local));
    assert!(kinds(&actions).contains(&ActionKind::UpdatePending(Update::Full)));
    assert!(kinds(&actions).contains(&ActionKind::ArmFlushTimer { at_ms: 160_000 }));
    for event in [
        Event::TrackingChanged(ChangeCause::Cloud),
        Event::FetchDrained {
            zone: Zone::Root,
            fetched: false,
        },
    ] {
        let (_, actions) = run(state.clone(), event);
        assert!(!kinds(&actions).contains(&ActionKind::UpdatePending(Update::Full)));
        assert!(!actions.iter().any(|a| matches!(
            a.kind,
            ActionKind::RequestSend { .. } | ActionKind::ArmFlushTimer { .. }
        )));
    }
}

#[test]
fn availability_uses_ordered_rows_and_review_registry_fact() {
    let mut state = ready();
    assert_eq!(state.sync_now_availability(), Ok(()));
    state.settings.automatic = false;
    assert_eq!(state.sync_now_availability(), Ok(()));
    state.facts.session = Some(19);
    state.settings.enabled = false;
    state.settings.paused = true;
    state.account.status = AccountStatus::NoAccount;
    assert_eq!(state.sync_now_availability(), Err(Unavailable::ReviewActive));
    state.facts.lifecycle = Lifecycle::Opening;
    assert_eq!(state.sync_now_availability(), Err(Unavailable::CollectionUnavailable));
}

#[test]
fn no_quiet_point_from_idle_or_one_drain_or_unsettled_apply() {
    let state = ready();
    assert_eq!(state.status().state, StatusState::AwaitingEvidence);
    let (state, a) = run(
        state,
        Event::FetchDrained {
            zone: Zone::Root,
            fetched: false,
        },
    );
    assert!(!a.iter().any(|a| matches!(a.kind, ActionKind::RecordQuietPoint { .. })));
    let mut blocked = state.clone();
    blocked.facts.summary.apply_in_progress = true;
    let (blocked, a) = run(
        blocked,
        Event::FetchDrained {
            zone: Zone::Collection,
            fetched: false,
        },
    );
    assert!(!a.iter().any(|a| matches!(a.kind, ActionKind::RecordQuietPoint { .. })));
    let mut settled = blocked;
    settled.facts.summary.apply_in_progress = false;
    settled.facts.summary.held = 3;
    let (settled, a) = run(settled, Event::TrackingChanged(ChangeCause::Cloud));
    assert_eq!(
        a.iter()
            .filter(|a| matches!(a.kind, ActionKind::RecordQuietPoint { .. }))
            .count(),
        1
    );
    // Evidence is published only after a real durable write, not at emission.
    assert_eq!(settled.status().state, StatusState::AwaitingEvidence);
    let (_, a) = run(settled, Event::TrackingChanged(ChangeCause::Cloud));
    assert!(!a.iter().any(|a| matches!(a.kind, ActionKind::RecordQuietPoint { .. })));
}

#[test]
fn quota_and_partial_errors_do_not_schedule_work_or_clear_on_failed_send() {
    let scope = Operation {
        zone: Zone::Collection,
        kind: OperationKind::Send,
    };
    let mut state = ready();
    state.facts.summary.parked = 10;
    let (state, actions) = run(
        state,
        Event::EngineError {
            instance: "sdk1".into(),
            operation: scope,
            error: OperationError::QuotaFull,
            code: 25,
            retry_after_ms: None,
        },
    );
    assert!(actions.iter().all(|a| matches!(a.kind, ActionKind::StatusChanged(_))));
    assert!(state.status().attention.contains_key(&Attention::QuotaFull));
    let (state, _) = run(
        state,
        Event::EngineActivity {
            instance: "sdk1".into(),
            operation: scope,
            phase: Activity::Ended {
                outcome: Outcome::Failed,
                saved: 1,
            },
        },
    );
    assert!(state.status().attention.contains_key(&Attention::QuotaFull));
    let (state, _) = run(
        state,
        Event::EngineActivity {
            instance: "sdk1".into(),
            operation: scope,
            phase: Activity::Ended {
                outcome: Outcome::Ok,
                saved: 1,
            },
        },
    );
    assert!(!state.status().attention.contains_key(&Attention::QuotaFull));
}

#[test]
fn error_cycles_count_operations_and_clear_only_matching_scope() {
    let send = Operation {
        zone: Zone::Collection,
        kind: OperationKind::Send,
    };
    let fetch = Operation {
        zone: Zone::Root,
        kind: OperationKind::Fetch,
    };
    let mut state = ready();
    for n in 0..3 {
        (state, _) = run(
            state,
            Event::EngineError {
                instance: "sdk1".into(),
                operation: send,
                error: OperationError::Other,
                code: 1,
                retry_after_ms: None,
            },
        );
        (state, _) = run(
            state,
            Event::EngineActivity {
                instance: "sdk1".into(),
                operation: send,
                phase: Activity::Ended {
                    outcome: Outcome::Failed,
                    saved: 0,
                },
            },
        );
        assert_eq!(state.status().attention.contains_key(&Attention::CloudError), n == 2);
    }
    (state, _) = run(
        state,
        Event::EngineActivity {
            instance: "sdk1".into(),
            operation: fetch,
            phase: Activity::Ended {
                outcome: Outcome::Ok,
                saved: 0,
            },
        },
    );
    assert!(state.status().attention.contains_key(&Attention::CloudError));
    (state, _) = run(
        state,
        Event::EngineActivity {
            instance: "sdk1".into(),
            operation: send,
            phase: Activity::Ended {
                outcome: Outcome::Ok,
                saved: 0,
            },
        },
    );
    assert!(!state.status().attention.contains_key(&Attention::CloudError));
}

#[test]
fn policy_authority_tracks_settings_account_and_sdk_events() {
    let state = ready();
    let token = state.token;
    let (state, _) = run(
        state,
        Event::DeviceSettings(DeviceSettings {
            automatic: false,
            ..DeviceSettings::default_connected()
        }),
    );
    assert_eq!(state.token, token);
    let (state, _) = run(state, Event::Account(state_account()));
    assert_eq!(state.token, token);
    let (state, _) = run(
        state,
        Event::EngineStopped {
            instance: "sdk1".into(),
        },
    );
    let (state, _) = run(
        state,
        Event::EngineStarted {
            instance: "sdk2".into(),
            binding: BindingId([1; 16]),
        },
    );
    assert_eq!(state.token, token);
    let before = state.clone();
    let (state, actions) = run(
        state,
        Event::EngineError {
            instance: "sdk1".into(),
            operation: Operation {
                zone: Zone::Collection,
                kind: OperationKind::Send,
            },
            error: OperationError::QuotaFull,
            code: 25,
            retry_after_ms: None,
        },
    );
    assert_eq!(state, before);
    assert!(actions.is_empty());
}
fn state_account() -> Account {
    Account {
        status: AccountStatus::Available,
        identity: Some(OpaqueBytes(vec![1])),
    }
}

#[test]
fn background_and_quit_budgets_include_auto_off_exchange() {
    let mut state = ready();
    state.settings.automatic = false;
    let (state, actions) = run(state, Event::SyncNowRequested(Source::Command));
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::RunExchange { .. }))
            .count(),
        1
    );
    let (state, actions) = run(state, Event::Background { deadline_ms: 118_000 });
    assert!(kinds(&actions).contains(&ActionKind::BackgroundFlush { until_ms: 113_000 }));
    let (_, actions) = run(state, Event::QuitRequested);
    assert!(kinds(&actions).contains(&ActionKind::QuitFlush { until_ms: 110_000 }));
}

#[test]
fn repeated_tracking_failures_have_one_local_restart_per_foreground() {
    let mut state = ready();
    for i in 0..200 {
        (state, _) = run(state, Event::TrackingWriteFailed { no_space: false });
        let (next, actions) = run(state, Event::TrackingChanged(ChangeCause::Local));
        assert_eq!(
            actions.iter().any(|a| matches!(a.kind, ActionKind::StartEngine { .. })),
            i == 0
        );
        state = next;
    }
    let (state, actions) = run(state, Event::Foreground);
    assert!(kinds(&actions).contains(&ActionKind::ProbeTracking));
    let (_, actions) = run(state, Event::TrackingChanged(ChangeCause::Probe));
    assert!(actions.iter().any(|a| matches!(a.kind, ActionKind::StartEngine { .. })));
}

#[test]
fn settings_and_status_precedence_table() {
    let mut state = ready();
    state.facts.summary.sendable = 2;
    let rows = [
        (false, true, AccountStatus::NoAccount, StatusState::LocalOnly),
        (
            true,
            true,
            AccountStatus::NoAccount,
            StatusState::ICloudUnavailable(UnavailableReason::NoAccount),
        ),
        (
            true,
            true,
            AccountStatus::Available,
            StatusState::Paused(PausedReason::UserPaused),
        ),
        (true, false, AccountStatus::Available, StatusState::ChangesPending),
    ];
    for (enabled, paused, account, expected) in rows {
        state.settings.enabled = enabled;
        state.settings.paused = paused;
        state.account.status = account;
        assert_eq!(state.status().state, expected);
    }
}

#[test]
fn gate_table_preserves_running_instance_for_scan_exclusive_and_crt() {
    for reason in 0..6 {
        let mut state = ready();
        let mut facts = state.facts.clone();
        match reason {
            0 => facts.summary.scan = Scan::Required,
            1 => facts.summary.scan = Scan::Running,
            2 => facts.summary.unsettled_intents = 1,
            3 => facts.lifecycle = Lifecycle::ExclusiveBusy,
            4 => facts.summary.crt_mismatch = true,
            _ => facts.lifecycle = Lifecycle::Failed,
        }
        state.flush_at_ms = Some(120_000);
        let (state, actions) = step(
            state,
            Input {
                now_ms: 100_000,
                facts: Some(facts),
                event: Event::TrackingChanged(ChangeCause::Cloud),
            },
        );
        assert!(!state.gate_open());
        assert_eq!(
            actions.iter().any(|a| matches!(a.kind, ActionKind::StopEngine { .. })),
            reason == 5
        );
        assert!(!actions.iter().any(|a| matches!(a.kind, ActionKind::RequestSend { .. })));
    }
}

#[test]
fn publication_progress_does_not_break_exchange_guard_but_cancelling_does() {
    let mut state = ready();
    state.settings.automatic = false;
    state.facts.binding.as_mut().unwrap().publish_step = Some(PublishStep::Bound);
    state.engine = None;
    let (mut state, _) = run(state, Event::SyncNowRequested(Source::Command));
    let exchange = state.exchange.as_ref().unwrap().id;
    let token = state.token;
    for phase in [
        PublishStep::Zones,
        PublishStep::Sent,
        PublishStep::Complete,
        PublishStep::Done,
    ] {
        let mut binding = state.facts.binding.clone().unwrap();
        binding.publish_step = Some(phase);
        let (next, actions) = run(state, Event::Binding(Some(binding)));
        assert_eq!(next.token, token);
        assert_eq!(next.exchange.as_ref().unwrap().id, exchange);
        if phase == PublishStep::Zones {
            assert!(
                actions
                    .iter()
                    .any(|a| matches!(a.kind, ActionKind::StartEngine { automatic: true, .. }))
            );
        }
        state = next;
    }
    let mut binding = state.facts.binding.clone().unwrap();
    binding.publish_step = Some(PublishStep::Cancelling);
    let (state, _) = run(state, Event::Binding(Some(binding)));
    assert!(state.token > token);
    assert!(!state.gate_open());
}

#[test]
fn status_remaining_precedence_and_unknown_media_are_honest() {
    let mut state = ready();
    state.facts.summary.attention.insert(Attention::Refused, 3);
    state.settings.paused = true;
    state.reachable = false;
    assert_eq!(state.status().state, StatusState::NeedsAttention);
    state.facts.fence = Fence::DecisionRequired;
    assert_eq!(
        state.status().state,
        StatusState::Paused(PausedReason::DecisionRequired)
    );
    state.facts.fence = Fence::Clear;
    state.settings.paused = false;
    state.facts.summary.attention.clear();
    assert_eq!(state.status().state, StatusState::Offline);
    state.reachable = true;
    state
        .cycles
        .entry(Operation {
            zone: Zone::Collection,
            kind: OperationKind::Fetch,
        })
        .or_default()
        .active = true;
    state.facts.summary.media.downloads_running = Some(1);
    assert_eq!(state.status().state, StatusState::SyncingCollection);
    state.cycles.clear();
    assert_eq!(state.status().state, StatusState::SyncingMedia);
    state.facts.summary.media.downloads_running = Some(0);
    state.evidence.quiet_ms = Some(900);
    state.facts.summary.held = 4;
    assert_eq!(state.status().state, StatusState::UpToDate { as_of_ms: 900 });
    assert_eq!(state.status().media.download_bytes, None);
    state.facts.summary.attention_known = false;
    assert_eq!(state.status().state, StatusState::AwaitingEvidence);
    assert_eq!(state.status().evidence.quiet_ms, Some(900));
}

#[test]
fn generated_event_orders_never_send_when_gate_closed_or_restart_a_live_engine() {
    for seed in 1..=24_u64 {
        let mut rng = seed;
        let mut state = ready();
        state.facts.summary.sendable = 1;
        for _ in 0..150 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let event = match (rng >> 32) % 12 {
                0 => Event::Foreground,
                1 => Event::Network {
                    reachable: rng & 1 == 0,
                },
                2 => Event::TrackingChanged(ChangeCause::Local),
                3 => Event::TrackingChanged(ChangeCause::Cloud),
                4 => Event::DeviceSettings(DeviceSettings {
                    enabled: true,
                    paused: rng & 1 != 0,
                    automatic: rng & 2 == 0,
                    media: false,
                }),
                5 => Event::FlushTimerFired,
                6 => Event::FetchDrained {
                    zone: Zone::Root,
                    fetched: false,
                },
                7 => Event::FetchDrained {
                    zone: Zone::Collection,
                    fetched: true,
                },
                8 => Event::Background { deadline_ms: 132_000 },
                9 => Event::BackgroundExpiring,
                10 => Event::SyncNowRequested(Source::Key),
                _ => Event::TrackingWriteFailed { no_space: rng & 1 == 0 },
            };
            let before = state.clone();
            let (next, actions) = run(state, event);
            for a in &actions {
                if matches!(a.kind, ActionKind::RequestSend { .. }) {
                    assert!(next.gate_open());
                }
                if matches!(a.kind, ActionKind::StartEngine { .. }) {
                    assert!(!before.engine_runs() || before.engine.as_ref().unwrap().stopping);
                }
                if let ActionKind::BackgroundFlush { until_ms } = a.kind {
                    assert!(until_ms <= 125_000);
                    assert!(until_ms <= 127_000);
                }
                assert_eq!(a.guard, next.guard());
            }
            state = next;
        }
    }
}
