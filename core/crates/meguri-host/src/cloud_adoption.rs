//! Composition only: Runtime owns V14; existing providers own capacity/backup.
use super::*;
use meguri_sync::cloud::{
    adoption::{AdoptionReply, AdoptionWork},
    publication::Preview,
    runtime::RuntimeError,
};
impl CoreHost {
    pub fn cloud_connection_directory(&self) -> &std::path::Path {
        &self.backup_inputs.collection_directory
    }
    /// Generation authority also exists while this retained Host owns no file.
    pub fn cloud_connection_generation(&self) -> Result<meguri_engine::Generation, EngineError> {
        if !matches!(
            *self.closed.lock().unwrap_or_else(PoisonError::into_inner),
            CloseState::Active
        ) {
            return Err(EngineError::CollectionUnavailable {
                reason: "cloud connection host closed".into(),
            });
        }
        Ok(self.engine().generation())
    }
    /// A retained native connection must never follow this process engine into
    /// a successor host after close. Drop the state guard before any core work.
    pub fn cloud_connection_collection(&self) -> Result<meguri_engine::Collection, EngineError> {
        self.cloud_connection_generation()?;
        self.collection()
    }
    pub fn begin_cloud_connection_preview(&self) -> Result<meguri_sync::cloud::publication::PreviewWork, RuntimeError> {
        self.cloud_connection_generation()?;
        let runtime = self.cloud_runtime().ok_or_else(unavailable)?;
        if matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection) {
            runtime.begin_connection_without_collection()
        } else {
            runtime.begin_connection_preview(&self.collection()?, &self.media()?)
        }
    }
    /// A connection sheet retains the Host generation it was presented for.
    pub fn resume_cloud_binding(
        &self,
        preview: &Preview,
        generation: meguri_engine::Generation,
    ) -> Result<(), RuntimeError> {
        if self.cloud_connection_generation()? != generation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let collection = self.cloud_connection_collection()?;
        if collection.generation() != generation {
            return Err(RuntimeError::StaleIncarnation);
        }
        self.cloud_runtime()
            .ok_or_else(unavailable)?
            .confirm_connection_resume(preview, &collection)
    }
    fn cloud_adoption_collection(&self) -> Result<Option<meguri_engine::Collection>, RuntimeError> {
        self.cloud_connection_generation()?;
        if matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection) {
            Ok(None)
        } else {
            Ok(Some(self.collection()?))
        }
    }
    pub fn next_cloud_publication_work(
        &self,
    ) -> Result<meguri_sync::cloud::publication::PublicationWork, RuntimeError> {
        let runtime = self.cloud_runtime().ok_or_else(unavailable)?;
        match self.cloud_adoption_collection()? {
            Some(collection) => runtime.next_publication_work(&collection),
            None => Ok(runtime
                .next_adoption_work(None)?
                .map(meguri_sync::cloud::publication::PublicationWork::Adoption)
                .unwrap_or(meguri_sync::cloud::publication::PublicationWork::Idle)),
        }
    }
    pub fn confirm_cloud_adoption(&self, preview: &Preview) -> Result<[u8; 16], RuntimeError> {
        self.cloud_runtime().ok_or_else(unavailable)?.confirm_cloud_adoption(
            preview,
            self.cloud_adoption_collection()?.as_ref(),
            self.engine(),
        )
    }
    pub fn cancel_cloud_adoption(&self, id: [u8; 16]) -> Result<(), RuntimeError> {
        self.cloud_runtime()
            .ok_or_else(unavailable)?
            .cancel_cloud_adoption(id, self.cloud_adoption_collection()?.as_ref())
    }
    pub fn cloud_adoption_reply(&self, work: &AdoptionWork, reply: AdoptionReply) -> Result<(), RuntimeError> {
        self.cloud_runtime().ok_or_else(unavailable)?.cloud_adoption_reply(
            work,
            reply,
            self.cloud_adoption_collection()?.as_ref(),
            self.backup_inputs.space.as_ref(),
        )
    }
    pub fn perform_cloud_adoption_local(&self, work: &AdoptionWork) -> Result<(), RuntimeError> {
        let runtime = self.cloud_runtime().ok_or_else(unavailable)?;
        match self.cloud_adoption_collection()? {
            Some(collection) => runtime.perform_cloud_adoption_local(
                work,
                &collection,
                &self.media()?,
                self.backup_inputs.space.as_ref(),
                self.backup.as_ref(),
            ),
            None => {
                let events = self.engine().stream().subscribe();
                let result = Arc::new(Mutex::new(None));
                let output = result.clone();
                let runtime = runtime.clone();
                let work = work.clone();
                let space = self.backup_inputs.space.clone();
                let backup = self.backup.clone();
                let config = self.initial_open.clone();
                // The durable operation already owns CloudAdopt's slot. This
                // worker uses the same process pool/stream as every other job.
                let job =
                    self.engine()
                        .jobs()
                        .start("iCloud adoption", self.engine().generation(), None, move |context| {
                            context.cancel.check()?;
                            let typed = runtime.perform_initial_cloud_adoption_local(
                                &work,
                                space.as_ref(),
                                backup.as_ref(),
                                config,
                            );
                            let outcome = typed
                                .as_ref()
                                .map(|_| ())
                                .map_err(|e| EngineError::CollectionUnavailable { reason: e.to_string() });
                            *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(typed);
                            outcome
                        });
                loop {
                    let event = events.recv().ok_or_else(|| EngineError::CollectionUnavailable {
                        reason: "cloud adoption stream closed".into(),
                    })?;
                    if let meguri_engine::EventKind::JobFinished { id, outcome } = &event.kind
                        && *id == job.id
                    {
                        let typed = result.lock().unwrap_or_else(PoisonError::into_inner).take();
                        return typed.unwrap_or_else(|| {
                            Err(match outcome {
                                meguri_engine::JobOutcome::Failed { error } => error.clone(),
                                _ => EngineError::Cancelled,
                            }
                            .into())
                        });
                    }
                }
            }
        }
    }
}
fn unavailable() -> RuntimeError {
    EngineError::Unsupported {
        feature: "cloud runtime unavailable".into(),
    }
    .into()
}
