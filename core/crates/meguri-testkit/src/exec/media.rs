//! Synthetic media scenarios use the production store; outside writes bypass it.
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{MediaFileSpec, ObservationRequest, Setup, Step};
use meguri_storage::media::{MediaError, MediaStore};
use serde_json::{Map, Value, json};
use sha1::{Digest, Sha1};
use std::{
    fs::{self, File, FileTimes},
    io::{Read, Write},
    path::Path,
    time::{Duration, SystemTime},
};

pub(super) fn register(registry: &mut Registry) {
    registry.setup("media_files", setup);
    registry.op("media_outside", outside);
    registry.op_rejecting("add_media", &["invalid_name", "ignored_name"], add);
    registry.op("trash_media", trash);
    registry.op("restore_media_trash", restore);
    registry.op("empty_media_trash", empty);
    registry.op("media_reset", reset);
    registry.observation("media_dir", directory);
    registry.observation("media_trash", trash_entries);
    registry.observation("media_pending", pending);
    registry.observation("media_quick_check", quick_check);
}
fn core(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}
fn invalid(message: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", message)
}
fn store(session: &mut Session<'_>) -> Result<MediaStore, ExecError> {
    session.storage.media(session.clock.clone(), session.random.clone())
}
fn time(seconds: i64) -> Result<SystemTime, ExecError> {
    let duration = Duration::from_secs(seconds.unsigned_abs());
    let value = if seconds < 0 {
        SystemTime::UNIX_EPOCH.checked_sub(duration)
    } else {
        SystemTime::UNIX_EPOCH.checked_add(duration)
    };
    value.ok_or_else(|| invalid("media timestamp out of range"))
}
fn set_time(path: &Path, value: SystemTime) -> Result<(), ExecError> {
    File::open(path)
        .and_then(|file| file.set_times(FileTimes::new().set_modified(value)))
        .map_err(core)
}
fn outside_files(session: &mut Session<'_>, entries: &[MediaFileSpec]) -> Result<(), ExecError> {
    let store = store(session)?;
    let root = store.media_dir();
    let instant = time(session.clock.now_secs())?;
    for entry in entries {
        let name = meguri_text::normalize::nfc(&entry.name);
        for present in fs::read_dir(root).map_err(core)? {
            let present = present
                .map_err(core)?
                .file_name()
                .into_string()
                .map_err(|_| invalid("non-UTF8 media name"))?;
            if present != entry.name
                && meguri_text::fold::compare_folded(&meguri_text::normalize::nfc(&present), &name).is_eq()
            {
                return Err(invalid("media_outside would create a case or normalization variant"));
            }
        }
        let path = root.join(&entry.name);
        let present = match fs::symlink_metadata(&path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(core(error)),
        };
        if present
            .as_ref()
            .is_some_and(|m| m.file_type().is_symlink() || (!m.is_file() && !m.is_dir()))
        {
            return Err(invalid("media_outside supports only regular files and directories"));
        }
        if entry.remove != Some(true)
            && present
                .as_ref()
                .is_some_and(|m| m.is_dir() != (entry.dir == Some(true)))
        {
            return Err(invalid(
                "media_outside cannot replace a file with a directory or a directory with a file",
            ));
        }
        if entry.remove == Some(true) {
            let metadata = fs::symlink_metadata(&path).map_err(|_| invalid("media_outside removes an absent entry"))?;
            if metadata.is_dir() {
                fs::remove_dir(&path).map_err(core)?;
            } else {
                fs::remove_file(&path).map_err(core)?;
            }
        } else if entry.dir == Some(true) {
            if present.is_none() {
                fs::create_dir(&path).map_err(core)?;
            }
        } else {
            let mut file = File::create(&path).map_err(core)?;
            if let Some(text) = &entry.text {
                file.write_all(text.as_bytes()).map_err(core)?;
            }
            if let Some(fill) = &entry.fill {
                if fill.byte == 0 {
                    file.set_len(fill.size).map_err(core)?;
                } else {
                    let chunk = [fill.byte; 65536];
                    let mut left = fill.size;
                    while left != 0 {
                        let n = left.min(chunk.len() as u64) as usize;
                        file.write_all(&chunk[..n]).map_err(core)?;
                        left -= n as u64;
                    }
                }
            }
            set_time(&path, instant)?;
        }
    }
    set_time(root, instant)
}
fn setup(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    outside_files(session, setup.media_files.as_deref().unwrap_or_default())
}
fn outside(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::MediaOutside(entries) = step else {
        return Err(invalid("wrong media_outside step"));
    };
    outside_files(session, entries)?;
    Ok(OpResult::Done)
}
fn add(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    session.engine_collection()?;
    let Step::AddMedia(entry) = step else {
        return Err(invalid("wrong add_media step"));
    };
    let bytes = if let Some(text) = &entry.text {
        text.as_bytes().to_vec()
    } else {
        let fill = entry
            .fill
            .as_ref()
            .ok_or_else(|| invalid("add_media has no contents"))?;
        vec![fill.byte; fill.size as usize]
    };
    match store(session)?.add(&entry.name, &bytes) {
        Ok(name) => Ok(OpResult::Output(json!({"name": name}))),
        Err(MediaError::InvalidName) => Ok(OpResult::Rejected("invalid_name")),
        Err(MediaError::IgnoredName) => Ok(OpResult::Rejected("ignored_name")),
        Err(error) => Err(core(error)),
    }
}
fn trash(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    session.engine_collection()?;
    let Step::TrashMedia(args) = step else {
        return Err(invalid("wrong trash_media step"));
    };
    store(session)?.trash(&args.names).map_err(core)?;
    Ok(OpResult::Done)
}
fn restore(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    session.engine_collection()?;
    store(session)?.restore().map_err(core)?;
    Ok(OpResult::Done)
}
fn empty(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    session.engine_collection()?;
    store(session)?.empty_trash().map_err(core)?;
    Ok(OpResult::Done)
}
fn hash(path: &Path) -> Result<String, ExecError> {
    let mut file = File::open(path).map_err(core)?;
    let mut digest = Sha1::new();
    let mut chunk = [0_u8; 65536];
    loop {
        let count = file.read(&mut chunk).map_err(core)?;
        if count == 0 {
            break;
        }
        digest.update(&chunk[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn directory(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let store = store(session)?;
    let mut values = Map::new();
    for entry in fs::read_dir(store.media_dir()).map_err(core)? {
        let entry = entry.map_err(core)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| invalid("non-UTF8 media name"))?;
        let metadata = entry.metadata().map_err(core)?;
        let value = if metadata.is_dir() {
            json!({"dir": true})
        } else {
            json!({"sha1": hash(&entry.path())?, "size": metadata.len()})
        };
        values.insert(name, value);
    }
    Ok(Value::Object(values))
}
fn trash_entries(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let mut entries = store(session)?.trash_entries().map_err(core)?;
    entries.sort_by(|a, b| (&a.name, &a.sha1).cmp(&(&b.name, &b.sha1)));
    Ok(Value::Array(
        entries
            .into_iter()
            .map(|entry| json!({"name": entry.name, "sha1": entry.sha1, "size": entry.size}))
            .collect(),
    ))
}
fn pending(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let store = store(session)?;
    store.register_full(|_, _| true).map_err(core)?;
    pending_value(&store)
}

/// Pin hosted generation/lifecycle and share its media-job exclusion. Raw
/// protocol sessions have no host job owner and keep their existing store.
fn local_check<T>(
    session: &mut Session<'_>,
    work: impl FnOnce(&MediaStore) -> Result<T, ExecError>,
) -> Result<T, ExecError> {
    let store = store(session)?;
    if let Some(super::storage::SessionCollection::Hosted { host, collection }) = session.storage.collection() {
        // The maintenance lease pins replacement/close while admission uses Query.
        let _reader = collection
            .reader(meguri_storage::ReaderRole::Maintenance)
            .map_err(core)?;
        let media = host.media().map_err(core)?;
        if let Some(job) = host.engine().jobs().running().into_iter().find(|job| {
            job.name == meguri_engine::JobKind::MediaSync.name()
                || job.name == meguri_engine::JobKind::CheckMedia.name()
        }) {
            return Err(core(meguri_engine::EngineError::Busy { running_op: job.name }));
        }
        let _job = media
            .try_job(meguri_engine::media::MediaJobKind::CheckMedia)
            .map_err(core)?;
        media.check_generation(collection.generation().0).map_err(core)?;
        return work(&store);
    }
    work(&store)
}
fn pending_value(store: &MediaStore) -> Result<Value, ExecError> {
    Ok(Value::Object(
        store
            .pending()
            .map_err(core)?
            .into_iter()
            .map(|entry| (entry.name, entry.sha1.map_or(Value::Null, Value::String)))
            .collect(),
    ))
}
fn quick_check(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    local_check(session, |store| {
        store.check_changes().map_err(core)?;
        pending_value(store)
    })
}
fn reset(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    local_check(session, |store| store.reset_sync().map_err(core))?;
    Ok(OpResult::Done)
}
