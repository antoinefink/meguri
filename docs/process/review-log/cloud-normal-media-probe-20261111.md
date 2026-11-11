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
