//! Login (specs/sync/login-endpoints.md L5, L6; R1498, R1499): a user name
//! and password become a host key through `hostKey`. The call returns the key
//! and nothing else; the core writes nothing to disk and keeps neither the
//! password nor the key. A 403 is `status` 403, the authentication failure
//! ([`SyncError::is_auth_failure`]).

use meguri_engine::ThreadRegistry;
use meguri_model::rng::RandomSource;
use serde_json::Value;

use crate::error::{InvalidInput, ProtocolError, SyncError};
use crate::job::{HostKey, Job, JobOptions};
use crate::transport::{Method, Payload, runtime};

/// Asks the job's server for a host key. The user name loses white space at
/// both ends; the password is sent as given. An empty user name or password
/// sends nothing.
pub async fn login(job: &mut Job, user_name: &str, password: &str) -> Result<HostKey, SyncError> {
    let body = login_body(user_name, password)?;
    let answer = job.request(Method::HostKey, Payload::Json(body)).await?.bytes().await?;
    host_key(&answer)
}

/// [`login`] as a blocking call on the sync runtime (T9), in a job of its
/// own with its own session key. `threads` is the engine's thread registry
/// ([`runtime::block_on`]).
pub fn login_blocking(
    threads: &ThreadRegistry,
    options: &JobOptions,
    random: &dyn RandomSource,
    user_name: &str,
    password: &str,
) -> Result<HostKey, SyncError> {
    login_body(user_name, password)?;
    let mut job = Job::new(options, None, random)?;
    runtime::block_on(threads, options.cancel_token(), login(&mut job, user_name, password))
}

/// `{"u":<user name>,"p":<password>}`, compact, members in that order.
fn login_body(user_name: &str, password: &str) -> Result<Vec<u8>, SyncError> {
    let user_name = user_name.trim();
    if user_name.is_empty() {
        return Err(SyncError::InvalidInput(InvalidInput::UserName));
    }
    if password.is_empty() {
        return Err(SyncError::InvalidInput(InvalidInput::Password));
    }
    let body = format!("{{\"u\":{},\"p\":{}}}", Value::from(user_name), Value::from(password));
    Ok(body.into_bytes())
}

/// The non-empty string `key` of a JSON object; other members are ignored.
fn host_key(answer: &[u8]) -> Result<HostKey, SyncError> {
    let bad = SyncError::Protocol(ProtocolError::BadLoginAnswer);
    let value: Value = serde_json::from_slice(answer).map_err(|_| bad.clone())?;
    match value.get("key") {
        Some(Value::String(key)) if !key.is_empty() => Ok(HostKey::new(key.clone())),
        _ => Err(bad),
    }
}
