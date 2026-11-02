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

Independent reviewer cloud_smoke_review reported P1, the
coreOpened shape mismatch above; P2, missing actual SDK/state-save ordering
and outgoing review hold-back evidence. The explicit completion Missing fence
also remains open. A filtered shared repair is in progress; no SDK run starts
before independent re-review.

On clean b139641a, focused checks pass:
selftest/20261102T140138Z-34631 (3/3 host wrappers),
swift-test/20261102T140139Z-34902 (1/1 real bridge inspection),
swift-test/20261102T140336Z-38293 (41/41 protocol/product tests), and
build-app/20261102T140512Z-40625 (Mac Debug CloudAgent compile only).
No test skips are reported in these focused results. The Mac build does not
launch the test host, prove signing or exercise CloudKit.

The combined repair cleanroom/20261102T140128Z-34085 was cancelled and joined
after 20 minutes with no source changes. Its error result is retained. Split
host repair cleanroom/20261102T142316Z-52904 fixes the exact launch fact and
validates current step encodings and retained observations. Independent review
found no required finding. Command 3 explicitly remains Missing for SDK order;
the app/core evidence repair runs separately. No signed pass is claimed.
Integration removes one redundant docstring after the privacy finding.

Filtered evidence export cleanroom/20261102T142707Z-55686 adds the bounded
SDK, own-operation and held-image observations. Fixer session:
s-20261102T142603Z-4626b865. Source start: 5e58e13a; integration start:5999cc58.
Independent review and all new focused checks remain pending. The host keeps
its explicit Missing boundary until it validates the exact added facts.
Integration renames one generated-test local binding after the privacy finding;
the assertion and executable checks are unchanged. Privacy68716 passes.
