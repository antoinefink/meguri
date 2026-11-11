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
