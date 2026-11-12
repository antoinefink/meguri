# Qualified collection dates: admission repair, 2026-11-12

The focused Rust and native checks pass. No unit or full gate is complete.
A guarded epoch read for legacy installations remains the next source task.

Original implementation start: `0b89b4ae`.
Test-first source: `df657044`.
Integration source: `1176651a`.
Pending full-gate base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Contracts: `sync/policy#P6`, `sync/policy#P12`, `sync/cloudkit-records#K20`,
`sync/cloudkit-recovery#V3.4` and `#V26.1`.

## Behavior

- Collection and media writes require qualified epoch authority and equal collection creation times.
- Current commits, open, scans and accepted epoch reads update the comparison.
- Barriers and actual writers check their own collection view.
- A prepared batch must still match its original date at native return.
- Unknown authority blocks writes without declaring a mismatch or isolating the binding.
- Existing reads remain available. A supported complete epoch commits with its verification result.
- Recovery can finish an already committed intent using its original file, row and WAL proof.
- Original callback ownership protects media staging and prevents alias handoff.
- Native cancellation is checked after preparation. Core runs outside the native lifetime mutex.

`attention_known` remains false. The approved purpose-specific manual read from
`1176651a` is a separate next step. This patch does not implement its trigger.

## Independent review

Every review used a separate filtered source copy. Reviewers ran no tests.
Root compared and integrated the exports, then ran checks through MG.

| Round | Review run | Required repair |
|---|---|---|
| 1 | `cleanroom/20261112T204431Z-26854` | Existing epoch reads, media mutation gates, final batch admission and committed replay |
| 2 | `cleanroom/20261112T210821Z-29922` | Two recursive reader acquisitions, obsolete media batches and optional note after-state |
| 3 | `cleanroom/20261112T212228Z-35221` | Old Saved staging ownership and original native callback routing |
| 4 | `cleanroom/20261112T213812Z-40792` | Native cancellation between delivery and return |
| 5 | `cleanroom/20261112T214741Z-44495` | No new P0 or P1 |
| 6 | `cleanroom/20261112T215322Z-46403` | No P0 or P1 in the fixture corrections |

The final R6 check, `cleanroom/20261112T220119Z-48758`, found no P0 or P1
in the pending-deletion proof. It checked the real canonical record and the
unchanged outcome assertions. Production source stayed unchanged after R5.

Residual P2 coverage: direct private CKSyncEngine callbacks through stop and
quiesce; a dedicated media deletion re-preparation case. These remain open.

## Verification evidence

| Job and run | Result | Scope |
|---|---|---|
| `core-test/20261112T202832Z-21986` | RED: 3 failures | Original actual-operation reproductions |
| `core-test/20261112T210839Z-30447` | 10/12 | Two new fixture failures before their target |
| `core-test/20261112T212040Z-34472` | 11/15 | Media fixtures needed actual transport order |
| `core-test/20261112T212401Z-37580` | 322/323 | Fixture expected set omitted its real epoch |
| `core-test/20261112T213827Z-41295` | 13/16 | Two sequence checks and one deletion premise |
| `core-test/20261112T214756Z-44941` | 2/3 | Diagnostic cases; no repair claim |
| `core-test/20261112T215333Z-46826` | 15/16 | Added deletion check read the wrong storage field |
| `core-test/20261112T220025Z-48325` | PASS: 16/16 | Actual authority, barriers, writers, replay, media and callbacks |
| `swift-test/20261112T220139Z-49235` | PASS: 5 tests | Native cancellation, delayed return, original callbacks, registry and media encoding |
| `core-test/20261112T220257Z-51221` | PASS: 323/323 | Affected libraries, adoption, policy, SDK and absence |
| `static-checks/20261112T220231Z-50526` | PASS: 3/3 | Existing static policies |
| `core-lint/20261112T221232Z-55164` | PASS: 3/3 | Lint, crate layers and host checks |
| `private/20261112T221237Z-55618` | PASS | 37 files scanned |
| `similarity/20261112T221236Z-55467` | PASS | 36 files, no findings or exceptions |
| `core-test/20261112T221317Z-56130` | PASS: 16/16 | Final shared helper and compiler cleanup |
| `static-checks/20261112T221325Z-56446` | PASS: 3/3 | Final source policies |

Earlier failed runs stay in the ignored MG archive. No failed or queued check
counts as a pass. Full-batch and milestone gates remain pending.

Compiler cleanup removed an identity error conversion and a borrowed temporary.
The shared test helper now loads once. Its bodies, assertions and limits did not
change. Privacy edits changed comments, this log and one private helper name.
The matcher and its exception list stayed unchanged.

## Fixture findings

The recorded trace identified local rollover commits that consumed a sequence
without row changes. Replay tests now call the public synchronous rollover check
before recording their sequence baseline. Exact sequence assertions stay intact.

Registration reported physical file absence as `Missing`. The intended deletion
case now uses the real trash operation and proves a `Removed` feed entry.
MediaName pending tombstones live in the media ledger. The prepared record has
its tombstone bit set. Its earlier ACK stays unchanged. The test also checks
file absence, the exact conflict copy, the intent and WAL. All assertions pass.

Temporary trace hooks were removed. Their exported patch and results remain
under ignored build output. The regression tests and adjacent comments retain
these lessons; a separate learning document would repeat them.
