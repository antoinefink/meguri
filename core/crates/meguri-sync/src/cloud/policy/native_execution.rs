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
        self.validate(command)?;
        if command.action.kind != ActionKind::RetryAssetEntries {
            return Err(DriverError::NotCoreAction);
        }
        Ok(self
            .runtime
            .begin_asset_read_pass_owned(sdk, collection, AssetReadKind::Retry, Some(command.clone()))?)
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
        if let Err(error) = self.validate(command) {
            if self.runtime.asset_request_command(request, command) {
                self.runtime.cancel_asset_read(request);
            }
            return Err(error);
        }
        if !self.runtime.asset_request_command(request, command) {
            return Err(DriverError::StaleCommand);
        }
        let mut command_error = None;
        let admission = self.runtime.submit_asset_read_queue(collection, request, || {
            // Validate the command after Runtime checks the file and reader.
            // Policy changes during preparation must be reflected at admission.
            if let Err(error) = self.validate(command) {
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
        if let Err(error) = self.validate(command) {
            if self.runtime.asset_request_command(request, command) {
                self.runtime.retire_asset_read(request);
            }
            return Err(error);
        }
        if !self.runtime.asset_request_command(request, command) {
            return Err(DriverError::StaleCommand);
        }
        Ok(self
            .runtime
            .asset_read_result(request.sdk_instance(), collection, request, result)?)
    }
}

impl Driver {
    /// P12.3 step 1. Only this query decides whether native may keep waiting.
    /// The original command guard is never rebound to a fresh account/token.
    pub fn native_exchange_readiness(
        &self,
        command: &Command,
        expired: bool,
        native_ready: bool,
    ) -> Result<(bool, Option<ExchangeOutcome>), DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
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
                class: OperationError::Other,
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
        let mut owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        self.runtime.validate_sdk(sdk)?;
        if let Some(cycle) = owned.state.cycles.get_mut(&operation) {
            cycle.active = false;
        }
        Ok(())
    }

    pub fn native_origin(&self, command: &Command) -> Result<Option<String>, DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        Ok(self.runtime.policy_sdk_origin(&command.backend_guard)?)
    }

    pub fn register_native_sdk(&self, command: &Command) -> Result<(String, Option<Vec<u8>>, String), DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        let automatic = match command.action.kind {
            ActionKind::StartEngine { automatic, .. } => automatic,
            ActionKind::ConfigureAutomatic { on } => on,
            _ => return Err(DriverError::NotCoreAction),
        };
        // validate_locked also checks the current scheduling preference. Holding
        // Driver authority through registration excludes a concurrent settings input.
        Ok(self.runtime.register_sdk_guarded(&command.backend_guard, automatic)?)
    }

    pub fn validate_native_sdk(&self, command: &Command, sdk: &str) -> Result<(), DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        Ok(self.runtime.validate_sdk(sdk)?)
    }

    pub fn retire_native_sdk(&self, command: &Command, sdk: &str) -> Result<(), DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        if !matches!(
            command.action.kind,
            ActionKind::StopEngine { .. } | ActionKind::ConfigureAutomatic { .. }
        ) {
            return Err(DriverError::NotCoreAction);
        }
        match self.runtime.policy_sdk_origin(&command.backend_guard)? {
            None => Ok(()), // The original creation/stop already drained its own SDK.
            Some(current) if current == sdk => Ok(self.runtime.retire_sdk(sdk, false)?),
            Some(_) => Err(DriverError::StaleCommand),
        }
    }

    pub fn begin_native_fetch(&self, command: &Command, sdk: &str, zones: Vec<String>) -> Result<u64, DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        let requested = match &command.action.kind {
            ActionKind::RequestFetch { zones } => zones.clone(),
            ActionKind::RunExchange { .. } | ActionKind::StartEngine { .. } | ActionKind::ConfigureAutomatic { .. } => {
                vec![Zone::Root, Zone::Collection]
            }
            _ => return Err(DriverError::NotCoreAction),
        };
        if self.runtime.policy_fetch_scope(&command.backend_guard, &requested)? != zones {
            return Err(DriverError::StaleCommand);
        }
        Ok(self.runtime.adapter_begin_fetch(sdk, zones)?)
    }

    pub fn begin_native_send(&self, command: &Command, sdk: &str) -> Result<Option<u64>, DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
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
        if matches!(
            command.action.kind,
            ActionKind::ResumeMedia | ActionKind::ReadBlobs { .. }
        ) {
            self.runtime
                .policy_update_pending(&command.backend_guard, Update::Full)?;
        }
        Ok(self.runtime.start_manual_send_call(sdk)?.map(|call| call.id))
    }
}

impl Driver {
    pub fn begin_native_media(&self, command: &Command, sdk: Option<&str>) -> Result<bool, DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        match command.action.kind {
            ActionKind::ResumeMedia if owned.state.settings.media && owned.state.gate_open() => {}
            ActionKind::ReadBlobs { .. }
                if owned.state.settings.media && owned.state.automatic() && owned.state.gate_open() => {}
            ActionKind::PauseMedia | ActionKind::PrioritizeMedia { .. } => {}
            _ => return Err(DriverError::StaleCommand),
        }
        Ok(self
            .runtime
            .policy_begin_media(&command.backend_guard, sdk, &command.action.kind)?)
    }
}

impl Driver {
    pub fn continue_native_media_after_send(
        &self,
        command: &Command,
        sdk: &str,
        code: Option<i64>,
    ) -> Result<bool, DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
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
        self.runtime.policy_media_quota(&command.backend_guard, sdk)?;
        Ok(true)
    }
}

impl Driver {
    pub fn prepare_native_flush(&self, command: &Command, collection: &Collection) -> Result<(), DriverError> {
        let owned = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.validate_locked(&owned, command)?;
        let until_ms = match command.action.kind {
            ActionKind::BackgroundFlush { until_ms } | ActionKind::QuitFlush { until_ms } => until_ms,
            _ => return Err(DriverError::NotCoreAction),
        };
        let remaining = until_ms.saturating_sub(self.clock.now_ms());
        if remaining <= 0 {
            return Err(meguri_engine::EngineError::Cancelled.into());
        }
        let timeout = Duration::from_millis(remaining as u64);
        if let crate::cloud::incoming::IncomingDisposition::Deferred(error) =
            self.runtime
                .policy_apply_ready(collection, &command.backend_guard, false, timeout)?
        {
            return Err(DriverError::ApplyDeferred(error));
        }
        self.runtime
            .policy_update_pending(&command.backend_guard, Update::Full)?;
        Ok(())
    }
}
