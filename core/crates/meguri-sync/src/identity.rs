//! Meguri's client identity (specs/sync/transport.md T3; R1509, R2030): one
//! string, `meguri,<version> (<build>),<platform>`, sent as the header
//! member `c` of every request, the `cv` member of `meta` and the `v` member
//! of media `begin`. It never names another client.

use std::sync::OnceLock;

/// The platform part, from the target the core was compiled for: iPhone and
/// iPad alike are `ios`; tests run as `linux`.
pub const PLATFORM: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "ios") {
    "ios"
} else {
    "linux"
};

/// `meguri,<version> (<build>),<platform>`, for example
/// `meguri,1.0.0 (3f2a9c1d),macos`.
pub fn client_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(|| format!("meguri,{} ({}),{PLATFORM}", version(), build()))
}

/// The core's workspace version, keeping only `0-9 A-Z a-z . -`.
fn version() -> String {
    env!("CARGO_PKG_VERSION")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        .collect()
}

/// The first 8 lowercase hex digits of the commit the core was built from,
/// which the build passes as `MEGURI_BUILD_COMMIT`; `dev` without one.
fn build() -> &'static str {
    match option_env!("MEGURI_BUILD_COMMIT") {
        Some(commit)
            if commit.len() >= 8
                && commit.as_bytes()[..8]
                    .iter()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)) =>
        {
            &commit[..8]
        }
        _ => "dev",
    }
}
