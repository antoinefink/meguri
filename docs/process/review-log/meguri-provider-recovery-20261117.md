# Provider recovery: unaccepted work in progress

Integration start: `8327cd0af1d930b9036b53d9c5a0644590be8855`.
Keep full-app gate base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
No Unit, milestone or working-service acceptance is recorded.

## Source and independent review

| Scope | Final export | Independent review |
|---|---|---|
| Core recovery, failed writes and setup without a collection | `012714-56067` | `014109-63904`: clear P0–P2 |
| Swift recovery actions and registered calls | `012448-54278` | `013844-61778`: clear P0–P2 |
| Owned test roots and explicit fixture setup | `012713-55686` | `013732-60957`: one caller finding |
| That caller and the observed registry test failure | `014441-64753` | `provider_test_delta_review`: clear P0–P2 |

These are source reviews. The integrator compared all reviewed source bytes.
The Swift review normalized diff order; its twelve seeded files are unchanged.
The composed 27 applied files match their selected exports exactly.
Every earlier export, finding and failed receipt remains retained.

Core recovery retains the durable original intent and outcome. Its explicit
current-owner action returns to local-only setup. Pending credential removal
and uncertain transfers retain their exclusions. Settings retry preserves its
failed mutation and error through ordinary refresh. Metadata publication holds
actual Host and engine ownership, including setup without a collection.
Handoff recovery keeps its stronger review and native-work restrictions.

Swift uses the real Core recovery and retry calls. It drains current owners,
fences replacements and composes a distinct successor. The killed original
process gains no join claim. Tests cover reopen, stale ownership and retained
removal intent. Storage-failure tests remain Core-owned; no Swift success stub
or proof setter was added.

The fixture changes preserve local-only and first-setup cases. Only declared
prebound fixtures import a prior choice before first open. Explicit previews
retain discovery through the real account read and successful confirmation.
A separate read-only audit checks remaining shared test roots before broad runs.
That prerequisite remains open; these changes do not authorize broad execution.

## Actual checks and limits

Before this composition, `core-test/20261117T012115Z-52998` passed three
FFI and original-media checks. `swift-test/20261117T013926Z-62367` built
the ordinary core for all three Apple targets and compiled Swift. It passed
15 selected tests and failed the registry-order test, with zero skips.
The correction preserves exact ordered equality and every old export name.
Neither receipt verifies the new recovery composition. Execution is pending.

## Public source snapshot

Independent reviewer `cleanroom/20261117T014109Z-63904` read the whole
`core/crates/meguri-sync/src/provider.rs`, including comments and literals.
It classified all 516 lines and 30,593 bytes as public under the recorded
filtered provenance, with no source or license concern. The SHA-256 is
`b4c6fdce499a059cf1f224165b5a5191955d5daeb6bfbf1dbd757b04bd0f21a6`.
The integrator verified the bytes and refreshed only this exact path digest.
Matcher rules, tokens and private-data access stay unchanged.
This classification provides no technical acceptance.

## First recovery execution

`core-test/20261117T015115Z-67000` ran the composed Core work at dirty
`8327cd0a`: 26 passed, three failed and zero skipped. All three failures
stop at the fixture requirement to start an actual review, before their
intended recovery or pause assertions. Keep those requirements intact.
Repair the fixture using the existing engine admission path.
No sync defect or successful active-review recovery follows from these failures.
The CPU window was released at the terminal receipt.

Normal privacy `20261117T015114Z-66173` flags three source candidates.
The complete public-content review below resolves classification; no private token was inspected.
Static policy `20261117T015114Z-66319` passes three checks.
Similarity `20261117T015114Z-66412` passes all 28 supplied code files,
with zero flags or exceptions. The filtered implementer report was read.

Independent non-author `provider_public_snapshot_review` read every line
of the three flagged candidates, including comments, literals, data and paths.
All are PUBLIC, with no actual private values or unresolved source/license concern.
The integrator checked these exact bytes and added only their path digests.

| Exact path | SHA-256 | Lines | Bytes |
|---|---|---:|---:|
| `app/Packages/MeguriKit/Sources/AppModel/Collection/SyncCoordinator.swift` | `47539f27d24565d47ab921dadcd21c0e3fc1c4664b3f09059489554f96e3bcae` | 2708 | 155866 |
| `app/Packages/MeguriKit/Tests/AppModelTests/ProviderAdmissionTests.swift` | `c25c16744537d8ca65527ea7c9eb36970266c48a54693279ce1f01a1074bd275` | 334 | 21205 |
| `core/crates/meguri-ffi/src/provider_tests.rs` | `95f3dfa62975a4741ad6719f56cca70ab2198aaa1db5d6b3eac90bcdfb1c7756` | 289 | 16901 |

The existing routine-admission rule covers these entries. Matcher behavior,
tokens and private-data access stay unchanged; later bytes remain checked.
Classification gives no technical pass. The failed scan remains retained.

Final normal privacy `20261117T015930Z-72828` passes all 30 supplied files.
Static policy `20261117T015930Z-72729` passes three checks.
Scanner regression `selftest/20261117T015741Z-71078` passes one case.
Final metadata similarity `20261117T015929Z-72376` passes its one file;
the filtered report was read. The earlier source scan covers the unchanged
27 source files. The failed privacy and runtime receipts remain retained.
