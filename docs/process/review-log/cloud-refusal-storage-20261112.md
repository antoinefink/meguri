# Cloud refusal evidence storage — 2026-11-12

- Role: fixer (implementer view). Supplied upstream base: `df308d23`; clean filtered baseline: `887e4cb3b57f5a0c591bde207edb5d8a8dd8952f`.
- Failure: `core-lint20261112T141500Z-16748`, `clippy::result_large_err` for `CommandAttemptFailure` in Driver and native execution.
- Scope: only Driver's opaque `OriginalIssuedGuardSuperseded` storage. Its private boxed evidence retains the owner and all four full issued/observed policy/backend guards with `Clone` and `Debug`. Allocation and the same clones remain lazy in the original pre-effect refusal closure.
- No authority checks, guards, clocks, SDK/cancellation/results/ACKs, error/refusal pairing, job lifetime/completed reads, FFI surfaces, schema, assertions, limits or tests changed. No new public API or lint suppression.
- Source consistency: reviewed the storage-only diff and ran `git diff --check`. No implementation-mirroring size test added.
- Native Cargo, Swift, lint, gates and tests: UNRUN in the filtered runner. Integrator owns compilation, Clippy, existing detailed-result/guard/refusal tests, independent review and provenance/privacy/similarity checks. This patch does not claim gate acceptance.
- Only permitted Meguri source and process instructions were read. No forbidden sources, private data, network or other agents used.

## Integration evidence

- Main source base: `df308d23`. Author: `cleanroom/20261112T142918Z-32519`. Independent review: `cleanroom/20261112T143108Z-38166`, no P0–P2 findings.
- Gate `20261112T141110Z-8802`: cancelled through MG after the lint and entry-export failures. Partial results remain; the gate did not pass.
- Build `core-test/20261112T144332Z-41454` stopped at a killed compiler. Serial build `20261112T144816Z-42091` then failed to link internal Rust symbols. Neither executed the selected tests.
- Serial compilation without incremental reuse passed all nine selected tests: `core-test/20261112T144929Z-42469`. These include the seven existing Host/FFI refusal tests and both export-fixture variants. Assertions and test limits were unchanged.
- Static checks passed: `static-checks/20261112T145057Z-42816`. The full gate from original batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9` remains required.
- Rust lint passed all four checks in `core-lint/20261112T145507Z-43397`; no rule was suppressed. Similarity passed both changed source files without exceptions in `20261112T145521Z-43710`.
