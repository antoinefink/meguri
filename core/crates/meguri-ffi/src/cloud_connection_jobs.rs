//! Internal typed work on the retained Host's existing registered pool.
use super::*;
use crate::cloud_policy::policy_jobs::{Cell, Completion};
use meguri_engine::Generation;

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
typed_result!(
    CloudPublicationWorkResult,
    CloudPublicationWorkJob,
    CloudPublicationWork
);
// A successful void reply must remain distinct from a pending typed result.
#[derive(Clone, uniffi::Record)]
pub struct CloudAdoptionApplied {
    pub applied: bool,
}
typed_result!(CloudAdoptionAppliedResult, CloudAdoptionResultJob, CloudAdoptionApplied);

impl CloudConnection {
    fn start_connection<T: Send + 'static>(
        self: Arc<Self>,
        name: &'static str,
        generation: u64,
        work: impl FnOnce(Arc<Self>, &mut c::policy::PolicyWork) -> Result<T, CoreError> + Send + 'static,
    ) -> Result<(crate::jobs::JobHandle, Arc<Cell<T>>), CoreError> {
        self.owner.check_generation(generation)?;
        let cell = Arc::new(Cell(Mutex::new(None)));
        let completion = Completion(cell.clone());
        let owner = self.clone();
        let handle = self
            .owner
            .host
            .start_cloud_connection_job(Generation(generation), name, move |job| {
                let mut authority = c::policy::PolicyWork::new(&job.cancel)
                    .with_generation(owner.owner.host.engine().downgrade(), Generation(generation));
                let value = job
                    .cancel
                    .check()
                    .map_err(CoreError::from)
                    .and_then(|()| owner.owner.check_generation(generation))
                    .and_then(|()| work(owner, &mut authority))
                    .and_then(|value| {
                        authority.check().map_err(error)?;
                        Ok(value)
                    });
                // Runtime latches at its actual pre-effect boundary. Do not sample
                // cancellation or active-owner state over its original outcome.
                completion.finish(value)
            })
            .map_err(|error| match error {
                meguri_engine::EngineError::StaleGeneration { .. } => failure(CloudConnectionFailure::AuthorityChanged),
                _ => failure(CloudConnectionFailure::Closed),
            })?;
        Ok((crate::jobs::wrap(handle), cell))
    }
}
#[uniffi::export]
impl CloudConnection {
    pub fn start_next_publication_work(self: Arc<Self>) -> Result<CloudPublicationWorkJob, CoreError> {
        callback::enter("CloudConnection.start_next_publication_work");
        let generation = self.owner.current_generation()?;
        let (handle, cell) = self.start_connection(
            "CloudConnection.next_publication_work",
            generation,
            move |owner, work| owner.next_publication_work_with_work(generation, work),
        )?;
        Ok(CloudPublicationWorkJob {
            handle,
            result: Arc::new(CloudPublicationWorkResult { cell }),
        })
    }
    pub fn start_adoption_result(
        self: Arc<Self>,
        handle: Arc<CloudAdoptionWork>,
        result: CloudAdoptionResult,
    ) -> Result<CloudAdoptionResultJob, CoreError> {
        callback::enter("CloudConnection.start_adoption_result");
        self.owns(&handle.owner)?;
        let (handle, cell) = self.start_connection(
            "CloudConnection.adoption_result",
            handle.generation,
            move |owner, work| {
                owner
                    .adoption_result_with_work(handle, result, work)
                    .map(|()| CloudAdoptionApplied { applied: true })
            },
        )?;
        Ok(CloudAdoptionResultJob {
            handle,
            result: Arc::new(CloudAdoptionAppliedResult { cell }),
        })
    }
}
