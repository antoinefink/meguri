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
    // Stable device streams survive reopen; explicit receiver reopens use 809 too.
    let seed = match directory.file_name().and_then(|name| name.to_str()) {
        Some("sender") => 808,
        Some("receiver") => 809,
        _ => panic!("expected synthetic sender or receiver fixture"),
    };
    open_with_seed(directory, identity, fresh, seed)
}
// A second publication after relaunch needs a distinct stream, while keeping
// the same platform key provider construction and entropy-free recovery path.
pub fn open_with_seed(directory: &Path, identity: IdentityReads, fresh: bool, seed: u64) -> PublicationRig {
    PublicationRig::open(
        directory,
        &scope(),
        identity,
        false,
        NOW,
        seed,
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
        assert_ne!(
            pair.sender.facts().unwrap().current.unwrap().id,
            pair.receiver.facts().unwrap().current.unwrap().id
        );
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

pub fn assert_replay_convergence(pair: &mut Pair, binding: meguri_sync::cloud::BindingId) {
    let expected = notes(&mut pair.receiver);
    for _ in 0..2 {
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    }
    assert_eq!(notes(&mut pair.receiver), expected);
    assert_eq!(notes(&mut pair.sender), expected);
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    assert!(pair.receiver.driver().inbox_entries(binding).unwrap().is_empty());
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files["photo.jpg"], REMOTE);
    assert_eq!(files[&hash_name()], LOCAL);
    assert_eq!(pair.sender.driver().media_files().unwrap(), files);
    assert_eq!(
        pair.receiver.driver().media_rows().unwrap(),
        pair.sender.driver().media_rows().unwrap()
    );

    let facts = pair.receiver.facts().unwrap();
    let key = facts.current.as_ref().unwrap().facts.record_key.0;
    let database = pair.service.inspect(&scope()).unwrap().unwrap();
    let zone = &database.zones[&pair.receiver.collection_zone().unwrap()];
    let mut rows: Vec<_> = expected.into_iter().map(Payload::Note).collect();
    rows.extend(pair.receiver.driver().media_rows().unwrap());
    for row in rows {
        let name = codec::entity_name(
            &key,
            row.kind(),
            &meguri_sync::cloud::row_payload::payload_key(&row).unwrap(),
        )
        .unwrap();
        assert_eq!(facts.entities[&name].payload, row);
        assert!(facts.entities[&name].tombstone_at.is_none());
    }
    // Match durable acknowledgements to real Service versions, including any
    // deleted note. A cleared dirty mark alone cannot satisfy this witness.
    for entity in facts
        .entities
        .values()
        .filter(|entity| matches!(entity.kind, codec::Kind::Note | codec::Kind::MediaName))
    {
        let record = &zone.records[&entity.name];
        let meguri_testkit::cloud::Field::EncryptedBytes(envelope) = &record.fields["e"] else {
            panic!("expected an actual encrypted Service entity")
        };
        let received = codec::receive(&key, &entity.name, envelope, codec::AssetInput::Absent).unwrap();
        assert_eq!(received.entity.fields.tombstone, entity.tombstone_at.is_some());
        if entity.tombstone_at.is_none() {
            assert_eq!(received.payload.as_ref(), Some(&entity.payload));
        }
        assert_eq!(entity.acked_envelope.0, *envelope);
        assert_eq!(entity.acked_system_fields, record.system_fields.bytes());
        assert_eq!(entity.server_erev, received.entity.fields.erev);
        assert!(entity.acked_rev > 0);
        assert_eq!(entity.local_rev, entity.acked_rev);
        assert!(entity.mark.is_none());
    }
    let moved_name = codec::entity_name(&key, codec::Kind::MediaName, hash_name().as_bytes()).unwrap();
    assert_eq!(
        facts
            .entities
            .values()
            .filter(|entity| matches!(&entity.payload,
                Payload::MediaName(media) if media.name == hash_name().as_bytes()))
            .count(),
        1
    );
    let mut moved_versions = 0;
    for change in &zone.changes {
        use meguri_testkit::cloud::{Field, RecordChange};
        match &change.change {
            RecordChange::Saved(record) if record.name == moved_name => {
                let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
                    panic!("expected an actual encrypted Service version")
                };
                let received = codec::receive(&key, &moved_name, envelope, codec::AssetInput::Absent).unwrap();
                assert!(!received.entity.fields.tombstone);
                moved_versions += 1;
            }
            RecordChange::Deleted { name, .. } => assert_ne!(name, &moved_name),
            _ => {}
        }
    }
    assert!(moved_versions > 0);
}
