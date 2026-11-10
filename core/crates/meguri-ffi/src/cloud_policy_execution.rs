//! Native execution authority stays attached to the issued command and original Host.
use super::*;
use crate::sync::CloudEngineRegistration;

#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicyReadiness {
    Ready,
    Wait,
    End { outcome: CloudPolicyEarlyEnd },
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyOperationFacts {
    pub zones: Vec<CloudPolicyZone>,
    pub kind: CloudPolicyOperationKind,
    pub started: bool,
    pub failed: bool,
    pub code: Option<i64>,
    pub retry_after_ms: Option<i64>,
    pub saved: u64,
    pub app_cancelled: bool,
}

#[derive(Default)]
struct Registration {
    sdk: Option<String>,
}

#[cfg(feature = "test-hooks")]
fn operation_policy_refusal(error: &CoreError) -> &'static str {
    match error {
        CoreError::CloudPolicy { failure } => match failure {
            CloudPolicyFailure::Closed => "Closed",
            CloudPolicyFailure::WrongOwner => "WrongOwner",
            CloudPolicyFailure::GenerationChanged => "GenerationChanged",
            CloudPolicyFailure::StaleCommand => "StaleCommand",
            CloudPolicyFailure::NotCoreAction => "NotCoreAction",
            CloudPolicyFailure::NotNativeAction => "NotNativeAction",
            CloudPolicyFailure::CoreCapabilityUnavailable => "CoreCapabilityUnavailable",
            CloudPolicyFailure::UnqualifiedCompletion => "UnqualifiedCompletion",
        },
        _ => "OtherCoreError",
    }
}

#[cfg(feature = "test-hooks")]
fn operation_driver_refusal(site: &'static str, error: &p::DriverError) {
    let (kind, variant) = match error {
        p::DriverError::Runtime(error) => error.refusal_tags_for_testing(),
        p::DriverError::CoreActionFailed { error, .. } => error.refusal_tags_for_testing(),
        p::DriverError::ApplyDeferred(_) => ("Driver", "ApplyDeferred"),
        p::DriverError::StaleCommand => ("Driver", "StaleCommand"),
        p::DriverError::Unavailable(_) => ("Driver", "Unavailable"),
        p::DriverError::NotCoreAction => ("Driver", "NotCoreAction"),
        p::DriverError::CoreCapabilityUnavailable(_) => ("Driver", "CoreCapabilityUnavailable"),
        p::DriverError::UnqualifiedCompletion => ("Driver", "UnqualifiedCompletion"),
    };
    eprintln!("cloudOperation.refused site={site} errorKind={kind} variant={variant}");
}

#[derive(uniffi::Object)]
pub struct CloudPolicyExecution {
    pub(super) command: Arc<CloudPolicyCommand>,
    original_sdk: Option<String>,
    registration: Mutex<Registration>,
    pub(crate) cleanup_begun: std::sync::atomic::AtomicBool,
    pub(crate) zone_deletion_begun: std::sync::atomic::AtomicBool,
}

impl CloudPolicyExecution {
    pub(super) fn operation_activity_with_work(
        &self,
        facts: CloudPolicyOperationFacts,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyOutput, CoreError> {
        let owner = self.policy().owns(&self.command);
        #[cfg(feature = "test-hooks")]
        let owner = owner.inspect_err(|error| {
            let variant = operation_policy_refusal(error);
            eprintln!("cloudOperation.refused site=commandValidation step=owner variant={variant}");
        });
        owner?;
        let driver = self.policy().driver();
        #[cfg(feature = "test-hooks")]
        let driver = driver.inspect_err(|error| {
            let variant = operation_policy_refusal(error);
            eprintln!("cloudOperation.refused site=commandValidation step=driver variant={variant}");
        });
        let validation = driver?.validate_with_work(&self.command.command, work);
        #[cfg(feature = "test-hooks")]
        let validation = validation.inspect_err(|error| operation_driver_refusal("commandValidation", error));
        validation.map_err(execution_error)?;
        let sdk = self.sdk();
        #[cfg(feature = "test-hooks")]
        let sdk = sdk.inspect_err(|error| {
            let variant = operation_policy_refusal(error);
            eprintln!("cloudOperation.refused site=originalSDKValidation step=originalSDK variant={variant}");
        });
        let sdk = sdk?;
        let runtime = self
            .policy()
            .host
            .cloud_runtime()
            .ok_or_else(|| failure(CloudPolicyFailure::Closed));
        #[cfg(feature = "test-hooks")]
        let runtime = runtime.inspect_err(|error| {
            let variant = operation_policy_refusal(error);
            eprintln!("cloudOperation.refused site=originalSDKValidation step=runtime variant={variant}");
        });
        let validation = runtime?.validate_sdk(&sdk);
        #[cfg(feature = "test-hooks")]
        let validation = validation.inspect_err(|error| {
            let (kind, variant) = error.refusal_tags_for_testing();
            eprintln!("cloudOperation.refused site=originalSDKValidation errorKind={kind} variant={variant}");
        });
        validation.map_err(crate::cloud_connection::error)?;
        let CloudPolicyOperationFacts {
            zones,
            kind,
            started,
            failed,
            code,
            retry_after_ms,
            saved,
            app_cancelled,
        } = facts;
        let mut commands = Vec::new();
        for zone in zones {
            let operation = p::Operation {
                zone: zone.into(),
                kind: kind.into(),
            };
            if !started && (app_cancelled || code == Some(20)) {
                self.policy()
                    .driver()?
                    .cancel_native_activity_with_work(&self.command.command, &sdk, operation, work)
                    .map_err(execution_error)?;
                continue;
            }
            if !started && failed {
                let code = code.unwrap_or(0);
                let class = operation_error_class(code);
                let output = self.policy().host.cloud_policy_input_with_work(
                    p::DeviceEvent::OperationError {
                        instance: sdk.clone(),
                        operation,
                        class,
                        code,
                        retry_after_ms,
                    },
                    work,
                );
                match self.policy().output(output)? {
                    CloudPolicyOutput::Commands { commands: emitted } => commands.extend(emitted),
                    output => return Ok(output),
                }
            }
            let phase = if started {
                p::Activity::Started
            } else {
                p::Activity::Ended {
                    outcome: if failed { p::Outcome::Failed } else { p::Outcome::Ok },
                    saved,
                }
            };
            let input = self.policy().host.cloud_policy_input_with_work(
                p::DeviceEvent::Operation {
                    instance: sdk.clone(),
                    operation,
                    phase,
                },
                work,
            );
            #[cfg(feature = "test-hooks")]
            let input = input.inspect_err(|error| operation_driver_refusal("operationInput", error));
            match self.policy().output(input)? {
                CloudPolicyOutput::Commands { commands: emitted } => commands.extend(emitted),
                output => return Ok(output),
            }
        }
        Ok(CloudPolicyOutput::Commands { commands })
    }

    pub(super) fn register_sdk_with_work(
        &self,
        work: &mut p::PolicyWork,
    ) -> Result<CloudEngineRegistration, CoreError> {
        self.policy().owns(&self.command)?;
        let mut registration = self.registration.lock().unwrap_or_else(PoisonError::into_inner);
        if registration.sdk.is_some() {
            return Err(failure(CloudPolicyFailure::StaleCommand));
        }
        let (instance_id, serialization, collection_zone) = self
            .policy()
            .driver()?
            .register_native_sdk_with_work(&self.command.command, work)
            .map_err(execution_error)?;
        registration.sdk = Some(instance_id.clone());
        let automatic = match self.command.command.action.kind {
            p::ActionKind::StartEngine { automatic, .. } => automatic,
            p::ActionKind::ConfigureAutomatic { on } => on,
            _ => false,
        };
        let runtime = self
            .policy()
            .host
            .cloud_runtime()
            .ok_or_else(|| failure(CloudPolicyFailure::Closed))?;
        Ok(CloudEngineRegistration {
            instance_id,
            serialization,
            collection_zone,
            needs_root_fetch: automatic && !runtime.sdk_root_confirmed(),
        })
    }
    pub(super) fn retire_original_with_work(&self, work: &mut p::PolicyWork) -> Result<(), CoreError> {
        self.policy().owns(&self.command)?;
        if let Some(sdk) = &self.original_sdk {
            self.policy()
                .driver()?
                .retire_native_sdk_with_work(&self.command.command, sdk, work)
                .map_err(execution_error)?;
        } else {
            self.policy()
                .driver()?
                .validate_with_work(&self.command.command, work)
                .map_err(execution_error)?;
        }
        Ok(())
    }
    pub(super) fn begin_media_with_work(&self, work: &mut p::PolicyWork) -> Result<bool, CoreError> {
        self.policy().owns(&self.command)?;
        self.policy()
            .driver()?
            .begin_native_media_with_work(&self.command.command, self.original_sdk.as_deref(), work)
            .map_err(execution_error)
    }
    pub(super) fn begin_fetch_with_work(&self, zones: Vec<String>, work: &mut p::PolicyWork) -> Result<u64, CoreError> {
        self.policy().owns(&self.command)?;
        self.policy()
            .driver()?
            .begin_native_fetch_with_work(&self.command.command, &self.sdk()?, zones, work)
            .map_err(execution_error)
    }
    pub(super) fn begin_send_with_work(&self, work: &mut p::PolicyWork) -> Result<Option<u64>, CoreError> {
        self.policy().owns(&self.command)?;
        self.policy()
            .driver()?
            .begin_native_send_with_work(&self.command.command, &self.sdk()?, work)
            .map_err(execution_error)
    }
    pub(super) fn validate_control_with_work(&self, work: &mut p::PolicyWork) -> Result<(), CoreError> {
        self.policy().owns(&self.command)?;
        self.policy()
            .driver()?
            .validate_with_work(&self.command.command, work)
            .map_err(execution_error)
    }
    pub(super) fn continue_media_after_send_with_work(
        &self,
        code: Option<i64>,
        work: &mut p::PolicyWork,
    ) -> Result<bool, CoreError> {
        self.policy().owns(&self.command)?;
        self.policy()
            .driver()?
            .continue_native_media_after_send_with_work(&self.command.command, &self.sdk()?, code, work)
            .map_err(execution_error)
    }
    pub(super) fn prepare_flush_with_work(&self, work: &mut p::PolicyWork) -> Result<CloudPolicyOutput, CoreError> {
        self.policy().owns(&self.command)?;
        let collection = self.policy().collection()?;
        self.policy()
            .driver()?
            .prepare_native_flush_with_work(&self.command.command, &collection, work)
            .map_err(execution_error)?;
        self.policy().output(
            self.policy()
                .host
                .cloud_policy_input_with_work(p::DeviceEvent::Refresh(p::ChangeCause::Cloud), work),
        )
    }
    // A terminal reply keeps its original command when policy has moved on.
    // Let Driver retire that command's read; validate new admission separately
    // so an FFI generation check cannot bypass cleanup of the original owner.
    pub(crate) fn validate_asset_read_activity_compact(&self, sdk: &str) -> Result<(), CoreError> {
        let policy = self.policy();
        if !std::ptr::eq(policy.as_ref(), Arc::as_ptr(&self.command.owner)) {
            return Err(failure(CloudPolicyFailure::WrongOwner));
        }
        if self.command.command.action.guard.generation != policy.generation {
            return Err(failure(CloudPolicyFailure::GenerationChanged));
        }
        self.policy()
            .driver()?
            .validate_native_sdk(&self.command.command, sdk)
            .map_err(execution_error)
    }

    pub(crate) fn asset_read_driver(&self) -> Result<(&p::Driver, &p::Command), CoreError> {
        Ok((self.policy().driver()?, &self.command.command))
    }
    pub(crate) fn asset_read_owner(
        &self,
    ) -> Result<
        (
            Arc<meguri_host::CoreHost>,
            Arc<meguri_sync::cloud::runtime::Runtime>,
            meguri_engine::Collection,
            String,
        ),
        CoreError,
    > {
        self.policy().owns(&self.command)?;
        let collection = self.policy().collection()?;
        let core = self
            .policy()
            .host
            .cloud_runtime()
            .cloned()
            .ok_or_else(|| failure(CloudPolicyFailure::Closed))?;
        Ok((self.policy().host.clone(), core, collection, self.sdk()?))
    }
    pub(crate) fn policy(&self) -> &Arc<CloudPolicy> {
        &self.command.owner
    }
    fn sdk(&self) -> Result<String, CoreError> {
        self.registration
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .sdk
            .clone()
            .or_else(|| self.original_sdk.clone())
            .ok_or_else(|| failure(CloudPolicyFailure::StaleCommand))
    }
}
pub(super) fn execution_error(error: p::DriverError) -> CoreError {
    match error {
        p::DriverError::Runtime(error) => crate::cloud_connection::error(error),
        p::DriverError::NotCoreAction => failure(CloudPolicyFailure::NotNativeAction),
        p::DriverError::CoreCapabilityUnavailable(_) => failure(CloudPolicyFailure::CoreCapabilityUnavailable),
        _ => failure(CloudPolicyFailure::StaleCommand),
    }
}

#[uniffi::export]
impl CloudPolicy {
    pub fn exchange_readiness(
        &self,
        command: Arc<CloudPolicyCommand>,
        expired: bool,
        native_ready: bool,
    ) -> Result<CloudPolicyReadiness, CoreError> {
        callback::enter("CloudPolicy.exchange_readiness");
        self.exchange_readiness_with_work(command, expired, native_ready, &mut p::PolicyWork::default())
    }

    pub fn prepare_execution(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
    ) -> Result<Option<Arc<CloudPolicyExecution>>, CoreError> {
        callback::enter("CloudPolicy.prepare_execution");
        self.prepare_execution_with_work(command, &mut p::PolicyWork::default())
    }
}

#[uniffi::export]
impl CloudPolicyExecution {
    /// Metadata of the pinned original, never a fresh-current SDK lookup.
    pub fn original_sdk(&self) -> Result<Option<String>, CoreError> {
        callback::enter("CloudPolicyExecution.original_sdk");
        Ok(self.original_sdk.clone())
    }
    pub fn validate_execution(&self) -> Result<(), CoreError> {
        callback::enter("CloudPolicyExecution.validate_execution");
        self.policy().owns(&self.command)?;
        // Missing pinned SDK remains stale, after the original policy/Runtime
        // checks. Never resample Runtime's current SDK to fill this selection.
        let sdk = self.sdk().ok();
        self.policy()
            .driver()?
            .validate_native_execution(&self.command.command, sdk.as_deref())
            .map_err(execution_error)
    }
    pub fn register_sdk(&self) -> Result<CloudEngineRegistration, CoreError> {
        callback::enter("CloudPolicyExecution.register_sdk");
        self.register_sdk_with_work(&mut p::PolicyWork::default())
    }
    pub fn retire_original(&self) -> Result<(), CoreError> {
        callback::enter("CloudPolicyExecution.retire_original");
        self.retire_original_with_work(&mut p::PolicyWork::default())
    }
    /// Actual operation facts only. Error classification is Core-owned P15;
    /// native supplies the SDK code/retry-after, never a policy outcome.
    pub fn operation_activity(&self, facts: CloudPolicyOperationFacts) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicyExecution.operation_activity");
        self.operation_activity_with_work(facts, &mut p::PolicyWork::default())
    }

    pub fn continue_media_after_send(&self, code: Option<i64>) -> Result<bool, CoreError> {
        callback::enter("CloudPolicyExecution.continue_media_after_send");
        self.continue_media_after_send_with_work(code, &mut p::PolicyWork::default())
    }

    pub fn begin_media(&self) -> Result<bool, CoreError> {
        callback::enter("CloudPolicyExecution.begin_media");
        self.begin_media_with_work(&mut p::PolicyWork::default())
    }

    pub fn begin_fetch(&self, zones: Vec<String>) -> Result<u64, CoreError> {
        callback::enter("CloudPolicyExecution.begin_fetch");
        self.begin_fetch_with_work(zones, &mut p::PolicyWork::default())
    }
    pub fn validate_control(&self) -> Result<(), CoreError> {
        callback::enter("CloudPolicyExecution.validate_control");
        self.validate_control_with_work(&mut p::PolicyWork::default())
    }
    pub fn prepare_flush(&self) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicyExecution.prepare_flush");
        self.prepare_flush_with_work(&mut p::PolicyWork::default())
    }
    pub fn begin_send(&self) -> Result<Option<u64>, CoreError> {
        callback::enter("CloudPolicyExecution.begin_send");
        self.begin_send_with_work(&mut p::PolicyWork::default())
    }
}

/// Shared P15 mapping for issued executions and original first-read passes.
pub(crate) fn operation_error_class(code: i64) -> p::OperationError {
    match code {
        3 | 4 => p::OperationError::Offline,
        6 | 7 | 23 => p::OperationError::Throttled,
        9 | 36 => p::OperationError::NoAccount,
        5 | 8 | 10 | 24 | 32 => p::OperationError::Configuration,
        25 => p::OperationError::QuotaFull,
        26 | 28 => p::OperationError::ConflictRecovery,
        18 => p::OperationError::UnsupportedSchema,
        _ => p::OperationError::Other,
    }
}

impl CloudPolicy {
    pub(super) fn exchange_readiness_with_work(
        &self,
        command: Arc<CloudPolicyCommand>,
        expired: bool,
        native_ready: bool,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyReadiness, CoreError> {
        self.owns(&command)?;
        let (ready, outcome) = self
            .driver()?
            .native_exchange_readiness_with_work(&command.command, expired, native_ready, work)
            .map_err(execution_error)?;
        work.check().map_err(crate::cloud_connection::error)?;
        Ok(match outcome {
            Some(p::ExchangeOutcome::Waiting { class, retry_after_ms }) => CloudPolicyReadiness::End {
                outcome: CloudPolicyEarlyEnd::Waiting {
                    class: class.into(),
                    retry_after_ms,
                },
            },
            Some(p::ExchangeOutcome::Failed { class }) => CloudPolicyReadiness::End {
                outcome: CloudPolicyEarlyEnd::Failed { class: class.into() },
            },
            Some(_) => CloudPolicyReadiness::End {
                outcome: CloudPolicyEarlyEnd::Cancelled,
            },
            None if ready => CloudPolicyReadiness::Ready,
            None => CloudPolicyReadiness::Wait,
        })
    }
    pub(super) fn prepare_execution_with_work(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
        work: &mut p::PolicyWork,
    ) -> Result<Option<Arc<CloudPolicyExecution>>, CoreError> {
        self.owns(&command)?;
        match &command.command.action.kind {
            p::ActionKind::StartEngine { .. }
            | p::ActionKind::StopEngine { .. }
            | p::ActionKind::ConfigureAutomatic { .. }
            | p::ActionKind::RequestFetch { .. }
            | p::ActionKind::RequestSend { .. }
            | p::ActionKind::RunExchange { .. }
            | p::ActionKind::CancelExchange { .. }
            | p::ActionKind::ResumeMedia
            | p::ActionKind::PauseMedia
            | p::ActionKind::ReadBlobs { .. }
            | p::ActionKind::RunCleanup
            | p::ActionKind::DeleteZones
            | p::ActionKind::RetryAssetEntries
            | p::ActionKind::PrioritizeMedia { .. }
            | p::ActionKind::BackgroundFlush { .. }
            | p::ActionKind::QuitFlush { .. }
            | p::ActionKind::CancelBackgroundFlush
            | p::ActionKind::ArmFlushTimer { .. }
            | p::ActionKind::CancelFlushTimer
            | p::ActionKind::QuitNow => {}
            _ => return Err(failure(CloudPolicyFailure::CoreCapabilityUnavailable)),
        }
        if self
            .driver()?
            .native_disposition_with_work(&command.command, work)
            .map_err(execution_error)?
            == p::NativeDisposition::NoOperation
        {
            return Ok(None);
        }
        let original_sdk = self
            .driver()?
            .native_origin_with_work(&command.command, work)
            .map_err(execution_error)?;
        work.check().map_err(crate::cloud_connection::error)?;
        Ok(Some(Arc::new(CloudPolicyExecution {
            command,
            original_sdk,
            registration: Mutex::new(Registration::default()),
            cleanup_begun: std::sync::atomic::AtomicBool::new(false),
            zone_deletion_begun: std::sync::atomic::AtomicBool::new(false),
        })))
    }
}
