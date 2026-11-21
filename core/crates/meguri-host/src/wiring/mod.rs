//! Construction and lifetime wiring only; business operations stay in providers.
mod media;
pub mod pending;

pub(super) use media::MediaPort;
use meguri_engine::{EngineError, PortKind, Ports, ports::ServicePort};

#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub port: ServicePort,
    pub name: &'static str,
    pub unit: &'static str,
    pub roles: &'static [PortKind],
}

pub const PROVIDED: [Binding; 7] = [
    Binding {
        port: ServicePort::Backup,
        name: "forced-safety-backup",
        unit: "U127",
        roles: &[], // Engine owns registration of the backup maintenance participant.
    },
    Binding {
        port: ServicePort::Review,
        name: "review-session",
        unit: "U43",
        roles: &PortKind::ALL,
    },
    Binding {
        port: ServicePort::DeckCounts,
        name: "deck-counts",
        unit: "U41",
        roles: &[PortKind::ChangeObserver, PortKind::GenerationListener],
    },
    Binding {
        port: ServicePort::NameIndex,
        name: "name-index",
        unit: "U88a",
        roles: &[PortKind::OpenStep, PortKind::ChangeObserver, PortKind::GenerationListener],
    },
    Binding {
        port: ServicePort::FilterCounts,
        name: "filter-counts",
        unit: "U263",
        roles: &[PortKind::OpenStep, PortKind::ChangeObserver, PortKind::GenerationListener],
    },
    Binding {
        port: ServicePort::DerivedStore,
        name: "derived-store",
        unit: "U26",
        roles: &PortKind::ALL,
    },
    Binding {
        port: ServicePort::Media,
        name: "media",
        unit: "U31",
        roles: &[PortKind::OpenStep, PortKind::SuspendParticipant],
    },
];

/// Each semantic binding appears exactly once, including unavailable providers.
pub fn validate() -> Result<(), EngineError> {
    let listed: Vec<_> = PROVIDED
        .iter()
        .map(|binding| binding.port)
        .chain(pending::PENDING.iter().map(|(port, _)| *port))
        .collect();
    if listed.len() != ServicePort::ALL.len()
        || ServicePort::ALL
            .iter()
            .any(|port| listed.iter().filter(|p| *p == port).count() != 1)
    {
        return Err(EngineError::Internal {
            message: "host service catalog has a missing or duplicate provider".into(),
        });
    }
    Ok(())
}

/// Validate actual trait registrations as well as the semantic catalog.
pub fn validate_ports(ports: &Ports) -> Result<(), EngineError> {
    validate()?;
    let actual = |role| -> Vec<&str> {
        match role {
            PortKind::ChangeObserver => ports.observers.iter().map(|p| p.name()).collect(),
            PortKind::GenerationListener => ports.generation_listeners.iter().map(|p| p.name()).collect(),
            PortKind::SuspendParticipant => ports.suspend_participants.iter().map(|p| p.name()).collect(),
            PortKind::OpenStep => ports.open_steps.iter().map(|p| p.name()).collect(),
        }
    };
    // The optional capture component does not fulfill ServicePort::Sync.
    let has_cloud = PortKind::ALL
        .iter()
        .any(|role| actual(*role).contains(&"cloud-note-capture"));
    for role in PortKind::ALL {
        let mut expected: Vec<_> = PROVIDED
            .iter()
            .filter(|b| b.roles.contains(&role))
            .map(|b| b.name)
            .collect();
        if has_cloud {
            expected.push("cloud-note-capture");
        }
        let mut found = actual(role);
        expected.sort_unstable();
        found.sort_unstable();
        if found != expected {
            return Err(EngineError::Internal {
                message: format!("host {role:?} registrations do not match the catalog"),
            });
        }
    }
    if ports.open_steps.first().map(|p| p.name()) != Some("derived-store") {
        return Err(EngineError::Internal {
            message: "derived-store must be the first open step".into(),
        });
    }
    Ok(())
}

pub fn require(port: ServicePort) -> Result<(), EngineError> {
    validate()?;
    match pending::PENDING.iter().find(|(p, _)| *p == port) {
        Some((_, unavailable)) => unavailable.require(),
        None => Ok(()),
    }
}
