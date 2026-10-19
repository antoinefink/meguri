# U64 scoped fixes (2026-10-19)

Fixer session: `s-20261019T192613Z-3d57456b`.
Restricted export start: `130c671491117ff1e8e9041062efce329d6c09aa`.
Runtime implementation identified by the orchestrator: `d558f518`.

- Added `compare_typed_answer` to the strict Swift instrumentation registry expectation.
- Added sorted `review_html` / `type_answer` supports expectations and language-membership assertions to the CLI capabilities test.
- Replaced `unic-ucd-category` with a private std-only lookup over 263 inclusive Unicode 10 Mn/Mc/Me ranges (2,177 scalars), authored from the neutral reference. Removed its workspace/template manifest entries. Normalization and separately pinned folding/data dependencies are unchanged.
- Recorded official data provenance and linked the existing full Unicode notice in THIRD_PARTY and source documentation. Added exhaustive scalar classification against the independent JSON, historical boundaries/counts, and token-start NBSP checks on both comparison lines. Existing corpus/golden tests remain intact.

No builds, tests or host verification ran in this export. Cargo.lock resolution,
format/lint, focused and full tests, core-deny and the unit gate remain the
orchestrator's responsibility. No forbidden sources/history, private data,
networking, external integrations or subagents were used. The patch remains
uncommitted for clean-room integration.
