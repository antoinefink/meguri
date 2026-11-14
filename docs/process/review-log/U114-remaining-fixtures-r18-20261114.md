# U114 remaining positive fixtures, R18 — 2026-11-14

Starting commit: 5c4fcfdc. Integration base:
1416a520542a8c5daa38a23bfe398da704c3d6c9.
This is a test-only repair. Production behavior is unchanged.

The sixteen remaining selected cases now establish matching epoch facts
through actual SDK intake before their measured work. The pending-start
cases then retire and release that prior SDK before selecting the original
command under test. They still require no current engine, no saved cursor,
no root confirmation and no drain proof. Default fixtures keep their original
unknown-epoch state. Cancellation, ordering, deadlines, error values and
NoCall versus Send remain checked.

The cursorless setup now checks the raw registration's actual pre-intake
flag. The later policy registration still checks the automatic-off result.
Independent review confirmed that this preserves the test's purpose: capture
before the first root fetch, with pending changes selected but sending still
blocked, followed by actual root delivery. No extra registration or invented
root proof is used.

Filtered author 20261114T220026Z-72774 exported four test files. Independent
review 20261114T220433Z-75766 found no P0, P1 or P2 issue. Initial focused
run 20261114T220611Z-77329 failed to compile because a helper was private to
its test module. Filtered correction 20261114T220716Z-78040 adds only the
same parent-module visibility as the existing helper; all other bytes were
compared and preserved. Corrected run 20261114T220837Z-78977 passed all
sixteen selected tests. Privacy 20261114T220841Z-79280 passed all four files.

Full FFI run 20261114T220942Z-79623 executed 390 tests: 386 passed, four
failed, none skipped. Every one of the 44 names in the earlier retained
failure list passed in this run. The four remaining failures are adoption
diagnostic error and cleanup cases. Their existing exact-result assertions
remain enabled. The full suite is failed, not accepted; those failures need
diagnosis. No retry, relaxed limit or quarantine was added.

Rust lint 20261114T221213Z-80071 passed all three checks. Similarity
20261114T221217Z-80348 scanned all four files with no findings. Only its
implementation report was read. The unit, CloudKit acceptance lanes and full batch gate
remain open.
