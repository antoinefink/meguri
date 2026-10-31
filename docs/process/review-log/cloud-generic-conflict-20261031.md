# Native conditional-save conflicts

This slice starts at `2df9441127eb989a81b3b6085abd3cd283cebd80`.
The original integration batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Filtered tests-only export: `cleanroom/20261031T221912Z-88842`.
Three regressions use actual SDKRig conditional Service replies: disjoint inline
edits, identical content from another writer, and a lost older save followed by
a newer local edit. They preserve collection intent, WAL, ACK and resend checks.
Test helpers expose actual transport and latch operations; routing is unchanged.

`core-test/20261031T222422Z-90457` compiles all three and fails all three at
`deliver_sent_event`. Independent review confirms the exact cause: the SDK error
becomes `Other`, with no server record passed to the core. `Runtime::sent_results`
then returns `Incomplete`. The fixtures already confirmed the root, built the original request and
proved the actual conditional Service conflict. This is the intended baseline
failure, rather than a missing fixture prerequisite.

Independent test review finds no required fixture repair. Keep the failing
boundary and subsequent assertions. Generic routing must preserve original
batch/handler authority. An older own-landed save needs retained-flight proof;
it cannot be passed off as a Saved reply for the current envelope. Reuse existing
incoming merge and ACK bookkeeping instead of replacing either algorithm.

## Core implementation checkpoint

`cleanroom/20261031T222712Z-92245` adds
`Runtime::sent_results_with_conflicts` and forwards actual conditional Service
records through the SDK test driver. The original Saved validator is unchanged.
Conflicts use their own validation, then the shared ACK code or SaveConflict
inbox. The existing collection write and WAL flush still precede its ACK.
Whole-receipt validation runs before ACK changes. Independent review found no
required production repair.

- `core-test/20261031T223316Z-98355`: the three baseline failures now pass.
- `core-test/20261031T223702Z-1891`: all 55 incoming-intent and reply tests pass,
  including existing payload-conflict, lost-save, undo and tombstone cases.
- `core-test/20261031T223918Z-5131`: all 184 FFI library tests pass; none skipped.
- `cleanroom/20261031T223550Z-1101`: removes an unused import only.
- `cleanroom/20261031T223827Z-4607`: comment-only privacy repair.
- `cleanroom/20261031T224030Z-6501`: equivalent historical-flight membership
  assertion and removal of one redundant comment. No production behavior change.
- `core-test/20261031T224139Z-7592`: historical-flight case passes after the
  membership assertion cleanup.
- `core-lint/20261031T224126Z-7028`: pass.
- `private/20261031T224137Z-7336`: final core and native transport source pass.
- `similarity/20261031T224140Z-7823`: final core and native transport source pass.

Further acceptance coverage must include a mixed Saved/conflict receipt whose
last result is invalid, and an actual historical tombstone conflict with a
newer local revision. Generation, rekey, Notetype and MediaName conflict paths
remain guarded and unfinished. Actual native conflict metadata/FFI delivery,
the SDK asset worker, signed cross-device proof and the batch gate remain open.
U113 and U114 remain unaccepted.
