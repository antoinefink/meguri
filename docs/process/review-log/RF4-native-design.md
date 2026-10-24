# RF4 native design preview

Starting commit: `1bb4916106cbfa81a56f3c9a0440fd9f0b3902f9`.

Antoine authorized this bounded implementation and opening the Mac app on
2026-10-24 after the HTML preview. The full application goal stays paused.
The agreed design is `docs/design/native-interface-contract.md` (N1–N10).
Spec session: `s-20261024T111732Z-b3754a3a`.

Scope: native Settings with Connection/Collection sidebar; one Connection
details disclosure containing real sanitized logs; inline connection test;
quiet-corners review, bottom answer controls and native menu shortcuts; simple
deck workspace and empty state. Preserve actual collection and transfer guards.
Do not apply the held file-media export patch or claim media-file export works.

Antoine explicitly deferred detailed verification until the native design is
reviewed together. This is a WIP preview, not RF4 or milestone acceptance.
Run lint, the focused affected Mac build and a bounded smoke/render check now;
retain broader suites, audits, privacy/provenance/similarity and full integration
gate as required acceptance work after visual agreement.

Implementation, independent review, fixes and actual receipts will be recorded
below. No completed-unit trailer is authorized by this checkpoint.
