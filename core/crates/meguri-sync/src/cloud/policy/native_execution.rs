//! Pure Rust execution ports. No provider or native callback runs under Driver authority.
use super::*;
use crate::cloud::incoming::{AssetReadKind, AssetReadPass, AssetReadRequest};

impl Driver {
    /// Retain this exact issued command for the entire once-per-action pass.
    pub fn begin_native_asset_read_pass(
        &self,
        command: &Command,
        sdk: &str,
        collection: &Collection,
    ) -> Result<AssetReadPass, DriverError> {
        self.begin_native_asset_read_pass_with_work(command, sdk, collection, &mut PolicyWork::default())
    }

    pub fn begin_native_asset_read_pass_with_work(
        &self,
        command: &Command,
        sdk: &str,
        collection: &Collection,
        work: &mut PolicyWork,
    ) -> Result<AssetReadPass, DriverError> {
        self.validate_with_work(command, work)?;
        if command.action.kind != ActionKind::RetryAssetEntries {
            return Err(DriverError::NotCoreAction);
        }
        Ok(self.runtime.begin_asset_read_pass_owned_with_work(
            sdk,
            collection,
            AssetReadKind::Retry,
            Some(command.clone()),
            work,
        )?)
    }

    pub fn next_native_asset_read(
        &self,
        command: &Command,
        collection: &Collection,
        pass: &AssetReadPass,
    ) -> Result<Option<AssetReadRequest>, DriverError> {
        self.validate(command)?;
        if !self.runtime.asset_pass_command(pass, command) {
            return Err(DriverError::StaleCommand);
        }
        Ok(self.runtime.next_asset_read(collection, pass)?)
    }

    pub fn next_native_asset_read_with_work(
        &self,
        command: &Command,
        collection: &Collection,
        pass: &AssetReadPass,
        work: &mut PolicyWork,
    ) -> Result<Option<AssetReadRequest>, DriverError> {
        self.validate_with_work(command, work)?;
        let mut command_error = None;
        let result = self
            .runtime
            .next_asset_read_checked(collection, pass, work, |retained| {
                // validate_with_work checked all command/Runtime facts before the
                // wait. with_scan rechecks the original guard and SDK under State.
                // For RetryAssetEntries there is no automatic-setting or quiet
                // proof: the committed policy guard completes the same proof here.
                // Never call validate_native_execution (or take Driver) under State.
                let check = if retained != Some(command) {
                    Err(DriverError::StaleCommand)
                } else {
                    self.committed_native_validation().validate_policy(command)
                };
                check.map_err(|error| {
                    command_error = Some(error);
                    RuntimeError::StaleIncarnation
                })
            });
        if let Some(error) = command_error {
            return Err(error);
        }
        Ok(result?)
    }

    /// Same synchronous queue contract as native control admission. Validation
    /// releases Driver authority before Runtime checks the exact selection and
    /// invokes foreign enqueue. Neither lock spans that callback or reply wait.
    pub fn submit_native_asset_read_queue(
        &self,
        command: &Command,
        collection: &Collection,
        request: &AssetReadRequest,
        enqueue: impl FnOnce() -> bool,
    ) -> Result<bool, DriverError> {
        self.submit_native_asset_read_queue_with_work(command, collection, request, &mut PolicyWork::default(), enqueue)
    }

    pub fn submit_native_asset_read_queue_with_work(
        &self,
        command: &Command,
        collection: &Collection,
        request: &AssetReadRequest,
        work: &mut PolicyWork,
        enqueue: impl FnOnce() -> bool,
    ) -> Result<bool, DriverError> {
        if let Err(error) = self.validate_with_work(command, work) {
            if self.runtime.asset_request_command(request, command) {
                self.runtime.cancel_asset_read(request);
            }
            return Err(error);
        }
        if !self.runtime.asset_request_command(request, command) {
            return Err(DriverError::StaleCommand);
        }
        let mut command_error = None;
        let admission = self
            .runtime
            .submit_asset_read_queue_with_work(collection, request, work, || {
                // Validate the command after Runtime checks the file and reader.
                // Policy changes during preparation must be reflected at admission.
                if let Err(error) = self.validate_native_sdk(command, request.sdk_instance()) {
                    command_error = Some(error);
                    return false;
                }
                enqueue()
            });
        if let Some(error) = command_error {
            return Err(error);
        }
        Ok(admission?)
    }

    pub fn native_asset_read_result(
        &self,
        command: &Command,
        collection: &Collection,
        request: &AssetReadRequest,
        result: Option<&crate::cloud::workflow::FetchedRecord>,
    ) -> Result<bool, DriverError> {
        self.native_asset_read_result_with_work(command, collection, request, result, &mut PolicyWork::default())
    }

    pub fn native_asset_read_result_with_work(
        &self,
        command: &Command,
        collection: &Collection,
        request: &AssetReadRequest,
        result: Option<&crate::cloud::workflow::FetchedRecord>,
        work: &mut PolicyWork,
    ) -> Result<bool, DriverError> {
        if let Err(error) = self.validate_with_work(command, work) {
            if self.runtime.asset_request_command(request, command) {
                self.runtime.retire_asset_read(request);
            }
            return Err(error);
        }
        if !self.runtime.asset_request_command(request, command) {
            return Err(DriverError::StaleCommand);
        }
        let mut command_error = None;
        let value =
            self.runtime
                .asset_read_result_checked(request.sdk_instance(), collection, request, result, work, || {
                    self.committed_native_validation()
                        .validate_policy(command)
                        .map_err(|error| {
                            command_error = Some(error);
                            RuntimeError::StaleIncarnation
                        })
                });
        if let Some(error) = command_error {
            return Err(error);
        }
        Ok(value?)
    }
}

impl Driver {
    /// Qualify only the owned RunExchange preparation; this supplies no fetch evidence.
    pub fn native_sync_now_epoch_read(&self, command: &Command, sdk: &str, collection: &Collection)
        -> Result<Option<crate::cloud::adapter::FetchRead>, DriverError>
    {
        {
            let owned = self.lock_for_work(&PolicyWork::default())?;
            self.validate_locked(&owned, command)?;
            if !matches!(command.action.kind, ActionKind::RunExchange { .. }) { return Err(DriverError::NotCoreAction); }
            let s = &owned.state;
            if !s.read_gate_open() || s.facts.summary.crt_known || s.facts.summary.crt_mismatch
                || s.facts.summary.apply_in_progress || !s.engine_runs()
            { return Ok(None); }
        }
        self.validate_native_sdk(command, sdk)?;
        Ok(self.runtime.sync_now_epoch_read_for_exchange(sdk, collection)?)
    }
    /// P12.3 step 1. Only this query decides whether native may keep waiting.
    /// The original command guard is never rebound to a fresh account/token.
    pub fn native_exchange_readiness(
        &self,
        command: &Command,
        expired: bool,
        native_ready: bool,
    ) -> Result<(bool, Option<ExchangeOutcome>), DriverError> {
        self.native_exchange_readiness_with_work(command, expired, native_ready, &mut PolicyWork::default())
    }
    pub fn native_exchange_readiness_with_work(
        &self,
        command: &Command,
        expired: bool,
        native_ready: bool,
        work: &mut PolicyWork,
    ) -> Result<(bool, Option<ExchangeOutcome>), DriverError> {
        let owned = self.lock_for_work(work)?;
        let ActionKind::RunExchange { exchange, .. } = command.action.kind else {
            return Err(DriverError::NotCoreAction);
        };
        let s = &owned.state;
        if !s
            .exchange
            .as_ref()
            .is_some_and(|e| e.id == exchange && e.guard == command.action.guard)
        {
            return Err(DriverError::StaleCommand);
        }
        if self.validate_locked(&owned, command).is_err() {
            let outcome = if !s.account_matches() || s.guard().binding != command.action.guard.binding {
                ExchangeOutcome::Failed {
                    class: OperationError::NoAccount,
                }
            } else {
                ExchangeOutcome::Cancelled
            };
            return Ok((false, Some(outcome)));
        }
        if !expired && s.gate_open() && s.engine_runs() && !s.facts.summary.apply_in_progress && native_ready {
            return Ok((true, None));
        }
        let temporary = s.settings.enabled
            && !s.settings.paused
            && s.account_matches()
            && s.facts.fence == Fence::Clear
            && !s.facts.summary.crt_mismatch
            && matches!(s.facts.lifecycle, Lifecycle::Ready | Lifecycle::ExclusiveBusy)
            && s.facts.binding.as_ref().is_some_and(|b| {
                b.usable && b.state == BindingState::Active && b.publish_step != Some(PublishStep::Cancelling)
            })
            && (s.tracking_stop.is_none() || s.probe_in_progress);
        if temporary && !expired {
            return Ok((false, None));
        }
        let outcome = if s.tracking_stop.is_some() && !s.probe_in_progress {
            ExchangeOutcome::Failed {
                class: OperationError::TrackingError,
            }
        } else if !s.account_matches() {
            ExchangeOutcome::Failed {
                class: OperationError::NoAccount,
            }
        } else if temporary {
            ExchangeOutcome::Waiting {
                class: Some(OperationError::Other),
                retry_after_ms: None,
            }
        } else {
            ExchangeOutcome::Cancelled
        };
        Ok((false, Some(outcome)))
    }

    /// App-owned cancellation ends activity without counting/resetting a failing
    /// cycle. The original command and SDK still have to be current.
    pub fn cancel_native_activity(
        &self,
        command: &Command,
        sdk: &str,
        operation: Operation,
    ) -> Result<(), DriverError> {
        self.cancel_native_activity_with_work(command, sdk, operation, &mut PolicyWork::default())
    }

    pub fn cancel_native_activity_with_work(
        &self,
        command: &Command,
        sdk: &str,
        operation: Operation,
        work: &mut PolicyWork,
    ) -> Result<(), DriverError> {
        let mut owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        self.runtime.validate_sdk(sdk)?;
        if let Some(cycle) = owned.state.cycles.get_mut(&operation) {
            work.effect()?;
            cycle.active = false;
        }
        Ok(())
    }

    /// Only an actual original pass may cancel first-read activity. The retained
    /// guard includes file/open/token/account and remains valid after finish.
    pub fn cancel_original_asset_read_activity(
        &self,
        collection: &Collection,
        guard: &crate::cloud::runtime::AssetReadActivityGuard,
    ) -> Result<(), DriverError> {
        self.cancel_original_asset_read_activity_with_work(collection, guard, &mut PolicyWork::default())
    }

    pub fn cancel_original_asset_read_activity_with_work(
        &self,
        collection: &Collection,
        guard: &crate::cloud::runtime::AssetReadActivityGuard,
        work: &mut PolicyWork,
    ) -> Result<(), DriverError> {
        self.runtime.validate_asset_read_activity(collection, guard)?;
        let mut owned = self.lock_for_work(work)?;
        self.runtime.validate_sdk(guard.sdk_instance())?;
        let operation = Operation {
            zone: Zone::Collection,
            kind: OperationKind::Read,
        };
        if let Some(cycle) = owned.state.cycles.get_mut(&operation) {
            work.effect()?;
            cycle.active = false;
        }
        Ok(())
    }

    pub fn native_origin(&self, command: &Command) -> Result<Option<String>, DriverError> {
        self.native_origin_with_work(command, &mut PolicyWork::default())
    }
    pub fn native_origin_with_work(
        &self,
        command: &Command,
        work: &mut PolicyWork,
    ) -> Result<Option<String>, DriverError> {
        self.native_origin_attempt_with_work(command, work)
            .map_err(|failure| failure.error)
    }
    pub fn native_origin_attempt_with_work(
        &self,
        command: &Command,
        work: &mut PolicyWork,
    ) -> Result<Option<String>, CommandAttemptFailure> {
        let owned = self.lock_for_work(work)?;
        self.validate_original_attempt(&owned, command, matches!(command.action.kind, ActionKind::ResumeMedia))?;
        let sdk = self
            .runtime
            .policy_sdk_origin(&command.backend_guard)
            .map_err(DriverError::from)?;
        work.check().map_err(DriverError::from)?;
        Ok(sdk)
    }

    pub fn native_media_origin_with_work(
        &self,
        command: &Command,
        work: &mut PolicyWork,
    ) -> Result<Option<crate::cloud::adapter::PolicyMediaOrigin>, CommandAttemptFailure> {
        let owned = self.lock_for_work(work)?;
        self.validate_original_attempt(&owned, command, true)?;
        if command.action.kind != ActionKind::ResumeMedia {
            return Err(DriverError::NotCoreAction.into());
        }
        let origin = self.runtime.policy_media_origin(&command.backend_guard).map_err(DriverError::from)?;
        work.check().map_err(DriverError::from)?;
        Ok(origin)
    }

    pub fn register_native_sdk(&self, command: &Command) -> Result<(String, Option<Vec<u8>>, String), DriverError> {
        self.register_native_sdk_with_work(command, &mut PolicyWork::default())
    }
    pub fn register_native_sdk_with_work(
        &self,
        command: &Command,
        work: &mut PolicyWork,
    ) -> Result<(String, Option<Vec<u8>>, String), DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        let automatic = match command.action.kind {
            ActionKind::StartEngine { automatic, .. } => automatic,
            ActionKind::ConfigureAutomatic { on } => on,
            _ => return Err(DriverError::NotCoreAction),
        };
        // validate_locked also checks the current scheduling preference. Holding
        // Driver authority through registration excludes a concurrent settings input.
        Ok(self
            .runtime
            .register_sdk_guarded_with_work(&command.backend_guard, automatic, work)?)
    }

    pub fn validate_native_sdk(&self, command: &Command, sdk: &str) -> Result<(), DriverError> {
        self.validate_native_execution(command, Some(sdk))
    }

    /// Final validation of the original issued command and pinned SDK only.
    /// Each compact read releases its mutex before any Runtime access. Preserve
    /// policy-stale precedence even if Runtime failed while policy committed.
    pub fn validate_native_execution(&self, command: &Command, sdk: Option<&str>) -> Result<(), DriverError> {
        let view = self.committed_native_validation();
        view.validate_policy(command)?;
        let result = (|| {
            let runtime = self.runtime.native_authority();
            self.validate_runtime_facts(&view, command, &runtime.guard, || {
                runtime.instance.clone().unwrap_or_default()
            })?;
            // Quiet completion is qualified Core work, never native authority.
            if command.quiet_proof.is_some() || matches!(command.action.kind, ActionKind::RecordQuietPoint { .. }) {
                return Err(DriverError::UnqualifiedCompletion);
            }
            let sdk = sdk.ok_or(DriverError::StaleCommand)?;
            if matches!(command.action.kind, ActionKind::RequestSend { .. } | ActionKind::ResumeMedia) {
                runtime.require_crt_equal()?;
            }
            Ok(runtime.validate_sdk(sdk)?)
        })();
        #[cfg(feature = "test-hooks")]
        {
            let hook = self
                .native_validation_hook
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(hook) = hook {
                hook();
            }
        }
        self.committed_native_validation().validate_policy(command)?;
        result
    }

    pub fn retire_native_sdk(&self, command: &Command, sdk: &str) -> Result<(), DriverError> {
        self.retire_native_sdk_with_work(command, sdk, &mut PolicyWork::default())
    }
    pub fn retire_native_sdk_with_work(
        &self,
        command: &Command,
        sdk: &str,
        work: &mut PolicyWork,
    ) -> Result<(), DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        if !matches!(
            command.action.kind,
            ActionKind::StopEngine { .. } | ActionKind::ConfigureAutomatic { .. }
        ) {
            return Err(DriverError::NotCoreAction);
        }
        let origin = self.runtime.policy_sdk_origin(&command.backend_guard)?;
        work.check()?;
        match origin {
            None => Ok(()), // The original creation/stop already drained its own SDK.
            Some(current) if current == sdk => {
                if matches!(command.action.kind, ActionKind::ConfigureAutomatic { .. }) {
                    Ok(self.runtime.retire_sdk_for_configuration_with_work(sdk, work)?)
                } else {
                    Ok(self.runtime.retire_sdk_with_work(sdk, false, work)?)
                }
            }
            Some(_) => Err(DriverError::StaleCommand),
        }
    }

    pub fn begin_native_fetch(&self, command: &Command, sdk: &str, zones: Vec<String>) -> Result<u64, DriverError> {
        self.begin_native_fetch_with_work(command, sdk, zones, &mut PolicyWork::default())
    }
    pub fn begin_native_fetch_with_work(
        &self,
        command: &Command,
        sdk: &str,
        zones: Vec<String>,
        work: &mut PolicyWork,
    ) -> Result<u64, DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        work.check()?;
        let requested = match &command.action.kind {
            ActionKind::RequestFetch { zones } => zones.clone(),
            ActionKind::RunExchange { .. } | ActionKind::StartEngine { .. } | ActionKind::ConfigureAutomatic { .. } => {
                vec![Zone::Root, Zone::Collection]
            }
            _ => return Err(DriverError::NotCoreAction),
        };
        let scope = self.runtime.policy_fetch_scope(&command.backend_guard, &requested)?;
        work.check()?;
        if scope != zones {
            return Err(DriverError::StaleCommand);
        }
        Ok(self.runtime.adapter_begin_fetch_with_work(sdk, zones, work)?)
    }

    pub fn begin_native_send(&self, command: &Command, sdk: &str) -> Result<Option<u64>, DriverError> {
        self.begin_native_send_with_work(command, sdk, &mut PolicyWork::default())
    }
    pub fn begin_native_send_with_work(
        &self,
        command: &Command,
        sdk: &str,
        work: &mut PolicyWork,
    ) -> Result<Option<u64>, DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        work.check()?;
        match &command.action.kind {
            ActionKind::RequestSend { zones } if zones == &[Zone::Collection] => {}
            ActionKind::RequestSend { zones }
                if zones == &[Zone::Media] && owned.state.settings.media && owned.state.gate_open() => {}
            ActionKind::ResumeMedia if owned.state.settings.media && owned.state.gate_open() => {}
            ActionKind::ReadBlobs { .. }
                if owned.state.settings.media && owned.state.automatic() && owned.state.gate_open() => {}
            ActionKind::RunExchange { .. } | ActionKind::BackgroundFlush { .. } | ActionKind::QuitFlush { .. } => {}
            ActionKind::RequestSend { .. } => return Err(DriverError::CoreCapabilityUnavailable("cloud media")),
            _ => return Err(DriverError::NotCoreAction),
        }
        self.runtime.validate_sdk(sdk)?;
        work.check()?;
        if matches!(
            command.action.kind,
            ActionKind::ResumeMedia | ActionKind::ReadBlobs { .. }
        ) {
            self.runtime
                .policy_update_pending_with_work(&command.backend_guard, Update::Full, work)?;
        }
        Ok(self
            .runtime
            .start_manual_send_call_with_work(sdk, work)?
            .map(|call| call.id))
    }
}

impl Driver {
    pub fn begin_native_media(&self, command: &Command, sdk: Option<&str>) -> Result<bool, DriverError> {
        self.begin_native_media_with_work(command, sdk, &mut PolicyWork::default())
    }
    pub fn begin_native_media_with_work(
        &self,
        command: &Command,
        sdk: Option<&str>,
        work: &mut PolicyWork,
    ) -> Result<bool, DriverError> {
        self.begin_native_media_origin_with_work(command, sdk, None, work)
    }
    pub fn begin_native_media_origin_with_work(
        &self,
        command: &Command,
        sdk: Option<&str>,
        origin: Option<&crate::cloud::adapter::PolicyMediaOrigin>,
        work: &mut PolicyWork,
    ) -> Result<bool, DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        work.check()?;
        match command.action.kind {
            ActionKind::ResumeMedia if owned.state.settings.media && owned.state.gate_open() => {}
            ActionKind::ReadBlobs { .. }
                if owned.state.settings.media && owned.state.automatic() && owned.state.gate_open() => {}
            ActionKind::PauseMedia | ActionKind::PrioritizeMedia { .. } => {}
            _ => return Err(DriverError::StaleCommand),
        }
        Ok(self.runtime.policy_begin_media_with_work(
            &command.backend_guard, sdk, &command.action.kind, origin, work,
        )?)
    }
}

impl Driver {
    pub fn continue_native_media_after_send(
        &self,
        command: &Command,
        sdk: &str,
        code: Option<i64>,
    ) -> Result<bool, DriverError> {
        self.continue_native_media_after_send_with_work(command, sdk, code, &mut PolicyWork::default())
    }
    pub fn continue_native_media_after_send_with_work(
        &self,
        command: &Command,
        sdk: &str,
        code: Option<i64>,
        work: &mut PolicyWork,
    ) -> Result<bool, DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        work.check()?;
        if code != Some(25)
            || !owned.state.quota_full
            || !owned.state.settings.media
            || !owned.state.gate_open()
            || !matches!(
                command.action.kind,
                ActionKind::ResumeMedia | ActionKind::ReadBlobs { .. }
            )
        {
            return Ok(false);
        }
        self.runtime
            .policy_media_quota_with_work(&command.backend_guard, sdk, work)?;
        Ok(true)
    }
}

impl Driver {
    pub fn prepare_native_flush(&self, command: &Command, collection: &Collection) -> Result<(), DriverError> {
        self.prepare_native_flush_with_work(command, collection, &mut PolicyWork::default())
    }
    pub fn prepare_native_flush_with_work(
        &self,
        command: &Command,
        collection: &Collection,
        work: &mut PolicyWork,
    ) -> Result<(), DriverError> {
        let owned = self.lock_for_work(work)?;
        self.validate_locked(&owned, command)?;
        work.check()?;
        let until_ms = match command.action.kind {
            ActionKind::BackgroundFlush { until_ms } | ActionKind::QuitFlush { until_ms } => until_ms,
            _ => return Err(DriverError::NotCoreAction),
        };
        let remaining = until_ms.saturating_sub(self.clock.now_ms());
        if remaining <= 0 {
            return Err(meguri_engine::EngineError::Cancelled.into());
        }
        let timeout = Duration::from_millis(remaining as u64);
        if let crate::cloud::incoming::IncomingDisposition::Deferred(error) = self
            .runtime
            .policy_apply_ready_with_work(collection, &command.backend_guard, false, timeout, work)?
        {
            return Err(DriverError::ApplyDeferred(error));
        }
        self.runtime
            .policy_update_pending_with_work(&command.backend_guard, Update::Full, work)?;
        Ok(())
    }
}
