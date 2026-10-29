//! Tests FIRST: M6 fixed-name writes through the admitted engine media service.
//! Root must extract and observe this target before the service/driver changes.
mod common;

use common::Harness;
use meguri_engine::{EngineError, media::MediaService};
use meguri_storage::media::MediaFileState;
use std::fs;

#[test]
fn fixed_name_replacement_keeps_the_name_and_records_the_new_file_facts() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    assert_eq!(media.write_exact("photo.jpg", b"initial image").unwrap(), "photo.jpg");
    let before = media.store().media_feed_status().unwrap();
    assert_eq!(
        media.write_exact("photo.jpg", b"replacement image").unwrap(),
        "photo.jpg"
    );
    assert_eq!(
        fs::read(media.store().media_dir().join("photo.jpg")).unwrap(),
        b"replacement image"
    );
    assert_eq!(media.tracked_names().unwrap().names.len(), 1);
    let feed = media.store().media_feed_snapshot().unwrap();
    assert_eq!(feed.status.feed_id, before.feed_id);
    assert!(feed.status.counter > before.counter);
    let item = feed.items.iter().rev().find(|item| item.name == "photo.jpg").unwrap();
    let MediaFileState::Present(facts) = &item.state else {
        panic!("replacement must record actual live file facts");
    };
    assert_eq!(facts.sha1, "8f5c982155b0fe4738fb5c214b5efbe30e3190cd");
    assert_eq!(facts.size, b"replacement image".len() as u64);
    let trash = media.store().trash_entries().unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].name, "photo.jpg");
    assert_eq!(trash[0].sha1, "5cdc8b23b93324c3cf373826c1e90fdd9a9dcc4c");
    // Restore uses the real trash payload, not a metadata-only witness.
    media.store().trash(&["photo.jpg".into()]).unwrap();
    media.store().restore().unwrap();
    let contents = media
        .tracked_names()
        .unwrap()
        .names
        .into_iter()
        .map(|entry| fs::read(media.store().media_dir().join(entry.name)).unwrap())
        .collect::<Vec<_>>();
    assert!(contents.iter().any(|bytes| bytes == b"initial image"));
    assert!(contents.iter().any(|bytes| bytes == b"replacement image"));
}

#[test]
fn repeating_identical_fixed_name_bytes_does_not_change_the_journal_or_trash() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    media.write_exact("photo.jpg", b"same image").unwrap();
    let before = media.store().media_feed_snapshot().unwrap();
    for _ in 0..100 {
        assert_eq!(media.write_exact("photo.jpg", b"same image").unwrap(), "photo.jpg");
    }
    assert_eq!(media.store().media_feed_snapshot().unwrap(), before);
    assert_eq!(media.store().trash_summary().unwrap().entries, 0);
}

#[test]
fn fixed_name_publish_failure_restores_old_bytes_and_does_not_publish_new_facts() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    media.write_exact("photo.jpg", b"old image").unwrap();
    let before = media.store().media_feed_snapshot().unwrap();
    meguri_model::faultpoint::arm(
        "media.install.before_publish",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = media.write_exact("photo.jpg", b"new image");
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(result, Err(EngineError::Media(_))));
    assert_eq!(
        fs::read(media.store().media_dir().join("photo.jpg")).unwrap(),
        b"old image"
    );
    assert_eq!(media.store().media_feed_snapshot().unwrap(), before);
    assert_eq!(media.store().trash_summary().unwrap().entries, 0);
}

#[test]
fn fixed_name_write_refuses_a_retained_service_from_an_older_generation() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    media.write_exact("photo.jpg", b"original image").unwrap();
    h.engine.close().unwrap();
    assert!(media.write_exact("photo.jpg", b"stale write").is_err());
    assert_eq!(
        fs::read(media.store().media_dir().join("photo.jpg")).unwrap(),
        b"original image"
    );
}

#[derive(Default)]
struct FeedReader(std::sync::Mutex<Vec<meguri_engine::Generation>>);
impl meguri_engine::media::MediaFeedConsumer for FeedReader {
    fn committed(
        &self,
        collection: &meguri_engine::Collection,
        media: &MediaService,
        generation: meguri_engine::Generation,
    ) {
        media.check_generation(generation.0).unwrap();
        let _reader = collection.reader(meguri_storage::ReaderRole::Query).unwrap();
        assert_eq!(generation, collection.generation());
        self.0.lock().unwrap().push(generation);
    }
}
#[test]
fn media_write_callbacks_can_acquire_the_query_reader_after_publication() {
    let h = Harness::open();
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let consumer = std::sync::Arc::new(FeedReader::default());
    let hook: std::sync::Arc<dyn meguri_engine::media::MediaFeedConsumer> = consumer.clone();
    media.set_feed_consumer(std::sync::Arc::downgrade(&hook));
    media.add("photo.jpg", b"first file").unwrap();
    media.write_exact("photo.jpg", b"second file").unwrap();
    assert_eq!(*consumer.0.lock().unwrap(), vec![h.collection.generation(); 2]);
    assert_eq!(
        fs::read(media.store().media_dir().join("photo.jpg")).unwrap(),
        b"second file"
    );
}
