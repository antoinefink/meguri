//! What decoding a `downloadFiles` zip's `_meta` holds in memory (specs/sync/
//! errors.md E6, R1852, R1562): `_meta` is the one entry read into memory and
//! parsed, so it has a limit of its own, far below what a file may expand
//! to. Its own test binary, so the counting allocator sees only this test.

use std::io::{Cursor, Write};

use meguri_sync::wire;
use meguri_sync::{ErrorKind, Method};
use meguri_testkit::bench::peak_alloc::PeakAlloc;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const MIB: usize = 1024 * 1024;
/// One `_meta` member with an empty name and an empty value, and its comma.
const MEMBER: &[u8] = b"\"\":\"\",";
/// About 10 million members: about 60 MB of text, well under the 100 MiB a
/// file may expand to.
const MEMBERS: usize = 10_000_000;
/// What decoding may hold at once: far below the `_meta` the zip carries.
const BOUND: usize = MIB;

/// Bytes deflate cannot shrink (xorshift).
fn incompressible(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// A download zip of a stored MiB that `_meta` maps to `padding.bin`, and a
/// deflated `_meta` padded with `members` empty members first: deflate
/// shrinks them about a thousand times, so the stored MiB keeps the zip's
/// expansion under 100 times its size. Also the length of `_meta`.
fn padded_meta_zip(members: usize) -> (Vec<u8>, usize) {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("0", stored).unwrap();
    zip.write_all(&incompressible(MIB)).unwrap();
    zip.start_file("_meta", deflated).unwrap();
    // Written a MiB at a time, so the test never holds the whole text.
    let piece = MEMBER.repeat(MIB / MEMBER.len());
    let tail = br#""0":"padding.bin"}"#;
    zip.write_all(b"{").unwrap();
    let mut left = members * MEMBER.len();
    while left > 0 {
        let size = left.min(piece.len());
        zip.write_all(&piece[..size]).unwrap();
        left -= size;
    }
    zip.write_all(tail).unwrap();
    let meta_len = 1 + members * MEMBER.len() + tail.len();
    (zip.finish().unwrap().into_inner(), meta_len)
}

/// Rewrites the expanded size `_meta` declares, in its central directory
/// record and its local header, to `declared`.
fn understate_meta(zip: &mut [u8], declared: u32) {
    let u16_at = |zip: &[u8], at: usize| usize::from(u16::from_le_bytes([zip[at], zip[at + 1]]));
    let u32_at = |zip: &[u8], at: usize| u32::from_le_bytes(zip[at..at + 4].try_into().unwrap()) as usize;
    let end = zip.windows(4).rposition(|window| window == b"PK\x05\x06").unwrap();
    let mut record = u32_at(zip, end + 16);
    for _ in 0..u16_at(zip, end + 10) {
        assert_eq!(&zip[record..record + 4], b"PK\x01\x02");
        let name_len = u16_at(zip, record + 28);
        let record_len = 46 + name_len + u16_at(zip, record + 30) + u16_at(zip, record + 32);
        if &zip[record + 46..record + 46 + name_len] == b"_meta" {
            zip[record + 24..record + 28].copy_from_slice(&declared.to_le_bytes());
            let local = u32_at(zip, record + 42);
            assert_eq!(&zip[local..local + 4], b"PK\x03\x04");
            zip[local + 22..local + 26].copy_from_slice(&declared.to_le_bytes());
            return;
        }
        record += record_len;
    }
    panic!("the zip has no _meta record");
}

#[test]
fn a_padded_meta_is_refused_before_it_fills_memory() {
    assert!(PeakAlloc::is_installed());
    // The zip's shape decodes when `_meta` is small: a thousand empty
    // members, the last value of the repeated empty name kept.
    let staging = tempfile::tempdir().unwrap();
    let files = wire::decode_download_zip(&padded_meta_zip(1000).0, staging.path()).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "padding.bin");
    drop(files);

    // The reviewed case: about 10 million members, which parsed would hold
    // at least 40 bytes each, past R1852's 300 MB.
    let (body, meta_len) = padded_meta_zip(MEMBERS);
    assert!(meta_len < 100 * MIB, "{meta_len}");
    assert!(MIB + meta_len < 100 * body.len(), "{}", body.len());

    for (case, body) in [
        ("declared", body.clone()),
        ("understated", {
            // The same `_meta` declaring 16 bytes: the bytes it expands to
            // are what counts.
            let mut body = body;
            understate_meta(&mut body, 16);
            body
        }),
    ] {
        let staging = tempfile::tempdir().unwrap();
        PeakAlloc::reset_peak();
        let before = PeakAlloc::current();
        let result = wire::decode_download_zip(&body, staging.path());
        let held = PeakAlloc::peak() - before;

        assert!(held < BOUND, "{case}: decoding held {held} bytes at once");
        let err = result.map(|files| files.len()).expect_err(case);
        assert_eq!(err.kind, ErrorKind::BadResponse, "{case}: {err}");
        assert_eq!(err.method, Some(Method::DownloadFiles), "{case}: {err}");
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0, "{case}");
    }
}
