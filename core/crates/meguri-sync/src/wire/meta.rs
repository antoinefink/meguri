//! `meta` (specs/sync/wire-format.md W3; R1513): the client's version and
//! identity out, the server's record back. What the status check does with
//! the record is `crate::status` (meta-status.md).

use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;

use super::read::{self, At};
use super::write;
use crate::error::JobError;
use crate::identity::client_identity;
use crate::transport::Method;

/// The request body: `{"v":11,"cv":<client identity>}`.
pub fn encode_meta_request() -> Vec<u8> {
    let mut out = String::from("{\"v\":11,\"cv\":");
    write::string(&mut out, client_identity());
    out.push('}');
    out.into_bytes()
}

/// The server's answer to `meta` (W3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMeta {
    /// `mod`: the server collection's modification time.
    pub modified: EpochMillis,
    /// `scm`: its schema time.
    pub schema_modified: EpochMillis,
    /// `usn`: its update sequence number.
    pub usn: Usn,
    /// `ts`: the server's clock now.
    pub server_time: EpochSecs,
    /// `msg`: a message for the user, often empty, exactly as received.
    pub message: String,
    /// `cont`: false when the server refuses to sync with this client.
    pub can_continue: bool,
    /// `empty`: the server collection has no cards (missing means false).
    pub empty: bool,
    /// `media_usn`: the server's media update sequence number.
    pub media_usn: i32,
}

/// Reads a `meta` answer (W3); `hostNum` and unknown members are ignored.
pub fn decode_meta(body: &[u8]) -> Result<ServerMeta, JobError> {
    decode(body).map_err(|err| JobError::bad_response(Method::Meta, err.to_string()))
}

fn decode(body: &[u8]) -> read::Result<ServerMeta> {
    let at = At::Root;
    let root = read::object(read::body(body)?, at)?;
    let modified = read::strict::<i64>(root.required("mod", at)?, at.member("mod"))?;
    let schema_modified = read::strict::<i64>(root.required("scm", at)?, at.member("scm"))?;
    let usn = read::strict::<i32>(root.required("usn", at)?, at.member("usn"))?;
    let server_time = read::strict::<i64>(root.required("ts", at)?, at.member("ts"))?;
    let message = read::string(root.required("msg", at)?, at.member("msg"))?;
    let can_continue = read::boolean(root.required("cont", at)?, at.member("cont"))?;
    let empty = match root.get("empty", at)? {
        Some(raw) => read::boolean(raw, at.member("empty"))?,
        None => false,
    };
    let media_usn = read::strict::<i32>(root.required("media_usn", at)?, at.member("media_usn"))?;
    Ok(ServerMeta {
        modified: EpochMillis(modified),
        schema_modified: EpochMillis(schema_modified),
        usn: Usn(usn),
        server_time: EpochSecs(server_time),
        message,
        can_continue,
        empty,
        media_usn,
    })
}
