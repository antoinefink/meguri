//! Media owns its journal and change channel, independently of collection ops.
use crate::{CoreError, callback, runtime};
#[uniffi::export]
pub fn add_media(name: String, bytes: Vec<u8>) -> Result<String, CoreError> {
    callback::enter("add_media");
    Ok(runtime::host()?.media()?.add(&name, &bytes)?)
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TrackedMediaName {
    pub name: String,
    pub case_key: String,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MediaNameSnapshot {
    pub generation: u64,
    pub root: String,
    pub names: Vec<TrackedMediaName>,
}
#[uniffi::export]
pub fn tracked_media_names() -> Result<MediaNameSnapshot, CoreError> {
    callback::enter("tracked_media_names");
    let snapshot = runtime::host()?.media()?.tracked_names()?;
    let root = snapshot
        .root
        .to_str()
        .ok_or_else(|| CoreError::InvalidArgument {
            message: "media root must be UTF-8".into(),
        })?
        .to_owned();
    Ok(MediaNameSnapshot {
        generation: snapshot.generation,
        root,
        names: snapshot
            .names
            .into_iter()
            .map(|entry| TrackedMediaName {
                name: entry.name,
                case_key: entry.case_key,
            })
            .collect(),
    })
}
