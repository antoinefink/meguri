//! Every sync answer decoder on arbitrary bytes (specs/sync/wire-format.md
//! W18, errors.md E6): each input either decodes or fails as `bad_response`
//! (a media `err` as `server_message`; an upload refusal as
//! `server_message`) with the method set, and never panics, aborts,
//! overflows the stack or allocates beyond the input's proportion (R1562).
//! The download zip's files go to the temporary directory and are removed
//! with their handles at the end of each input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_engine::{CancelToken, ThreadRegistry};
use meguri_sync::wire;
use meguri_sync::{ErrorKind, JobError, Method};
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::OnceLock;

// At most 6.25 MiB of expanded file data per raw ZIP, plus its 64 KiB
// input. The structured candidate is smaller. Decode and drop them in order.
const MAX_ZIP_INPUT: usize = 64 * 1024;

fn production_zip(bytes: &[u8], staging: &Path, names: &[&str]) {
    static THREADS: OnceLock<ThreadRegistry> = OnceLock::new();
    let mut input = tempfile::NamedTempFile::new().unwrap();
    input.write_all(bytes).unwrap();
    let result = meguri_sync::transport::runtime::block_on(
        THREADS.get_or_init(ThreadRegistry::new),
        &CancelToken::default(),
        async {
            Ok(wire::decode_media_file(
                input.reopen().unwrap(),
                staging.into(),
                names.iter().map(|name| (*name).into()).collect(),
                CancelToken::new(),
            )
            .await)
        },
    )
    .unwrap();
    checked(Method::DownloadFiles, result);
    assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
}

// Writer-built fixtures take mutations past signature checks into the real
// directory preflight and decoder. This only constructs/mutates candidates;
// no harness parser decides whether they are valid.
fn zip_seed(mode: usize) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in ["0", "1", "_meta"] {
        writer.start_file(name, options).unwrap();
        writer
            .write_all(match name {
                "_meta" if mode == 3 => br#"{"0":"a","1":"a"}"#,
                "_meta" => br#"{"0":"a","1":"b"}"#,
                _ => b"file bytes",
            })
            .unwrap();
    }
    let mut bytes = writer.finish().unwrap().into_inner();
    if mode == 2 {
        // Both known one-byte member names must agree in local and central
        // records; make the second duplicate the first before indexing.
        let locations: Vec<_> = bytes
            .windows(4)
            .enumerate()
            .filter_map(|(i, signature)| match signature {
                b"PK\x03\x04" => Some(i + 30),
                b"PK\x01\x02" => Some(i + 46),
                _ => None,
            })
            .collect();
        for i in locations {
            if bytes[i] == b'1' {
                bytes[i] = b'0';
            }
        }
    }
    if mode % 2 == 1 {
        // These fields come from the writer's fixed-size, comment-free footer.
        let end = bytes.len() - 22;
        let size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
        let offset = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap());
        let footer = bytes.split_off(end);
        bytes.extend_from_slice(b"PK\x06\x06");
        bytes.extend_from_slice(&44_u64.to_le_bytes());
        bytes.extend_from_slice(&[45, 0, 45, 0]);
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&3_u64.to_le_bytes());
        bytes.extend_from_slice(&3_u64.to_le_bytes());
        bytes.extend_from_slice(&u64::from(size).to_le_bytes());
        bytes.extend_from_slice(&u64::from(offset).to_le_bytes());
        bytes.extend_from_slice(b"PK\x06\x07");
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&(end as u64).to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&footer);
    }
    bytes
}

fn download_zips(data: &[u8]) {
    static SEEDS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    let staging = tempfile::tempdir().unwrap();
    let bounded = &data[..data.len().min(MAX_ZIP_INPUT)];
    checked(
        Method::DownloadFiles,
        wire::decode_download_zip(bounded, staging.path()),
    );
    assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
    production_zip(bounded, staging.path(), &["a", "b"]);

    let seeds = SEEDS.get_or_init(|| (0..4).map(zip_seed).collect());
    let mode = data.first().copied().unwrap_or(0);
    let mut candidate = seeds[usize::from(mode % 4)].clone();
    // Bounded byte mutations include central/local extents, ZIP64 records,
    // duplicate members and _meta. All names asked for are synthetic constants.
    for change in data.get(1..).unwrap_or_default().chunks_exact(3).take(64) {
        let offset = usize::from(u16::from_le_bytes([change[0], change[1]])) % candidate.len();
        candidate[offset] ^= change[2];
    }
    let names: &[&str] = if mode & 4 == 0 {
        &["a", "b"]
    } else {
        &["a", "other"]
    };
    production_zip(&candidate, staging.path(), names);
}

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
    download_zips(data);
});
