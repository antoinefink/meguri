//! Independently authored archives exercise the actual publication verifier.
use super::*;
use std::io::Write;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    encoder.set_pledged_src_size(Some(bytes.len() as u64)).unwrap();
    encoder.include_contentsize(true).unwrap();
    encoder.include_checksum(true).unwrap();
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
fn expected(bytes: &[u8]) -> SnapshotDigest {
    SnapshotDigest {
        length: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}
fn archive(names: [&str; 4], meta: &[u8], collection: &[u8], media: &[u8], method: CompressionMethod) -> Vec<u8> {
    let mut placeholder = [0; 100];
    placeholder[..16].copy_from_slice(b"SQLite format 3\0");
    placeholder[18..20].copy_from_slice(&[1, 1]);
    archive_entries(names, [meta, collection, &placeholder, media], method)
}
fn archive_entries(names: [&str; 4], payloads: [&[u8]; 4], method: CompressionMethod) -> Vec<u8> {
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in names.into_iter().zip(payloads) {
        zip.start_file(name, SimpleFileOptions::default().compression_method(method))
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn check(bytes: &[u8], expected: &SnapshotDigest) -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.colpkg");
    std::fs::write(&path, bytes).unwrap();
    let result = verify(&path, expected, &|| false);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    result
}
fn package(meta: &[u8], collection: &[u8], media: &[u8]) -> Vec<u8> {
    archive(NAMES, meta, collection, media, CompressionMethod::Stored)
}

#[test]
fn reads_stored_deflated_and_concatenated_frames_and_checks_digest_and_length() {
    let mut collection = frame(b"first");
    // RFC 8878 skippable frame, one arbitrary payload byte.
    collection.extend_from_slice(&[0x50, 0x2a, 0x4d, 0x18, 1, 0, 0, 0, 0xaa]);
    collection.extend(frame(b"second"));
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let bytes = archive(NAMES, &[8, 3], &collection, &frame(b""), method);
        check(&bytes, &expected(b"firstsecond")).unwrap();
        assert!(matches!(check(&bytes, &expected(b"first")), Err(Error::Corrupt { .. })));
        assert!(matches!(
            check(&bytes, &expected(b"firstseconD")),
            Err(Error::Corrupt { .. })
        ));
    }
}

#[test]
fn meta_wire_rules_and_versions_are_checked_through_real_archives() {
    for (meta, error) in [
        (vec![8, 4], Error::TooNew),
        (vec![], Error::TooNew),
        (vec![8, 1], Error::UnsupportedFormat),
        (vec![8, 2], Error::UnsupportedFormat),
    ] {
        assert_eq!(
            check(&package(&meta, &frame(b"x"), &frame(b"")), &expected(b"x")),
            Err(error)
        );
    }
    for meta in [
        vec![0xff],
        vec![0x0a, 1, 3],
        vec![0],
        vec![0x3b],
        vec![0x3c],
        vec![0x3b, 0x44],
        vec![0x08; 65_537],
    ] {
        assert!(matches!(
            check(&package(&meta, &frame(b"x"), &frame(b"")), &expected(b"x")),
            Err(Error::Corrupt { .. })
        ));
    }
    for meta in [
        vec![8, 0x83, 0x80, 0x80, 0x80, 0x10],
        vec![8, 4, 8, 3],
        vec![0x3b, 8, 4, 0x3c, 8, 3],
    ] {
        check(&package(&meta, &frame(b"x"), &frame(b"")), &expected(b"x")).unwrap();
    }
}

#[test]
fn refuses_invalid_checksums_trailing_data_truncated_frames_and_large_windows() {
    let good = frame(b"collection bytes");
    let mut bad_checksum = good.clone();
    *bad_checksum.last_mut().unwrap() ^= 1;
    let mut trailing = good.clone();
    trailing.extend_from_slice(b"not another frame");
    let window_256_mib = vec![0x28, 0xb5, 0x2f, 0xfd, 0, 0x90, 1, 0, 0];
    for broken in [
        vec![],
        b"not zstd".to_vec(),
        good[..good.len() - 1].to_vec(),
        bad_checksum,
        trailing,
        window_256_mib,
    ] {
        assert!(matches!(
            check(&package(&[8, 3], &broken, &frame(b"")), &expected(b"collection bytes")),
            Err(Error::Corrupt { .. })
        ));
    }
    assert!(matches!(
        check(
            &package(&[8, 3], &good, &frame(b"\x0a\x00")),
            &expected(b"collection bytes")
        ),
        Err(Error::Corrupt { .. })
    ));
}

#[test]
fn stops_high_ratio_collection_at_first_byte_past_sixteen_mib() {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    let chunk = [0; 64 * 1024];
    for _ in 0..257 {
        encoder.write_all(&chunk).unwrap();
    }
    let bomb = encoder.finish().unwrap();
    assert!(bomb.len() < 1024);
    let error = check(&package(&[8, 3], &bomb, &frame(b"")), &expected(b"")).unwrap_err();
    assert!(matches!(error, Error::Corrupt { detail } if detail == "decoded byte limit"));
}

#[test]
fn archive_directory_validation_covers_entry_metadata_errors() {
    let good = package(&[8, 3], &frame(b"x"), &frame(b""));
    let mut huge_count = good.clone();
    let end = huge_count.len() - 22;
    huge_count[end + 8..end + 12].copy_from_slice(&[0xff; 4]);
    assert!(matches!(
        check(&huge_count, &expected(b"x")),
        Err(Error::Corrupt { .. })
    ));
    let mut understated = good.clone();
    understated[end + 8..end + 10].copy_from_slice(&3_u16.to_le_bytes());
    understated[end + 10..end + 12].copy_from_slice(&3_u16.to_le_bytes());
    assert!(matches!(
        check(&understated, &expected(b"x")),
        Err(Error::Corrupt { .. })
    ));
    let mut duplicate = good.clone();
    let central = duplicate.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    duplicate[central + 28..central + 30].copy_from_slice(&4_u16.to_le_bytes());
    duplicate[central + 46..central + 50].copy_from_slice(b"meta");
    duplicate.remove(central + 50);
    let end = duplicate.len() - 22;
    let size = u32_at(&duplicate, end + 12) - 1;
    duplicate[end + 12..end + 16].copy_from_slice(&size.to_le_bytes());
    assert!(matches!(check(&duplicate, &expected(b"x")), Err(Error::Corrupt { .. })));
    assert!(matches!(
        check(b"not a ZIP archive", &expected(b"")),
        Err(Error::Corrupt { .. })
    ));
}

#[test]
fn crc_mismatch_and_placeholder_header_damage_are_refused() {
    let good = package(&[8, 3], &frame(b"x"), &frame(b""));
    let mut bad_crc = good.clone();
    // First local header has a four-byte name and no extra field.
    let payload = 30 + usize::from(u16_at(&bad_crc, 26)) + usize::from(u16_at(&bad_crc, 28));
    bad_crc[payload + 1] = 4;
    assert!(matches!(check(&bad_crc, &expected(b"x")), Err(Error::Corrupt { .. })));
    let damaged = archive_entries(
        NAMES,
        [&[8, 3], &frame(b"x"), &[0; 100], &frame(b"")],
        CompressionMethod::Stored,
    );
    assert!(
        matches!(check(&damaged, &expected(b"x")), Err(Error::Corrupt { detail }) if detail == "placeholder SQLite header")
    );
}

#[test]
fn reads_zip64_end_records_and_prefix_without_unbounded_metadata() {
    let good = package(&[8, 3], &frame(b"x"), &frame(b""));
    let end = good.len() - 22;
    let mut zip64 = good[..end].to_vec();
    zip64.extend_from_slice(b"PK\x06\x06");
    zip64.extend_from_slice(&44_u64.to_le_bytes());
    zip64.extend_from_slice(&[45, 0, 45, 0]);
    zip64.extend_from_slice(&[0; 8]);
    zip64.extend_from_slice(&4_u64.to_le_bytes());
    zip64.extend_from_slice(&4_u64.to_le_bytes());
    zip64.extend_from_slice(&u64::from(u32_at(&good, end + 12)).to_le_bytes());
    zip64.extend_from_slice(&u64::from(u32_at(&good, end + 16)).to_le_bytes());
    zip64.extend_from_slice(b"PK\x06\x07");
    zip64.extend_from_slice(&0_u32.to_le_bytes());
    zip64.extend_from_slice(&(end as u64).to_le_bytes());
    zip64.extend_from_slice(&1_u32.to_le_bytes());
    zip64.extend_from_slice(&good[end..]);
    let new_end = zip64.len() - 22;
    zip64[new_end + 8..new_end + 20].fill(0xff);
    check(&zip64, &expected(b"x")).unwrap();
    let mut prefix = b"ignored prefix".to_vec();
    prefix.extend_from_slice(&good);
    check(&prefix, &expected(b"x")).unwrap();
    // ZIP64 declares far too many records; refuse before the ZIP index.
    zip64[end + 24..end + 32].copy_from_slice(&400_001_u64.to_le_bytes());
    zip64[end + 32..end + 40].copy_from_slice(&400_001_u64.to_le_bytes());
    assert!(
        matches!(check(&zip64, &expected(b"x")), Err(Error::Corrupt { detail }) if detail == "central directory entry limit")
    );
}

#[test]
fn optional_zip64_counts_with_ordinary_classic_fields_are_bounded_before_indexing() {
    let good = package(&[8, 3], &frame(b"x"), &frame(b""));
    let end = good.len() - 22;
    let mut bytes = good[..end].to_vec();
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&44_u64.to_le_bytes());
    bytes.extend_from_slice(&[45, 0, 45, 0]);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&400_001_u64.to_le_bytes());
    bytes.extend_from_slice(&400_001_u64.to_le_bytes());
    bytes.extend_from_slice(&u64::from(u32_at(&good, end + 12)).to_le_bytes());
    bytes.extend_from_slice(&u64::from(u32_at(&good, end + 16)).to_le_bytes());
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&(end as u64).to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&good[end..]); // ordinary classic count stays four
    assert!(
        matches!(check(&bytes, &expected(b"x")), Err(Error::Corrupt { detail }) if detail == "central directory entry limit")
    );
}
