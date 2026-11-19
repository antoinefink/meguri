//! `sync/meguri-provider#MP6.9`: settle one upload whose acceptance is unknown
//! from a fresh read of the server's facts. No request is made here, and only
//! a judged outcome can change the retained record.
use super::*;

/// What the server's facts prove about a possibly accepted upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadSettlement {
    /// The server's facts are the candidate's: the upload was accepted.
    Accepted,
    /// The server's facts are still those read after the retained copy.
    NotAccepted,
    /// The server holds something else. Nothing is recorded.
    Neither,
}

/// `None` when this record is not an upload waiting for its acceptance.
/// `candidate` are the facts of the retained file this device sent.
pub fn judge_upload(operation: &Operation, candidate: &LocalFacts, server: &RemoteFacts) -> Option<UploadSettlement> {
    if operation.direction != Direction::Export
        || operation.acceptance != Acceptance::PossiblyAccepted
        || operation.collection_complete
        || operation.activation_identity.is_some()
        || !matches!(
            operation.phase,
            Phase::ConfirmedIntent | Phase::Transferring | Phase::NeedsDecision
        )
    {
        return None;
    }
    let baseline = operation.remote.as_ref()?;
    let is_candidate = server.modified == candidate.modified
        && server.schema_modified == candidate.schema_modified
        && server.usn == candidate.usn
        && server.empty == (candidate.cards == 0);
    let is_baseline = server == baseline;
    Some(match (is_candidate, is_baseline) {
        (true, false) => UploadSettlement::Accepted,
        (false, true) => UploadSettlement::NotAccepted,
        // Facts that fit both sides distinguish nothing.
        _ => UploadSettlement::Neither,
    })
}

impl Artifact {
    /// True only when the file at `path` has this artifact's length and digest.
    pub fn matches_file(&self, path: &Path) -> bool {
        use std::io::Read;
        let Ok(mut file) = std::fs::File::open(path) else {
            return false;
        };
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 1 << 16];
        let mut length = 0u64;
        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    hash.update(&buffer[..n]);
                    length += n as u64;
                }
                Err(_) => return false,
            }
        }
        length == self.identity.length && hash.finalize().as_slice() == self.identity.sha256
    }
}

impl TransferPolicy {
    /// One durable transition of exactly the record the caller read.
    /// Accepted records the exact acceptance and leaves local activation to
    /// the retained recovery path, still under the exclusion. Not accepted
    /// records that nothing was sent and releases the exclusion. Neither
    /// writes nothing: the exclusion stands.
    pub fn settle_upload(
        &self,
        expected: &Operation,
        candidate: &LocalFacts,
        server: &RemoteFacts,
    ) -> Result<(UploadSettlement, Operation), PolicyError> {
        self.update(|state| {
            let current = state.operations.get(expected.id.as_str()).ok_or(PolicyError::Stale)?;
            if current != expected || state.active.as_ref() != Some(&current.id) {
                return Err(PolicyError::Stale);
            }
            let outcome = judge_upload(current, candidate, server).ok_or(PolicyError::Invalid)?;
            let mut settled = current.clone();
            match outcome {
                UploadSettlement::Neither => return Ok((outcome, settled)),
                UploadSettlement::Accepted => {
                    settled.acceptance = Acceptance::ExactOk;
                    settled.phase = Phase::NeedsDecision;
                    settled.boundary = Some(Boundary::ActivationInterrupted);
                }
                UploadSettlement::NotAccepted => {
                    settled.acceptance = Acceptance::NotSent;
                    settled.phase = Phase::Failed;
                    settled.boundary = Some(Boundary::Server);
                }
            }
            settled.revision = settled.revision.checked_add(1).ok_or(PolicyError::Invalid)?;
            state.operations.insert(settled.id.0.clone(), settled.clone());
            if outcome == UploadSettlement::NotAccepted {
                state.active = None;
            }
            Ok((outcome, settled))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(modified: i64, schema_modified: i64, usn: i32) -> RemoteFacts {
        RemoteFacts {
            modified,
            schema_modified,
            usn,
            empty: false,
            media_usn: 0,
        }
    }
    fn candidate() -> LocalFacts {
        LocalFacts {
            modified: 900,
            schema_modified: 900,
            last_sync: 900,
            usn: 6,
            cards: 2,
            notes: 2,
            reviews: 1,
            pending_reviews: 0,
            pending_rows: [0; 8],
            last_review: None,
        }
    }
    fn waiting(policy: &TransferPolicy) -> Operation {
        let account = AccountRef::derive(&Endpoint::parse("https://settle.synthetic.example/").unwrap(), "user").unwrap();
        let operation = Operation {
            id: OperationId(String::new()),
            account,
            configured_endpoint: "https://settle.synthetic.example/".into(),
            target_endpoint: "https://settle.synthetic.example/".into(),
            direction: Direction::Export,
            scope: Scope::CollectionAndMedia,
            phase: Phase::Preparing,
            acceptance: Acceptance::NotSent,
            boundary: None,
            guard: None,
            recovery_guard: None,
            recovery_local: None,
            local: None,
            remote: Some(remote(500, 400, 5)),
            candidate: None,
            activation_identity: None,
            original: None,
            safety_backups: Vec::new(),
            server_copy: None,
            prepared_at: 900,
            actual_generation: Some(1),
            collection_complete: false,
            resulting_publication: None,
            media: MediaOutcome::default(),
            receipt: None,
            revision: 0,
        };
        let mut op = policy.begin(operation, 7, 900).unwrap();
        op.phase = Phase::NeedsDecision;
        op.acceptance = Acceptance::PossiblyAccepted;
        op.boundary = Some(Boundary::RemoteCompletionUnknown);
        policy.save(&mut op).unwrap();
        op
    }
    fn policy() -> (tempfile::TempDir, TransferPolicy) {
        let root = tempfile::tempdir().unwrap();
        let policy = TransferPolicy::new(root.path());
        (root, policy)
    }

    #[test]
    fn the_candidates_facts_record_the_exact_acceptance_and_keep_the_exclusion() {
        let (_root, policy) = policy();
        let op = waiting(&policy);
        let (outcome, settled) = policy.settle_upload(&op, &candidate(), &remote(900, 900, 6)).unwrap();
        assert_eq!(outcome, UploadSettlement::Accepted);
        assert_eq!(settled.acceptance, Acceptance::ExactOk);
        assert_eq!(settled.phase, Phase::NeedsDecision);
        assert_eq!(settled.boundary, Some(Boundary::ActivationInterrupted));
        assert_eq!(policy.active().unwrap(), Some(settled.clone()));
        // The settled record is not settled twice.
        assert_eq!(
            policy.settle_upload(&settled, &candidate(), &remote(900, 900, 6)),
            Err(PolicyError::Invalid)
        );
    }

    #[test]
    fn the_baselines_facts_record_nothing_sent_and_release_the_exclusion() {
        let (_root, policy) = policy();
        let op = waiting(&policy);
        let (outcome, settled) = policy.settle_upload(&op, &candidate(), &remote(500, 400, 5)).unwrap();
        assert_eq!(outcome, UploadSettlement::NotAccepted);
        assert_eq!(settled.acceptance, Acceptance::NotSent);
        assert_eq!(settled.phase, Phase::Failed);
        assert!(policy.active().unwrap().is_none());
        assert_eq!(policy.get(&op.id).unwrap(), settled);
    }

    #[test]
    fn other_facts_write_nothing_and_the_exclusion_stands() {
        let (_root, policy) = policy();
        let op = waiting(&policy);
        for server in [
            remote(700, 400, 6),
            remote(500, 400, 6),
            remote(900, 900, 7),
            RemoteFacts {
                media_usn: 1,
                ..remote(500, 400, 5)
            },
            RemoteFacts {
                empty: true,
                ..remote(900, 900, 6)
            },
        ] {
            let (outcome, same) = policy.settle_upload(&op, &candidate(), &server).unwrap();
            assert_eq!(outcome, UploadSettlement::Neither);
            assert_eq!(same, op);
            assert_eq!(policy.active().unwrap(), Some(op.clone()));
        }
    }

    #[test]
    fn facts_that_fit_both_sides_settle_nothing() {
        let (_root, policy) = policy();
        let op = waiting(&policy);
        let mut same = candidate();
        same.modified = 500;
        same.schema_modified = 400;
        same.usn = 5;
        let (outcome, _) = policy.settle_upload(&op, &same, &remote(500, 400, 5)).unwrap();
        assert_eq!(outcome, UploadSettlement::Neither);
        assert_eq!(policy.active().unwrap(), Some(op));
    }

    #[test]
    fn a_changed_record_or_another_kind_of_record_is_refused() {
        let (_root, policy) = policy();
        let op = waiting(&policy);
        let mut stale = op.clone();
        stale.revision += 1;
        assert_eq!(
            policy.settle_upload(&stale, &candidate(), &remote(900, 900, 6)),
            Err(PolicyError::Stale)
        );
        for change in [0, 1, 2] {
            let mut other = op.clone();
            match change {
                0 => other.direction = Direction::Import,
                1 => other.acceptance = Acceptance::ExactOk,
                _ => other.remote = None,
            }
            assert_eq!(judge_upload(&other, &candidate(), &remote(900, 900, 6)), None);
        }
        assert_eq!(policy.active().unwrap(), Some(op));
    }
}
