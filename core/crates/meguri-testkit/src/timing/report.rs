//! The report (T7.1): `conformance/schema/timing.schema.json`, side `meguri`,
//! in the goldens' text layout (dsl.md D11). It names no deck, tag, field,
//! note, file or directory (R1940).

use serde_json::{Map, Value, json};

use super::catalogue::Operation;
use super::image::Summary;
use super::protocol::Outcome;

/// T7.1 `meguri`: the build that ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeguriBuild {
    /// The 40-hex git commit Meguri was built from; `None` when the build
    /// has none.
    pub commit: Option<String>,
    /// The Cargo profile (`release` for a comparison, T3.5).
    pub profile: String,
}

/// One requested operation and its result (T7.2).
#[derive(Clone, Debug, PartialEq)]
pub struct OperationEntry {
    pub op: Operation,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// Where the side ran: `macos-arm64`, `linux-arm64` or `linux-x86_64`.
    pub platform: &'static str,
    pub build: MeguriBuild,
    /// T1 `--repeat`: timed runs per operation.
    pub repeat: u32,
    /// The frozen instant, epoch seconds.
    pub now: i64,
    pub tz: String,
    pub collection: Summary,
    /// In catalogue order.
    pub operations: Vec<OperationEntry>,
}

/// The platform this build runs on, as an oracle worker's hello names it;
/// `None` where the oracle has no lane.
pub fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("linux-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x86_64")
    } else {
        None
    }
}

impl Report {
    pub fn to_json(&self) -> Value {
        let operations = self
            .operations
            .iter()
            .map(|entry| {
                let mut out = Map::new();
                out.insert("name".into(), json!(entry.op.name()));
                out.extend(entry.outcome.to_json());
                Value::Object(out)
            })
            .collect::<Vec<_>>();
        json!({
            "report_version": 1,
            "side": "meguri",
            "platform": self.platform,
            "meguri": {"commit": self.build.commit, "profile": self.build.profile},
            "protocol": {"warmup": 1, "repeat": self.repeat},
            "now": self.now,
            "tz": self.tz,
            "collection": self.collection.to_json(),
            "operations": operations,
        })
    }

    /// `timing.json`'s text.
    pub fn text(&self) -> String {
        crate::observe::golden_text(&self.to_json())
    }
}
