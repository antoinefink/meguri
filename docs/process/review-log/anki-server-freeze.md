# Anki server scope freeze — 2026-10-26

Starting commit: `6433086c6cde1c7db9595f288f6736b2b4bbbe27`.
Authority: Antoine approved an indefinite freeze, retained plan items and evidence,
updated acceptance, and a local commit. He excluded all AGENTS.md files.

## Scope and retained work

The plan retains eight server units in a Frozen Unit Index, with their original
content and dependencies. Active milestone memberships exclude those units.
Mixed units keep their CloudKit, file and safety requirements.

Active live-oracle selection excludes the 44 retained Anki server scenarios.
Other scenario coverage remains required; the existing tier check verifies the
selection against the current scenario tree. The sync-actor route check is
disabled. Its job, tests and failed evidence remain. No server was started.

The harvest catalog excludes 32 server-only requirements from active acceptance.
Original IDs, reasons, classifications and scenario links remain available.
A mixed row retains local media registration acceptance and records its old
server links separately. No historical failure is marked passing.

## Independent review and corrections

Coherence and scope reviews checked the document changes independently.
Follow-up review found an incorrect selector-array offset and a mixed media
row that lost active coverage. Both are corrected. A third review stopped
because its model was unavailable; its reported indirect gate concern was
addressed by the active selectors and catalog scope changes.

## Verification

Focused tools/mg checks cover milestone bindings, the real tier configuration,
scenario selection, static rules and privacy. Initial failed attempts remain
in build/mg: Docker was unavailable in the restricted sandbox, the first new
array assertion was wrong, and new prose required privacy-safe rewrites.
The tests and scan rules were preserved. No protocol or app build was needed.
This record does not accept any unfinished sync unit or milestone.

Final focused results:

- Milestone bindings: `20261026T082140Z-70189`, PASS.
- Static checks: `20261026T082140Z-70190`, PASS.
- Privacy over the 13 changed files: `20261026T082140Z-70188`, PASS.
- Docker selftest: `20261026T082145Z-71215`, two passed, zero failed or skipped.
- Independent follow-up review confirms both corrections; no remaining P0–P2 finding.
- Tooling similarity: `20261026T082253Z-72834`, PASS.
