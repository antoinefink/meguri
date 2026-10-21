//! U32 control calls retain all host participants and session durability.
use crate::{CoreError, callback, runtime};
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SuspendOutcome {
    Suspended,
    Deferred { job_id: u64 },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ResumeReport {
    pub external_change: bool,
    pub rolled_over: bool,
    pub next_day_at: i64,
    pub active_session: Option<u64>,
}
#[uniffi::export]
pub fn flush_for_background() -> Result<(), CoreError> {
    callback::enter("flush_for_background");
    Ok(runtime::host()?.engine().flush_for_background()?)
}
#[uniffi::export]
pub fn suspend(timeout_ms: u32) -> Result<SuspendOutcome, CoreError> {
    callback::enter("suspend");
    Ok(
        match runtime::host()?
            .engine()
            .suspend(std::time::Duration::from_millis(timeout_ms.into()))?
        {
            meguri_engine::suspend::SuspendOutcome::Suspended => SuspendOutcome::Suspended,
            meguri_engine::suspend::SuspendOutcome::Deferred { job_id } => {
                SuspendOutcome::Deferred { job_id: job_id.0 }
            }
        },
    )
}
#[uniffi::export]
pub fn resume() -> Result<ResumeReport, CoreError> {
    callback::enter("resume");
    let host = runtime::host()?;
    let r = host.engine().resume()?;
    Ok(ResumeReport {
        external_change: r.external_change,
        rolled_over: r.rolled_over,
        next_day_at: r.next_day_at,
        active_session: host.review().active_session().map(|s| s.0),
    })
}
