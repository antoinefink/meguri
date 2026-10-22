//! Removed by the named provider units when their real wiring lands.
use meguri_engine::ports::{ServicePort, Unavailable};

pub const PENDING: [(ServicePort, Unavailable); 3] = [
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
