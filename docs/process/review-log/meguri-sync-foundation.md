# Foundation integration

The first integration boundary is complete at `aa8565719af435051de7d5f068c0c817286b568e`.
Original batch base: `38235917e6a025daa57625c582e90d253aae62e3`.
Earlier unit logs' pending gate notes are historical. Open feature-specific reference probes remain open as the neutral specs state.

The full gate ran on a clean checkout in `/private/tmp/meguri-sync-first-batch-verify-20261115`:
`build/mg/gate/20261115T182513Z-62626/receipt.json`.
It reports pass and merge-ready, with no blocking, incomplete or queued runs.
The MG result is warn because the generated anonymizer fixture has two review items; all five checks passed. No real collection was used or published.

| Selected check | Actual result |
|---|---|
| Static stages | Both passed, three checks each |
| Account-service tests | 95 passed, zero skipped |
| Server lint and dependency review | Passed; exact locked graph retained |
| Current reference selftests | 37 passed, zero skipped |
| Tooling selftests | 928 passed, zero skipped |
| Host orchestration selftests | 29 passed, zero skipped |
| Synthetic private-vault selftest | Seven passed |
| Milestone/performance catalogs | Passed |
| Similarity | 37 supported source files passed, zero flags or allowlist entries |
| Similarity calibration | 94 passed |
| Current Desktop reference smoke | 21 passed, zero skipped |
| Current Desktop reference edges | Eight passed, zero skipped |
| Existing baseline oracle tests | 671 passed, 16 skipped; no current-server credit from skips |
| Generated anonymizer check | Five passed; two review items retained as warning |

Exact nested receipt paths and all selected outcomes are in the gate receipt.
This closes tooling, contracts, reference-harness and account-substrate integration. It establishes no candidate-server collection interoperability, web interface, backup, package, retained-Meguri peer or AnkiMobile acceptance.

## Retained repairs

The first run, `20261115T171637Z-12177`, was interrupted after proving that dedicated server JSON inputs unnecessarily selected old app workloads. Its cancelled result and console remain retained; no completion was credited.

Existing negative globs now route those inputs to their actual reference consumers. Old, shared and mixed inputs retain their checks; milestone/release all-path coverage is unchanged. The strengthened real-tier test failed on the original configuration, then the focused three-case run passed. Independent consumer investigation: `/private/tmp/meguri-sync-gate-routing-scope.md`.

Run `20261115T174530Z-28669` failed on host PATH and tooling checks. The launcher exposes existing rg/rustup paths solely for orchestration and health metadata. No tool was installed; server builds/tests remain Docker jobs. The host selftest clears inherited corpus overrides, preserving its synthetic isolation; its poisoned-environment run passed all 29 cases. The context check accepts both exact Docker deny-all patterns, retaining recursive exclusion and private-path checks. No scanner threshold or copied-code assertion changed.

Source, repair and session commits remain separate. The next coherent batch starts from the accepted commit above. Source reviewers and their evidence limits remain recorded in per-unit logs and the provenance ledger.
