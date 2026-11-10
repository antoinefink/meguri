use super::*;

fn bump(s: &mut State) {
    if let Some(next) = s.token.checked_add(1) {
        s.token = next;
    } else {
        s.settings.enabled = false;
    }
}
fn cancel_timer(s: &mut State, a: &mut Vec<ActionKind>) {
    s.flush_at_ms = None;
    a.push(ActionKind::CancelFlushTimer);
}
fn cancel_exchange(s: &State, a: &mut Vec<ActionKind>) {
    if let Some(e) = &s.exchange {
        a.push(ActionKind::CancelExchange { exchange: e.id });
    }
}
fn stop(s: &mut State, a: &mut Vec<ActionKind>, reason: StopReason) {
    if s.engine.as_ref().is_some_and(|e| !e.stopping) {
        bump(s);
        if let Some(e) = &mut s.engine {
            e.stopping = true;
        }
        s.drained = [false; 2];
        s.root_confirmed = false;
        for cycle in s.cycles.values_mut() {
            cycle.active = false;
        }
        a.push(ActionKind::StopEngine { reason });
    }
}
fn sendable(s: &State) -> bool {
    s.facts.summary.sendable > 0 || s.facts.summary.parked > 0
}
fn send(s: &State, a: &mut Vec<ActionKind>) {
    if sendable(s) {
        a.push(ActionKind::RequestSend {
            zones: vec![Zone::Collection],
        });
    }
}
fn fetch(a: &mut Vec<ActionKind>) {
    a.push(ActionKind::RequestFetch {
        zones: vec![Zone::Root, Zone::Collection],
    });
}
fn resume_media(s: &State, a: &mut Vec<ActionKind>) {
    if s.settings.media {
        a.push(ActionKind::ResumeMedia);
    }
}
fn probe(s: &mut State, a: &mut Vec<ActionKind>) {
    if s.tracking_stop.is_some() && !s.probe_in_progress {
        s.probe_in_progress = true;
        a.push(ActionKind::ProbeTracking);
    }
}
fn full_trigger(event: &Event) -> bool {
    matches!(
        event,
        Event::TrackingChanged(ChangeCause::Local)
            | Event::Foreground
            | Event::Account(_)
            | Event::SessionEnded(_)
            | Event::SyncNowRequested(_)
            | Event::FetchDrained {
                zone: Zone::Collection,
                ..
            }
    )
}
fn binding_progress(before: &Option<BindingView>, after: &Option<BindingView>) -> bool {
    match (before, after) {
        (Some(a), Some(b)) => {
            a.publish_step != b.publish_step
                && a.id == b.id
                && a.state == b.state
                && a.account == b.account
                && a.usable == b.usable
                && a.verified == b.verified
                && (matches!(
                    b.publish_step,
                    Some(PublishStep::Zones | PublishStep::Sent | PublishStep::Complete | PublishStep::Done)
                ) || (b.publish_step.is_none()
                    // SaveActive removes the Complete row. Own-operation replies
                    // can advance Sent through Complete before the next projection.
                    && matches!(
                        a.publish_step,
                        Some(PublishStep::Sent | PublishStep::Complete | PublishStep::Done)
                    )))
        }
        _ => false,
    }
}
fn instance_event(event: &Event) -> Option<&str> {
    match event {
        Event::EngineActivity { instance, .. }
        | Event::EngineError { instance, .. }
        | Event::RootConfirmed { instance } => Some(instance),
        _ => None,
    }
}
/// P2: all time and durable/provider evidence is supplied. No I/O or timer owner.
pub fn step(mut s: State, input: Input) -> (State, Vec<Action>) {
    if let Some(instance) = instance_event(&input.event)
        && !s.engine.as_ref().is_some_and(|e| e.instance == instance && !e.stopping)
    {
        return (s, Vec::new());
    }
    let old = s.clone();
    let was_open = old.gate_open();
    let mut a = Vec::new();
    let now = input.now_ms;
    if let Some(facts) = input.facts {
        s.facts = facts;
    }
    match &input.event {
        Event::Lifecycle { state, generation } => {
            s.facts.lifecycle = *state;
            s.facts.generation = *generation;
        }
        Event::CollectionReplaced { generation } => {
            s.facts.generation = *generation;
            s.facts.fence = Fence::DecisionRequired;
        }
        Event::Binding(binding) => s.facts.binding = binding.clone(),
        Event::PublicationFence(fence) => s.facts.fence = *fence,
        Event::SessionStarted(session) => s.facts.session = Some(*session),
        Event::SessionEnded(session) => {
            if s.facts.session == Some(*session) {
                s.facts.session = None;
            }
        }
        Event::DeviceSettings(settings) => s.settings = *settings,
        Event::Account(account) => {
            if &s.account != account {
                bump(&mut s);
            }
            s.account = account.clone();
        }
        Event::Network { reachable } => {
            s.reachable = *reachable;
            if *reachable
                && s.last_error
                    .as_ref()
                    .is_some_and(|e| e.class == OperationError::Offline)
            {
                s.last_error = None;
            }
        }
        Event::TrackingWriteFailed { no_space } => {
            s.tracking_stop = Some(*no_space);
            s.probe_in_progress = false;
        }
        Event::TrackingChanged(cause) if s.tracking_stop.is_some() => {
            let probe_commit = *cause == ChangeCause::Probe && s.probe_in_progress;
            let local_commit = *cause == ChangeCause::Local && !s.local_restart_used;
            if probe_commit || local_commit {
                s.tracking_stop = None;
                s.probe_in_progress = false;
                if local_commit {
                    s.local_restart_used = true;
                }
            }
        }
        Event::Foreground => {
            s.foreground = true;
            s.local_restart_used = false;
        }
        Event::Background { .. } => s.foreground = false,
        Event::EngineStarted { instance, binding } => {
            if s.facts.binding.as_ref().is_none_or(|b| b.id != *binding)
                || s.engine
                    .as_ref()
                    .is_some_and(|e| !e.stopping && e.instance != *instance)
            {
                return (old, Vec::new());
            }
            s.engine = Some(Engine {
                instance: instance.clone(),
                binding: *binding,
                stopping: false,
            });
            s.root_confirmed = false;
            s.drained = [false; 2];
            for cycle in s.cycles.values_mut() {
                cycle.active = false;
            }
        }
        Event::EngineStopped { instance } => {
            if s.engine.as_ref().is_some_and(|e| e.instance == *instance) {
                s.engine = None;
                s.root_confirmed = false;
                s.drained = [false; 2];
            } else {
                return (old, Vec::new());
            }
        }
        Event::EngineActivity { operation, phase, .. } => {
            let cycle = s.cycles.entry(*operation).or_default();
            match phase {
                Activity::Started => {
                    cycle.active = true;
                    cycle.error = None;
                    if operation.kind == OperationKind::Fetch {
                        match operation.zone {
                            Zone::Root => s.drained[0] = false,
                            Zone::Collection => s.drained[1] = false,
                            Zone::Media => {}
                        }
                    }
                }
                Activity::Ended { outcome, saved } => {
                    cycle.active = false;
                    if *outcome == Outcome::Ok {
                        cycle.failures = 0;
                        cycle.cloud_error = false;
                        cycle.error = None;
                        if operation.kind != OperationKind::Read {
                            s.transient_auth = false;
                        }
                        if operation.kind == OperationKind::Send && *saved > 0 {
                            s.quota_full = false;
                        }
                        if s.last_error.as_ref().is_some_and(|e| e.operation == *operation) {
                            s.last_error = None;
                        }
                    } else {
                        cycle.failures = cycle.failures.saturating_add(1);
                        if cycle.failures >= 3 && cycle.error.as_ref().is_none_or(|e| e.class == OperationError::Other)
                        {
                            cycle.cloud_error = true;
                        }
                    }
                }
            }
        }
        Event::EngineError {
            operation,
            error,
            code,
            retry_after_ms,
            ..
        } => {
            let detail = ErrorDetail {
                class: *error,
                code: *code,
                retry_after_ms: *retry_after_ms,
                operation: *operation,
            };
            let cycle = s.cycles.entry(*operation).or_default();
            cycle.error = Some(detail.clone());
            if *code == 12 {
                cycle.cloud_error = true;
            } // CKError.invalidArguments, immediately.
            s.last_error = Some(detail);
            match error {
                OperationError::QuotaFull => s.quota_full = true,
                OperationError::NoAccount if operation.kind != OperationKind::Read => s.transient_auth = true,
                OperationError::Configuration => {
                    s.account.status = AccountStatus::Configuration;
                    bump(&mut s);
                }
                OperationError::UnsupportedSchema => {
                    s.facts.summary.attention.insert(Attention::UnsupportedSchema, 1);
                }
                _ => {}
            }
        }
        Event::FetchDrained { zone, .. } => {
            match zone {
                Zone::Root => s.drained[0] = true,
                Zone::Collection => s.drained[1] = true,
                Zone::Media => {}
            }
            if *zone != Zone::Media {
                s.drain_serial = s.drain_serial.saturating_add(1);
            }
        }
        Event::RootConfirmed { .. } => s.root_confirmed = true,
        Event::EvidenceCommitted(evidence) => s.evidence = evidence.as_ref().clone(),
        _ => {}
    }
    let binding_changed = s.facts.binding != old.facts.binding;
    let owner_changed = s.facts.generation != old.facts.generation
        || (binding_changed && !binding_progress(&old.facts.binding, &s.facts.binding));
    if matches!(input.event, Event::Binding(_)) && !binding_changed {
        bump(&mut s);
    }
    if owner_changed
        || matches!(input.event, Event::CollectionReplaced { .. })
        || (s.facts.lifecycle == Lifecycle::NoCollection && old.facts.lifecycle != Lifecycle::NoCollection)
    {
        bump(&mut s);
        s.drained = [false; 2];
        s.last_cleanup_ms = None;
        if s.facts.generation != old.facts.generation
            || s.facts.binding.as_ref().map(|b| b.id) != old.facts.binding.as_ref().map(|b| b.id)
        {
            s.evidence = Evidence::default();
        }
        s.quota_full = false;
        s.transient_auth = false;
        s.cycles.clear();
        s.last_error = None;
    }
    if s.facts
        .binding
        .as_ref()
        .is_some_and(|b| b.publish_step == Some(PublishStep::Done))
        && s.engine_runs()
    {
        s.root_confirmed = true;
    }
    let open = s.gate_open();
    let stopped_row = !s.engine_allowed();
    let tracking_failure = matches!(input.event, Event::TrackingWriteFailed { .. });
    if tracking_failure {
        stop(&mut s, &mut a, StopReason::TrackingFailed);
        cancel_timer(&mut s, &mut a);
        cancel_exchange(&s, &mut a);
        a.push(ActionKind::CancelBackgroundFlush);
        a.push(ActionKind::PauseMedia);
    } else if (!open && was_open) || (stopped_row && s.engine.as_ref().is_some_and(|e| !e.stopping)) {
        cancel_timer(&mut s, &mut a);
        if stopped_row {
            a.push(ActionKind::CancelBackgroundFlush);
            cancel_exchange(&s, &mut a);
            a.push(ActionKind::PauseMedia);
            stop(&mut s, &mut a, StopReason::GateClosed);
        }
    }
    let binding_swap = open
        && s.engine
            .as_ref()
            .is_some_and(|e| s.facts.binding.as_ref().is_some_and(|b| e.binding != b.id));
    if binding_swap {
        stop(&mut s, &mut a, StopReason::BindingChanged);
    }
    let opening = open && (!was_open || binding_swap);
    if opening {
        if !s.engine_runs() {
            if let Some(b) = &s.facts.binding {
                a.push(ActionKind::StartEngine {
                    binding: b.id,
                    automatic: s.automatic(),
                });
            }
        } else {
            a.push(ActionKind::UpdatePending(Update::Full));
        }
        if s.automatic() {
            fetch(&mut a);
            send(&s, &mut a);
            if !s.engine_runs() {
                resume_media(&s, &mut a);
            }
        }
    } else if !tracking_failure {
        match &input.event {
            Event::TrackingChanged(cause) if open => {
                a.push(ActionKind::UpdatePending(if *cause == ChangeCause::Local {
                    Update::Full
                } else {
                    Update::Changes
                }));
                if *cause == ChangeCause::Local
                    && s.automatic()
                    && s.foreground
                    && sendable(&s)
                    && s.flush_at_ms.is_none()
                {
                    let at_ms = now.saturating_add(60_000);
                    s.flush_at_ms = Some(at_ms);
                    a.push(ActionKind::ArmFlushTimer { at_ms });
                }
                if *cause == ChangeCause::Local {
                    media_reads(&s, &mut a);
                }
            }
            Event::DeviceSettings(_) => {
                if s.automatic() != old.automatic() && (!s.automatic() || open) {
                    a.push(ActionKind::ConfigureAutomatic { on: s.automatic() });
                    if !s.automatic() {
                        cancel_timer(&mut s, &mut a);
                    } else if open {
                        a.push(ActionKind::UpdatePending(Update::Full));
                        fetch(&mut a);
                        send(&s, &mut a);
                        resume_media(&s, &mut a);
                    }
                }
                if s.settings.media != old.settings.media {
                    if !s.settings.media {
                        a.push(ActionKind::PauseMedia);
                    } else if open && s.automatic() {
                        resume_media(&s, &mut a);
                    }
                }
            }
            Event::FlushTimerFired => {
                s.flush_at_ms = None;
                if open && s.automatic() && s.foreground && s.facts.summary.sendable > 0 {
                    a.push(ActionKind::RequestSend {
                        zones: vec![Zone::Collection],
                    });
                }
            }
            Event::SessionEnded(_) if open => session_ended(&mut s, &mut a),
            Event::Account(_) if open => a.push(ActionKind::UpdatePending(Update::Full)),
            Event::Binding(_) if open => {
                if old.automatic() && !s.automatic() {
                    a.push(ActionKind::ConfigureAutomatic { on: false });
                }
                a.push(ActionKind::UpdatePending(Update::Full));
                if s.automatic() {
                    send(&s, &mut a);
                }
            }
            Event::RootConfirmed { .. } if open => {
                a.push(ActionKind::UpdatePending(Update::Full));
                if s.automatic() {
                    send(&s, &mut a);
                }
            }
            Event::FetchDrained {
                zone: Zone::Collection, ..
            } if open => {
                let cleanup = s.root_confirmed
                    && s.last_cleanup_ms
                        .is_none_or(|at| now.checked_sub(at).is_some_and(|age| age >= 86_400_000));
                if cleanup {
                    s.last_cleanup_ms = Some(now);
                    a.push(ActionKind::RunCleanup);
                }
                a.push(ActionKind::UpdatePending(Update::Full));
                if s.root_confirmed && (cleanup || s.facts.summary.zone_deletions_due > 0) {
                    a.push(ActionKind::DeleteZones);
                }
                if s.facts.summary.asset_retryable > 0 {
                    a.push(ActionKind::RetryAssetEntries);
                }
                media_reads(&s, &mut a);
            }
            Event::Foreground => {
                probe(&mut s, &mut a);
                if open {
                    a.push(ActionKind::UpdatePending(Update::Full));
                    if s.automatic() {
                        fetch(&mut a);
                        send(&s, &mut a);
                        resume_media(&s, &mut a);
                    }
                } else if s.engine_runs() && s.engine_allowed() && !s.facts.summary.crt_mismatch && s.automatic() {
                    fetch(&mut a);
                }
            }
            Event::Background { deadline_ms } => {
                cancel_timer(&mut s, &mut a);
                if s.exchange.is_some() || (open && s.automatic() && flush_work(&s)) {
                    let until_ms = deadline_ms.saturating_sub(5_000).min(now.saturating_add(25_000));
                    s.background_until_ms = Some(until_ms);
                    a.push(ActionKind::BackgroundFlush { until_ms });
                }
            }
            Event::BackgroundExpiring => {
                s.background_until_ms = None;
                a.push(ActionKind::CancelBackgroundFlush);
                cancel_exchange(&s, &mut a);
            }
            Event::QuitRequested => {
                if s.exchange.is_some() || (open && s.automatic() && flush_work(&s)) {
                    let until_ms = now.saturating_add(10_000);
                    s.quit_until_ms = Some(until_ms);
                    a.push(ActionKind::QuitFlush { until_ms });
                } else {
                    a.push(ActionKind::QuitNow);
                }
            }
            Event::VolumeSpaceAvailable => {
                probe(&mut s, &mut a);
                if open && s.automatic() && s.facts.summary.media.no_space == Some(true) {
                    resume_media(&s, &mut a);
                }
            }
            Event::VisibleMedia { names } if s.settings.media => {
                a.push(ActionKind::PrioritizeMedia { names: names.clone() });
            }
            _ => {}
        }
    }
    // Session facts come only from the registry in the composition driver.
    if old.facts.session.is_some()
        && s.facts.session != old.facts.session
        && open
        && !matches!(input.event, Event::SessionEnded(_))
        && !opening
    {
        session_ended(&mut s, &mut a);
    }
    if s.tracking_stop.is_some() && matches!(input.event, Event::Account(_)) && s.account != old.account {
        probe(&mut s, &mut a);
    }
    match &input.event {
        Event::SyncNowRequested(source) if s.sync_now_availability().is_ok() && s.exchange.is_none() => {
            probe(&mut s, &mut a);
            if let Some(id) = s.next_exchange.checked_add(1) {
                s.next_exchange = id;
                s.exchange = Some(Exchange {
                    id,
                    source: *source,
                    guard: s.guard(),
                });
                a.push(ActionKind::RunExchange {
                    exchange: id,
                    source: *source,
                });
            }
        }
        Event::SyncNowCancelled { exchange } if s.exchange.as_ref().is_some_and(|e| e.id == *exchange) => {
            a.push(ActionKind::CancelExchange { exchange: *exchange })
        }
        Event::ExchangeEnded { exchange, outcome } if s.exchange.as_ref().is_some_and(|e| e.id == *exchange) => {
            let e = s.exchange.take().expect("matched exchange");
            let start_media = open
                && s.settings.media
                && e.guard == s.guard()
                && matches!(
                    outcome,
                    ExchangeOutcome::Succeeded { .. } | ExchangeOutcome::Incomplete { .. }
                );
            let record = ExchangeRecord {
                exchange: e.id,
                ended_ms: now,
                source: e.source,
                outcome: outcome.clone(),
            };
            let record_action = Action {
                guard: Guard {
                    token: s.token,
                    ..e.guard
                },
                kind: ActionKind::RecordExchange(record),
            };
            let mut actions: Vec<_> = a.into_iter().map(|kind| Action { guard: s.guard(), kind }).collect();
            actions.push(record_action);
            // P12.3 step 7 follows accounting, independently of automatic scheduling.
            if start_media {
                actions.push(Action {
                    guard: s.guard(),
                    kind: ActionKind::ResumeMedia,
                });
            }
            finish_status(&mut s, &mut actions);
            return (s, actions);
        }
        _ => {}
    }
    let root_trigger = matches!(
        input.event,
        Event::TrackingChanged(ChangeCause::Local)
            | Event::FetchDrained {
                zone: Zone::Collection,
                ..
            }
    ) || matches!(input.event, Event::Network { reachable: true }) && !old.reachable;
    if !opening
        && open
        && s.automatic()
        && !s.root_confirmed
        && root_trigger
        && !s
            .facts
            .binding
            .as_ref()
            .is_some_and(|b| b.publish_step.is_some_and(|p| p != PublishStep::Done))
        && !s
            .cycles
            .iter()
            .any(|(op, c)| op.zone == Zone::Root && op.kind == OperationKind::Fetch && c.active)
    {
        a.push(ActionKind::RequestFetch {
            zones: vec![Zone::Root],
        });
    }
    if !old.reachable && s.reachable && open && s.automatic() {
        resume_media(&s, &mut a);
    }
    let report_only = matches!(
        input.event,
        Event::EngineError { .. } | Event::EngineActivity { .. } | Event::EvidenceCommitted(_)
    );
    if open
        && !report_only
        && !s.facts.summary.apply_in_progress
        && (s.facts.summary.ready > 0 || full_trigger(&input.event) && s.facts.summary.apply_failed > 0)
    {
        a.push(ActionKind::ApplyReady {
            retry_failed: full_trigger(&input.event),
        });
    }
    if open
        && s.engine_runs()
        && s.drained == [true, true]
        && s.drain_serial != s.quiet_serial
        && s.facts.summary.collection_known
        && s.facts.summary.pending_unheld == 0
        && s.facts.summary.sendable == 0
        && s.facts.summary.parked == 0
        && s.facts.summary.in_flight == 0
        && s.facts.summary.ready == 0
        && !s.facts.summary.apply_in_progress
        && s.facts.summary.unsettled_intents == 0
    {
        s.quiet_serial = s.drain_serial;
        a.push(ActionKind::RecordQuietPoint { at_ms: now });
    }
    let mut actions = a.into_iter().map(|kind| Action { guard: s.guard(), kind }).collect();
    finish_status(&mut s, &mut actions);
    (s, actions)
}
fn finish_status(s: &mut State, actions: &mut Vec<Action>) {
    let status = s.status();
    if s.last_status.as_ref() != Some(&status) {
        s.last_status = Some(status.clone());
        actions.push(Action {
            guard: s.guard(),
            kind: ActionKind::StatusChanged(Box::new(status)),
        });
    }
}
fn flush_work(s: &State) -> bool {
    sendable(s) || s.facts.summary.apply_in_progress || s.facts.summary.ready > 0
}
fn session_ended(s: &mut State, a: &mut Vec<ActionKind>) {
    if s.automatic() {
        cancel_timer(s, a);
    }
    a.push(ActionKind::UpdatePending(Update::Full));
    if s.automatic() {
        send(s, a);
    }
}
fn media_reads(s: &State, a: &mut Vec<ActionKind>) {
    if s.automatic()
        && s.settings.media
        && s.facts.summary.media.root_acked == Some(true)
        && s.facts.summary.media.unknown_blobs.is_some_and(|n| n > 0)
        && !s
            .cycles
            .iter()
            .any(|(op, c)| op.zone == Zone::Media && op.kind == OperationKind::Read && c.active)
    {
        a.push(ActionKind::ReadBlobs { include_missing: false });
    }
}
