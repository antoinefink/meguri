//! V8.2 ordinary active reopen only. All live and pending authorities stay native.
use meguri_storage::persistent_identity::{CloudFileEvidence, PersistentIdentity};
use serde::{Deserialize, Serialize};

use super::{
    Error,
    runtime::{NativeFileIdentity, RuntimeError, StoredRuntimeState},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableFileIdentity {
    pub version: u32,
    pub identity: PersistentIdentity,
    pub created_seconds: i64,
    pub created_nanos: u32,
}

impl DurableFileIdentity {
    pub(super) fn validate(self) -> Result<(), Error> {
        let inode = match self.identity {
            PersistentIdentity::Darwin { inode, .. } | PersistentIdentity::NonDarwin { inode, .. } => inode,
        };
        if self.version != 1 || !self.identity.valid_for_platform() || inode == 0 || self.created_nanos >= 1_000_000_000
        {
            return Err(Error::InvalidData);
        }
        Ok(())
    }

    pub(super) fn paired(self, file: NativeFileIdentity) -> Result<(), Error> {
        self.validate()?;
        let matches = match self.identity {
            PersistentIdentity::Darwin { inode, .. } => inode == file.inode,
            PersistentIdentity::NonDarwin { device, inode } => (device, inode) == (file.device, file.inode),
        };
        if !matches || (self.created_seconds, self.created_nanos) != (file.created_seconds, file.created_nanos) {
            return Err(Error::InvalidData);
        }
        Ok(())
    }

    pub(super) fn from_evidence(
        evidence: Option<CloudFileEvidence>,
        file: NativeFileIdentity,
    ) -> Result<Self, RuntimeError> {
        let evidence = evidence.ok_or(RuntimeError::FileIdentityUnavailable)?;
        if evidence.native != (file.device, file.inode, file.created_seconds, file.created_nanos) {
            return Err(RuntimeError::FileIdentityUnavailable);
        }
        let value = Self {
            version: 1,
            identity: evidence.identity,
            created_seconds: evidence.native.2,
            created_nanos: evidence.native.3,
        };
        value.paired(file).map_err(|_| RuntimeError::FileIdentityUnavailable)?;
        Ok(value)
    }
}

// A present null/partial/unknown value must never become a legacy absence.
pub(super) fn present<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<DurableFileIdentity>, D::Error> {
    let value = DurableFileIdentity::deserialize(de)?;
    value.validate().map_err(serde::de::Error::custom)?;
    Ok(Some(value))
}

impl StoredRuntimeState {
    pub(super) fn reopen_matches(&self, file: NativeFileIdentity, durable: DurableFileIdentity) -> Result<bool, Error> {
        self.validate()?;
        durable.paired(file)?;
        Ok(match self.durable_file {
            Some(prior) => prior == durable && (cfg!(target_vendor = "apple") || self.file == Some(file)),
            None => self.file.is_none_or(|prior| prior == file),
        })
    }

    pub(super) fn record_identity(
        &mut self,
        file: NativeFileIdentity,
        durable: DurableFileIdentity,
    ) -> Result<(), Error> {
        durable.paired(file)?;
        if self.file.is_some_and(|prior| prior != file) {
            self.note_scan = None;
            self.coverage = None;
            self.whole_scan = false;
            self.drained = false;
            self.close_metadata = None;
        }
        self.file = Some(file);
        self.durable_file = Some(durable);
        Ok(())
    }
}
