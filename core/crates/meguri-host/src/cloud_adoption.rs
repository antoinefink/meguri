//! Composition only: Runtime owns V14; existing providers own capacity/backup.
use super::*;
use meguri_sync::cloud::{
    adoption::{AdoptionCancellation, AdoptionReply, AdoptionWork},
    publication::Preview,
    runtime::RuntimeError,
};
impl CoreHost {
    /// V12.2 local facts are available even without cloud configuration. This
    /// does not open a missing collection or construct account/network ports.
    pub fn local_publication_information(
        &self,
        generation: meguri_engine::Generation,
    ) -> Result<meguri_sync::cloud::local_publication::LocalPublicationInformation, RuntimeError> {
        use meguri_sync::cloud::{local_publication as local, publication::PublicationError::AuthorityChanged};
        if self.cloud_connection_generation()? != generation {
            return Err(AuthorityChanged.into());
        }
        let information = if matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection) {
            let path = self.cloud_connection_directory().join("collection.anki2");
            let absent = || -> Result<(), RuntimeError> {
                match path.symlink_metadata() {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(meguri_storage::StorageError::from(e).into()),
                    Ok(_) => Err(AuthorityChanged.into()),
                }
            };
            absent()?;
            let decision = meguri_engine::device_state::DeviceState::for_collection(&path).publication_decision()?;
            let mut info = local::LocalPublicationInformation::absent(generation.0);
            if let Some(runtime) = self.cloud_runtime() {
                info.tracking = runtime.local_publication_tracking_information()?;
            }
            absent()?;
            if meguri_engine::device_state::DeviceState::for_collection(&path).publication_decision()? != decision {
                return Err(AuthorityChanged.into());
            }
            info
        } else {
            let collection = self.cloud_connection_collection()?;
            if collection.generation() != generation {
                return Err(AuthorityChanged.into());
            }
            local::read_local_publication_information(
                &collection,
                &self.media()?,
                self.cloud_runtime().map(|runtime| runtime.as_ref()),
            )?
        };
        if self.cloud_connection_generation()? != generation {
            return Err(AuthorityChanged.into());
        }
        Ok(information)
    }
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
    /// Linearize these internal starts with the original Host's Closing
    /// transition. No provider, Runtime lock or slot admission runs here.
    pub fn start_cloud_connection_job(
        &self,
        generation: meguri_engine::Generation,
        name: &str,
        work: impl FnOnce(&meguri_engine::JobContext) -> Result<(), EngineError> + Send + 'static,
    ) -> Result<meguri_engine::JobHandle, EngineError> {
        let closed = self.closed.lock().unwrap_or_else(PoisonError::into_inner);
        if !matches!(*closed, CloseState::Active) {
            return Err(EngineError::CollectionUnavailable {
                reason: "cloud connection host closed".into(),
            });
        }
        let current = self.engine().generation();
        if current != generation {
            return Err(EngineError::StaleGeneration {
                planned: generation,
                current,
            });
        }
        Ok(self.engine().jobs().start(name, generation, None, work))
    }
    /// A retained native connection must never follow this process engine into
    /// a successor host after close. Drop the state guard before any core work.
    pub fn cloud_connection_collection(&self) -> Result<meguri_engine::Collection, EngineError> {
        self.cloud_connection_generation()?;
        self.collection()
    }
    pub fn prepare_cloud_runtime(&self, timeout: std::time::Duration) -> Result<(), RuntimeError> {
        self.cloud_connection_generation()?;
        self.cloud_runtime().ok_or_else(unavailable)?
            .prepare_binding(&self.cloud_connection_collection()?, &self.media()?, timeout)
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
    pub fn stop_using_icloud(&self) -> Result<(), RuntimeError> {
        let collection = self.cloud_connection_collection()?;
        self.cloud_runtime().ok_or_else(unavailable)?.stop_using_icloud(&collection)
    }
    pub fn keep_cloud_local_only(
        &self, generation: meguri_engine::Generation, preview: Option<&Preview>,
    ) -> Result<(), RuntimeError> {
        if self.cloud_connection_generation()? != generation { return Err(RuntimeError::StaleIncarnation); }
        let collection = self.cloud_connection_collection()?;
        if collection.generation() != generation { return Err(RuntimeError::StaleIncarnation); }
        self.cloud_runtime().ok_or_else(unavailable)?.keep_local_only(&collection, preview)
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
        self.next_cloud_publication_work_with_work(&mut meguri_sync::cloud::policy::PolicyWork::default())
    }
    pub fn next_cloud_publication_work_with_work(
        &self,
        work: &mut meguri_sync::cloud::policy::PolicyWork,
    ) -> Result<meguri_sync::cloud::publication::PublicationWork, RuntimeError> {
        let runtime = self.cloud_runtime().ok_or_else(unavailable)?;
        match self.cloud_adoption_collection()? {
            Some(collection) => runtime.next_publication_work_with_work(&collection, work),
            None => Ok(runtime
                .next_adoption_work_with_work(None, work)?
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
    pub fn cloud_adoption_cancellable(&self, id: [u8; 16]) -> Result<bool, RuntimeError> {
        Ok(self.cloud_adoption_cancellation(id)? != AdoptionCancellation::NotOffered)
    }
    pub fn cloud_adoption_cancellation(&self, id: [u8; 16]) -> Result<AdoptionCancellation, RuntimeError> {
        self.cloud_adoption_collection()?;
        self.cloud_runtime().ok_or_else(unavailable)?.cloud_adoption_cancellation(id)
    }
    pub fn cancel_cloud_adoption(&self, id: [u8; 16]) -> Result<(), RuntimeError> {
        let generation = self.cloud_connection_generation()?;
        let collection = self.cloud_adoption_collection()?;
        self.cloud_runtime()
            .ok_or_else(unavailable)?
            .cancel_cloud_adoption_for_host(id, collection.as_ref(), self.engine(), generation)
    }
    pub fn resume_cloud_adoption_with_work(
        &self,
        id: [u8; 16],
        work: &mut meguri_sync::cloud::policy::PolicyWork,
    ) -> Result<bool, RuntimeError> {
        self.cloud_runtime()
            .ok_or_else(unavailable)?
            .resume_cloud_adoption_with_work(id, self.cloud_adoption_collection()?.as_ref(), work)
    }
    pub fn cloud_adoption_reply(&self, work: &AdoptionWork, reply: AdoptionReply) -> Result<(), RuntimeError> {
        self.cloud_adoption_reply_with_work(work, reply, &mut meguri_sync::cloud::policy::PolicyWork::default())
    }
    pub fn cloud_adoption_reply_with_work(
        &self,
        work: &AdoptionWork,
        reply: AdoptionReply,
        authority: &mut meguri_sync::cloud::policy::PolicyWork,
    ) -> Result<(), RuntimeError> {
        self.cloud_adoption_reply_with_diagnostic(
            work,
            reply,
            authority,
            &mut meguri_sync::cloud::adoption::AdoptionDiagnosticRecorder::new(&work.task),
        )
    }
    pub fn cloud_adoption_reply_with_diagnostic(
        &self,
        work: &AdoptionWork,
        reply: AdoptionReply,
        authority: &mut meguri_sync::cloud::policy::PolicyWork,
        diagnostic: &mut meguri_sync::cloud::adoption::AdoptionDiagnosticRecorder,
    ) -> Result<(), RuntimeError> {
        self.cloud_runtime()
            .ok_or_else(unavailable)?
            .cloud_adoption_reply_with_diagnostic(
                work,
                reply,
                self.cloud_adoption_collection()?.as_ref(),
                self.backup_inputs.space.as_ref(),
                authority,
                diagnostic,
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
