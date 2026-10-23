//! RF4 wire component coverage. No journal acknowledgement or transfer policy.
mod common;
mod media_files_support;
use common::block_on;
use media_files_support::*;
use meguri_engine::CancelToken;
use meguri_sync::{
    ErrorKind, Method, Payload, ServerKind,
    wire::{self, MediaUploadInput},
};
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
};

fn decode(bytes: &[u8], requested: &[&str], staging: &Path) -> Result<Vec<wire::MediaFile>, meguri_sync::JobError> {
    let input = tempfile::tempdir().unwrap();
    block_on(wire::decode_media_file(
        disk_zip(input.path(), bytes),
        staging.into(),
        requested.iter().map(|s| (*s).into()).collect(),
        CancelToken::new(),
    ))
}
fn refused(bytes: &[u8], requested: &[&str]) {
    let staging = tempfile::tempdir().unwrap();
    let err = decode(bytes, requested, staging.path()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::BadResponse, "{}", err.detail);
    assert_eq!(count(staging.path()), 0);
}

#[test]
fn file_decoder_matches_generic_and_never_uses_remote_paths() {
    let bytes = zip(&[
        ("../../entry", b"moon", true),
        (
            "_meta",
            br#"{"../../entry":"../../remote","../../entry":"../final"}"#,
            false,
        ),
    ]);
    let staging = tempfile::tempdir().unwrap();
    let generic = wire::decode_download_zip(&bytes, staging.path()).unwrap();
    let files = decode(&bytes, &["../final"], staging.path()).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, generic[0].name);
    assert_eq!(std::fs::read(files[0].file.path()).unwrap(), b"moon");
    assert_eq!(files[0].file.path().parent(), Some(staging.path()));
    drop((generic, files));
    assert_eq!(count(staging.path()), 0);
}

#[test]
fn zero_file_after_valid_member_rejects_both_decoders_and_leaves_no_staged_files() {
    for deflated in [false, true] {
        let bytes = zip(&[
            ("0", b"valid", deflated),
            ("1", b"", deflated),
            ("_meta", br#"{"0":"a","1":"b"}"#, false),
        ]);
        let staging = tempfile::tempdir().unwrap();
        for result in [
            wire::decode_download_zip(&bytes, staging.path()),
            decode(&bytes, &["a", "b"], staging.path()),
        ] {
            let err = result.unwrap_err();
            assert_eq!(err.kind, ErrorKind::BadResponse);
            assert_eq!(err.method, Some(Method::DownloadFiles));
            assert_eq!(count(staging.path()), 0);
        }
        // Invalid wire data must fail before a staging I/O failure can mask it.
        let missing = staging.path().join("missing");
        assert_eq!(
            wire::decode_download_zip(&bytes, &missing).unwrap_err().kind,
            ErrorKind::BadResponse
        );
        assert_eq!(
            decode(&bytes, &["a", "b"], &missing).unwrap_err().kind,
            ErrorKind::BadResponse
        );

        let mut bytes = bytes;
        let record = central_records(&bytes)[1];
        let local = local_offset(&bytes, record);
        bytes[record + 24..record + 28].copy_from_slice(&1u32.to_le_bytes());
        bytes[local + 22..local + 26].copy_from_slice(&1u32.to_le_bytes());
        for result in [
            wire::decode_download_zip(&bytes, staging.path()),
            decode(&bytes, &["a", "b"], staging.path()),
        ] {
            let err = result.unwrap_err();
            assert_eq!(err.kind, ErrorKind::BadResponse);
            assert_eq!(err.method, Some(Method::DownloadFiles));
            assert_eq!(count(staging.path()), 0);
        }
    }
}

#[test]
fn request_membership_is_production_only() {
    let repeated = zip(&[
        ("0", b"a", false),
        ("1", b"b", false),
        ("_meta", br#"{"0":"same","1":"same"}"#, false),
    ]);
    let staging = tempfile::tempdir().unwrap();
    assert_eq!(wire::decode_download_zip(&repeated, staging.path()).unwrap().len(), 2);
    refused(&repeated, &["same", "other"]);
    refused(
        &zip(&[("0", b"a", false), ("_meta", br#"{"0":"unknown"}"#, false)]),
        &["asked"],
    );
    refused(&zip(&[("_meta", b"{}", false)]), &["asked"]);
    assert!(
        decode(&zip(&[("_meta", b"{}", false)]), &[], staging.path())
            .unwrap()
            .is_empty()
    );
}

fn central_records(bytes: &[u8]) -> Vec<usize> {
    let end = bytes.windows(4).rposition(|b| b == b"PK\x05\x06").unwrap();
    let mut position = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[end + 10..end + 12].try_into().unwrap());
    let mut records = Vec::new();
    for _ in 0..count {
        records.push(position);
        let length = |o| u16::from_le_bytes(bytes[position + o..position + o + 2].try_into().unwrap()) as usize;
        position += 46 + length(28) + length(30) + length(32);
    }
    records
}
fn local_offset(bytes: &[u8], record: usize) -> usize {
    u32::from_le_bytes(bytes[record + 42..record + 46].try_into().unwrap()) as usize
}

#[test]
fn raw_duplicate_members_are_refused_before_indexmap_can_hide_them() {
    let mut bytes = zip(&[
        ("0", b"a", false),
        ("1", b"b", false),
        ("_meta", br#"{"0":"asked"}"#, false),
    ]);
    let records = central_records(&bytes);
    let local = local_offset(&bytes, records[1]);
    bytes[records[1] + 46] = b'0';
    bytes[local + 30] = b'0';
    refused(&bytes, &["asked", "second"]);
}

#[test]
fn crc_failure_after_a_good_entry_and_truncations_leave_no_staged_handles() {
    let mut bytes = zip(&[
        ("0", b"first", false),
        ("1", b"bad CRC", false),
        ("_meta", br#"{"0":"a","1":"b"}"#, false),
    ]);
    let position = bytes.windows(7).position(|b| b == b"bad CRC").unwrap();
    bytes[position] ^= 1;
    refused(&bytes, &["a", "b"]);
    for len in [0, 3, bytes.len() / 2, bytes.len() - 1] {
        refused(&bytes[..len], &["a", "b"]);
    }
}

#[test]
fn directory_counts_and_extents_are_bounded_before_zip_construction() {
    let original = zip(&[("0", b"a", false), ("_meta", br#"{"0":"a"}"#, false)]);
    let end = original.windows(4).rposition(|b| b == b"PK\x05\x06").unwrap();
    for field in [8, 10] {
        let mut bytes = original.clone();
        bytes[end + field..end + field + 2].copy_from_slice(&65_534u16.to_le_bytes());
        refused(&bytes, &["a"]);
    }
    let mut bytes = original;
    bytes[end + 12..end + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    refused(&bytes, &["a"]);
}

#[test]
fn declared_and_actual_file_and_meta_bounds_apply_to_file_decoder() {
    let mut bytes = zip(&[("0", b"a", false), ("_meta", br#"{"0":"a"}"#, false)]);
    let records = central_records(&bytes);
    bytes[records[0] + 24..records[0] + 28].copy_from_slice(&104_857_601u32.to_le_bytes());
    refused(&bytes, &["a"]);
    let meta = format!("{{{}\"0\":\"a\"}}", "\"\":\"\",".repeat(12_000));
    let mut bytes = zip(&[("0", b"a", false), ("_meta", meta.as_bytes(), true)]);
    refused(&bytes, &["a"]);
    let records = central_records(&bytes);
    let record = records[1];
    let local = local_offset(&bytes, record);
    bytes[record + 24..record + 28].copy_from_slice(&1u32.to_le_bytes());
    bytes[local + 22..local + 26].copy_from_slice(&1u32.to_le_bytes());
    refused(&bytes, &["a"]);
    let zeros = vec![0; 2 * 1024 * 1024];
    refused(&zip(&[("0", &zeros, true), ("_meta", br#"{"0":"a"}"#, false)]), &["a"]);
}

#[test]
fn disk_encoder_matches_generic_batch_positions_and_captured_facts() {
    let dir = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    // Deliberately advance a caller-opened handle: the source is the whole file.
    let mut file = source(dir.path(), "a", b"moon");
    file.seek(SeekFrom::Start(2)).unwrap();
    let artifact = block_on(wire::prepare_media_upload(
        vec![
            MediaUploadInput::Remove {
                name: "removed".into(),
                stamp: 11,
            },
            MediaUploadInput::Put {
                name: "a".into(),
                file,
                stamp: 13,
                expected: content(b"old"),
            },
            MediaUploadInput::Put {
                name: "b".into(),
                file: source(dir.path(), "b", b"sea"),
                stamp: 14,
                expected: content(b"sea"),
            },
        ],
        stage.path().into(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(artifact.records()[1].stamp, 13);
    assert_eq!(artifact.records()[1].encoded.as_ref(), Some(&content(b"moon")));
    assert!(!artifact.records()[1].matches_capture());
    assert!(artifact.records()[2].matches_capture());
    let (payload, receipt) = artifact.stream_payload(CancelToken::new());
    let length = artifact.byte_len();
    let expected = wire::encode_upload_zip(&[
        wire::UploadChange::Delete { name: "removed" },
        wire::UploadChange::Add {
            name: "a",
            data: b"moon",
        },
        wire::UploadChange::Add {
            name: "b",
            data: b"sea",
        },
    ])
    .unwrap();
    drop(artifact);
    assert_eq!(count(stage.path()), 1, "payload retains its ZIP owner");
    block_on(async {
        let Payload::Stream { mut source, total } = payload else {
            panic!("expected stream")
        };
        assert_eq!(total, Some(length));
        let mut sent = Vec::new(); // This test's small format comparison only.
        while let Some(bytes) = next(&mut source).await {
            sent.extend_from_slice(&bytes.unwrap());
        }
        receipt.drain().await.unwrap();
        assert_eq!(sent, expected);
        drop(source);
    });
    assert_eq!(count(stage.path()), 0);
}

#[test]
fn upload_fill_accepts_a_large_final_file_and_stops_before_the_next_change() {
    let dir = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let bytes = vec![1; 3 * 1024 * 1024];
    let add = || MediaUploadInput::Put {
        name: "large".into(),
        file: source(dir.path(), "large", &bytes),
        stamp: 1,
        expected: content(&bytes),
    };
    let artifact = block_on(wire::prepare_media_upload(
        vec![add()],
        stage.path().into(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(
        artifact.records()[0].encoded.as_ref().unwrap().length,
        bytes.len() as u64
    );
    drop(artifact);
    let result = block_on(wire::prepare_media_upload(
        vec![
            add(),
            MediaUploadInput::Remove {
                name: "later".into(),
                stamp: 2,
            },
        ],
        stage.path().into(),
        CancelToken::new(),
    ));
    assert!(matches!(result, Err(ref e) if e.kind == ErrorKind::LocalStorage));
    assert_eq!(count(stage.path()), 0);
}

#[test]
fn ineligible_unreadable_and_cancelled_encoding_leave_no_zip() {
    let dir = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let mut write_only = File::create(dir.path().join("write-only")).unwrap();
    write_only.write_all(b"a").unwrap();
    for file in [
        source(dir.path(), "empty", b""),
        write_only,
        File::open(dir.path()).unwrap(),
    ] {
        let result = block_on(wire::prepare_media_upload(
            vec![MediaUploadInput::Put {
                name: "a".into(),
                file,
                stamp: 1,
                expected: content(b"a"),
            }],
            stage.path().into(),
            CancelToken::new(),
        ));
        assert!(matches!(result, Err(ref e) if e.kind == ErrorKind::LocalStorage));
        assert_eq!(count(stage.path()), 0);
    }
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = block_on(wire::prepare_media_upload(vec![], stage.path().into(), cancel));
    assert!(matches!(result, Err(ref e) if e.kind == ErrorKind::Cancelled));
}

#[test]
fn changed_or_truncated_artifact_cannot_be_a_verified_send() {
    for truncate in [false, true] {
        let input = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        let artifact = block_on(wire::prepare_media_upload(
            vec![MediaUploadInput::Put {
                name: "a".into(),
                file: source(input.path(), "a", &[5; 128 * 1024]),
                stamp: 2,
                expected: content(&[5; 128 * 1024]),
            }],
            stage.path().into(),
            CancelToken::new(),
        ))
        .unwrap();
        let path = std::fs::read_dir(stage.path()).unwrap().next().unwrap().unwrap().path();
        let (payload, receipt) = artifact.stream_payload(CancelToken::new());
        drop(artifact);
        block_on(async {
            let Payload::Stream { mut source, .. } = payload else {
                panic!()
            };
            assert!(next(&mut source).await.unwrap().is_ok());
            let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            if truncate {
                file.set_len(64 * 1024).unwrap();
            } else {
                file.seek(SeekFrom::Start(80 * 1024)).unwrap();
                file.write_all(b"changed").unwrap();
            }
            let mut failed = false;
            while let Some(piece) = next(&mut source).await {
                failed |= piece.is_err();
            }
            assert!(failed);
            assert_eq!(receipt.drain().await.unwrap_err().kind, ErrorKind::LocalStorage);
            drop(source);
        });
        assert_eq!(count(stage.path()), 0);
    }
}

#[test]
fn cancelled_or_abandoned_send_drains_before_cleanup() {
    let stage = tempfile::tempdir().unwrap();
    let artifact = block_on(wire::prepare_media_upload(
        vec![],
        stage.path().into(),
        CancelToken::new(),
    ))
    .unwrap();
    let cancel = CancelToken::new();
    let (payload, receipt) = artifact.stream_payload(cancel.clone());
    drop(artifact);
    cancel.cancel();
    block_on(async {
        let Payload::Stream { mut source, .. } = payload else {
            panic!()
        };
        assert!(next(&mut source).await.unwrap().is_err());
        assert_eq!(receipt.drain().await.unwrap_err().kind, ErrorKind::Cancelled);
        drop(source);
    });
    assert_eq!(count(stage.path()), 0);
    let artifact = block_on(wire::prepare_media_upload(
        vec![],
        stage.path().into(),
        CancelToken::new(),
    ))
    .unwrap();
    let (payload, receipt) = artifact.stream_payload(CancelToken::new());
    drop(artifact);
    drop(payload);
    assert_eq!(block_on(receipt.drain()).unwrap_err().kind, ErrorKind::Interrupted);
    assert_eq!(count(stage.path()), 0);
}

#[test]
fn receive_consumes_framing_eof_and_preserves_cancel_network_and_local_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut zip = File::create(dir.path().join("zip")).unwrap();
    let mut writer = zip::ZipWriter::new(&mut zip);
    writer
        .start_file("0", zip::write::SimpleFileOptions::default())
        .unwrap();
    noise(&mut writer, 512 * 1024);
    writer
        .start_file("_meta", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(br#"{"0":"a"}"#).unwrap();
    writer.finish().unwrap();
    drop(zip);
    let mut zip = File::open(dir.path().join("zip")).unwrap();
    for mode in 0..9 {
        let stage = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        let mut transport = FileTransport::new(&mut zip, dir.path());
        if mode == 1 {
            transport.fail_after = Some(1);
        }
        if mode == 2 {
            transport.cancel_after = Some((1, cancel.clone()));
        }
        if mode == 3 {
            transport.declared += 1;
        }
        if mode == 6 {
            transport.declared -= 1;
        }
        if mode == 7 {
            transport.fail_after = Some(transport.compressed.metadata().unwrap().len().div_ceil(64 * 1024) as usize);
        }
        if mode == 8 {
            let writer = std::fs::OpenOptions::new()
                .write(true)
                .open(dir.path().join("response.zst"))
                .unwrap();
            writer.set_len(writer.metadata().unwrap().len() - 1).unwrap();
        }
        let mut job = transport.job();
        let staging = if mode == 5 {
            stage.path().join("missing")
        } else {
            stage.path().into()
        };
        let result = block_on(async {
            let body = job.request(Method::DownloadFiles, Payload::empty()).await.unwrap();
            wire::spool_media_reply(
                body,
                staging,
                vec!["a".into()],
                if mode == 4 { 1 } else { 1024 * 1024 },
                ServerKind::SelfHosted,
                cancel,
            )
            .await
        });
        match mode {
            0 => {
                let files = result.unwrap();
                assert_eq!(files[0].file.size(), 512 * 1024);
                drop(files);
            }
            1 | 7 => assert!(matches!(result, Err(ref e) if matches!(e.kind, ErrorKind::Network { .. }))),
            2 => assert!(matches!(result, Err(ref e) if e.kind == ErrorKind::Cancelled)),
            3 | 4 | 6 | 8 => assert!(matches!(result, Err(ref e) if e.kind == ErrorKind::BadResponse)),
            5 => assert!(matches!(result, Err(ref e) if e.kind == ErrorKind::LocalStorage)),
            _ => unreachable!(),
        }
        assert_eq!(count(stage.path()), 0);
    }
}

#[test]
fn meta_exact_bound_is_accepted_and_limit_plus_one_is_refused_on_disk() {
    let mut meta = br#"{"0":"a"}"#.to_vec();
    meta.resize(64 * 1024, b' ');
    let staging = tempfile::tempdir().unwrap();
    let bytes = zip(&[("0", b"a", false), ("_meta", &meta, false)]);
    assert_eq!(decode(&bytes, &["a"], staging.path()).unwrap().len(), 1);
    meta.push(b' ');
    refused(&zip(&[("0", b"a", false), ("_meta", &meta, false)]), &["a"]);
}

#[test]
fn source_receipt_requires_verified_eof_even_when_transport_returns_success() {
    let input = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let path = input.path().join("json");
    std::fs::write(&path, b"{}").unwrap();
    for drain in [true, false] {
        let artifact = block_on(wire::prepare_media_upload(
            vec![],
            stage.path().into(),
            CancelToken::new(),
        ))
        .unwrap();
        let mut response = File::open(&path).unwrap();
        let mut transport = FileTransport::new(&mut response, input.path());
        transport.drain_upload = drain;
        let mut job = transport.job();
        let (payload, receipt) = artifact.stream_payload(CancelToken::new());
        drop(artifact);
        block_on(async {
            let mut answer = job.request(Method::UploadChanges, payload).await.unwrap();
            while answer.chunk().await.unwrap().is_some() {}
            let result = receipt.drain().await;
            if drain {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().kind, ErrorKind::Interrupted);
            }
        });
        assert_eq!(count(stage.path()), 0);
    }
}

#[test]
fn network_failure_and_mid_send_cancel_release_artifact_only_after_source_drain() {
    let input = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let path = input.path().join("json");
    std::fs::write(&path, b"{}").unwrap();
    let data = [7; 256 * 1024];
    for network in [true, false] {
        let artifact = block_on(wire::prepare_media_upload(
            vec![MediaUploadInput::Put {
                name: "a".into(),
                file: source(input.path(), "a", &data),
                stamp: 2,
                expected: content(&data),
            }],
            stage.path().into(),
            CancelToken::new(),
        ))
        .unwrap();
        let cancel = CancelToken::new();
        let (payload, receipt) = artifact.stream_payload(cancel.clone());
        drop(artifact);
        if network {
            let mut response = File::open(&path).unwrap();
            let mut transport = FileTransport::new(&mut response, input.path());
            transport.drain_upload = true;
            transport.fail_upload_after = Some(1);
            let mut job = transport.job();
            block_on(async {
                assert!(job.request(Method::UploadChanges, payload).await.is_err());
                assert_eq!(receipt.drain().await.unwrap_err().kind, ErrorKind::Interrupted);
            });
        } else {
            block_on(async {
                let Payload::Stream { mut source, .. } = payload else {
                    panic!()
                };
                assert!(next(&mut source).await.unwrap().is_ok());
                cancel.cancel();
                assert!(next(&mut source).await.unwrap().is_err());
                assert_eq!(receipt.drain().await.unwrap_err().kind, ErrorKind::Cancelled);
                drop(source);
            });
        }
        assert_eq!(count(stage.path()), 0);
    }
}

#[test]
fn optional_zip64_extensions_and_prefixes_stay_bounded_and_keep_normal_crc_reads() {
    let original = zip(&[("0", b"bytes", false), ("_meta", br#"{"0":"a"}"#, false)]);
    let end = original.windows(4).rposition(|b| b == b"PK\x05\x06").unwrap();
    let central_size = u32::from_le_bytes(original[end + 12..end + 16].try_into().unwrap());
    let central_offset = u32::from_le_bytes(original[end + 16..end + 20].try_into().unwrap());
    let mut bytes = original[..end].to_vec();
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&(44 + 128 * 1024u64).to_le_bytes());
    bytes.extend_from_slice(&[45, 0, 45, 0]);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&2u64.to_le_bytes());
    bytes.extend_from_slice(&2u64.to_le_bytes());
    bytes.extend_from_slice(&u64::from(central_size).to_le_bytes());
    bytes.extend_from_slice(&u64::from(central_offset).to_le_bytes());
    bytes.extend_from_slice(&vec![0; 128 * 1024]);
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(end as u64).to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&original[end..]);
    let staging = tempfile::tempdir().unwrap();
    for prefix in [false, true] {
        let mut input = if prefix { b"archive prefix".to_vec() } else { Vec::new() };
        input.extend_from_slice(&bytes);
        let files = decode(&input, &["a"], staging.path()).unwrap();
        assert_eq!(std::fs::read(files[0].file.path()).unwrap(), b"bytes");
        drop(files);
    }
    let end64 = bytes.len() - 22;
    bytes[end64 + 8..end64 + 12].fill(0xff);
    bytes[end + 24..end + 40].fill(0xff);
    refused(&bytes, &["a"]);
}
