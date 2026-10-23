//! One allocator measurement covers file receive, decode, encode and send.
//! Large fixtures stay in files; incoming chunks count toward the heap bound.

mod common;
mod media_files_support;

use std::fs::File;
use std::io::{Seek, Write};

use media_files_support::{FileTransport, count, noise};
use meguri_engine::CancelToken;
use meguri_sync::wire::{self, MediaUploadInput};
use meguri_sync::{Method, Payload, ServerKind};
use meguri_testkit::bench::peak_alloc::PeakAlloc;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;
const LENGTH: usize = 32 * 1024 * 1024;
const HEAP_BOUND: usize = 3 * 1024 * 1024;

#[test]
fn streamed_download_and_upload_keep_heap_bounded_including_received_bytes() {
    let input = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let uploads = tempfile::tempdir().unwrap();
    let path = input.path().join("large.zip");
    let mut file = File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(&mut file);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("0", stored).unwrap();
    let facts = noise(&mut zip, LENGTH);
    zip.start_file("_meta", stored).unwrap();
    zip.write_all(br#"{"0":"noise.bin"}"#).unwrap();
    zip.finish().unwrap();
    drop(file);
    let mut file = File::open(path).unwrap();
    let transport = FileTransport::new(&mut file, input.path());
    let admitted = file.metadata().unwrap().len();
    let mut job = transport.job();
    common::block_on(async {}); // Runtime startup is outside the interval.
    let warmup = common::block_on(wire::prepare_media_upload(
        vec![],
        uploads.path().into(),
        CancelToken::new(),
    ))
    .unwrap();
    drop(warmup);

    PeakAlloc::reset_peak();
    let before = PeakAlloc::current();
    let files = common::block_on(async {
        let body = job.request(Method::DownloadFiles, Payload::empty()).await.unwrap();
        wire::spool_media_reply(
            body,
            stage.path().into(),
            vec!["noise.bin".into()],
            admitted,
            ServerKind::SelfHosted,
            CancelToken::new(),
        )
        .await
        .unwrap()
    });
    let download_peak = PeakAlloc::peak().saturating_sub(before);
    assert_eq!(files[0].file.size(), LENGTH as u64);
    assert!(download_peak < HEAP_BOUND, "receive/spool/decode peak {download_peak}");
    assert_eq!(count(stage.path()), 1, "only decoded media remains, no spool");

    // The response is also generated on disk, and is deliberately small.
    let response_path = input.path().join("small-json");
    std::fs::write(&response_path, b"{}").unwrap();
    let mut response = File::open(response_path).unwrap();
    let mut transport = FileTransport::new(&mut response, input.path());
    transport.drain_upload = true;
    let mut job = transport.job();
    let source = File::open(files[0].file.path()).unwrap();
    PeakAlloc::reset_peak();
    let before = PeakAlloc::current();
    common::block_on(async {
        let zip = wire::prepare_media_upload(
            vec![MediaUploadInput::Put {
                name: "noise.bin".into(),
                file: source,
                stamp: 81,
                expected: facts.clone(),
            }],
            uploads.path().into(),
            CancelToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(zip.records()[0].encoded.as_ref(), Some(&facts));
        assert!(zip.byte_len() > LENGTH as u64);
        let (payload, receipt) = zip.stream_payload(CancelToken::new());
        drop(zip);
        let mut response = job.request(Method::UploadChanges, payload).await.unwrap();
        while response.chunk().await.unwrap().is_some() {}
        receipt.drain().await.unwrap();
    });
    let upload_peak = PeakAlloc::peak().saturating_sub(before);
    assert!(upload_peak < HEAP_BOUND, "encode/send/compress peak {upload_peak}");
    assert_eq!(count(uploads.path()), 0);
    drop(files);
    assert_eq!(count(stage.path()), 0);
    file.rewind().unwrap();
}
