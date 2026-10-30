//! Scoped V14 transitions on the production tracking connection.
use super::{
    adoption::*,
    publication_store::remove_work,
    runtime::StoredRuntimeState,
    store::{decode, encode},
    *,
};
use meguri_engine::device_state::{AccountBinding, CloudAdoptActivation, Publication, PublicationId};
use rusqlite::{OptionalExtension, params};

impl TrackingStore {
    pub(super) fn install_adoption(&mut self) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS cloud_adoption_ops(
            container TEXT NOT NULL, environment INTEGER NOT NULL, content TEXT NOT NULL CHECK(json_valid(content)),
            PRIMARY KEY(container,environment));
            CREATE TABLE IF NOT EXISTS cloud_adoption_authority(
            binding BLOB PRIMARY KEY REFERENCES bindings(id), controls TEXT NOT NULL CHECK(json_valid(controls)),
            activated_file TEXT CHECK(activated_file IS NULL OR json_valid(activated_file)));
            UPDATE cloud_publication_schema SET version=2;",
        )?;
        // Older publication readers must refuse a store carrying adoption work.
        tx.commit()?;
        Ok(())
    }
    pub fn adoption_op(&self) -> Result<Option<AdoptOp>, Error> {
        let text: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_adoption_ops WHERE container=?1 AND environment=?2",
                params![self.scope.container.as_str(), self.scope.environment.code()],
                |r| r.get(0),
            )
            .optional()?;
        text.map(|t| decode(&t)).transpose()
    }
    pub(super) fn record_adoption(&mut self, op: &AdoptOp) -> Result<(), Error> {
        if self.adoption_op()?.is_some() || self.publication_op()?.is_some() {
            return Err(Error::Constraint);
        }
        self.connection.execute(
            "INSERT INTO cloud_adoption_ops VALUES(?1,?2,?3)",
            params![
                self.scope.container.as_str(),
                self.scope.environment.code(),
                encode(op)?
            ],
        )?;
        Ok(())
    }
    pub(super) fn update_adoption(&mut self, op: &AdoptOp) -> Result<(), Error> {
        if self.adoption_op()?.as_ref().map(|p| p.id) != Some(op.id) {
            return Err(Error::InvalidData);
        }
        self.connection.execute(
            "UPDATE cloud_adoption_ops SET content=?3 WHERE container=?1 AND environment=?2",
            params![
                self.scope.container.as_str(),
                self.scope.environment.code(),
                encode(op)?
            ],
        )?;
        Ok(())
    }
    pub(super) fn ready_adoption(
        &mut self,
        op: &mut AdoptOp,
        record_key: RecordKey,
        media_key: RecordKey,
        crt: i64,
        entities: impl Iterator<Item = Result<workflow::EntityFacts, Error>>,
    ) -> Result<(), Error> {
        if op.step != AdoptStep::Staged || self.adoption_op()?.as_ref() != Some(&*op) {
            return Err(Error::InvalidData);
        }
        let before = op
            .binding_before
            .as_ref()
            .map(|b| {
                self.binding(b.id)?
                    .filter(|row| row.facts.state == b.state && row.facts.account == op.account)
                    .ok_or(Error::InvalidData)
            })
            .transpose()?;
        let tx = self.connection.transaction()?;
        if let Some(mut before) = before
            && before.facts.state == BindingState::Active
        {
            before.facts.state = BindingState::Isolated(IsolationReason::ReplacedLocally);
            tx.execute(
                "UPDATE bindings SET facts=?2 WHERE id=?1",
                params![before.id.0.as_slice(), encode(&before.facts)?],
            )?;
        }
        let facts = BindingFacts {
            account: op.account.clone(),
            epoch: op.epoch,
            record_key,
            publication: op.candidate.identity,
            state: BindingState::Active,
        };
        tx.execute(
            "INSERT INTO bindings(id,container,environment,created_at_ms,facts) VALUES(?1,?2,?3,?4,?5)",
            params![
                op.binding_after.0.as_slice(),
                self.scope.container.as_str(),
                self.scope.environment.code(),
                op.started_at_ms,
                encode(&facts)?
            ],
        )?;
        super::runtime_store::put_binding(&tx, op.binding_after, &StoredRuntimeState::default())?;
        tx.execute(
            "INSERT INTO cloud_publication_bindings(binding,media_key,root_acked,crt) VALUES(?1,?2,1,?3)",
            params![op.binding_after.0.as_slice(), media_key.0.as_slice(), crt],
        )?;
        for entity in entities {
            super::workflow_store::put(&tx, op.binding_after, &entity?)?;
        }
        tx.execute(
            "INSERT INTO cloud_media_state VALUES(?1,?2)",
            params![
                op.binding_after.0.as_slice(),
                encode(&super::media::Ledger {
                    adoption: true,
                    ..Default::default()
                })?
            ],
        )?;
        op.step = AdoptStep::Ready;
        tx.execute(
            "INSERT INTO cloud_adoption_authority(binding,controls) VALUES(?1,?2)",
            params![op.binding_after.0.as_slice(), encode(op)?],
        )?;
        tx.execute(
            "UPDATE cloud_adoption_ops SET content=?3 WHERE container=?1 AND environment=?2",
            params![
                self.scope.container.as_str(),
                self.scope.environment.code(),
                encode(op)?
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn end_adoption(
        &mut self,
        op: &AdoptOp,
        activation: Option<&CloudAdoptActivation>,
        file: Option<runtime::NativeFileIdentity>,
    ) -> Result<bool, Error> {
        if self.adoption_op()?.as_ref() != Some(op) {
            return Err(Error::InvalidData);
        }
        let activated = activation.is_some();
        if let Some(proof) = activation {
            let candidate = self.binding(op.binding_after)?.ok_or(Error::BindingNotFound)?;
            if candidate.facts.state == BindingState::Isolated(IsolationReason::FileMismatch) {
                return Ok(false);
            }
            if candidate.facts.state != BindingState::Active
                || candidate.facts.account != op.account
                || candidate.facts.publication != op.candidate.identity
                || candidate.facts.epoch != op.epoch
            {
                return Err(Error::RecoveryRefused);
            }
            let account_matches = matches!(&op.candidate.binding,
                AccountBinding::Connected { account, configuration, environment }
                if account.as_bytes() == op.account.0.as_slice()
                    && configuration == self.scope.container.as_str()
                    && environment == match self.scope.environment { Environment::Development => "development", Environment::Production => "production" });
            if op.step != AdoptStep::Ready
                || !account_matches
                || !match (op.file_before, &op.before) {
                    (Some(_), Some(before)) => {
                        proof.matches(PublicationId::from_bytes(op.id), &before.publication, &op.candidate)
                    }
                    (None, before) => proof.matches_initial_prior(
                        PublicationId::from_bytes(op.id),
                        before.as_ref().map(|b| &b.publication),
                        &op.candidate,
                    ),
                    _ => false,
                }
            {
                return Err(Error::RecoveryRefused);
            }
            let (controls, activated_file): (String, Option<String>) = self.connection.query_row(
                "SELECT controls,activated_file FROM cloud_adoption_authority WHERE binding=?1",
                [op.binding_after.0.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if decode::<AdoptOp>(&controls)? != *op {
                return Err(Error::RecoveryRefused);
            }
            let recorded: Option<runtime::NativeFileIdentity> = activated_file.map(|s| decode(&s)).transpose()?;
            let file = file.ok_or(Error::RecoveryRefused)?;
            if proof.native_file() != (file.device, file.inode)
                || recorded.is_some_and(|prior| prior != file)
                || self
                    .runtime_binding(op.binding_after)?
                    .file
                    .is_some_and(|prior| prior != file)
            {
                self.isolate_runtime_binding(op.binding_after, IsolationReason::FileMismatch)?;
                return Ok(false);
            }
        }
        let before = op
            .binding_before
            .as_ref()
            .map(|b| self.binding(b.id)?.ok_or(Error::BindingNotFound))
            .transpose()?;
        let mut state = if activated {
            Some(self.runtime_binding(op.binding_after)?)
        } else {
            None
        };
        if let Some(state) = &mut state {
            state.file = Some(file.ok_or(Error::RecoveryRefused)?);
            state.drained = false;
        }
        let tx = self.connection.transaction()?;
        if activated {
            if tx.execute(
                "UPDATE cloud_adoption_authority SET activated_file=?2 WHERE binding=?1",
                params![op.binding_after.0.as_slice(), encode(&file)?],
            )? != 1
            {
                return Err(Error::InvalidData);
            }
            super::runtime_store::put_binding(&tx, op.binding_after, state.as_ref().ok_or(Error::InvalidData)?)?;
            if let Some(before) = before {
                remove_work(&tx, before.id)?;
            }
        } else if op.step == AdoptStep::Ready {
            remove_work(&tx, op.binding_after)?;
            if let Some(mut before) = before {
                before.facts.state = op.binding_before.as_ref().ok_or(Error::InvalidData)?.state;
                tx.execute(
                    "UPDATE bindings SET facts=?2 WHERE id=?1",
                    params![before.id.0.as_slice(), encode(&before.facts)?],
                )?;
                // Edits during staging must reconcile before this binding sends.
                let text: Option<String> = tx
                    .query_row(
                        "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
                        [before.id.0.as_slice()],
                        |r| r.get(0),
                    )
                    .optional()?;
                let mut state: StoredRuntimeState = text.map(|s| decode(&s)).transpose()?.unwrap_or_default();
                state.drained = false;
                super::runtime_store::put_binding(&tx, before.id, &state)?;
            }
        }
        tx.execute(
            "DELETE FROM cloud_adoption_ops WHERE container=?1 AND environment=?2",
            params![self.scope.container.as_str(), self.scope.environment.code()],
        )?;
        tx.commit()?;
        Ok(true)
    }
    pub(super) fn recover_adoption(
        &mut self,
        publication: Option<&Publication>,
        activation: Option<&CloudAdoptActivation>,
        file: Option<runtime::NativeFileIdentity>,
    ) -> Result<(), Error> {
        let Some(op) = self.adoption_op()? else {
            return Ok(());
        };
        let activated = publication.is_some_and(|p| p.identity == op.candidate.identity);
        if activated {
            if op.step != AdoptStep::Ready {
                return Err(Error::RecoveryRefused);
            }
            let Some(proof) = activation else {
                self.isolate_runtime_binding(op.binding_after, IsolationReason::FileMismatch)?;
                return Ok(());
            };
            // Complete this already-proven swap before V9 checks the current
            // account. R6's Pending decision remains untouched; end_adoption
            // still requires the exact operation/prior/candidate/native proof.
            let matches = publication.is_some_and(|p| {
                p == &op.candidate
                    || (op.candidate.decision == meguri_engine::device_state::PublicationDecision::Clear
                        && p.decision == meguri_engine::device_state::PublicationDecision::Pending
                        && p.identity == op.candidate.identity
                        && p.binding == op.candidate.binding)
            });
            if !matches {
                return Err(Error::RecoveryRefused);
            }
            self.end_adoption(&op, Some(proof), file)?;
        } else {
            self.end_adoption(&op, None, file)?;
        }
        Ok(())
    }
}
