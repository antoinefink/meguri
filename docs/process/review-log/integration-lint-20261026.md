# Pending integration lint repairs

Starting commit: `be370f3d9a0e7d0f80aa4d03e94b07302294be6a`.
Intake: `build/unit-starts/pending-batch-lint-20261026.json`.
Integration base stays `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Filtered fixer `s-20261025T162405Z-9dc656e4` exported the three testkit repairs
in `cleanroom/20261026T014937Z-67370`: direct mutable actor binding, unchanged
dump function moved before tests, and exclusive status-memory access without
a mutex guard across await. Independent review found no P0–P2 issue.

The private actor state has one owner. Its only temporary Arc clone belongs
to the normal job, whose consumed closure drops its captures before JobFinished;
the actor waits for that event. Unexpected sharing now returns an invariant
error. Cached status is retained; no default replacement or copied cache is used.
This changes the harness ownership expression, not a protocol or public API.

`core-lint/20261026T015253Z-68896` got past those failures and found an existing
unnecessary-clone warning in the journal-image test. Export
`cleanroom/20261026T015617Z-70420` compares the same exact key values by reference.
Independent review confirmed the image-read, byte-limit, thread and lifetime
assertions are unchanged. The standard formatter resolved the retained fmt
failure in `core-lint/20261026T015753Z-71354`.

## Evidence and open failures

- `core-lint/20261026T015857Z-72593`: PASS; rustfmt, Clippy, layering and policy.
- `core-test/20261026T020259Z-75129`: FAIL; 1,070 passed, 7 failed, 1 skipped.
- `core-test/20261026T023337Z-93711`: clean-master baseline without these repairs;
  the same seven selected tests failed. This establishes that the failures are
  not introduced by the lint changes. It does not resolve or waive them.

Six cases reject unregistered media-tracking fields (`media_quick_check` and
`media_sync`). The remaining format test still expects the now-registered
review-session executor to be unavailable. The scenarios and assertions remain
enabled. Their provider/contract repair is separate; do not repeat the full
suite until that repair is ready. Normal-sync runtime evidence remains open.

No full integration gate or unit is accepted by this cleanup. The pending
original-base gate, tracked sync work and all remaining acceptance checks stay open.

Final scoped privacy check `031500Z-14467` passes for these four source files
and this log. Similarity `031502Z-14709` scans all four files with no flags.
The independent final source review is clear. The seven baseline failures
remain open and block the required integration gate.
