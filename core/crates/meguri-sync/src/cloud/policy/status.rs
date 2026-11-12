use super::*;
use crate::cloud::{BindingState, IsolationReason};

impl State {
    pub fn guard(&self) -> Guard {
        Guard {
            generation: self.facts.generation,
            binding: self.facts.binding.as_ref().map(|b| b.id),
            token: self.token,
        }
    }
    pub fn automatic(&self) -> bool {
        self.settings.automatic
            || self
                .facts
                .binding
                .as_ref()
                .is_some_and(|b| b.publish_step == Some(PublishStep::Zones))
    }
    pub fn account_matches(&self) -> bool {
        self.facts.binding.as_ref().is_some_and(|b| {
            b.verified
                && self.account.status == AccountStatus::Available
                && self.account.identity.as_ref() == Some(&b.account)
        })
    }
    pub fn local_only(&self) -> bool {
        !self.settings.enabled
            || matches!(self.facts.fence, Fence::LocalOnly | Fence::EnvironmentChanged)
            || self
                .facts
                .binding
                .as_ref()
                .is_some_and(|b| b.state == BindingState::Isolated(IsolationReason::Stopped))
    }
    pub fn engine_allowed(&self) -> bool {
        self.settings.enabled
            && !self.settings.paused
            && self.account_matches()
            && self.facts.fence == Fence::Clear
            && matches!(self.facts.lifecycle, Lifecycle::Ready | Lifecycle::ExclusiveBusy)
            && self.tracking_stop.is_none()
            && self.facts.binding.as_ref().is_some_and(|b| {
                b.usable
                    && b.state == BindingState::Active
                    && !matches!(b.publish_step, Some(PublishStep::Bound | PublishStep::Cancelling))
            })
    }
    pub fn gate_open(&self) -> bool {
        self.read_gate_open() && self.facts.summary.crt_known && !self.facts.summary.crt_mismatch
    }
    // P6 closes collection sends/applies, never the existing engine/read path.
    pub(super) fn read_gate_open(&self) -> bool {
        self.engine_allowed()
            && self.facts.lifecycle == Lifecycle::Ready
            && self.facts.summary.collection_known
            && self.facts.summary.scan == Scan::None
            && self.facts.summary.unsettled_intents == 0
    }
    pub fn engine_runs(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(|e| !e.stopping && self.facts.binding.as_ref().is_some_and(|b| b.id == e.binding))
    }
    pub fn sync_now_availability(&self) -> Result<(), Unavailable> {
        if self.facts.lifecycle != Lifecycle::Ready {
            return Err(Unavailable::CollectionUnavailable);
        }
        if self.facts.session.is_some() {
            return Err(Unavailable::ReviewActive);
        }
        if self.local_only() || self.facts.binding.is_none() {
            return Err(Unavailable::NotConnected);
        }
        if self.settings.paused {
            return Err(Unavailable::Paused);
        }
        if self.account.status != AccountStatus::Available {
            return Err(Unavailable::ICloudUnavailable);
        }
        if self.facts.fence == Fence::DecisionRequired {
            return Err(Unavailable::DecisionRequired);
        }
        if self
            .facts
            .binding
            .as_ref()
            .is_some_and(|b| b.state != BindingState::Active)
            || !self.account_matches()
            || self.facts.summary.crt_mismatch
            || matches!(self.facts.fence, Fence::UnprovenAdoption | Fence::ChooseCollection)
        {
            return Err(Unavailable::RecoveryRequired);
        }
        Ok(())
    }
    pub fn controls(&self) -> Controls {
        let binding = self.facts.binding.as_ref();
        let usable = binding.is_some_and(|b| b.usable && b.state == BindingState::Active);
        let active = binding.is_some_and(|b| b.state == BindingState::Active);
        let stopped = binding.is_some_and(|b| b.state == BindingState::Isolated(IsolationReason::Stopped));
        let decision = self.facts.fence == Fence::DecisionRequired;
        let enable = if self.settings.paused {
            EnableControl::ResumePause
        } else if decision {
            EnableControl::DecisionRequired
        } else if stopped {
            EnableControl::AwaitingResumeOffer
        } else if !usable {
            EnableControl::Connect
        } else {
            EnableControl::Pause
        };
        // Reuse the real engine gate; incomplete attention evidence does not erase
        // a usable binding or claim that the collection has reached a quiet point.
        let automatic_eligible = self.engine_allowed()
            && self.automatic()
            && !self.transient_auth
            && !self.quota_full
            && !self.facts.summary.crt_mismatch
            && !self.facts.summary.attention.iter().any(|(item, count)| {
                *count > 0
                    && matches!(
                        item,
                        Attention::Isolated
                            | Attention::RootUnreadable
                            | Attention::ChooseCollection
                            | Attention::CollectionCorrupt
                            | Attention::RecoveryOpWaiting
                            | Attention::QuotaFull
                            | Attention::LocalStorageFull
                            | Attention::TrackingError
                            | Attention::CloudError
                    )
            });
        Controls {
            enable,
            binding_usable: usable,
            binding_active: active,
            binding_stopped: stopped,
            decision_required: decision,
            user_paused: self.settings.paused,
            resume_binding_offered: self.facts.resume_binding_offered,
            automatic_eligible,
        }
    }
    pub fn status(&self) -> Status {
        let s = &self.facts.summary;
        let mut attention = s.attention.clone();
        attention.retain(|_, count| *count > 0);
        let recovery = attention.contains_key(&Attention::RecoveryOpWaiting);
        let mut add = |kind, condition| {
            if condition {
                attention.entry(kind).or_insert(1);
            }
        };
        add(Attention::Isolated, self.facts.binding.as_ref().is_some_and(|b| matches!(b.state, BindingState::Isolated(reason)
            if !matches!(reason, IsolationReason::Stopped | IsolationReason::ReplacedLocally | IsolationReason::TrackingDamaged))) && !recovery);
        add(
            Attention::ChooseCollection,
            self.settings.enabled
                && (self.facts.binding.is_none() || self.facts.fence == Fence::ChooseCollection)
                && self.facts.fence != Fence::EnvironmentChanged,
        );
        add(Attention::CrtMismatch, s.crt_mismatch);
        add(Attention::CollectionCorrupt, self.facts.lifecycle == Lifecycle::Corrupt);
        add(Attention::QuotaFull, self.quota_full);
        add(Attention::LocalStorageFull, self.tracking_stop == Some(true));
        add(Attention::TrackingError, self.tracking_stop == Some(false));
        add(Attention::CloudError, self.cycles.values().any(|c| c.cloud_error));
        add(Attention::MediaZoneMissing, s.media.zone_missing == Some(true));
        if let Some(count) = s.media.missing_names.filter(|n| *n > 0) {
            attention.insert(Attention::MediaMissing, count);
        }
        let unavailable = match self.account.status {
            AccountStatus::Available if self.transient_auth => Some(UnavailableReason::Temporary),
            AccountStatus::Available => None,
            AccountStatus::NoAccount => Some(UnavailableReason::NoAccount),
            AccountStatus::Restricted => Some(UnavailableReason::Restricted),
            AccountStatus::Configuration => Some(UnavailableReason::SetupNeeded),
            AccountStatus::Temporary | AccountStatus::Undetermined => Some(UnavailableReason::Temporary),
        };
        let collection_active = self
            .cycles
            .iter()
            .any(|(op, c)| c.active && op.zone != Zone::Media && op.kind != OperationKind::Read);
        let media_active = self.cycles.iter().any(|(op, c)| c.active && op.zone == Zone::Media)
            || s.media.uploads_running.is_some_and(|n| n > 0)
            || s.media.downloads_running.is_some_and(|n| n > 0);
        let state = if self.local_only() {
            StatusState::LocalOnly
        } else if let Some(reason) = unavailable {
            StatusState::ICloudUnavailable(reason)
        } else if self.facts.fence == Fence::DecisionRequired {
            StatusState::Paused(PausedReason::DecisionRequired)
        } else if !attention.is_empty() {
            StatusState::NeedsAttention
        } else if self.settings.paused {
            StatusState::Paused(PausedReason::UserPaused)
        } else if !self.automatic() {
            StatusState::Paused(PausedReason::AutomaticOff)
        } else if !self.reachable
            || self
                .last_error
                .as_ref()
                .is_some_and(|e| e.class == OperationError::Offline)
        {
            StatusState::Offline
        } else if self.exchange.is_some() || collection_active {
            StatusState::SyncingCollection
        } else if media_active {
            StatusState::SyncingMedia
        } else if s.pending_unheld > 0 || s.sendable > 0 || s.parked > 0 || s.in_flight > 0 || s.ready > 0 {
            StatusState::ChangesPending
        } else if !s.collection_known
            || !s.crt_known
            || !s.attention_known
            || s.scan != Scan::None
            || s.apply_in_progress
            || s.unsettled_intents > 0
        {
            StatusState::AwaitingEvidence
        } else if let Some(as_of_ms) = self.evidence.as_of_ms() {
            StatusState::UpToDate { as_of_ms }
        } else {
            StatusState::AwaitingEvidence
        };
        let media_paused = !self.settings.media || self.settings.paused || s.media.no_space == Some(true);
        let mut media = s.media.clone();
        if media_paused {
            media.remaining_download_ms = None;
        }
        Status {
            state,
            pending: s.sendable,
            held_back: s.held,
            in_flight: s.in_flight,
            incoming_waiting: s.incoming_waiting,
            attention,
            media,
            media_paused,
            error: self.last_error.clone(),
            account: self.account.clone(),
            account_matches_binding: self.account_matches(),
            evidence: self.evidence.clone(),
            sync_now: self.sync_now_availability(),
            controls: self.controls(),
        }
    }
}
