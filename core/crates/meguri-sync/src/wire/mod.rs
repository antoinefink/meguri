//! The bodies of protocol v11 (specs/sync/wire-format.md): what Meguri
//! writes into each request and how it reads each answer. How a body travels
//! is the transport's (`crate::transport`); what a sync does with the records
//! is U107's (normal), U109's (one-way) and U111's (media).
//!
//! Encoders write compact JSON with members in the spec's order (W1.1).
//! Decoders follow W1.2 and return a [`JobError`] whose kind is
//! `bad_response` (errors.md E6), with the method set and a detail naming the
//! member or element, never the body. No input makes a decoder panic, and
//! `core/fuzz/fuzz_targets/sync_json.rs` feeds every one of them (W18).

pub mod changes;
pub mod chunk;
pub mod graves;
pub mod media;
pub mod meta;
pub(crate) mod read;
pub mod sanity;

pub use changes::{IncomingChanges, OutgoingChanges, decode_changes, encode_apply_changes};
pub use chunk::{
    ChunkIds, IncomingChunk, IncomingNote, OutgoingChunk, PendingIds, decode_chunk, encode_apply_chunk, note_tags,
};
pub use graves::{Graves, decode_graves, encode_apply_graves, encode_start};
pub use media::{
    MediaChange, MediaDigest, MediaFile, MediaSanity, MediaSendReceipt, MediaUploadInput, MediaUploadRecord,
    PreparedMediaUpload, UploadChange, decode_begin, decode_download_zip, decode_media_changes, decode_media_file,
    decode_media_sanity, decode_upload_changes, encode_begin, encode_download_files, encode_media_changes,
    encode_media_sanity, encode_upload_zip, prepare_media_upload, spool_media_reply,
};
pub use meta::{ServerMeta, decode_meta, encode_meta_request};
pub use sanity::{SanityAnswer, SanityCounts, decode_finish, decode_sanity_answer, encode_sanity_check};

use crate::error::{ErrorKind, JobError};
use crate::transport::Method;

/// W14: the `upload` answer is plain text, exactly `OK` on success; any other
/// text is the server's refusal (`server_message`, U109).
pub fn decode_upload_answer(body: &[u8]) -> Result<(), JobError> {
    if body == b"OK" {
        return Ok(());
    }
    let text = String::from_utf8_lossy(body).into_owned();
    Err(JobError::new(
        ErrorKind::ServerMessage { text },
        Some(Method::Upload),
        "the upload was refused",
    ))
}

/// Compact JSON writing (W1.1; `model/json-numbers` N3, N8).
pub(crate) mod write {
    use std::fmt::Write as _;

    /// A JSON string with only the escapes JSON requires (N8).
    pub(crate) fn string(out: &mut String, text: &str) {
        meguri_model::json_text::push_string(out, text);
    }

    /// An integer in decimal (N3).
    pub(crate) fn int(out: &mut String, value: impl Into<i128>) {
        let _ = write!(out, "{}", value.into());
    }

    pub(crate) fn boolean(out: &mut String, value: bool) {
        out.push_str(if value { "true" } else { "false" });
    }

    /// `[a,b,c]` of integers.
    pub(crate) fn ints<T: Copy + Into<i128>>(out: &mut String, values: &[T]) {
        out.push('[');
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            int(out, *value);
        }
        out.push(']');
    }
}
