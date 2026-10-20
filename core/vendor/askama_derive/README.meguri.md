# Vendored Askama derive parser repair

This is third-party upstream `askama_derive` 0.16.1 production source, not
Meguri-authored code. Do not restyle or refactor it.

The integrator supplied the exact published artifact:

- Archive: <https://static.crates.io/crates/askama_derive/askama_derive-0.16.1.crate>
- SHA-256: `071ee5ebf2138e3ad180e0aacf6940c2cab5e6d8333741d9925c7bee2b153f39`
  (the original Cargo.lock registry checksum).
- Upstream VCS: `9ae4d5bdf4352b35e34106e38944c90d7a7ede3e`, path
  `askama_derive`, as recorded in `.cargo_vcs_info.json`.
- Original archive: 41 entries, 502,959 bytes, no links. The seed preserves
  40 exact regular UTF-8 files; its sole omission is the irrelevant nested
  `askama_derive-0.16.1/Cargo.lock`. Keep that lockfile omitted.

Both complete upstream licenses, `LICENSE-MIT` and `LICENSE-APACHE`, remain
unchanged under `MIT OR Apache-2.0`. Licensing and Meguri's cargo-deny policy
are unchanged. `Cargo.toml.orig` is the unchanged upstream reference manifest
with workspace fields; the normalized `Cargo.toml` is the actual build manifest.

The patch removes unmaintained `basic-toml` 0.1.10 (RUSTSEC-2026-0374; no
patched release) from UniFFI 0.32.2 → Askama 0.16.1's config dependency chain.
Config remains enabled: UniFFI needs its six template directories and custom
syntaxes. The three production edits are:

1. `core/Cargo.toml` patches crates.io's `askama_derive` to this directory and
   excludes it from workspace membership, preserving the existing member globs.
2. This package's normalized manifest changes config's `dep:basic-toml` to
   `dep:toml` and replaces that optional dependency with `toml = "=1.1.8"`,
   default features disabled, features `std`, `parse`, `serde`. All other
   metadata and features remain upstream.
3. `src/config.rs` changes the sole production call from
   `basic_toml::from_str` to `toml::from_str`, preserving borrowed deserialization
   and the Display-to-CompileError mapping.

The only additional source delta is `test_invalid_toml_error_mapping` in the
existing config unit tests. Existing directory and custom syntax tests are
preserved. This README is the only added file. The integrator owns root lockfile
resolution and build, generation, config-test and cargo-deny verification.

The XCFramework/bindgen fingerprint in
`tools/scripts/build-core-xcframework.sh::compute_hash` already recursively
includes tracked and non-ignored untracked files under `core/`, including this
vendor directory. Bindgen generation shares that fingerprint; no tooling repair
is needed.

Remove this vendored package and the workspace patch/exclusion once an upstream
Askama release replaces the parser, after validating the upstream dependency
chain through Meguri's normal integration gates.
