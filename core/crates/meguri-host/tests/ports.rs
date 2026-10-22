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
        if binding.port == ServicePort::Backup {
            assert!(binding.roles.is_empty(), "backup is an explicit synchronous service");
        } else {
            assert!(!binding.roles.is_empty());
        }
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

fn check_pending(manifest: &serde_json::Value, target: &str, owners: &[&str]) -> Result<(), String> {
    let milestones = manifest["milestones"].as_array().ok_or("missing milestone order")?;
    let target = milestones
        .iter()
        .position(|m| m["id"].as_str() == Some(target))
        .ok_or("unknown milestone")?;
    let mut blocked = Vec::new();
    for owner in owners {
        let position = milestones.iter().position(|m| {
            m["units"]
                .as_array()
                .is_some_and(|units| units.iter().any(|u| u.as_str() == Some(owner)))
        });
        match position {
            None => return Err(format!("unknown provider owner {owner}")),
            Some(position) if position <= target => blocked.push(*owner),
            _ => {}
        }
    }
    if blocked.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "milestone still has unavailable providers: {}",
            blocked.join(", ")
        ))
    }
}

#[test]
fn pending_owners_follow_manifest_order_and_unknown_owners_fail() {
    let manifest = serde_json::json!({"milestones": [
        {"id":"Setup", "units":["U26"]},
        {"id":"Reviews", "units":["U127"]},
        {"id":"Daily", "units":["U95","U100"]}
    ]});
    assert!(check_pending(&manifest, "Reviews", &["U95", "U100"]).is_ok());
    assert!(check_pending(&manifest, "Reviews", &["U127"]).is_err());
    assert!(check_pending(&manifest, "Reviews", &["U26"]).is_err());
    assert!(check_pending(&manifest, "Reviews", &["Unknown"]).is_err());
    assert!(check_pending(&manifest, "Daily", &["U95", "U100"]).is_err());
    assert!(check_pending(&manifest, "Daily", &[]).is_ok());
    assert!(check_pending(&manifest, "Unknown", &[]).is_err());
}

/// Expected pending providers fail only when evaluating their milestone.
#[test]
#[ignore = "tools/mg u38-wiring supplies the milestone and validated manifest"]
fn milestone_provider_owners() {
    let path = std::env::var("MEGURI_WIRING_MANIFEST").expect("missing manifest path");
    let target = std::env::var("MEGURI_WIRING_TARGET").expect("missing milestone");
    let manifest = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let owners: Vec<_> = wiring::pending::PENDING
        .iter()
        .map(|(_, provider)| provider.provider_unit)
        .collect();
    check_pending(&manifest, &target, &owners).unwrap();
}
