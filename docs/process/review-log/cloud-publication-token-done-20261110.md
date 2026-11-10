# Publication completion — 2026-11-10

Start: `b36a5559`. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Recorded fixer: `s-20261103T133750Z-f276f58e`. Contract: `sync/policy#P4`.

Successful SaveActive removes the publication row. Its next projection has no
publication step. Policy now retains the token across that change from Sent,
Complete or Done, with every other authority check retained. Earlier steps and
cancellation still invalidate the token.

The Host test drives the actual Service replies through completion. It checks
the original command before and after row removal. Pure policy tests cover
binding, account, generation, lifecycle and cancellation changes.

Independent source review: clear of P0–P2 findings.
`core-test/20261110T013604Z-26201`: four selected cases passed, none skipped.
`core-format` and `core-lint/20261110T013952Z-28236`: passed.
`similarity/20261110T014039Z-28840`: passed.

`swift-test/20261110T013712Z-26523` failed at peer adoption. Publication
completed without the former token refusal. The page contained 41 records;
adoption accepts 32. That fixture defect remains open.

U113/U114 and the batch gate remain unaccepted. No signed exchange is claimed.
