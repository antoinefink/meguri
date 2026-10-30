use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
use meguri_testkit::{
    cloud::{Config, Field, Scope, Service, coordinator::PublicationRig, devices::IdentityReads},
    generate::recipe::{Recipe, generate_recipe},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Keys(AtomicUsize);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        match self.0.fetch_add(1, Ordering::SeqCst) {
            0 => Ok([31; 32]),
            1 => Ok([124; 32]),
            _ => Err(KeySourceError::Unavailable),
        }
    }
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

#[test]
fn export_initial_app_adoption_replies_from_real_publication() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let recipe = Recipe::parse("recipe: initial-app-adoption\ndescription: Public native startup fixture\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Initial Cloud Study\n    notetype: basic\n    new: 2\n    fields: [\"public front {n}\", \"public back {n}\"]\n").unwrap();
    generate_recipe(&recipe, 801, &source).unwrap();
    let scope = Scope {
        container: "synthetic-initial-app".into(),
        environment: "development".into(),
        account: "synthetic-initial-account".into(),
    };
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut sender = PublicationRig::open(
        &source,
        &scope,
        IdentityReads::Available(scope.account.clone()),
        false,
        1_735_689_600_000,
        809,
        Arc::new(Keys(AtomicUsize::new(0))),
    )
    .unwrap();
    let result = (|| -> meguri_testkit::cloud::devices::Result<_> {
        let preview = sender.preview(&mut service, 10_000)?;
        sender.confirm_publication(&preview)?;
        sender.drive_ready(&mut service, 10_000)?;
        let rows = sender.driver().collection_rows()?;
        let inventory = service.inspect(&scope)?.unwrap();
        let mut records = Vec::new();
        for (zone, state) in inventory.zones {
            for (name, record) in state.records {
                let encrypted = match record.fields.get("e") {
                    Some(Field::EncryptedBytes(bytes)) => bytes,
                    _ => return Err(std::io::Error::other("expected production encrypted record").into()),
                };
                records.push(serde_json::json!({
                    "zone": zone, "name": name, "recordType": record.record_type,
                    "encrypted": encrypted, "systemFields": record.system_fields.bytes(),
                }));
            }
        }
        assert!(records.iter().any(|r| r["recordType"] == "MGActive"));
        assert!(records.iter().any(|r| r["recordType"] == "MGEpoch"));
        assert!(records.iter().any(|r| r["recordType"] == "MGMediaRoot"));
        assert!(records.iter().any(|r| r["recordType"] == "MGEntity"));
        Ok(serde_json::json!({ "records": records, "sourceRows": rows }))
    })();
    // Join the actual child even if export/inspection fails.
    sender.close_clean().unwrap();
    let fixture = result.unwrap();
    if let Some(destination) = std::env::var_os("MEGURI_INITIAL_ADOPTION_FIXTURE") {
        std::fs::write(destination, serde_json::to_vec(&fixture).unwrap()).unwrap();
    }
}
