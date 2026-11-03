//! Device-local C12 identity. Live replacement guards continue to use dev/inode.

use crate::StorageError;
use serde::{Deserialize, Serialize};
use std::{fs::File, os::unix::fs::MetadataExt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case", deny_unknown_fields)]
pub enum PersistentIdentity {
    Darwin { volume_uuid: [u8; 16], inode: u64 },
    NonDarwin { device: u64, inode: u64 },
}
impl PersistentIdentity {
    pub fn valid_for_platform(self) -> bool {
        match self {
            Self::Darwin { volume_uuid, .. } => cfg!(target_vendor = "apple") && volume_uuid != [0; 16],
            Self::NonDarwin { .. } => !cfg!(target_vendor = "apple"),
        }
    }
}

/// Cloud reopen evidence captured from the Writer's retained descriptor.
/// The tuple includes exact birthtime; this does not change C12's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloudFileEvidence {
    pub native: (u64, u64, i64, u32),
    pub identity: PersistentIdentity,
}

pub(crate) fn cloud_native(file: &File) -> Result<(u64, u64, i64, u32), StorageError> {
    let metadata = file.metadata().map_err(|_| unavailable())?;
    let (seconds, nanos) = {
        let created = metadata.created().map_err(|_| unavailable())?;
        match created.duration_since(std::time::UNIX_EPOCH) {
            Ok(value) => (
                i64::try_from(value.as_secs()).map_err(|_| unavailable())?,
                value.subsec_nanos(),
            ),
            Err(error) => {
                let value = error.duration();
                let seconds = i64::try_from(value.as_secs()).map_err(|_| unavailable())?;
                if value.subsec_nanos() == 0 {
                    (-seconds, 0)
                } else {
                    (
                        seconds
                            .checked_neg()
                            .and_then(|s| s.checked_sub(1))
                            .ok_or_else(unavailable)?,
                        1_000_000_000 - value.subsec_nanos(),
                    )
                }
            }
        }
    };
    if metadata.ino() == 0 || nanos >= 1_000_000_000 {
        return Err(unavailable());
    }
    Ok((metadata.dev(), metadata.ino(), seconds, nanos))
}

pub(crate) fn checked_cloud(
    bound: (u64, u64),
    before: (u64, u64, i64, u32),
    identity: PersistentIdentity,
    after: (u64, u64, i64, u32),
) -> Result<CloudFileEvidence, StorageError> {
    if before != after || (before.0, before.1) != bound {
        return Err(mismatch());
    }
    Ok(CloudFileEvidence {
        native: before,
        identity,
    })
}
fn unavailable() -> StorageError {
    StorageError::Io {
        detail: "pending_answer_stable_identity_unavailable".into(),
    }
}
fn mismatch() -> StorageError {
    StorageError::Io {
        detail: "pending_answer_identity_mismatch".into(),
    }
}

/// Borrow the original retained descriptor; never open or close another database
/// descriptor while SQLite owns POSIX locks. Recovery-only ownership may supply
/// its retained descriptor before opening SQLite.
pub fn read_from(file: &File, bound: (u64, u64)) -> Result<PersistentIdentity, StorageError> {
    let metadata = file.metadata().map_err(|_| unavailable())?;
    let before = (metadata.dev(), metadata.ino());
    if before != bound {
        return Err(mismatch());
    }
    #[cfg(target_vendor = "apple")]
    let observed = volume_uuid(file);
    let metadata = file.metadata().map_err(|_| unavailable())?;
    let after = (metadata.dev(), metadata.ino());
    #[cfg(target_vendor = "apple")]
    {
        checked(bound, before, observed, after)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        if after != bound {
            return Err(mismatch());
        }
        Ok(PersistentIdentity::NonDarwin {
            device: before.0,
            inode: before.1,
        })
    }
}
#[cfg(any(target_vendor = "apple", test))]
fn checked(
    bound: (u64, u64),
    before: (u64, u64),
    uuid: Result<[u8; 16], StorageError>,
    after: (u64, u64),
) -> Result<PersistentIdentity, StorageError> {
    if before != bound || after != bound {
        return Err(mismatch());
    }
    let volume_uuid = uuid?;
    if volume_uuid == [0; 16] {
        return Err(unavailable());
    }
    Ok(PersistentIdentity::Darwin {
        volume_uuid,
        inode: bound.1,
    })
}

// Public Darwin sys/attr.h ABI: attribute_set_t contains five u32 bitmaps;
// attrlist starts with bitmapcount/reserved u16 followed by those bitmaps.
// ATTR_CMN_RETURNED_ATTRS is returned ahead of the requested volume UUID.
#[cfg(any(target_vendor = "apple", test))]
const RETURNED_ATTRS: u32 = 0x8000_0000;
#[cfg(any(target_vendor = "apple", test))]
const VOL_INFO: u32 = 0x8000_0000;
#[cfg(any(target_vendor = "apple", test))]
const VOL_UUID: u32 = 0x0004_0000;
#[cfg(any(target_vendor = "apple", test))]
fn decode_uuid(bytes: &[u8]) -> Result<[u8; 16], StorageError> {
    if bytes.len() != 40 {
        return Err(unavailable());
    }
    let word = |offset| u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
    if word(0) != 40
        || word(4) != RETURNED_ATTRS
        || word(8) & VOL_UUID == 0
        || word(8) & !(VOL_INFO | VOL_UUID) != 0
        || word(12) != 0
        || word(16) != 0
        || word(20) != 0
    {
        return Err(unavailable());
    }
    let uuid = bytes[24..40].try_into().unwrap();
    if uuid == [0; 16] {
        return Err(unavailable());
    }
    Ok(uuid)
}
#[cfg(target_vendor = "apple")]
fn volume_uuid(file: &File) -> Result<[u8; 16], StorageError> {
    use std::os::fd::AsRawFd;
    #[repr(C)]
    struct AttrList {
        bitmapcount: u16,
        reserved: u16,
        commonattr: u32,
        volattr: u32,
        dirattr: u32,
        fileattr: u32,
        forkattr: u32,
    }
    // Public unistd.h declaration; no private SPI or implementation dependency.
    unsafe extern "C" {
        fn fgetattrlist(
            fd: libc::c_int,
            attributes: *mut AttrList,
            buffer: *mut libc::c_void,
            size: libc::size_t,
            options: libc::c_ulong,
        ) -> libc::c_int;
    }
    let mut attributes = AttrList {
        bitmapcount: 5,
        reserved: 0,
        commonattr: RETURNED_ATTRS,
        volattr: VOL_INFO | VOL_UUID,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    // u32-aligned buffer, with explicit byte decoding (no packed-field reads).
    let mut buffer = [0u32; 10];
    // SAFETY: the live borrowed descriptor, public ABI attrlist and writable
    // 40-byte buffer remain valid throughout this synchronous call.
    let result = unsafe {
        fgetattrlist(
            file.as_raw_fd(),
            &mut attributes,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer),
            0,
        )
    };
    if result != 0 {
        return Err(unavailable());
    }
    let bytes: Vec<u8> = buffer.into_iter().flat_map(u32::to_ne_bytes).collect();
    decode_uuid(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_identity_capture_checks_both_birthtime_fields_and_live_pair() {
        let before = (17, 29, 41, 53);
        let identity = PersistentIdentity::NonDarwin { device: 17, inode: 29 };
        assert!(checked_cloud((17, 29), before, identity, before).is_ok());
        for after in [(18, 29, 41, 53), (17, 30, 41, 53), (17, 29, 42, 53), (17, 29, 41, 54)] {
            assert!(checked_cloud((17, 29), before, identity, after).is_err());
        }
        assert!(checked_cloud((99, 29), before, identity, before).is_err());
    }

    fn reply() -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [40u32, RETURNED_ATTRS, VOL_INFO | VOL_UUID, 0, 0, 0] {
            bytes.extend(word.to_ne_bytes());
        }
        bytes.extend([23; 16]);
        bytes
    }

    #[test]
    fn persistent_identity_validates_full_uuid_length_and_all_returned_masks() {
        assert_eq!(decode_uuid(&reply()).unwrap(), [23; 16]);
        for length in 0..40 {
            assert!(decode_uuid(&reply()[..length]).is_err());
        }
        for offset in [0, 4, 8, 12, 16, 20] {
            let mut bytes = reply();
            bytes[offset..offset + 4].copy_from_slice(&1u32.to_ne_bytes());
            assert!(decode_uuid(&bytes).is_err(), "word {offset}");
        }
        let mut bytes = reply();
        bytes[24..].fill(0);
        assert!(decode_uuid(&bytes).is_err());
    }

    #[test]
    fn persistent_identity_unavailable_and_descriptor_change_have_fixed_categories() {
        assert!(
            checked((1, 2), (1, 2), Err(unavailable()), (1, 2))
                .unwrap_err()
                .to_string()
                .contains("pending_answer_stable_identity_unavailable")
        );
        assert!(
            checked((1, 2), (3, 2), Ok([23; 16]), (3, 2))
                .unwrap_err()
                .to_string()
                .contains("pending_answer_identity_mismatch")
        );
        assert!(checked((1, 2), (1, 2), Ok([23; 16]), (1, 3)).is_err());
        assert_eq!(
            checked((1, 2), (1, 2), Ok([23; 16]), (1, 2)).unwrap(),
            PersistentIdentity::Darwin {
                volume_uuid: [23; 16],
                inode: 2
            }
        );
    }

    #[cfg(target_vendor = "apple")]
    #[test]
    fn darwin_generated_regular_fd_reads_full_volume_uuid_and_retains_binding() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("generated-regular-file");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let metadata = file.metadata().unwrap();
        let bound = (metadata.dev(), metadata.ino());
        let first = read_from(&file, bound).unwrap();
        let PersistentIdentity::Darwin { volume_uuid, inode } = first else {
            panic!("Darwin tag required")
        };
        assert!(volume_uuid != [0; 16], "full nonzero volume UUID required");
        assert!(inode == metadata.ino(), "inode must come from the borrowed descriptor");
        std::fs::rename(&path, dir.path().join("retained-original")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        assert!(
            read_from(&file, bound).unwrap() == first,
            "pathname replacement must not change the descriptor binding"
        );
        let replacement = std::fs::metadata(&path).unwrap();
        assert!(
            replacement.ino() != inode,
            "generated replacement must have a distinct inode"
        );
        assert!(read_from(&file, (replacement.dev(), replacement.ino())).is_err());
    }
}
