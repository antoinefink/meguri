//! Media owns its journal and change channel, independently of collection ops.
use crate::{CoreError, callback, runtime};
#[uniffi::export]
pub fn add_media(name: String, bytes: Vec<u8>) -> Result<String, CoreError> {
    callback::enter("add_media");
    Ok(runtime::host()?.media()?.add(&name, &bytes)?)
}
