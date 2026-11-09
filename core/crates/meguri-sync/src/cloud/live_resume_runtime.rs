//! S2.1: active resume observes the original authority; only released suspension binds.
use super::*;
use meguri_engine::device_state::{AccountBinding, DeviceState, Publication, PublicationDecision};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ConnectionLifecycle {
    #[default]
    Active,
    Suspended,
    Released,
}

struct LiveOwner {
    incarnation: u64,
    invalidation: u64,
    bound: Bound,
    store_address: usize,
    bound_address: usize,
    adapter: super::super::adapter::Guard,
    sdk: Option<String>,
    publication_attempt: u64,
    publication_preview: u64,
    preview: Option<(super::super::publication::Preview, bool)>,
}

impl Runtime {
    fn live_resume_eligible(s: &State) -> bool {
        s.connection_lifecycle == ConnectionLifecycle::Active
            && s.store.is_some()
            && s.bound
                .as_ref()
                .is_some_and(|b| b.usable && b.verified && b.binding.facts.state == BindingState::Active)
            && !s.invalidation_exhausted
            && !s.tracking_stopped
            && !s.adapter.paused
            && s.adapter.sdk.as_ref().is_none_or(|sdk| {
                // Validate the originally issued SDK, never two fresh guard samples.
                NativeAuthority::from_state(s).validate_sdk(&sdk.id).is_ok()
            })
    }

    fn live_publication_settled(s: &State) -> bool {
        s.suspended_adoption_cleanup.is_none()
            && s.publication.running.is_none()
            && s.publication.resume.is_none()
            && !s.publication.waiting
            && !s.publication.submitted
            && !s.publication.restarted_confirmed
    }

    fn live_publication_matches(&self, p: &Publication, b: &Bound) -> bool {
        p.identity == b.binding.facts.publication
            && p.decision == PublicationDecision::Clear
            && b.binding.scope == self.inputs.scope
            && matches!(&p.binding, AccountBinding::Connected { account, configuration, environment }
            if account.as_bytes() == b.binding.facts.account.0.as_slice()
                && configuration == b.binding.scope.container.as_str()
                && environment == match b.binding.scope.environment {
                    Environment::Development => "development",
                    Environment::Production => "production",
                })
    }

    // No durable isolation is inferred from an observation, including a failed one.
    // Publish denial before any later tracking work; retain all original cleanup.
    fn refuse_live_resume(s: &mut State) {
        if let Some(b) = s.bound.as_mut() {
            b.usable = false;
            b.verified = false;
        }
        Self::invalidate_authority_locked(s);
    }

    fn same_live_owner(s: &State, original: &LiveOwner) -> bool {
        s.incarnation == original.incarnation
            && s.invalidation == original.invalidation
            && s.bound.as_ref() == Some(&original.bound)
            && s.store.as_ref().map(|store| store as *const TrackingStore as usize) == Some(original.store_address)
            && s.bound.as_ref().map(|bound| bound as *const Bound as usize) == Some(original.bound_address)
            && Self::adapter_guard_locked(s) == original.adapter
            && s.adapter.sdk.as_ref().map(|sdk| &sdk.id) == original.sdk.as_ref()
            && s.publication.next_attempt == original.publication_attempt
            && s.publication.next_preview == original.publication_preview
            && s.publication.preview == original.preview
            && Self::live_publication_settled(s)
            && Self::live_resume_eligible(s)
            && original
                .sdk
                .as_ref()
                .is_none_or(|sdk| NativeAuthority::from_state(s).validate_sdk(sdk).is_ok())
    }

    pub(super) fn retain_live_resume(&self, collection: &Collection) -> Result<(), RuntimeError> {
        // Adoption's mutex precedes State in its production owner. Never acquire
        // it under State, and never retain it across any external observation.
        let adopting = self.adoption.lock().unwrap_or_else(PoisonError::into_inner).is_some();
        let (original, stored) = {
            let mut s = self.state();
            if s.connection_lifecycle == ConnectionLifecycle::Active
                && (adopting || !Self::live_publication_settled(&s))
            {
                return Ok(());
            }
            if s.store.is_none() && s.bound.as_ref().is_some_and(|b| b.usable && b.verified) {
                Self::refuse_live_resume(&mut s);
                return Err(RuntimeError::StaleIncarnation);
            }
            // Missing, refused or stale authority cannot be repaired by resume.
            if !Self::live_resume_eligible(&s) {
                return Ok(());
            }
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            let store = s.store.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            let original = LiveOwner {
                incarnation: s.incarnation,
                invalidation: s.invalidation,
                bound: b.clone(),
                store_address: store as *const TrackingStore as usize,
                bound_address: b as *const Bound as usize,
                adapter: Self::adapter_guard_locked(&s),
                sdk: s.adapter.sdk.as_ref().map(|sdk| sdk.id.clone()),
                publication_attempt: s.publication.next_attempt,
                publication_preview: s.publication.next_preview,
                preview: s.publication.preview.clone(),
            };
            let stored = with_tracking_access(|| -> Result<Option<bool>, RuntimeError> {
                if store.adoption_op()?.is_some() || store.publication_op()?.is_some() {
                    return Ok(None);
                }
                let durable = store.runtime_binding(b.binding.id)?;
                Ok(Some(
                    store.binding(b.binding.id)?.as_ref() == Some(&b.binding)
                        && store.selected_binding()?.as_ref() == Some(&b.binding)
                        && durable.open_counter == b.open_counter
                        && durable.file == Some(b.file)
                        && durable.durable_file
                            == Some(DurableFileIdentity::from_evidence(b.evidence.cloud_file, b.file)?),
                ))
            });
            (original, stored)
        };
        if matches!(stored, Ok(None)) {
            return Ok(());
        }
        // Privileged Collection/descriptor, path provider and sidecar observations
        // occur outside both State and tracking. No account or native work drain.
        let observed = (|| -> Result<bool, RuntimeError> {
            if stored? != Some(true) {
                return Ok(false);
            }
            let evidence = collection.open_evidence()?;
            let file = self.inputs.files.identity(collection.path())?;
            let publication = DeviceState::for_collection(collection.path())
                .settled_publication()?
                .ok_or(RuntimeError::Publication(
                    super::super::publication::PublicationError::AuthorityChanged,
                ))?;
            if evidence.cloud_file.is_none() || original.bound.evidence.cloud_file.is_none() {
                return Err(RuntimeError::FileIdentityUnavailable);
            }
            if file != original.bound.file || evidence != original.bound.evidence {
                return Ok(false);
            }
            // Presence and validity are required independently of equality.
            DurableFileIdentity::from_evidence(evidence.cloud_file, file)?;
            DurableFileIdentity::from_evidence(original.bound.evidence.cloud_file, original.bound.file)?;
            Ok(evidence == original.bound.evidence
                && evidence.generation == collection.generation()
                && collection.is_current()
                && file == original.bound.file
                && evidence.native_file == file.pair()
                && self.live_publication_matches(&publication, &original.bound))
        })();
        // Final check is in-memory only: no tracking, providers, callbacks,
        // ordering locks or waits. It cannot restore observer invalidation.
        let mut s = self.state();
        if !Self::live_publication_settled(&s) && matches!(observed, Ok(true)) {
            return Ok(());
        }
        if !Self::same_live_owner(&s, &original) {
            // A concurrent owner change keeps its own refusal and resources.
            // If it left authority usable, deny it rather than accepting old reads.
            if Self::live_resume_eligible(&s) {
                Self::refuse_live_resume(&mut s);
            }
            return Err(RuntimeError::StaleIncarnation);
        }
        match observed {
            Ok(true) => Ok(()),
            Ok(false) => {
                Self::refuse_live_resume(&mut s);
                Err(RuntimeError::StaleIncarnation)
            }
            Err(error) => {
                Self::refuse_live_resume(&mut s);
                Err(error)
            }
        }
    }
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    pub fn forget_live_descriptor_for_testing(&self) {
        if let Some(bound) = self.state().bound.as_mut() {
            bound.evidence.cloud_file = None;
        }
    }
}
