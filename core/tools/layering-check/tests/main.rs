//! This crate's integration tests in one binary (`[[test]] layering_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

/// The crate's `layering-check` binary. nextest sets `CARGO_BIN_EXE_layering-check` at run
/// time to where the binary is, which follows a build reused from an archive
/// (`tools/mg core-test --archive`); `env!` (the path at build time) serves
/// plain `cargo test`.
pub fn bin_exe() -> std::path::PathBuf {
    std::env::var_os("CARGO_BIN_EXE_layering-check").map_or_else(|| env!("CARGO_BIN_EXE_layering-check").into(), Into::into)
}

#[path = "layering.rs"]
pub mod layering;
#[path = "pins.rs"]
pub mod pins;
