//! Fixed test-hook capture of admitted original-role bytes. No source path,
//! SQL, writer, schema initialization or caller-selected reader role.
use crate::{Reader, ReaderFactory, ReaderRole, StorageError, conn_init::Initializers};
use std::io::Write;

fn invalid() -> StorageError {
    StorageError::Io {
        detail: "invalid original snapshot".into(),
    }
}

impl ReaderFactory {
    pub fn original_role_bytes(main: &[u8], wal: Option<&[u8]>) -> Result<Reader, StorageError> {
        const FILE_CAP: usize = 64 * 1024 * 1024;
        if main.len() < 100
            || main.len() > FILE_CAP
            || wal.is_some_and(|w| w.len() > FILE_CAP)
            || main
                .len()
                .checked_add(wal.map_or(0, <[u8]>::len))
                .is_none_or(|n| n > 128 * 1024 * 1024)
            || !main.starts_with(b"SQLite format 3\0")
        {
            return Err(invalid());
        }
        if let Some(wal) = wal.filter(|w| !w.is_empty()) {
            validate_wal(wal)?;
            let page = u16::from_be_bytes([main[16], main[17]]);
            let page = if page == 1 { 65536 } else { u32::from(page) };
            if u32::from_be_bytes(wal[8..12].try_into().unwrap()) != page {
                return Err(invalid());
            }
        }
        let private = tempfile::tempdir()?;
        let path = private.path().join("collection.anki2");
        for (path, bytes) in [
            (path.clone(), Some(main)),
            (private.path().join("collection.anki2-wal"), wal),
        ] {
            if let Some(bytes) = bytes {
                use std::os::unix::fs::OpenOptionsExt;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?;
                file.write_all(bytes)?;
            }
        }
        // Recovery retains its own exact private copy for the reader lifetime.
        Self::recovery(&path, &Initializers::default()).open(ReaderRole::Query)
    }
}

// SQLite WAL format checks before SQLite can silently ignore a damaged tail.
// Validate current frames; a reset may leave an obsolete physical suffix.
fn validate_wal(bytes: &[u8]) -> Result<(), StorageError> {
    let word = |at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    if bytes.len() < 32 || !matches!(word(0), 0x377f0682 | 0x377f0683) || word(4) != 3_007_000 {
        return Err(invalid());
    }
    let page = word(8) as usize;
    if !(512..=65536).contains(&page) || !page.is_power_of_two() || !(bytes.len() - 32).is_multiple_of(page + 24) {
        return Err(invalid());
    }
    let big = word(0) & 1 != 0;
    let sum = |data: &[u8], mut state: [u32; 2]| {
        for pair in data.as_chunks::<8>().0 {
            let read = |b: &[u8]| {
                if big {
                    u32::from_be_bytes(b.try_into().unwrap())
                } else {
                    u32::from_le_bytes(b.try_into().unwrap())
                }
            };
            state[0] = state[0].wrapping_add(read(&pair[..4])).wrapping_add(state[1]);
            state[1] = state[1].wrapping_add(read(&pair[4..])).wrapping_add(state[0]);
        }
        state
    };
    let mut state = sum(&bytes[..24], [0, 0]);
    if state != [word(24), word(28)] {
        return Err(invalid());
    }
    let mut committed = false;
    let mut obsolete = false;
    for at in (32..bytes.len()).step_by(page + 24) {
        if bytes[at + 8..at + 16] != bytes[16..24] {
            if !committed {
                return Err(invalid());
            }
            obsolete = true;
            continue;
        }
        if obsolete || word(at) == 0 {
            return Err(invalid());
        }
        state = sum(&bytes[at..at + 8], state);
        state = sum(&bytes[at + 24..at + 24 + page], state);
        if state != [word(at + 16), word(at + 20)] {
            return Err(invalid());
        }
        committed |= word(at + 4) != 0;
    }
    Ok(())
}
