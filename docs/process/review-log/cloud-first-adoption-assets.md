# First-adoption payload assets

This slice starts at `4e8b18c675b6b75807a1bcf10454d484714b126c`.
The original batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Tests precede production changes:

- `core-test/20261031T173310Z-10642` reaches the real adoption refusal after
  the sender saves a payload asset. The live collection stays unchanged before
  activation. The test also requires exact rows, ACK, reopen and review entry.
- `swift-test/20261031T174512Z-15524` runs the native record callback with a
  real asset. It fails with `unsupportedAsset`, where acceptance must instead
  reach the existing missing-zone-reply check.
- Earlier Swift runs failed to compile an opaque SDK token or selected no test.
  Neither provides acceptance evidence. The corrected test uses no fabricated token.

Independent review found no required test fixes. Native file ownership after
SDK-source deletion still needs coverage when the new path is implemented.
All existing assertions remain. This is tests-only WIP; U113 and U114 remain
unaccepted, without a Unit trailer.

## Implementation and review

Implementation starts from `3eaaa849a64fb27d903831b52b47221d88e70e72`.
Native record projection owns copied payload files through the awaited Rust
handoff. Rust validates bytes in the unpublished stage and durably stores relative
file names. Existing inline stages remain readable. Control reads reject assets.

Independent Rust and native reviews found no required fixes after repair of one
page-capacity finding. The requested page holds at most two records, derived from
the existing file limits. Overflow fails before another copy and returns no cursor.
Cancellation and errors retain files until the original callback ends, then release them.

- `core-test/20261031T180349Z-20964`: 31 adoption and host guard tests pass,
  including actual payload replacement, ACK, reopen, review and source failures.
- `swift-test/20261031T181116Z-22955`: 27 connection and transport tests pass.
- `swift-test/20261031T181727Z-27843`: new test fails to compile; throwing
  file read is subsequently separated from the assertion macro.
- `swift-test/20261031T182118Z-31891`: 29 tests pass, including two real
  maximum-size files, overflow, cancellation, error and source-file removal.
- `swift-test/20261031T182306Z-34832`: all 178 affected tests pass across
  12 CloudKit and event suites.

`core-lint/20261031T182341Z-36336` and `similarity/20261031T182341Z-36028`
pass. The latter scans 1,524 files with no new flag. Privacy cleanup removes
two redundant test comment lines and rewrites one review-note paragraph;
it changes no assertion or guard.

The earlier sandbox-limited attempt built nothing. Native SDK continuation and
throughput, structural generations, remaining media paths and final gates stay open.
This is a completed slice of WIP, with no Unit acceptance.
