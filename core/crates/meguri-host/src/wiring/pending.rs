//! Removed by the named provider units when their real wiring lands.
use meguri_engine::ports::{ServicePort, Unavailable};

pub const PENDING: [(ServicePort, Unavailable); 6] = [
    (
        ServicePort::Backup,
        Unavailable {
            feature: "safety backups",
            provider_unit: "U127",
        },
    ),
    (
        ServicePort::DeckCounts,
        Unavailable {
            feature: "deck counts",
            provider_unit: "U41",
        },
    ),
    (
        ServicePort::Review,
        Unavailable {
            feature: "review replay and flush",
            provider_unit: "U43",
        },
    ),
    (
        ServicePort::Sync,
        Unavailable {
            feature: "sync policy",
            provider_unit: "U113",
        },
    ),
    (
        ServicePort::Statistics,
        Unavailable {
            feature: "statistics observer",
            provider_unit: "U95",
        },
    ),
    (
        ServicePort::Widgets,
        Unavailable {
            feature: "widget snapshot",
            provider_unit: "U100",
        },
    ),
];
