//! Every sync answer decoder on arbitrary bytes (specs/sync/wire-format.md
//! W18, errors.md E6): each input either decodes or fails as `bad_response`
//! (a media `err` as `server_message`; an upload refusal as
//! `server_message`) with the method set, and never panics, aborts,
//! overflows the stack or allocates beyond the input's proportion (R1562).

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_sync::wire;
use meguri_sync::{ErrorKind, JobError, Method};

fn checked<T>(method: Method, result: Result<T, JobError>) {
    if let Err(err) = result {
        assert_eq!(err.method, Some(method), "{err}");
        assert!(
            matches!(err.kind, ErrorKind::BadResponse | ErrorKind::ServerMessage { .. }),
            "{err}"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    // A login answer that does not decode is the transport's protocol error.
    let _ = meguri_sync::decode_login_answer(data);
    checked(Method::Meta, wire::decode_meta(data));
    checked(Method::Start, wire::decode_graves(data));
    checked(Method::ApplyChanges, wire::decode_changes(data));
    checked(Method::Chunk, wire::decode_chunk(data));
    checked(Method::SanityCheck2, wire::decode_sanity_answer(data));
    checked(Method::Finish, wire::decode_finish(data));
    checked(Method::Upload, wire::decode_upload_answer(data));
    checked(Method::MediaBegin, wire::decode_begin(data));
    checked(Method::MediaChanges, wire::decode_media_changes(data));
    checked(Method::UploadChanges, wire::decode_upload_changes(data));
    checked(Method::MediaSanity, wire::decode_media_sanity(data));
    checked(Method::DownloadFiles, wire::decode_download_zip(data));
});
