# Normal app media probe — 2026-11-11

Role: fixer. Requested starting commit: `3752724a`. Filtered implementation
checkout baseline: `4b6896748028abdeb53e6607149ab281bdc7711b`.

Spec: media/media-journal#J2, media/media-journal#J6,
sync/cloudkit-records#K9.4, R1952, R1953.

Adds an explicit host MG diagnostic with run-ID-only stage and phone verification
actions. Stage publishes one fresh 10,240-byte synthetic file with atomic
no-replace semantics into the admitted normal development app's existing media
directory. Staging alone leaves transfer unverified. Verification performs one
fixed copy-from against the device and development bundle admitted from original
normal preparation evidence, and requires an exact byte match. Neither action
grants signed-lane acceptance. Normal Sync Now remains a separate user action.

No native calls, builds or device tests were performed in the filtered session.
Focused verification, independent review and provenance/privacy/similarity gates
remain pending with integration. Generated fixture safety tests are wired through
`tools/mg selftest --filter '^normal media probe'`; one consistency check is
recorded in the handoff. No private receipts or account inputs were inspected.

Dynamic integration checks still required: admit actual normal Release preview
settings/receipt and version-2 physical preparation shapes; verify the observed
`devicectl copy from` API returns a single regular file at the requested private
destination and the usual `info.outcome == success` / object `result` JSON
envelope. Unknown shapes fail closed; there is no listing or alternate path.

## Public CLI admission follow-up — 2026-11-11

Role: fixer. Requested start: `a963f900`; filtered baseline:
`d9db837bda98c23a78412552763df03f798a5a87` (upstream history unavailable).
Inspection confirms the forwarding risk: MG dispatch passes caller globals to
the helper after its internal globals, and argparse accepts duplicate globals
before the action. Actual MG/stub reproduction is unconfirmed in this runner.

Generated Bats regressions invoke the actual sandbox MG wrapper with a harmless
marker/closed-report helper replacement. They check non-execution for internal
overrides, extra/unknown/missing arguments and paths, plus exact forwarding and
internal root/run-ID preservation for valid public forms. The wrapper now admits
only the documented run-ID grammar before Python, retaining both phone flag
orders. Usage failures use fixed text and a fixed repro command. The production
helper, source/receipt/byte guards and existing tests/limits are unchanged.

`tools/mg selftest --filter '^normal media probe'` was attempted with tests only
before the wrapper fix and again afterward. Both exited 2 before any test:
`tools/mg: jq is required (macOS ships /usr/bin/jq)`. These are unavailable
checks, not observed red/green results or passes. Integration must apply the Bats
change first and retain the marker failure, then apply the wrapper fix and run
the same filter. No native calls or real home/app/device/private inputs were
used. Focused verification and independent/provenance/privacy/similarity review
remain with integration.

## Explicit Finder preparation extension — 2026-11-11

Role: fixer. Requested start: `cb241e15`; filtered source baseline:
`c3215329da10b496117e4a180de3f49a361d5191` (upstream history unavailable).
Spec: media/media-journal#J2, media/media-journal#J6,
sync/cloudkit-records#K9.4, R1952, R1953.

Operator-supplied runtime evidence: stage run
`cloud-media-probe/20261111T181033Z-68316` failed before `source.bin` or any
media payload was created. Codesign completed with empty diagnostics. A scoped
read-only open check reached the Mac app container's `Data` directory and got
EPERM (errno 1); every preceding directory component opened. Preserve this
privacy refusal: readable stat/signature evidence is not app-data permission.
No permission broadening, alternate raw-container access or automatic fallback
is added; the stage function, guards and original assertions are retained.

The operator reports that the authorized ordinary Finder workflow opened the
exact existing `collection.media` folder and showed 565 items. This is folder
access evidence, not evidence that the generated file was copied or synced.
The explicit run-ID-only `prepare-ui --preview-build RUN_ID` operation admits
the same development preview, exclusively creates the private diagnostic
directory and freezes 10,240 fresh random bytes as `source.bin` plus an
independent same-byte `filename(current_run)` file for Finder. Neither file is a
hardlink. Preparation accesses no app-data directory, reports `prepared-only`,
`transfer_verified: false` and `signed_lane_acceptance: false`, and emits no
absolute private path. It claims neither publication nor a GUI action.

Phone verification accepts only the exact report shape/outcome derived from
the original `stage` or `prepare-ui` action, retaining original source hash,
bytes and binding checks, normal phone preparation, the fixed remote filename
and one-way copy-from with byte/SDK refusal checks. Peer evidence records
`source_action`; it does not assert programmatic publication of a prepared
source. Exact phone bytes plus separately observed authorized Finder copy and
normal app sync are diagnostic evidence, never Q7 or signed-lane acceptance.
There are no copy-to-device commands.

Generated Docker fixture tests cover untouched media/database and refusal on
even attempted app-data access, fresh independent source/UI-copy files, public
preparation-only reporting, strict action/outcome binding, exact peer bytes,
tampering/binding changes and native failures. CLI positives and invalid inputs
extend the existing wrapper tests; no CLI test-home bypass is introduced.
Integration owns `tools/mg selftest --filter '^normal media probe'`, independent
review and provenance/privacy/similarity checks. The filtered session performs
one `git diff --check` consistency check only, recorded in the handoff.

Pending live checks: actual preview/preparation admission, explicit preparation
under the real privacy boundary, separately observed exact-file Finder copy,
normal app sync and exact phone copy-from/SDK evidence. Actual data, Finder
access and iCloud transfer were not tested in this filtered run; no acceptance
is claimed.

## Integration evidence

The extension passed focused selftest `20261111T191149Z-94385`: seven Bats
checks, including the generated filesystem and native-command tests. Privacy
`20261111T191154Z-94662`, static checks `20261111T191250Z-95757` and similarity
`20261111T191254Z-96085` passed. Independent filtered review
`20261111T191231Z-95048` found no P0–P2 issues. The review made no source edits.

Real preparation `20261111T191241Z-95439` passed with `prepared-only` and both
transfer and signed acceptance false. Finder selected its exact generated file,
copied it into the existing Mac media folder, and then selected that installed
filename. The folder count increased from 565 to 566. Finder showed 10 KB.
No replacement dialog or access grant occurred. The original source stayed in
private diagnostic storage.

The normal Mac app detected one new change. A visible Sync Now interaction
advanced its last-sync time from 18:09 to 20:14. It still reported the existing
2,389 missing files, with 2,390 files waiting. This proves no peer bytes yet.
The connected phone's normal foreground observation and exact byte check remain
pending. No collection replacement or review change occurred in these actions.

Phone foreground run `20261111T191547Z-97490` completed all six interactions.
Its overall result failed on 98 hierarchy issues; those remain unresolved.
The snapshot retained the earlier completion instant, so it does not prove a
fresh automatic exchange. Read-back `20261111T191732Z-99050` failed with native
CoreDevice error 7000 before returning bytes. The read returned no file.
Transfer remains unverified.

A later Mac observation showed the waiting count return from 2,390 to the
existing 2,389. The extra waiting item was no longer shown. Server upload
was not proven. A separate
manual phone exchange is the next phase, not a repeat of the automatic claim.

Manual phone run `20261111T191841Z-99437` completed five interactions, with
63 unresolved hierarchy issues. Its immediate snapshot was stale; the next
read-back (`20261111T192015Z-311`) failed and proves no transfer. Later observation
`20261111T192050Z-698` showed a newer successful exchange at
`1794424803536` with pending, held-back, in-flight and incoming-waiting counts
zero. Its three interactions completed; 36 hierarchy issues kept the result red.
The post-completion read-back `20261111T192242Z-1520` still failed with native
error 7000. Thus successful exchange status alone does not establish media
arrival. The product metadata comparisons show matching development app identity,
cloud container, environment and enabled schema on both products. Values remain
private; no account or connection settings are recorded here.

The read-only phone summary in `20261111T192741Z-2547` showed 49 reviews and
565 media files. Cancel and return to Decks completed. All eleven interactions
completed; 223 hierarchy issues kept the receipt red. This independently
confirms that the phone did not have the additional file at that observation.

A disposable empty deck was created in the normal Mac app to distinguish
collection propagation from media propagation. The sender showed two pending
changes, then no pending count after a native File-menu Sync Now request.
The immediate phone observations in `20261111T193238Z-3717` did not show the
new deck or a fresh completion yet. All eleven interactions completed, with
133 hierarchy issues. These immediate observations are not final exchange
results. A current signed Mac diagnostic build is next; no production fix is
selected without sender evidence.

## External-file incremental Service reproduction — 2026-11-11

Role: fixer. Requested start: `e56fcdab`.
Spec: media/media-journal#J2, media/media-journal#J6,
media/media-files#M7, sync/cloudkit-records#K9.4.

Adds one test, `realServiceExternalMediaReachesRetainedBindingIncrementally`,
using the existing ServiceAppHarness, ServiceWorkflowWire and CloudServiceProcess.
Both sequential owners adopt and baseline-exchange before the new file exists.
A reads its media inventory after reopening. Foundation then writes
10,240 generated bytes under a fresh mixed-case ASCII `.bin` name. Ordinary
exchange and the existing worker join must yield original MediaName and exact-byte
MGBlob Saved receipts. B reopens its retained paths/defaults/tracking, exchanges
normally and must receive that exact name and bytes. Original media names/bytes
and note/review counts are compared; no collection edit wakes sync.

No production code, helpers, old tests, assertions or timing limits change.
The test misses a long-lived native Apple CKSyncEngine and does not prove real
Apple transfer. It is unrun here: integration owns the single focused test and
independent review. Only one `git diff --check` consistency check is performed
before export; no native SDK, tests or other verification jobs are run here.

## Original native sender callback diagnostics — 2026-11-11

Role: fixer. Requested starting commit: `8f0a0e7c`. Filtered source baseline:
`3bf2e7c` (upstream history unavailable).
Spec: sync/cloudkit-records#K12.4, R1952, R1953.

Adds Debug-only process-wide `sentC`, `sentM`, `sendFailC`, `sendFailM`
observations after the existing original SDK identity admission in handleEvent,
before conversion or delivery can fail. Only actual savedRecords,
failedRecordSaves and failedRecordDeletes feed them. Successful deletions,
pending counts, input requests and command completion supply no save receipt.
Routing compares full CKRecordZone.ID values, owner included, against the
registered collection and current-user media zone; foreign owners are ignored.
No identifiers, records, errors, settings or payloads survive projection.

The existing processAll block, UInt64 saturation, sticky overflow and less-than-
256-byte bound remain unchanged. A separate processSent block extends the same
safe status prefix with four numeric totals and its own sticky overflow; its
worst-case bound is less than 160 bytes. These aggregate across engine instances
and time. They are diagnostic observations, not selected-leg or signed acceptance
evidence, current-owner authority, convergence or an explanation of the issue.
All additions are under DEBUG. No production behavior, waits, scheduling,
journaling, policy, SQL or SDK calls change.

New tests use fresh local recorders and generated record IDs for full-owner,
absent-registration and unrelated-zone routing, safe output, counter independence,
saturation and bounds. Existing assertions and limits remain intact. SDK Event
initializers and live inputs are not used. Native compilation, focused tests,
independent review and provenance/privacy/similarity checks remain unrun here
and belong to integration. Only one git diff --check consistency check is run
before export. No native calls or private inputs were used; no cause or pass
claim is made.
