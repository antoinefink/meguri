# First Mac CloudKit smoke

Status: WIP; no signed execution or Unit acceptance.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Client integration start: 74ea4958.
Contract: conformance/cloudkit-agent J2, J3.1; cloudkit-acceptance Q5.

Filtered client export cleanroom/20261102T133207Z-6004 adds the ten-action
program and fixed storage inspection. The recorded author is fixer
s-20261025T162405Z-9dc656e4; the handoff prose names a different session in
error. The export receipt supplies the actual provenance.

Independent review cloud_smoke_review is in progress. Parent coordination is
still being authored separately. All new focused checks and builds are unrun.
No SDK work may start before both halves are reviewed together. Generated
checks and compilation are not signed execution evidence.

Integration removes one redundant comment after a privacy finding; executable
code is unchanged. Staged privacy scan private/20261102T135313Z-24442 passes.

Host integration start: 394edd34. Filtered export
cleanroom/20261102T133203Z-5702 adds the tools-only parent and generated tests.
Its completion validator remains implementation_missing. Review also found
that coreOpened's actual count field is rejected by the parent's current fact
parser. Both need one shared serialization repair before execution. Integration
renames internal tooling names and removes redundant comments after the privacy
scan; no command, assertion or process behavior changes.

The first focused Rust invocation lacked the required test-hooks feature and
did not compile tests. The corrected invocation then found an unsupported
rusqlite u64 conversion. Filtered compiler cleanup
cleanroom/20261102T135550Z-27512 reads i64 and checks conversion; independent
review found no required finding. After that fix,
core-test/20261102T135755Z-29493 passed all three selected storage/FFI tests,
with no skips. This is local generated-data proof, not CloudKit execution.
