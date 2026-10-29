//! Public synthetic input only. No baseline, tracking or acknowledgement seeding.
#![allow(dead_code)]

use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    entity_codec::{self as codec, Payload},
    runtime::{KeyByteSource, KeySourceError},
};
use meguri_testkit::{
    cloud::{Config, Scope, Service, coordinator::PublicationRig, devices::IdentityReads},
    generate::recipe::{Recipe, generate_recipe},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub const NOW: i64 = 1_735_689_600_000;
pub const STEPS: usize = 10_000;
pub const LOCAL: &[u8] = b"synthetic local created image";
pub const REMOTE: &[u8] = b"synthetic remote created image";

// Independent 32-byte platform inputs. Relaunch is deliberately entropy-free.
struct Keys {
    calls: AtomicUsize,
    available: bool,
}
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.available || call >= 2 {
            return Err(KeySourceError::Unavailable);
        }
        Ok(std::array::from_fn(|index| {
            (index as u8).wrapping_mul(17).wrapping_add(31 + 93 * call as u8)
        }))
    }
}
pub fn generated(root: &Path, name: &str) -> PathBuf {
    let directory = root.join(name);
    let recipe = Recipe::parse("recipe: incoming-publication\ndescription: Public synthetic incoming tests\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Incoming\n    notetype: basic\n    new: 2\n    due: 0\n    tags: []\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, if name == "sender" { 801 } else { 802 }, &directory).unwrap();
    directory
}
pub fn scope() -> Scope {
    Scope {
        container: "test.meguri.incoming-publication".into(),
        environment: "development".into(),
        account: "synthetic-incoming-account".into(),
    }
}
pub fn open(directory: &Path, identity: IdentityReads, fresh: bool) -> PublicationRig {
    PublicationRig::open(
        directory,
        &scope(),
        identity,
        false,
        NOW,
        809,
        Arc::new(Keys {
            calls: AtomicUsize::new(0),
            available: fresh,
        }),
    )
    .unwrap()
}
pub fn online(directory: &Path, fresh: bool) -> PublicationRig {
    open(directory, IdentityReads::Available(scope().account), fresh)
}
pub fn publish(rig: &mut PublicationRig, service: &mut Service<SeededRandom>) {
    let preview = rig.preview(service, STEPS).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(service, STEPS).unwrap();
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
}
pub fn notes(rig: &mut PublicationRig) -> Vec<codec::Note> {
    rig.driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .filter_map(|row| {
            if let Payload::Note(note) = row {
                Some(note)
            } else {
                None
            }
        })
        .collect()
}
pub fn hash_name() -> String {
    use sha1::{Digest, Sha1};
    format!("photo-{:x}.jpg", Sha1::digest(LOCAL))
}
pub struct Pair {
    pub service: Service<SeededRandom>,
    pub sender: PublicationRig,
    pub receiver: PublicationRig,
    pub receiver_dir: PathBuf,
    pub root: tempfile::TempDir,
}
impl Pair {
    pub fn published() -> Self {
        let root = tempfile::tempdir().unwrap();
        let sender_dir = generated(root.path(), "sender");
        let receiver_dir = generated(root.path(), "receiver");
        let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
        let mut sender = online(&sender_dir, true);
        publish(&mut sender, &mut service);
        let receiver = online(&receiver_dir, true);
        Self {
            service,
            sender,
            receiver,
            receiver_dir,
            root,
        }
    }
    pub fn adopted() -> Self {
        let mut pair = Self::published();
        let rows = pair.sender.driver().collection_rows().unwrap();
        let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
        pair.receiver.adopt_cloud(&preview).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        pair
    }
    pub fn reopen_before_identity(&mut self) {
        self.receiver = open(&self.receiver_dir, IdentityReads::FailAllSession, false);
    }
    pub fn verify_and_drive(&mut self) {
        self.receiver.verify_identity(scope().account).unwrap();
        self.receiver.drive_ready(&mut self.service, STEPS).unwrap();
    }
    pub fn close(&mut self) {
        self.receiver.close_clean().unwrap();
        self.sender.close_clean().unwrap();
    }
}
