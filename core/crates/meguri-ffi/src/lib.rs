//! UniFFI exports and the core event dispatcher. The only crate built for
//! Apple targets and the only one that knows UniFFI (KTD2, KTD9).
//!
//! Every export is a short blocking call that returns `Result<_, CoreError>`
//! and starts with `callback::enter`. Long work runs as a job (`jobs`). The
//! exported surface is the same on every slice of one build configuration:
//! platform features gate implementations, never exports.

uniffi::setup_scaffolding!("meguri_ffi");

mod callback;
pub mod cancel;
pub mod changes;
pub mod day_clock;
pub mod error;
pub mod events;
pub mod jobs;
pub mod progress;
mod runtime;
#[cfg(feature = "test-hooks")]
mod test_hooks;

pub use cancel::CancelToken;
pub use changes::{CardChange, ChangeKind, ChangeSet, IdChange, NameChange, NotetypeChange, OpSource};
pub use day_clock::DayClockReport;
pub use error::CoreError;
pub use events::{CoreEvent, CoreEventKind, CoreEventListener};
pub use jobs::{JobHandle, JobOutcome};
pub use progress::{JobProgress, ProgressListener, ProgressUpdate};

/// What this core build is: its version, target and compiled feature set.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CoreBuildInfo {
    /// The Rust workspace version (`core/Cargo.toml`).
    pub version: String,
    /// The Rust target triple, for example `aarch64-apple-ios-sim`.
    pub target: String,
    pub debug_assertions: bool,
    /// Enabled build features, sorted: `ankiweb`, `test-hooks`.
    pub features: Vec<String>,
}

#[uniffi::export]
pub fn core_version() -> Result<String, CoreError> {
    callback::enter("core_version");
    Ok(env!("CARGO_PKG_VERSION").to_owned())
}

#[uniffi::export]
pub fn core_build_info() -> Result<CoreBuildInfo, CoreError> {
    callback::enter("core_build_info");
    Ok(build_info())
}

fn build_info() -> CoreBuildInfo {
    let mut features = Vec::new();
    if cfg!(feature = "ankiweb") {
        features.push("ankiweb".to_owned());
    }
    if cfg!(feature = "test-hooks") {
        features.push("test-hooks".to_owned());
    }
    CoreBuildInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: target_name(),
        debug_assertions: cfg!(debug_assertions),
        features,
    }
}

/// The Rust target triple on Apple slices (`aarch64-apple-darwin`,
/// `aarch64-apple-ios`, `aarch64-apple-ios-sim`); `<arch>-<os>` elsewhere.
fn target_name() -> String {
    let os = match std::env::consts::OS {
        "macos" => "apple-darwin",
        "ios" => "apple-ios",
        other => other,
    };
    let abi = if cfg!(target_abi = "sim") { "-sim" } else { "" };
    format!("{}-{os}{abi}", std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_workspace_version() {
        // Read independently of the crate: the [workspace.package] version.
        let manifest = include_str!("../../../Cargo.toml");
        let workspace_version = manifest
            .split("[workspace.package]")
            .nth(1)
            .and_then(|section| section.lines().find_map(|line| line.strip_prefix("version = ")))
            .map(|v| v.trim().trim_matches('"'))
            .expect("core/Cargo.toml has a [workspace.package] version");
        assert_eq!(core_version().unwrap(), workspace_version);
        assert_eq!(core_build_info().unwrap().version, workspace_version);
    }

    #[test]
    fn build_info_lists_exactly_the_compiled_features() {
        let info = core_build_info().unwrap();
        assert_eq!(info.features.contains(&"ankiweb".to_owned()), cfg!(feature = "ankiweb"));
        assert_eq!(
            info.features.contains(&"test-hooks".to_owned()),
            cfg!(feature = "test-hooks")
        );
        assert!(info.features.windows(2).all(|w| w[0] < w[1]), "{:?}", info.features);
        assert_eq!(info.debug_assertions, cfg!(debug_assertions));
    }
}
