//! What decoding a `downloadFiles` zip holds in memory (specs/sync/
//! errors.md E6, R1852): its files go to the staging directory as they
//! expand, so the decoder's heap does not grow with them. Its own test
//! binary, so the counting allocator sees only this test.

use std::io::{Cursor, Write};

use meguri_sync::wire;
use meguri_testkit::bench::peak_alloc::PeakAlloc;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const MIB: usize = 1024 * 1024;
/// The deflated files of the zip, each of zeros.
const FILES: usize = 4;
const FILE_BYTES: usize = 16 * MIB;
/// What decoding may hold at once: far below one file.
const BOUND: usize = 2 * MIB;

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

#[test]
fn a_download_zip_expands_into_files_not_into_memory() {
    assert!(PeakAlloc::is_installed());
    // A stored entry of 1 MiB pads the body so that the files, 64 MiB of
    // zeros deflated, stay under the expansion ratio (about 60 times).
    let padding = incompressible(MIB);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("0", stored).unwrap();
    zip.write_all(&padding).unwrap();
    let piece = vec![0u8; MIB];
    let mut meta = String::from(r#"{"0":"padding.bin""#);
    for index in 1..=FILES {
        zip.start_file(index.to_string(), deflated).unwrap();
        for _ in 0..FILE_BYTES / MIB {
            zip.write_all(&piece).unwrap();
        }
        meta.push_str(&format!(r#","{index}":"{index}.wav""#));
    }
    meta.push('}');
    zip.start_file("_meta", stored).unwrap();
    zip.write_all(meta.as_bytes()).unwrap();
    let body = zip.finish().unwrap().into_inner();
    drop(piece);
    let expanded = MIB + FILES * FILE_BYTES;
    assert!(expanded < 100 * body.len(), "{}", body.len());

    let staging = tempfile::tempdir().unwrap();
    PeakAlloc::reset_peak();
    let before = PeakAlloc::current();
    let files = wire::decode_download_zip(&body, staging.path()).unwrap();
    let held = PeakAlloc::peak() - before;

    assert_eq!(files.len(), FILES + 1);
    assert_eq!(files[0].name, "padding.bin");
    assert!(std::fs::read(files[0].file.path()).unwrap() == padding);
    for (index, file) in files.iter().enumerate().skip(1) {
        assert_eq!(file.name, format!("{index}.wav"));
        assert_eq!(file.file.size(), FILE_BYTES as u64);
        let on_disk = std::fs::metadata(file.file.path()).unwrap().len();
        assert_eq!(on_disk, FILE_BYTES as u64);
    }
    assert!(held < BOUND, "decoding held {held} bytes at once");
}
