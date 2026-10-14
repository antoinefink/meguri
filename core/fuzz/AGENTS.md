# core/fuzz

- cargo-fuzz targets, run only through `tools/mg core-fuzz` (fuzz container, pinned nightly, `-O -a`). Each target runs for `--seconds` (default 600), seeded from the text corpus. Findings go to the run directory. Never commit corpora or findings.
- This directory is its own workspace, outside core's. Its packages skip core-lint, core-deny and layering-check, and libfuzzer-sys (NCSA) never reaches a shipped build.
- `Cargo.lock` started as a copy of `core/Cargo.lock`, so targets fuzz the pinned parity crates. When core's lock moves a crate both use, update it here with `cargo update -p <crate> --precise <version>` (never a bare `cargo update`).
- A new target is a file in `fuzz_targets/` plus a `[[bin]]` entry in `Cargo.toml`. The owning unit's crates go in `[dependencies]`.
