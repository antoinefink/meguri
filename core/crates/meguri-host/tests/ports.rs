use meguri_engine::{
    Ports,
    ports::{BackupService, ServicePort, Unavailable},
};
use meguri_host::wiring;

#[test]
fn every_semantic_binding_has_one_provider_and_pending_features_refuse() {
    wiring::validate().unwrap();
    assert_eq!(
        wiring::PROVIDED.len() + wiring::pending::PENDING.len(),
        ServicePort::ALL.len()
    );
    for (port, _) in wiring::pending::PENDING {
        assert!(wiring::require(port).is_err());
    }
    for binding in wiring::PROVIDED {
        wiring::require(binding.port).unwrap();
        assert!(!binding.roles.is_empty());
    }
    assert!(wiring::validate_ports(&Ports::new()).is_err());
}

#[test]
fn an_unavailable_safety_backup_never_acknowledges_a_backup() {
    let unavailable = Unavailable {
        feature: "safety backups",
        provider_unit: "U127",
    };
    assert!(unavailable.before_deck_delete().is_err());
}
