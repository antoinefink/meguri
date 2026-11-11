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
