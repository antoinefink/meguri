# core/

- Crate layering is `tools/layering-check/edges.toml`, the KTD2 table as data; change it only when the plan's table changes. `tools/mg core-lint` enforces it for every dependency kind. A new workspace crate needs a place there. tokio is allowed only in meguri-sync and meguri-servers, axum only in servers, reqwest only in sync.
- Only meguri-storage writes SQL against `collection.anki2`. Only meguri-ffi knows UniFFI. Only meguri-engine and meguri-host may name the engine's raw open entry point, `open_raw`; every other crate opens collections through `CoreHost::open`.
- Parity crates in `pins.toml` are `=`-pinned in `[workspace.dependencies]` at the oracle lockfile's versions and change only with an oracle re-pin. Implementers never choose their versions: while a pin is `pending`, no crate may depend on that crate. rand 0.10.1 is pinned only in `Cargo.lock` (fsrs brings it), so a bare `cargo update` breaks the pin canary; update with `cargo update -p <crate>`.
- Time and randomness come only from `meguri_model::clock` and `meguri_model::rng` (R1875). `clippy.toml` rejects the system clock and thread or OS randomness; only a clock module or a bench may `#[allow(clippy::disallowed_methods)]`.
- Test files named `tests/canary_*.rs` run first and alone in `tools/mg core-test`, and a failure there stops the run (AE82 ordering). Keep them fast.
- Keep domain crates Linux-buildable: Docker runs all Rust tests. Apple-only dependencies go under `cfg(target_vendor = "apple")`.
- The `test-hooks` (engine, ffi) and `ankiweb` (sync, ffi) features are never default.
- Host builds write `core/target/`; Docker builds use the `cargo-target` volume.
