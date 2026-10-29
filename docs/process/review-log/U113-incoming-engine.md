# U113 incoming engine prerequisites

Status: enabled tests-first contracts; implementation and execution pending.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Phase start: `ac4b0eca786180bcfe29bb0765fe0e515d469f7d`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Source author `cleanroom/20261029T120616Z-34760` adds 20 deterministic
engine tests for CloudKit sources, stamps, cancellation, selective upload flags,
acknowledgements, targeted undo/redo cuts and retained journal images.
Independent R1 clears those cases and requests the M9.10 mixed-history property.
Fixer `cleanroom/20261029T122008Z-41264` adds 16 deterministic seeds with
1,376 real engine actions and an independent history model. Independent R2
finds no P0-P2 issues in the tests or model. Existing executable assertions
are preserved; custom rekey names and comments are changed mechanically.

- `core-format/20261029T122831Z-44469`: formatting run; see its receipt.
- `core-test/20261029T122846Z-44805`: expected compile RED on the missing
  CloudKit sources, hold/ACK APIs and detached-run result. Zero tests executed;
  this is not evidence that the runtime invariants pass.
- `private/20261029T122855Z-45192`: flags one new comment. Only that comment
  is deleted; independent R2 confirms equivalent executable tokens.
- Final privacy and similarity receipts are recorded below before commit.

The normal manual-transfer source remains separate and unchanged. The future
FFI cases must append tags 8 and 9 while retaining the existing tags 1 to 7.
Actual inbox application, durable intents, WAL/restart handling, media, native
integration and signed exchanges still remain. No Unit or P24 closure, full
integration gate, or working-sync claim is made by this tests-first checkpoint.

Privacy: `private/20261029T122923Z-45792`: pass.

Similarity: `similarity/20261029T122855Z-45126`: pass.

## Engine implementation R1

Producer `cleanroom/20261029T123136Z-46709` adds separate CloudKit sources,
entry-owned upload key maps, lazy held runs, targeted undo/redo cuts and the
cancellable catalog operation. It appends source variants after the existing
seven through the model, FFI and Swift wrapper. Proof tests remain unchanged.

- `core-test/20261029T123638Z-49021`: 79 executed, 78 pass, one failure;
  one existing test is excluded by the default profile. All 15 undo CloudKit
  cases, including the 1,376-action property, pass. The failure is the exact
  collection modification-time assertion in `cloud_op_sources`.
- `similarity/20261029T123652Z-49424`: 16 files, no findings or exemptions.
- `private/20261029T123651Z-49337`: one new AGENTS prose line flagged.
  Only that new prose line is removed; executable source is unchanged.

Independent production R1 requires three fixes: P1 incoming collection-modified
stamping on real writes; P2 queue rebuilds for both CloudKit sources; P2 the
new result field in an FFI test constructor. No other findings in flag ownership,
manual separation, lazy journals, prefix cuts or append-only source ordering.
The checkpoint is WIP and remains unaccepted. The fixer must keep the committed
source proof tests unchanged and add queue coverage before its production fix.

## Queue regression before R1 repair

Fixer `cleanroom/20261029T123932Z-51123` adds assertions to the existing queue
classification test. Independent review clears both CloudKit sources across
empty, note-only and flag-only changes, with and without explicit ownership.
The panic callback proves source classification precedes payload access.
Only the test portion is integrated before the three production repairs.

- `core-test/20261029T124221Z-52575`: one executed failure, expected
  `Rebuild` but received `None`; five other cases excluded by this exact filter.
- `private/20261029T124232Z-52980`: pass.
- `similarity/20261029T124232Z-52901`: pass, no findings or exemptions.

All prior assertions and CloudKit source proof tests stay unchanged.

## Verified R2 repair

Independent production R2 is clear of P0-P2 findings. The changed incoming
scope now advances `col.mod` through the normal rollback boundary only when
it made a real change. Compact modification tracking includes a same-clock
stamp. Supplied payload stamps and other collection sync stamps stay intact.
Both CloudKit sources rebuild the review queue before ownership and payload
shortcuts; the FFI test constructor carries the new optional result field.

- `core-test/20261029T124328Z-54151`: 108 focused engine/scheduler cases
  pass, one existing default-profile exclusion. Includes the failed timestamp
  case, queue regression, seeded CloudKit history and executor ordering.
- `core-test/20261029T124418Z-55194`: all 128 selected model, scheduler,
  engine and FFI library cases pass, no exclusions.
- `swift-test/20261029T124444Z-55580`: all 81 selected Swift cases pass.
  Debug Mac and simulator core slices build; bindings are regenerated. The
  generated source retains tags 1-7 and appends CloudIncoming/CloudRekey as
  8/9 for both encoding and decoding. No generated file is edited manually.
- `private/20261029T124343Z-54462`: all three repair paths pass.
- `similarity/20261029T124344Z-54645`: three paths, no findings or exemptions.
- `core-lint/20261029T124445Z-55996`: fails on the 18 existing missing-method
  and missing-Point diagnostics in incoming/media contract tests. No new
  engine/queue/FFI Clippy diagnostics; formatting, 17-crate layering and the
  feature-disabled object check pass. Missing behavior stays enabled and open.
- `doctor/20261029T124720Z-58183`: VM answers; host free space remains over
  the required floor. Manual account/device/reference evidence is still final.

This completes the focused engine prerequisite, not U113/U114 or working sync.
Actual inbox processing is next. It must persist apply intents and use the
operation result's seq. WAL coverage and restart recovery remain required.
The existing review, edit and media contracts stay enabled. Media, conflicts,
structural changes, deletions, native integration, signed exchanges and full
batch gates remain required. The original batch base is unchanged.

Final repair formatting: `core-format/20261029T124258Z-53624`: pass.
