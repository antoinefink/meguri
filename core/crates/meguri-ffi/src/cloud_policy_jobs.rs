//! Bounded facade work on the ORIGINAL Host engine's registered pool.
//! These starts neither validate through Driver/Runtime nor acquire a slot.
use super::*;
use meguri_engine::{EngineError, Generation};

// The cell preserves the FFI error, which conversion to EngineError can erase.
// It owns no Host; completed reads must survive close and replacement.
struct Cell<T>(Mutex<Option<Result<T, CoreError>>>);
struct Completion<T>(Arc<Cell<T>>);
impl<T> Completion<T> {
    fn finish(self, value: Result<T, CoreError>) -> Result<(), EngineError> {
        let terminal = value
            .as_ref()
            .map(|_| ())
            .map_err(|error| EngineError::from(error.clone()));
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
        terminal
    }
}
impl<T> Drop for Completion<T> {
    fn drop(&mut self) {
        let mut cell = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        // The registry reports panics with their original message. A queued
        // cancellation drops the closure without calling it; retain that result.
        if cell.is_none() && !std::thread::panicking() {
            *cell = Some(Err(CoreError::Cancelled));
        }
    }
}
macro_rules! typed_result {
    ($result:ident, $start:ident, $value:ty) => {
        #[derive(uniffi::Object)]
        pub struct $result {
            cell: Arc<Cell<$value>>,
        }
        #[uniffi::export]
        impl $result {
            pub fn result(&self) -> Result<Option<$value>, CoreError> {
                callback::enter(concat!(stringify!($result), ".result"));
                self.cell
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .transpose()
            }
        }
        #[derive(Clone, uniffi::Record)]
        pub struct $start {
            pub handle: crate::jobs::JobHandle,
            pub result: Arc<$result>,
        }
    };
}
typed_result!(CloudPolicyOutputResult, CloudPolicyOutputJob, CloudPolicyOutput);
typed_result!(CloudPolicyStatusResult, CloudPolicyStatusJob, CloudPolicyStatus);
typed_result!(
    CloudPolicyAdmissionResult,
    CloudPolicyAdmissionJob,
    CloudPolicyAdmission
);
typed_result!(CloudPolicyCaptureResult, CloudPolicyCaptureJob, CloudPolicyCapture);
typed_result!(
    CloudPolicyReadinessResult,
    CloudPolicyReadinessJob,
    CloudPolicyReadiness
);
// Keep Pending distinct from the successful NoOperation (execution = None).
#[derive(Clone, uniffi::Record)]
pub struct CloudPolicyPreparedExecution {
    pub execution: Option<Arc<CloudPolicyExecution>>,
}
typed_result!(
    CloudPolicyExecutionResult,
    CloudPolicyExecutionJob,
    CloudPolicyPreparedExecution
);

impl CloudPolicy {
    fn start_policy<T: Send + 'static>(
        self: Arc<Self>,
        name: &'static str,
        work: impl FnOnce(Arc<Self>, &mut p::PolicyWork) -> Result<T, CoreError> + Send + 'static,
    ) -> Result<(crate::jobs::JobHandle, Arc<Cell<T>>), CoreError> {
        // Preserve immediate Closed/GenerationChanged errors, even after this
        // engine has paused its registry. Command starts first check owns()
        // to preserve WrongOwner precedence. Repeat ownership on the worker.
        self.validate_owner()?;
        let cell = Arc::new(Cell(Mutex::new(None)));
        let completion = Completion(cell.clone());
        let engine = self.host.engine();
        let owner = self.clone();
        let handle = engine
            .jobs()
            .start(name, Generation(self.generation), None, move |job| {
                // The explicit authority reaches the actual pre-effect checks,
                // including acquisition behind a projection. It stops cancelling
                // only once an effect occurred, preserving exact failure follow-ups.
                let owner_error = Arc::new(Mutex::new(None));
                let rejected_owner = owner_error.clone();
                let validation_owner = owner.clone();
                let mut work_authority = p::PolicyWork::new(&job.cancel).with_owner_check(Arc::new(move || {
                    validation_owner.validate_owner().map_err(|error| {
                        *rejected_owner.lock().unwrap_or_else(PoisonError::into_inner) = Some(error.clone());
                        EngineError::from(error)
                    })
                }));
                let value = job
                    .cancel
                    .check()
                    .map_err(CoreError::from)
                    .and_then(|()| owner.validate_owner())
                    .and_then(|()| job.cancel.check().map_err(CoreError::from))
                    .and_then(|()| work(owner, &mut work_authority));
                // Retain the precise FFI owner failure sampled after contention;
                // never perform a fresh owner lookup over an effected outcome.
                let value = match owner_error.lock().unwrap_or_else(PoisonError::into_inner).take() {
                    Some(error) => Err(error),
                    None => value,
                };
                completion.finish(value)
            });
        Ok((crate::jobs::wrap(handle), cell))
    }
}

#[uniffi::export]
impl CloudPolicy {
    pub fn start_input(self: Arc<Self>, event: CloudPolicyEvent) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicy.start_input");
        let (handle, cell) = self.start_policy("CloudPolicy.input", move |owner, work| {
            owner.input_with_work(event, work)
        })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    pub fn start_refresh_account(self: Arc<Self>) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicy.start_refresh_account");
        let (handle, cell) = self.start_policy("CloudPolicy.refresh_account", move |owner, work| {
            owner.refresh_account_with_work(work)
        })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    pub fn start_project_identity_read(self: Arc<Self>, request: u64) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicy.start_project_identity_read");
        let (handle, cell) = self.start_policy("CloudPolicy.project_identity_read", move |owner, work| {
            owner.project_identity_read_with_work(request, work)
        })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    pub fn start_status(self: Arc<Self>) -> Result<CloudPolicyStatusJob, CoreError> {
        callback::enter("CloudPolicy.start_status");
        let (handle, cell) =
            self.start_policy("CloudPolicy.status", move |owner, work| owner.status_with_work(work))?;
        Ok(CloudPolicyStatusJob {
            handle,
            result: Arc::new(CloudPolicyStatusResult { cell }),
        })
    }
    pub fn start_execute_core(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
    ) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicy.start_execute_core");
        self.owns(&command)?;
        let (handle, cell) = self.start_policy("CloudPolicy.execute_core", move |owner, work| {
            owner.execute_core_with_work(command, work)
        })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    pub fn start_submit_native(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
        queue: Arc<dyn CloudPolicyQueue>,
    ) -> Result<CloudPolicyAdmissionJob, CoreError> {
        callback::enter("CloudPolicy.start_submit_native");
        self.owns(&command)?;
        let (handle, cell) = self.start_policy("CloudPolicy.submit_native", move |owner, work| {
            owner.submit_native_with_work(command, queue, work)
        })?;
        Ok(CloudPolicyAdmissionJob {
            handle,
            result: Arc::new(CloudPolicyAdmissionResult { cell }),
        })
    }
    pub fn start_capture_exchange(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
    ) -> Result<CloudPolicyCaptureJob, CoreError> {
        callback::enter("CloudPolicy.start_capture_exchange");
        self.owns(&command)?;
        let (handle, cell) = self.start_policy("CloudPolicy.capture_exchange", move |owner, work| {
            owner.capture_exchange_with_work(command, work)
        })?;
        Ok(CloudPolicyCaptureJob {
            handle,
            result: Arc::new(CloudPolicyCaptureResult { cell }),
        })
    }
    pub fn start_complete_exchange(self: Arc<Self>, exchange: u64) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicy.start_complete_exchange");
        let (handle, cell) = self.start_policy("CloudPolicy.complete_exchange", move |owner, work| {
            owner.complete_exchange_with_work(exchange, work)
        })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    pub fn start_end_before_capture(
        self: Arc<Self>,
        exchange: u64,
        outcome: CloudPolicyEarlyEnd,
    ) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicy.start_end_before_capture");
        let (handle, cell) = self.start_policy("CloudPolicy.end_before_capture", move |owner, work| {
            owner.end_before_capture_with_work(exchange, outcome, work)
        })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    pub fn start_exchange_readiness(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
        expired: bool,
        native_ready: bool,
    ) -> Result<CloudPolicyReadinessJob, CoreError> {
        callback::enter("CloudPolicy.start_exchange_readiness");
        self.owns(&command)?;
        let (handle, cell) = self.start_policy("CloudPolicy.exchange_readiness", move |owner, work| {
            owner.exchange_readiness_with_work(command, expired, native_ready, work)
        })?;
        Ok(CloudPolicyReadinessJob {
            handle,
            result: Arc::new(CloudPolicyReadinessResult { cell }),
        })
    }
    pub fn start_prepare_execution(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
    ) -> Result<CloudPolicyExecutionJob, CoreError> {
        callback::enter("CloudPolicy.start_prepare_execution");
        self.owns(&command)?;
        let (handle, cell) = self.start_policy("CloudPolicy.prepare_execution", move |owner, work| {
            owner
                .prepare_execution_with_work(command, work)
                .map(|execution| CloudPolicyPreparedExecution { execution })
        })?;
        Ok(CloudPolicyExecutionJob {
            handle,
            result: Arc::new(CloudPolicyExecutionResult { cell }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_result_retains_exact_policy_and_connection_errors() {
        for expected in [
            failure(CloudPolicyFailure::WrongOwner),
            CoreError::CloudConnection {
                failure: crate::cloud_connection::CloudConnectionFailure::Tracking,
            },
        ] {
            let cell = Arc::new(Cell::<CloudPolicyOutput>(Mutex::new(None)));
            let terminal = Completion(cell.clone()).finish(Err(expected.clone())).unwrap_err();
            // The terminal failure omits details that the typed result must preserve.
            assert_ne!(CoreError::from(terminal), expected);
            let result = CloudPolicyOutputResult { cell };
            assert!(matches!(result.result(), Err(error) if error == expected));
        }
    }
}
