#[path = "support/publication_incoming.rs"]
mod support;
use support::*;

#[test]
fn actual_blob_bytes_survive_adoption_reopen_and_reverse_direction() {
    let mut pair = Pair::published();
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    let result = pair.sender.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.sender.last_sync_phase()));
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    pair.receiver.driver().put_media("reverse.bin", LOCAL).unwrap();
    let result = pair.receiver.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.receiver.last_sync_phase()));
    let result = pair.sender.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.sender.last_sync_phase()));
    assert_eq!(pair.sender.driver().media_files().unwrap()["reverse.bin"], LOCAL);
    pair.close();
}
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
