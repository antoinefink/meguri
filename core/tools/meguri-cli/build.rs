//! Names the Cargo profile this build uses (`MEGURI_CLI_PROFILE`) for
//! `timing run`'s report (timed-operations.md T7.1 `meguri.profile`).
//! Cargo tells a build script only whether the profile inherits from `dev`
//! or `release`, so the name comes from `OUT_DIR`, which is
//! `<target>/[<triple>/]<profile dir>/build/<package>-<hash>/out`; the dev
//! profile's directory is `debug`.

use std::path::Path;

fn main() {
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    let dir = Path::new(&out)
        .ancestors()
        .nth(3)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let profile = if dir == "debug" { "dev" } else { dir };
    println!("cargo:rustc-env=MEGURI_CLI_PROFILE={profile}");
    println!("cargo:rerun-if-changed=build.rs");
}
