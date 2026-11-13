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
                crt_known: true,
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
fn policy_initial_automatic_closed_gate_retains_preference_without_engine_work() {
    for reason in 0..9 {
        let mut state = ready();
        state.engine = None;
        state.settings.automatic = false;
        match reason {
            0 => state.facts.binding = None,
            1 => state.facts.binding.as_mut().unwrap().verified = false,
            2 => state.facts.lifecycle = Lifecycle::Opening,
            3 => state.facts.summary.scan = Scan::Required,
            4 => state.facts.summary.scan = Scan::Running,
            5 => state.facts.summary.unsettled_intents = 1,
            6 => state.settings.paused = true,
            7 => state.account.status = AccountStatus::Undetermined,
            8 => state.facts.summary.crt_mismatch = true,
            _ => unreachable!(),
        }
        assert!(!state.gate_open());
        let mut settings = state.settings;
        settings.automatic = true;
        let (state, actions) = run(state, Event::DeviceSettings(settings));
        assert!(state.settings.automatic && state.engine.is_none(), "reason={reason}, actions={actions:?}");
        let status = ActionKind::StatusChanged(Box::new(state.status()));
        if reason == 8 {
            // Approved P6: known crt mismatch closes writes/apply, retaining
            // the read gate. Configure + full pending update + fetch are due.
            assert_eq!(kinds(&actions), vec![
                ActionKind::ConfigureAutomatic { on: true },
                ActionKind::UpdatePending(Update::Full),
                ActionKind::RequestFetch { zones: vec![Zone::Root, Zone::Collection] },
                status,
            ], "reason={reason}, actions={actions:?}");
            assert!(!actions.iter().any(|a| matches!(a.kind, ActionKind::RequestSend { .. } | ActionKind::ApplyReady { .. })),
                "reason={reason}, actions={actions:?}");
        } else {
            assert_eq!(kinds(&actions), vec![status], "reason={reason}, actions={actions:?}");
        }
    }
}

#[test]
fn policy_initial_automatic_off_preserves_closed_gate_configuration_and_timer_cancellation() {
    let mut state = ready();
    state.facts.binding.as_mut().unwrap().verified = false;
    state.engine = None;
    state.flush_at_ms = Some(120_000);
    let token = state.token;
    let mut settings = state.settings;
    settings.automatic = false;
    let (state, actions) = run(state, Event::DeviceSettings(settings));
    assert!(!state.gate_open() && !state.settings.automatic);
    assert_eq!(state.token, token);
    assert_eq!(state.flush_at_ms, None);
    assert_eq!(
        kinds(&actions)
            .into_iter()
            .filter(|a| !matches!(a, ActionKind::StatusChanged(_)))
            .collect::<Vec<_>>(),
        vec![
            ActionKind::ConfigureAutomatic { on: false },
            ActionKind::CancelFlushTimer
        ]
    );
}

#[test]
fn policy_initial_automatic_publication_zones_force_scheduling_until_sent() {
    let mut state = ready();
    state.engine = None;
    state.settings.automatic = false;
    state.facts.binding.as_mut().unwrap().publish_step = Some(PublishStep::Bound);
    let mut binding = state.facts.binding.clone().unwrap();
    binding.publish_step = Some(PublishStep::Zones);
    let token = state.token;
    let (state, actions) = run(state, Event::Binding(Some(binding.clone())));
    assert!(state.gate_open() && state.automatic() && !state.settings.automatic);
    assert_eq!(state.token, token);
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::StartEngine { automatic: true, .. }))
            .count(),
        1
    );
    let (mut state, _) = run(
        state,
        Event::EngineStarted {
            instance: "publication-sdk".into(),
            binding: binding.id,
        },
    );
    for automatic in [true, false] {
        let mut settings = state.settings;
        settings.automatic = automatic;
        let (next, actions) = run(state, Event::DeviceSettings(settings));
        assert!(next.automatic());
        assert!(!actions.iter().any(|a| matches!(
            a.kind,
            ActionKind::ConfigureAutomatic { .. } | ActionKind::CancelFlushTimer
        )));
        state = next;
    }
    binding.publish_step = Some(PublishStep::Sent);
    let (state, actions) = run(state, Event::Binding(Some(binding)));
    assert!(!state.automatic() && state.gate_open() && state.engine_runs());
    assert_eq!(state.token, token);
    let actions = kinds(&actions);
    assert!(actions.starts_with(&[
        ActionKind::ConfigureAutomatic { on: false },
        ActionKind::UpdatePending(Update::Full)
    ]));
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, ActionKind::StartEngine { .. } | ActionKind::RequestSend { .. }))
    );
}

#[test]
fn policy_initial_automatic_reopening_retains_stopping_instance_until_engine_stopped() {
    let mut state = ready();
    let mut settings = state.settings;
    settings.paused = true;
    let (next, _) = run(state, Event::DeviceSettings(settings));
    state = next;
    assert!(state.engine.as_ref().unwrap().stopping && !state.engine_runs());
    settings.automatic = false;
    let (state, actions) = run(state, Event::DeviceSettings(settings));
    assert!(kinds(&actions).contains(&ActionKind::ConfigureAutomatic { on: false }));
    settings.paused = false;
    let (state, actions) = run(state, Event::DeviceSettings(settings));
    assert!(state.gate_open() && !state.engine_runs());
    assert!(state.engine.as_ref().unwrap().stopping);
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::StartEngine { automatic: false, .. }))
            .count(),
        1
    );
    let (state, actions) = run(
        state,
        Event::EngineStopped {
            instance: "sdk1".into(),
        },
    );
    assert!(state.engine.is_none());
    assert!(!actions.iter().any(|a| matches!(a.kind, ActionKind::StartEngine { .. })));
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
fn manual_exchange_completion_records_before_resuming_media_with_auto_off() {
    for automatic in [false, true] {
        for outcome in [
            ExchangeOutcome::Succeeded {
                selected: 1,
                held_back: 0,
                newer_pending: 0,
                incoming_waiting: 0,
                media: MediaFacts::default(),
            },
            ExchangeOutcome::Incomplete {
                outstanding: 1,
                blocked: 0,
            },
        ] {
            let mut state = ready();
            state.settings.automatic = automatic;
            state.settings.media = true;
            // Step 7 repeats even while media runs and pending byte counts are unknown.
            state.facts.summary.media.downloads_running = Some(1);
            let (state, actions) = run(state, Event::SyncNowRequested(Source::Command));
            assert!(!kinds(&actions).contains(&ActionKind::ResumeMedia));
            let exchange = state.exchange.as_ref().unwrap().id;
            let guard = state.guard();
            let (state, actions) = run(
                state,
                Event::ExchangeEnded {
                    exchange,
                    outcome: outcome.clone(),
                },
            );
            assert!(state.exchange.is_none());
            assert_eq!(
                &kinds(&actions)[..2],
                &[
                    ActionKind::RecordExchange(ExchangeRecord {
                        exchange,
                        ended_ms: 100_000,
                        source: Source::Command,
                        outcome,
                    }),
                    ActionKind::ResumeMedia,
                ]
            );
            assert_eq!(actions[0].guard, guard);
            assert_eq!(actions[1].guard, guard);
            assert_eq!(actions.iter().filter(|a| a.kind == ActionKind::ResumeMedia).count(), 1);
            // A duplicate completion cannot start another media run or record again.
            let (_, actions) = run(
                state,
                Event::ExchangeEnded {
                    exchange,
                    outcome: ExchangeOutcome::Cancelled,
                },
            );
            assert!(
                !actions
                    .iter()
                    .any(|a| matches!(a.kind, ActionKind::RecordExchange(_) | ActionKind::ResumeMedia))
            );
        }
    }
}

#[test]
fn exchange_completion_requires_media_preference_and_current_open_gate_to_resume_media() {
    type GateCase = (&'static str, fn(&mut State));
    let rows: &[GateCase] = &[
        ("media off", |s| s.settings.media = false),
        ("disabled", |s| s.settings.enabled = false),
        ("paused", |s| s.settings.paused = true),
        ("no account", |s| s.account.status = AccountStatus::NoAccount),
        ("setup needed", |s| s.account.status = AccountStatus::Configuration),
        ("wrong account", |s| s.account.identity = Some(OpaqueBytes(vec![2]))),
        ("unverified account", |s| {
            s.facts.binding.as_mut().unwrap().verified = false
        }),
        ("unusable binding", |s| s.facts.binding.as_mut().unwrap().usable = false),
        ("publication pending", |s| {
            s.facts.binding.as_mut().unwrap().publish_step = Some(PublishStep::Bound)
        }),
        ("fenced", |s| s.facts.fence = Fence::DecisionRequired),
        ("exclusive", |s| s.facts.lifecycle = Lifecycle::ExclusiveBusy),
        ("closed", |s| s.facts.lifecycle = Lifecycle::NoCollection),
        ("unknown collection", |s| s.facts.summary.collection_known = false),
        ("scan required", |s| s.facts.summary.scan = Scan::Required),
        ("scan running", |s| s.facts.summary.scan = Scan::Running),
        ("unsettled intent", |s| s.facts.summary.unsettled_intents = 1),
        ("crt mismatch", |s| s.facts.summary.crt_mismatch = true),
        ("tracking stopped", |s| s.tracking_stop = Some(false)),
    ];
    for (reason, close_gate) in rows {
        let mut state = ready();
        state.settings.automatic = false;
        state.settings.media = true;
        let (mut state, _) = run(state, Event::SyncNowRequested(Source::Command));
        let exchange = state.exchange.as_ref().unwrap().id;
        close_gate(&mut state);
        let (_, actions) = run(
            state,
            Event::ExchangeEnded {
                exchange,
                outcome: ExchangeOutcome::Incomplete {
                    outstanding: 1,
                    blocked: 0,
                },
            },
        );
        assert!(!kinds(&actions).contains(&ActionKind::ResumeMedia), "{reason}");
        assert_eq!(
            actions
                .iter()
                .filter(|a| matches!(a.kind, ActionKind::RecordExchange(_)))
                .count(),
            1,
            "{reason}"
        );
    }
}

#[test]
fn early_stop_exchange_outcomes_record_without_resuming_media() {
    for outcome in [
        ExchangeOutcome::Waiting {
            class: Some(OperationError::Offline),
            retry_after_ms: None,
        },
        ExchangeOutcome::Failed {
            class: OperationError::QuotaFull,
        },
        ExchangeOutcome::Cancelled,
    ] {
        let mut state = ready();
        state.settings.automatic = false;
        state.settings.media = true;
        let (state, _) = run(state, Event::SyncNowRequested(Source::Command));
        let exchange = state.exchange.as_ref().unwrap().id;
        let (_, actions) = run(state, Event::ExchangeEnded { exchange, outcome });
        assert!(matches!(actions[0].kind, ActionKind::RecordExchange(_)));
        assert!(!kinds(&actions).contains(&ActionKind::ResumeMedia));
    }
}

#[test]
fn stale_exchange_guard_cannot_resume_media_after_gate_reopens() {
    for component in 0..3 {
        let mut state = ready();
        state.settings.automatic = false;
        state.settings.media = true;
        let (mut state, _) = run(state, Event::SyncNowRequested(Source::Command));
        let original_guard = &mut state.exchange.as_mut().unwrap().guard;
        match component {
            0 => original_guard.token += 1,
            1 => original_guard.generation += 1,
            _ => original_guard.binding = Some(BindingId([2; 16])),
        }
        let original_guard = original_guard.clone();
        let exchange = state.exchange.as_ref().unwrap().id;
        assert!(state.gate_open());
        let (state, actions) = run(
            state,
            Event::ExchangeEnded {
                exchange,
                outcome: ExchangeOutcome::Incomplete {
                    outstanding: 1,
                    blocked: 0,
                },
            },
        );
        assert!(matches!(actions[0].kind, ActionKind::RecordExchange(_)));
        assert_eq!(actions[0].guard.generation, original_guard.generation);
        assert_eq!(actions[0].guard.binding, original_guard.binding);
        assert_eq!(actions[0].guard.token, state.token);
        assert!(!kinds(&actions).contains(&ActionKind::ResumeMedia));
    }
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
fn publication_terminal_row_removal_keeps_original_guard() {
    for phase in [PublishStep::Sent, PublishStep::Complete, PublishStep::Done] {
        let mut state = ready();
        state.facts.binding.as_mut().unwrap().publish_step = Some(phase);
        let (state, _) = run(state, Event::SyncNowRequested(Source::Command));
        let original = state.guard();
        let exchange = state.exchange.as_ref().unwrap().id;
        let mut facts = state.facts.clone();
        facts.binding.as_mut().unwrap().publish_step = None;
        let (state, _) = step(
            state,
            Input {
                now_ms: 100_000,
                facts: Some(facts),
                event: Event::TrackingChanged(ChangeCause::Cloud),
            },
        );
        assert_eq!(state.guard(), original, "terminal phase {phase:?}");
        assert_eq!(state.exchange.as_ref().unwrap().id, exchange);
        assert_eq!(state.exchange.as_ref().unwrap().guard, original);
    }
}

#[test]
fn publication_row_removal_does_not_hide_cancellation_or_authority_changes() {
    for phase in [
        PublishStep::Bound,
        PublishStep::Zones,
        PublishStep::Sent,
        PublishStep::Complete,
        PublishStep::Done,
        PublishStep::Cancelling,
    ] {
        for change in 0..11 {
            let mut state = ready();
            state.facts.binding.as_mut().unwrap().publish_step = Some(phase);
            let original = state.guard();
            let mut facts = state.facts.clone();
            let binding = facts.binding.as_mut().unwrap();
            binding.publish_step = None;
            let event = match change {
                0 => Event::TrackingChanged(ChangeCause::Cloud),
                1 => {
                    binding.id = BindingId([2; 16]);
                    Event::TrackingChanged(ChangeCause::Cloud)
                }
                2 => {
                    binding.state = BindingState::Isolated(meguri_sync::cloud::IsolationReason::SignedOut);
                    Event::TrackingChanged(ChangeCause::Cloud)
                }
                3 => {
                    binding.account = OpaqueBytes(vec![2]);
                    Event::TrackingChanged(ChangeCause::Cloud)
                }
                4 => {
                    binding.usable = false;
                    Event::TrackingChanged(ChangeCause::Cloud)
                }
                5 => {
                    binding.verified = false;
                    Event::TrackingChanged(ChangeCause::Cloud)
                }
                6 => {
                    facts.binding = None;
                    Event::TrackingChanged(ChangeCause::Cloud)
                }
                7 => Event::Account(Account {
                    status: AccountStatus::Available,
                    identity: Some(OpaqueBytes(vec![2])),
                }),
                8 => Event::CollectionReplaced {
                    generation: facts.generation + 1,
                },
                9 => Event::Lifecycle {
                    state: Lifecycle::NoCollection,
                    generation: facts.generation,
                },
                10 => {
                    facts.generation += 1;
                    Event::TrackingChanged(ChangeCause::Cloud)
                }
                _ => unreachable!(),
            };
            let (state, _) = step(
                state,
                Input {
                    now_ms: 100_000,
                    facts: Some(facts),
                    event,
                },
            );
            let terminal = matches!(phase, PublishStep::Sent | PublishStep::Complete | PublishStep::Done);
            assert_eq!(
                state.token == original.token,
                change == 0 && terminal,
                "phase {phase:?}, change {change}"
            );
        }
    }
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

#[test]
fn control_facts_do_not_use_display_completion_or_attention_as_binding_evidence() {
    let mut state = ready();
    state.facts.summary.attention_known = false;
    let status = state.status();
    assert_eq!(status.state, StatusState::AwaitingEvidence);
    assert_eq!(status.controls.enable, EnableControl::Pause);
    assert!(status.controls.binding_usable && status.controls.binding_active);
    assert!(status.controls.automatic_eligible);
    assert_eq!(status.evidence.as_of_ms(), None);

    state.facts.summary.attention.insert(Attention::Conflicts, 2);
    let status = state.status();
    assert_eq!(status.state, StatusState::NeedsAttention);
    assert_eq!(status.controls.enable, EnableControl::Pause);
    assert!(status.controls.binding_usable && status.controls.automatic_eligible);
    state.facts.summary.attention.insert(Attention::RootUnreadable, 1);
    assert!(!state.status().controls.automatic_eligible);
    assert!(state.status().controls.binding_usable);
}

#[test]
fn account_controls_follow_pause_decision_and_binding_priority_even_when_unavailable() {
    use meguri_sync::cloud::IsolationReason;
    let mut state = ready();
    state.account.status = AccountStatus::NoAccount;
    let status = state.status();
    assert_eq!(
        status.state,
        StatusState::ICloudUnavailable(UnavailableReason::NoAccount)
    );
    assert_eq!(status.controls.enable, EnableControl::Pause);
    assert!(status.controls.binding_active && status.controls.binding_usable);
    assert!(!status.controls.automatic_eligible);
    state.facts.binding.as_mut().unwrap().state = BindingState::Isolated(IsolationReason::SignedOut);
    let signed_out = state.status();
    assert_eq!(
        signed_out.state,
        StatusState::ICloudUnavailable(UnavailableReason::NoAccount)
    );
    assert!(!signed_out.controls.binding_active && !signed_out.controls.binding_usable);
    assert!(!signed_out.controls.automatic_eligible);
    assert_eq!(signed_out.controls.resume_binding_offered, None);
    state.facts.binding.as_mut().unwrap().state = BindingState::Active;

    state.facts.fence = Fence::DecisionRequired;
    assert_eq!(state.status().controls.enable, EnableControl::DecisionRequired);
    assert!(state.status().controls.decision_required);
    state.settings.paused = true;
    assert_eq!(state.status().controls.enable, EnableControl::ResumePause);
    assert!(state.status().controls.decision_required);
    assert!(state.status().controls.user_paused);

    state.facts.binding.as_mut().unwrap().state = BindingState::Isolated(IsolationReason::Stopped);
    assert_eq!(state.status().controls.enable, EnableControl::ResumePause);
    state.settings.paused = false;
    assert_eq!(state.status().controls.enable, EnableControl::DecisionRequired);
    state.facts.fence = Fence::Clear;
    let status = state.status();
    assert_eq!(status.controls.enable, EnableControl::AwaitingResumeOffer);
    assert!(status.controls.binding_stopped);
    assert!(!status.controls.binding_active && !status.controls.binding_usable);
    assert_eq!(status.controls.resume_binding_offered, None);
    assert!(!status.controls.automatic_eligible);
    state.facts.binding = None;
    assert_eq!(state.status().controls.enable, EnableControl::Connect);
    state.settings.paused = true;
    assert_eq!(state.status().controls.enable, EnableControl::ResumePause);
}

#[test]
fn review_copy_eligibility_reuses_core_engine_admission_with_unknown_status_evidence() {
    let mut state = ready();
    state.facts.session = Some(91);
    state.facts.summary.attention_known = false;
    assert_eq!(state.status().sync_now, Err(Unavailable::ReviewActive));
    assert!(state.status().controls.automatic_eligible);
    state.settings.automatic = false;
    assert!(!state.status().controls.automatic_eligible);
    state.settings.automatic = true;
    state.facts.binding.as_mut().unwrap().verified = false;
    assert!(!state.status().controls.automatic_eligible);
    state.facts.binding.as_mut().unwrap().verified = true;
    state.tracking_stop = Some(false);
    assert!(!state.status().controls.automatic_eligible);
    state.tracking_stop = None;
    state.settings.paused = true;
    assert!(!state.status().controls.automatic_eligible);
    state.settings.paused = false;
    state.facts.fence = Fence::DecisionRequired;
    assert!(!state.status().controls.automatic_eligible);
    state.facts.fence = Fence::Clear;
    state.account.status = AccountStatus::Configuration;
    assert!(!state.status().controls.automatic_eligible);
}

#[test]
fn epoch_crt_unknown_closes_writes_without_recovery_or_stopping_reads() {
    let mut state = ready();
    state.facts.summary.crt_known = false;
    state.facts.summary.attention_known = false;
    state.drained = [true; 2];
    state.drain_serial = 1;
    assert!(!state.gate_open());
    assert!(state.engine_allowed());
    assert_eq!(state.sync_now_availability(), Ok(()));
    assert!(!state.status().attention.contains_key(&Attention::CrtMismatch));
    let (_, actions) = run(state.clone(), Event::Foreground);
    assert!(actions.iter().any(|a| matches!(a.kind, ActionKind::RequestFetch { .. })));
    assert!(!actions.iter().any(|a| matches!(a.kind, ActionKind::RequestSend { .. } | ActionKind::ApplyReady { .. } | ActionKind::StopEngine { .. } | ActionKind::RecordQuietPoint { .. })));
    state.engine = None;
    let mut closed = state.clone();
    closed.facts.lifecycle = Lifecycle::Opening;
    let (_, actions) = step(closed, Input { now_ms: 1000, facts: Some(state.facts), event: Event::Lifecycle { state: Lifecycle::Ready, generation: 7 } });
    assert!(actions.iter().any(|a| matches!(a.kind, ActionKind::StartEngine { .. })));
    assert!(actions.iter().any(|a| matches!(a.kind, ActionKind::RequestFetch { .. })));
}

#[test]
fn epoch_crt_known_mismatch_keeps_automatic_fetch_and_reports_recovery_required() {
    let mut state = ready();
    state.facts.summary.crt_mismatch = true;
    assert_eq!(state.sync_now_availability(), Err(Unavailable::RecoveryRequired));
    let (_, actions) = run(state, Event::Foreground);
    assert!(actions.iter().any(|a| matches!(a.kind, ActionKind::RequestFetch { .. })));
    assert!(!actions.iter().any(|a| matches!(a.kind, ActionKind::RequestSend { .. } | ActionKind::StopEngine { .. } | ActionKind::RecordQuietPoint { .. })));
}

#[test]
fn sync_now_unknown_epoch_starts_preparation_with_current_automatic_setting() {
    for automatic in [false, true] {
        let mut state = ready();
        state.engine = None;
        state.settings.automatic = automatic;
        state.facts.summary.crt_known = false;
        state.facts.summary.attention_known = false;
        let (state, actions) = run(state, Event::SyncNowRequested(Source::Command));
        assert!(!state.gate_open());
        assert!(!state.facts.summary.attention_known);
        assert_eq!(actions.iter().filter(|a| matches!(a.kind, ActionKind::StartEngine { automatic: on, .. } if on == automatic)).count(), 1);
        assert_eq!(actions.iter().filter(|a| matches!(a.kind, ActionKind::RunExchange { .. })).count(), 1);
        assert!(!actions.iter().any(|a| matches!(a.kind, ActionKind::RequestSend { .. } | ActionKind::ApplyReady { .. } | ActionKind::RecordQuietPoint { .. })));
    }
}

#[test]
fn waiting_exchange_record_roundtrips_absent_and_legacy_classes() {
    let classes = [
        OperationError::NoAccount, OperationError::Configuration, OperationError::Offline,
        OperationError::Throttled, OperationError::QuotaFull, OperationError::LocalStorageFull,
        OperationError::TrackingError, OperationError::ConflictRecovery, OperationError::UnsupportedSchema,
        OperationError::Other,
    ];
    for class in std::iter::once(None).chain(classes.into_iter().map(Some)) {
        let record = ExchangeRecord {
            exchange: 1, ended_ms: 100_000, source: Source::Command,
            outcome: ExchangeOutcome::Waiting { class, retry_after_ms: Some(5_000) },
        };
        let encoded = serde_json::to_value(&record).unwrap();
        let expected_class = class.map_or(serde_json::Value::Null, |value| serde_json::to_value(value).unwrap());
        assert_eq!(encoded["outcome"]["Waiting"]["class"], expected_class);
        assert_eq!(serde_json::from_value::<ExchangeRecord>(encoded).unwrap(), record);
    }
    // The pre-optional durable shape still decodes as its original class.
    let legacy = r#"{"Waiting":{"class":"Offline","retry_after_ms":9000}}"#;
    assert_eq!(serde_json::from_str::<ExchangeOutcome>(legacy).unwrap(),
        ExchangeOutcome::Waiting { class: Some(OperationError::Offline), retry_after_ms: Some(9_000) });
}
