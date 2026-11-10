# Original ACK inspection foundation — 2026-11-10

Role: implementer, filtered source-only WIP. Original main base: `ef39c65a`.
Filtered runner baseline: `56a8603512d3d1c4160999f57fa6edfaa9858450`
(the filtered snapshot identity, not a replacement batch base).
Scope: only packet steps 1–2, authorized in the task. No acceptance claim.

Factored only the original/current entity selection out of the existing edit
read; its checks and unACKed edit read remain. The new ACK projection shares the
exact existing live original-role ACK predicate. Both actual retained pre-scan
and current owned rows must pass it and agree on their nonzero local/ACK
revision, positive server revision, full opaque system-field bytes, canonical
payload digest and complete ACK envelope. A later fresh ACK
cannot repair an unACKed retained row. Full row rereads preserve actual envelope
bytes as well as its digest. Existing identity/open/edit-witness selection,
selected control/snapshot rechecks, and the current held WAL identity/frontier
remain required. The historical edit sequence never replaces the current seq.
No Query reader is acquired by the runtime projection; selected control retains
its existing separate Cloud reader. FFI binds the actual Host/generation and
retains the immutable baseline, current integrity, current WAL coverage, final
Host/generation/snapshot frontier checks and independent final ACK reread.

Added a separate bounded test-only Rust/UniFFI/Swift ACK DTO/read, QueryLane route,
source-descriptor and generation revalidation, and exact instrumentation registry
entry/expectation. No generated bindings edited. No product schema, persisted
receipt, capture/tracking write, callback, fresh-ledger import, journal parser,
recovered terminal or selectedCloseProof added. Existing Mac recovery refusal
and its tests are untouched. No deadline, assertion or authority guard weakened.

Source coverage added: both sides of every live ACK predicate plus tombstones,
independently valid revision/envelope mismatches, malformed/missing witnesses,
changed account, retained-unACKed/current-fresh-ACK refusal, and an actual existing
Core/Service writer-kill-reopened-owner positive in cloud_original_role.rs. The
positive uses a real fixed edit, actual batch/Service Saved results and persisted
SDK exchange accounting; the reopened read repeats no mutation/exchange or
Service request. The original edit-only process case remains and additionally
refuses the later fresh ACK. Its original post-commit metadata and row/seq checks,
readiness deadlines and SIGKILL assertions remain. Original observations JSON is
synthetic fixture-only; no envelope fact is added to a product/agent journal.
These sources have NOT been executed. The fixture's SDK state is protocol-test
state and is not CKSyncEngine or signed evidence. This does not establish the
policy issued-exchange witness / historical composite journal join.

Review/fix log: source inspection reused observe_inner's predicate verbatim,
retained all shared edit-selection checks, added explicit FFI owner/generation
refusal, and used the existing process harness. No independent review performed
inside this runner. One final source consistency check: `git diff --check` passed. No build/test/formatter/MG job run, per task authorization.

Integrator pending: apply/review/export patch; MG formatting/lint, Rust focused
predicate/original-observation/process tests (including unchanged live provider,
file/publication/runtime/open/generation/frontier refusals), independent review,
provenance/privacy/similarity checks, generate ABI bindings through MG with
identical same-configuration slices, Swift/CoreBridge/instrumentation/app-hosted
compilation and focused tests. Dedicated historical ACK reread-race/stale-frontier
and expanded cross-owner/file/open/generation process cases still need execution
and review; the runtime uses the existing original-note authority route and a
bounded outside-lock read hook for that coverage. No positive is reported passed.

Remaining outside this foundation: steps 3–4, full historical original namespace /
ACK-join/tick/batch-return/later-state-save parser, exact original issued-exchange
readback agreement, recovery terminal and typed close successor; fresh SDK ledger
separation and no-replay acceptance; S2/S3 spec amendments; full MG/milestone,
real-process recovery and signed/device/final evidence. Mac recovery must remain
refused until the complete proof and required verification exist. Do not implement
any Under review section. This foundation grants no signed/recovery acceptance.

## Bounded R2 fixer — 2026-11-10

Role: fixer, own-source only. Main starting commit: `e6b2b93f`.
This resumed runner's synthetic filtered baseline:
`73d55a3b4ac1409626e2cedee8367e1a6512e637`. The earlier seed bases above
remain historical records; neither synthetic baseline replaces the main base.
Resumed the prepared foundation seed without recreating it.

Addressed exactly the two independent R1 P2 findings supplied in the task:

- Core: `original_ack_pair` checks matching positive `server_erev` and all
  `acked_system_fields` bytes. New cases change these values on each side and
  require refusal. Both rows still meet the live ACK conditions. The existing
  revision/envelope cases and complete final rereads stay in place.
- Bridge: `read_original_ack_with_recheck_for_testing` takes baseline/witness
  as one tuple, and its caller passes the same values. The new lint annotation
  is gone.
  No new DTO/ABI; held WAL identity/current frontier, historical edit sequence,
  outside-lock hook and every authority/final reread guard remain unchanged.

R2 edits are limited to `selected_observation.rs` and this source-only log.
The seeded real process-kill positive, FFI/Swift seam and existing Mac recovery
refusal are preserved. No parser, recovered terminal, selectedCloseProof,
product schema/receipt, replay, SDK history import or Under review amendment.

R2 source consistency: one `git diff --check` passed. No builds, tests, formatter
or MG jobs run here. New negatives are authored, unrun. Integrator owns all MG
verification and the previously recorded proof gaps; no acceptance is claimed.
# Integrator compilation evidence

`core-test/20261110T161945Z-2819` stops at compilation: the new pure fixture's
entity key expects `Vec<u8>`, while `codec::id_key(1)` returns an array. No
selected test ran. A bounded fixture conversion fix is required before focused
verification. R2's independent reviews are clear; no unit acceptance is claimed.

The one-line fixture conversion was exported by filtered run `8518` and applied.
`core-format/20261110T163406Z-11652` passed. Focused
`core-test/20261110T163411Z-11651` compiled and ran five tests: four passed,
including the new ACK writer/kill/reopen case and both pure predicate tests.
The existing edit-only reopen case failed its sequence comparison after the
new fixture deliberately performed a fresh ACK. That write must be separated
from the read-only observation checks; all assertions remain required.

