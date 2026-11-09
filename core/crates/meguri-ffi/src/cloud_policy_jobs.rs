//! Bounded facade work on the ORIGINAL Host engine's registered pool.
//! These starts neither validate through Driver/Runtime nor acquire a slot.
use super::*;
use meguri_engine::{EngineError, Generation};

// The cell preserves the FFI error, which conversion to EngineError can erase.
// It owns no Host; completed reads must survive close and replacement.
pub(crate) struct Cell<T>(pub(crate) Mutex<Option<Result<T, CoreError>>>);
pub(crate) struct Completion<T>(pub(crate) Arc<Cell<T>>);
impl<T> Completion<T> {
    pub(crate) fn finish(self, value: Result<T, CoreError>) -> Result<(), EngineError> {
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
// Required completions have no destructor placeholder: no body, panic or drop
// without a normal return must remain distinguishable from actual Cancelled.
struct RequiredCompletion<T>(Arc<Cell<T>>);
impl<T> RequiredCompletion<T> {
    fn finish(self, value: Result<T, CoreError>) -> Result<(), EngineError> {
        let terminal = value
            .as_ref()
            .map(|_| ())
            .map_err(|error| EngineError::from(error.clone()));
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
        terminal
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
typed_result!(
    CloudPolicyRegistrationResult,
    CloudPolicyRegistrationJob,
    crate::sync::CloudEngineRegistration
);
typed_result!(CloudPolicyRetirementResult, CloudPolicyRetirementJob, bool);
typed_result!(CloudPolicyMediaResult, CloudPolicyMediaJob, bool);
typed_result!(CloudPolicyControlResult, CloudPolicyControlJob, bool);
typed_result!(
    CloudPolicyMediaContinuationResult,
    CloudPolicyMediaContinuationJob,
    bool
);
typed_result!(CloudPolicyFetchResult, CloudPolicyFetchJob, u64);
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CloudPolicySendCall {
    pub call: Option<u64>,
}
typed_result!(CloudPolicySendResult, CloudPolicySendJob, CloudPolicySendCall);
typed_result!(
    CloudPolicyAssetReadPassResult,
    CloudPolicyAssetReadPassJob,
    Arc<crate::cloud_asset_reads::CloudAssetReadPass>
);

impl CloudPolicy {
    pub(crate) fn start_policy<T: Send + 'static>(
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
        let original_engine = engine.downgrade();
        let generation = Generation(self.generation);
        let owner = self.clone();
        let handle = engine.jobs().start(name, generation, None, move |job| {
            // The explicit authority reaches the actual pre-effect checks,
            // including acquisition behind a projection. It stops cancelling
            // only once an effect occurred, preserving exact failure follow-ups.
            let owner_error = Arc::new(Mutex::new(None));
            let rejected_owner = owner_error.clone();
            let validation_owner = owner.clone();
            let mut work_authority = p::PolicyWork::new(&job.cancel)
                .with_generation(original_engine, generation)
                .with_owner_check(Arc::new(move || {
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
                None if work_authority.generation_was_rejected() => Err(failure(CloudPolicyFailure::GenerationChanged)),
                None => value,
            };
            completion.finish(value)
        });
        Ok((crate::jobs::wrap(handle), cell))
    }
}

#[uniffi::export]
impl CloudPolicyExecution {
    pub fn start_begin_asset_read_pass(self: Arc<Self>) -> Result<CloudPolicyAssetReadPassJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_begin_asset_read_pass");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.begin_asset_read_pass", move |_, work| {
                self.begin_asset_read_pass_with_work(work)
            })?;
        Ok(CloudPolicyAssetReadPassJob {
            handle,
            result: Arc::new(CloudPolicyAssetReadPassResult { cell }),
        })
    }
    pub fn start_validate_control(self: Arc<Self>) -> Result<CloudPolicyControlJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_validate_control");
        self.policy().owns(&self.command)?;
        let (handle, cell) =
            self.policy()
                .clone()
                .start_policy("CloudPolicyExecution.validate_control", move |_, work| {
                    self.validate_control_with_work(work)?;
                    Ok(true)
                })?;
        Ok(CloudPolicyControlJob {
            handle,
            result: Arc::new(CloudPolicyControlResult { cell }),
        })
    }
    pub fn start_continue_media_after_send(
        self: Arc<Self>,
        code: Option<i64>,
    ) -> Result<CloudPolicyMediaContinuationJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_continue_media_after_send");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.continue_media_after_send", move |_, work| {
                self.continue_media_after_send_with_work(code, work)
            })?;
        Ok(CloudPolicyMediaContinuationJob {
            handle,
            result: Arc::new(CloudPolicyMediaContinuationResult { cell }),
        })
    }
    pub fn start_prepare_flush(self: Arc<Self>) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_prepare_flush");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.prepare_flush", move |_, work| {
                self.prepare_flush_with_work(work)
            })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    /// Started facts are constructed here; this ordinary job keeps queued cancellation.
    pub fn start_report_invocation(
        self: Arc<Self>,
        zones: Vec<CloudPolicyZone>,
        kind: CloudPolicyOperationKind,
    ) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_report_invocation");
        self.policy().owns(&self.command)?;
        let (handle, cell) =
            self.policy()
                .clone()
                .start_policy("CloudPolicyExecution.report_invocation", move |_, work| {
                    self.operation_activity_with_work(
                        CloudPolicyOperationFacts {
                            zones,
                            kind,
                            started: true,
                            failed: false,
                            code: None,
                            retry_after_ms: None,
                            saved: 0,
                            app_cancelled: false,
                        },
                        work,
                    )
                })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    /// Full Started facts only, on the ordinary pool with queued cancellation.
    pub fn start_report_started(
        self: Arc<Self>,
        facts: CloudPolicyOperationFacts,
    ) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_report_started");
        self.policy().owns(&self.command)?;
        if !facts.started {
            return Err(CoreError::InvalidArgument {
                message: "started report requires Started facts".into(),
            });
        }
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.report_started", move |_, work| {
                self.operation_activity_with_work(facts, work)
            })?;
        Ok(CloudPolicyOutputJob {
            handle,
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    /// Actual Ended facts for this original execution only. This starts no SDK
    /// work and retains normal typed returns even when cancellation is requested.
    pub fn start_report_completion(
        self: Arc<Self>,
        facts: CloudPolicyOperationFacts,
    ) -> Result<CloudPolicyOutputJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_report_completion");
        self.policy().owns(&self.command)?;
        if facts.started {
            return Err(CoreError::InvalidArgument {
                message: "completion requires Ended facts".into(),
            });
        }
        let cell = Arc::new(Cell(Mutex::new(None)));
        let completion = RequiredCompletion(cell.clone());
        let engine = self.policy().host.engine().clone();
        let original_engine = engine.downgrade();
        let generation = Generation(self.policy().generation);
        let handle =
            engine
                .jobs()
                .start_required_completion("CloudPolicyExecution.report_completion", generation, move |job| {
                    let owner_error = Arc::new(Mutex::new(None));
                    let rejected_owner = owner_error.clone();
                    let owner = self.policy().clone();
                    let mut work = p::PolicyWork::required_completion(&job.cancel)
                        .with_generation(original_engine, generation)
                        .with_owner_check(Arc::new(move || {
                            owner.validate_owner().map_err(|error| {
                                *rejected_owner.lock().unwrap_or_else(PoisonError::into_inner) = Some(error.clone());
                                EngineError::from(error)
                            })
                        }));
                    let value = self.operation_activity_with_work(facts, &mut work);
                    // Use the rejection sampled at the actual authority boundary,
                    // never a fresh lookup over the operation's returned outcome.
                    let value = match owner_error.lock().unwrap_or_else(PoisonError::into_inner).take() {
                        Some(error) => Err(error),
                        None if work.generation_was_rejected() => Err(failure(CloudPolicyFailure::GenerationChanged)),
                        None => value,
                    };
                    completion.finish(value)
                });
        Ok(CloudPolicyOutputJob {
            handle: crate::jobs::wrap(handle),
            result: Arc::new(CloudPolicyOutputResult { cell }),
        })
    }
    pub fn start_begin_media(self: Arc<Self>) -> Result<CloudPolicyMediaJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_begin_media");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.begin_media", move |_, work| {
                self.begin_media_with_work(work)
            })?;
        Ok(CloudPolicyMediaJob {
            handle,
            result: Arc::new(CloudPolicyMediaResult { cell }),
        })
    }
    pub fn start_begin_fetch(self: Arc<Self>, zones: Vec<String>) -> Result<CloudPolicyFetchJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_begin_fetch");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.begin_fetch", move |_, work| {
                self.begin_fetch_with_work(zones, work)
            })?;
        Ok(CloudPolicyFetchJob {
            handle,
            result: Arc::new(CloudPolicyFetchResult { cell }),
        })
    }
    pub fn start_begin_send(self: Arc<Self>) -> Result<CloudPolicySendJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_begin_send");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.begin_send", move |_, work| {
                self.begin_send_with_work(work).map(|call| CloudPolicySendCall { call })
            })?;
        Ok(CloudPolicySendJob {
            handle,
            result: Arc::new(CloudPolicySendResult { cell }),
        })
    }
    pub fn start_register_sdk(self: Arc<Self>) -> Result<CloudPolicyRegistrationJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_register_sdk");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.register_sdk", move |_, work| {
                self.register_sdk_with_work(work)
            })?;
        Ok(CloudPolicyRegistrationJob {
            handle,
            result: Arc::new(CloudPolicyRegistrationResult { cell }),
        })
    }
    pub fn start_retire_original(self: Arc<Self>) -> Result<CloudPolicyRetirementJob, CoreError> {
        callback::enter("CloudPolicyExecution.start_retire_original");
        self.policy().owns(&self.command)?;
        let (handle, cell) = self
            .policy()
            .clone()
            .start_policy("CloudPolicyExecution.retire_original", move |_, work| {
                self.retire_original_with_work(work).map(|()| true)
            })?;
        Ok(CloudPolicyRetirementJob {
            handle,
            result: Arc::new(CloudPolicyRetirementResult { cell }),
        })
    }
}

#[uniffi::export]
impl CloudWorkflow {
    pub fn start_begin_first_asset_read_pass(
        self: Arc<Self>,
        sdk: String,
    ) -> Result<CloudPolicyAssetReadPassJob, CoreError> {
        callback::enter("CloudWorkflow.start_begin_first_asset_read_pass");
        // Metadata only: retain this original workflow and its policy before
        // queueing. Collection acquisition and all Runtime work stay on worker.
        let policy = CloudPolicy::for_workflow(&self)?;
        let (handle, cell) = policy
            .start_policy("CloudWorkflow.begin_first_asset_read_pass", move |policy, work| {
                self.begin_first_asset_read_pass_with_work(sdk, policy, work)
            })?;
        Ok(CloudPolicyAssetReadPassJob {
            handle,
            result: Arc::new(CloudPolicyAssetReadPassResult { cell }),
        })
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

    #[test]
    fn required_sdk_completion_actual_cancelled_and_panic_have_distinct_stored_results() {
        use meguri_engine::{ChangeStream, EventKind, JobOutcome, JobRegistry, ThreadRegistry};
        use std::sync::mpsc;
        use std::time::Duration;

        let threads = ThreadRegistry::new();
        let stream = ChangeStream::new();
        let events = stream.subscribe();
        let registry = JobRegistry::new(&threads, stream, 1).unwrap();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        registry.start("hold existing pool", Generation(0), None, move |_| {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        });
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut cases = Vec::new();
        let called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for (name, value) in [
            ("actual Ok", Ok(73_u64)),
            ("actual Cancelled", Err(CoreError::Cancelled)),
            ("actual typed error", Err(failure(CloudPolicyFailure::WrongOwner))),
        ] {
            let cell = Arc::new(Cell(Mutex::new(None)));
            let completion = RequiredCompletion(cell.clone());
            let expected = value.clone();
            let body_called = called.clone();
            let job = registry.start_required_completion(name, Generation(0), move |context| {
                body_called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                assert!(context.cancel.is_cancelled());
                assert!(context.slot().is_none());
                completion.finish(value)
            });
            job.cancel.cancel(); // Models cancellation after listener setup failure.
            assert!(cell.0.lock().unwrap().is_none());
            cases.push((job, cell, Some(expected)));
        }
        let cell = Arc::new(Cell::<u64>(Mutex::new(None)));
        let completion = RequiredCompletion(cell.clone());
        let panicked = registry.start_required_completion("panic", Generation(0), move |_| {
            let _completion = completion;
            panic!("required completion panic")
        });
        cases.push((panicked, cell, None));
        release.send(()).unwrap();
        let mut outcomes = std::collections::BTreeMap::new();
        // A FIFO fence bounds collection and detects duplicate terminal events.
        let fence = registry.start("fence", Generation(0), None, |_| Ok(()));
        loop {
            let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
            if let EventKind::JobFinished { id, outcome } = &event.kind {
                assert!(outcomes.insert(*id, outcome.clone()).is_none());
                if *id == fence.id {
                    break;
                }
            }
        }
        for (job, cell, expected) in cases {
            assert_eq!(*cell.0.lock().unwrap(), expected);
            match expected {
                Some(Ok(_)) => assert_eq!(outcomes[&job.id], JobOutcome::Succeeded),
                Some(Err(CoreError::Cancelled)) => assert_eq!(outcomes[&job.id], JobOutcome::Cancelled),
                Some(Err(_)) => assert!(matches!(outcomes[&job.id], JobOutcome::Failed { .. })),
                None => assert!(matches!(
                    &outcomes[&job.id],
                    JobOutcome::Failed { error: EngineError::Internal { message } }
                        if message.contains("required completion panic")
                )),
            }
        }
        assert_eq!(called.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(outcomes.len(), 6); // blocker, three returns, panic, fence.
        assert_eq!(
            threads
                .threads()
                .iter()
                .filter(|thread| thread.name.starts_with("meguri-job-"))
                .count(),
            1
        );
        let required = Arc::new(Cell::<u64>(Mutex::new(None)));
        drop(RequiredCompletion(required.clone()));
        assert!(required.0.lock().unwrap().is_none());
        let ordinary = Arc::new(Cell::<u64>(Mutex::new(None)));
        drop(Completion(ordinary.clone()));
        assert_eq!(*ordinary.0.lock().unwrap(), Some(Err(CoreError::Cancelled)));
    }
}
