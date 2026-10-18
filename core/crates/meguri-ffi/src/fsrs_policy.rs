//! Policy reads are short; explicit enable runs on the bounded job pool.
use crate::{
    callback,
    error::CoreError,
    jobs::{self, JobHandle},
    runtime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FsrsPolicyStatus {
    Ok,
    NeedsEnable,
    NeedsReEnable,
}

#[uniffi::export]
pub fn fsrs_policy_status() -> Result<FsrsPolicyStatus, CoreError> {
    callback::enter("fsrs_policy_status");
    let collection = runtime::engine()?.collection()?;
    use meguri_engine::sched::fsrs_policy::PolicyStatus;
    Ok(match meguri_engine::sched::fsrs_policy::policy_status(&collection)? {
        PolicyStatus::Ok => FsrsPolicyStatus::Ok,
        PolicyStatus::NeedsEnable => FsrsPolicyStatus::NeedsEnable,
        PolicyStatus::NeedsReEnable => FsrsPolicyStatus::NeedsReEnable,
    })
}

#[uniffi::export]
pub fn enable_fsrs() -> Result<JobHandle, CoreError> {
    callback::enter("enable_fsrs");
    let collection = runtime::engine()?.collection()?;
    jobs::start("enable_fsrs", collection.generation().0, move |job| {
        meguri_engine::sched::fsrs_policy::enable_fsrs(
            &collection,
            &meguri_engine::tags::RunOptions {
                cancel: Some(job.cancel.engine_token().clone()),
                progress: Some(job.progress.engine_sink()),
            },
        )?;
        Ok(())
    })
}
