//! Tests FIRST for PublicationRig's fixed-name media mutation, independently
//! runnable without the incoming/staging API. Root applies this before wiring.
use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
use meguri_testkit::{
    cloud::{Scope, coordinator::PublicationRig, devices::IdentityReads},
    generate::recipe::{Recipe, generate_recipe},
};
use std::sync::Arc;

// This target never publishes. A media write must not draw cloud keys.
struct NoKeys;
impl KeyByteSource for NoKeys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        panic!("local media writes must not draw publication keys")
    }
}

#[test]
fn driver_put_media_replaces_the_requested_name_and_survives_a_process_kill() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("device");
    let recipe = Recipe::parse(
        "recipe: exact-media\ndescription: Public synthetic media input\nnow: \"2025-01-01T00:00:00Z\"\ndecks: []\n",
    )
    .unwrap();
    generate_recipe(&recipe, 821, &directory).unwrap();
    let scope = Scope {
        container: "test.meguri.exact-media".into(),
        environment: "development".into(),
        account: "synthetic-account".into(),
    };
    let open = || {
        PublicationRig::open(
            &directory,
            &scope,
            IdentityReads::Available(scope.account.clone()),
            false,
            1_735_689_600_000,
            823,
            Arc::new(NoKeys),
        )
        .unwrap()
    };
    let mut rig = open();
    assert_eq!(
        rig.driver().put_media("photo.jpg", b"initial image").unwrap(),
        "photo.jpg"
    );
    assert_eq!(
        rig.driver().put_media("photo.jpg", b"replacement image").unwrap(),
        "photo.jpg"
    );
    let files = rig.driver().media_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files["photo.jpg"], b"replacement image");
    assert_eq!(rig.kill().unwrap().signal, Some(9)); // SIGKILL on the Unix child transport.
    rig = open();
    let files = rig.driver().media_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files["photo.jpg"], b"replacement image");
    assert!(rig.facts().unwrap().current.is_none());
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
    rig.close_clean().unwrap();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
