# Durable media synchronization

The precommit canonical media suite passed 188 server tests and both lint configurations.
Actual current-Desktop full, incremental and media selections pass against one clean candidate executable.
The coherent collection/media/web batch gate remains pending; this is not final server acceptance.
The original batch base is aa8565719af435051de7d5f068c0c817286b568e.
The unit started at 4e34620f6973c04396b21bdf73fdbaae5b7e5d67.
Original 25-file patch SHA-256: 51dc338ad328db5b62a562af937fba5539f2a5bf03aec528bb0ca8de8875cd8f.
The original isolated 25-file manifest is /private/tmp/media-final-hashes.sha256.
The first 13 original metadata-only commits end at e8209a17db5682a8063308440abd2117a2c254da.
The full receipt handoff is /private/tmp/media-final-handoff.json.
Targeted review closed both efficiency findings without a regression.

## Publication and storage

The service stores per-account media state in the shared metadata database.
A transactional migration adds the media schema to version one deployments.
Names remain exact logical strings; generated account-private paths select immutable blobs.
Uploads validate the complete archive before metadata can reference durable bytes.
Publication rechecks account authority under the existing short barrier and writer order.
Repeated identical additions and absent deletions do not create new USNs.
Downloads pin a metadata view and verify selected bytes before sending the ZIP.
Exclusive startup ownership permits cleanup of owned, unreferenced files.

## Archive and dependency boundary

The reader uses rawzip 0.5.2 with defaults disabled and std enabled.
The package adds no normal dependencies. flate2 1.1.11 uses the existing zlib-rs backend.
zip 9.0.5 remains the download writer. All versions and selected features are locked.
The scoped dependency audit passed; this does not certify absence of security risk.
The core lock and application code remain unchanged.

Fixed buffers, explicit metadata extents, count limits and decompression budgets bound archive work.
The adapter checks metadata selected by the library; it does not search through media payloads.
ZIP64, extra-field-dependent forms, prefixes, padding and several other unobserved forms remain unsupported.
The dependency note records exact admission limits. Current measured Desktop uploads use finalized stored headers.
Mobile and universal ZIP compatibility remain unverified.

## Evidence and repair

The initial real HTTP proof failed with 404 before implementation.
Malformed-archive proofs later exposed incorrect descriptor handling and a directory size mismatch.
A speculative archive adapter was withdrawn when its bounds could not be established.
The streaming reader closed all three existing archive failures without changing their assertions.
A physical disk-full test uses the dedicated 16 MiB tmpfs and observes operating-system ENOSPC.
Its first run failed a directory-permission setup assertion before filling the filesystem.
The corrected proof verifies rejection, rollback, restart, lost acknowledgement and replay.
No shared temporary directory or host filesystem is filled.

One fresh combined correctness and five-rubric review found no correctness defect.
It proposed two efficiency fixes: remove duplicate startup validation and omit a production no-op test-hook task.
A fresh filtered fixer changed only those two statements. Protected tests and the lock stayed unchanged.
The targeted review first failed admission because its supplied session ID was mistyped.
The original failure remains retained. The real committed session ID was then used without altering the guard.

All following paths are relative to /private/tmp/meguri-media-u7/build/mg/.

| Check | Receipt | Result |
| --- | --- | --- |
| Physical disk-full proof | sync-server-test/20261116T035504Z-85991/result.json | 1 passed |
| Exact dependency audit | sync-server-deps/20261116T040827Z-95770/result.json | Passed |
| Archive and media suite | sync-server-test/20261116T041236Z-3617/result.json | 26 passed |
| Combined independent review | cleanroom/20261116T041915Z-7810/result.json | Two efficiency findings; no correctness finding |
| Affected recovery proofs after fixes | sync-server-test/20261116T043006Z-11543/result.json | 11 passed |
| Lint after fixes | sync-server-lint/20261116T043006Z-11788/result.json | Both configurations passed |
| Full final suite | sync-server-test/20261116T043135Z-12198/result.json | 188 passed, zero skips or retries |
| Targeted independent closure | cleanroom/20261116T043443Z-13423/result.json | Both findings closed; all final hashes verified |
| Refused review launch | cleanroom/20261116T043357Z-13009/result.json | No actor execution; wrong supplied session ID |

At that isolated-review checkpoint, no actual candidate media pass, restore procedure, Mobile result or release artifact had been established.

## Canonical signal coordination repair

The first canonical run passed 187 of 188 tests. The migration kill test observed an empty marker immediately after its creation. The helper reported file existence before the hook completed its write and sync.

A fresh filtered fixer changed only test_hooks.rs. It writes and syncs an exclusively created sibling, then publishes the final marker with a no-clobber hard link. All original 25 files and all 188 proofs remain unchanged. Independent targeted review 074101Z-67103 closed the race without findings. The isolated six affected process-kill checks, full 188 and both lint configurations passed.

The parent applied the exact one-file repair and verified all 26 final hashes. Canonical checks now pass: sync-server-test074612Z-70043 (188/188, zero skips), sync-server-lint074613Z-70792, static-checks074612Z-69884 and similarity074613Z-70286. The earlier187/188failure remains retained at sync-server-test071403Z-44279.

Original added metadata-only commits 829d4f42b8ca2fa144bc3c5e1de8f6a980171230 and 17879cde3d48aae98ef1a72bff891e1ce047f05e preserve fixer and reviewer provenance. All 15 original metadata commits are now retained through normal merges 83e7cba9 and 414c671b. Fresh non-author content review and exact-prefix/original-commit checks satisfied the standing ledger-maintenance rule approved by Antoine on 2026-11-16; no record was rewritten.

## Committed current-Desktop verification

Canonical source commit: `8506d4879dd3b76b4aa5e1ab0825409671b84549`.
All 26 final source identities match the reviewed manifest; the source commit includes the test-only signal repair.
The final manifest is `/private/tmp/media-signal-final26-hashes.sha256`, SHA-256 `62a4840573e53579810699012793fa21ca5a22ba0bbf55a138e41e38b9cfe77a`.
The parent and a fresh evidence reviewer rehashed all 26 current files against it. The upcoming coherent gate repeats verification on its clean committed tip.
Precommit canonical tests (dirty `8ee4f155`; not a clean `8506d487` execution): `build/mg/sync-server-test/20261116T074612Z-70043/result.json`, 188/188, zero skipped tests.
Precommit canonical lint (dirty `8ee4f155`): `build/mg/sync-server-lint/20261116T074613Z-70792/result.json`, both configurations.
Precommit dependency audit (dirty `8ee4f155`): `build/mg/sync-server-deps/20261116T071404Z-44533/result.json`.
Precommit source privacy/static/similarity (dirty `fbcab4ef`):
`build/mg/private/20261116T092314Z-95841/result.json`,
`build/mg/static-checks/20261116T092315Z-96198/result.json`,
`build/mg/similarity/20261116T092326Z-2032/result.json`; all pass, no new similarity allowance.

The clean, committed Docker build is
`build/mg/sync-server-candidate-build/20261116T092612Z-7997/result.json`.
Producer session: `s-20261115T203832Z-6ecc17e8`.
Production source archive SHA-256: `151b4b8977da57e8ca163a25577a9eefc12d742adf7d9e87114d993185ce171d`.
Server lock SHA-256: `867679c49c310e081f89e4c09750d57fc03e17165ddbd06b9c362816f5a923dd`.
Executable SHA-256: `67e39967d2c1366ee1045e92eddbdd77f5dc7abe776c1db83dbaaf2adf73759a`.
All three actual Desktop SDK 26.09.4 selections reuse this executable with no default features.
The debug artifact verifies the service protocol; the final packaged release requires its own exact-artifact evidence.

| Actual selection | Receipt under build/mg/sync-server-interop/ | Result |
| --- | --- | --- |
| Full collection transfer | 20261116T092628Z-8898/result.json | 11/11 passed |
| Incremental collection sync | 20261116T092638Z-10055/result.json | 10/10 passed |
| Media sync | 20261116T092711Z-12078/result.json | 4/4 passed |

Each result grants only its exact finite selection, records zero failed/skipped cases,
and binds source, lock, executable, runtime and producer identities.
The media selection exercises bidirectional additions/replacements/deletions, an ordinary no-change run,
26 requested downloads and a 1,005-change listing through the real SDK.
This is the first candidate-media evidence for the implemented service.
The results do not establish restore/reset procedures, retained-Meguri acceptance, actual AnkiMobile behavior,
or complete optional ZIP format compatibility. Those remain required in the later recovery/delivery work.
