# K10 row tracking: WIP checkpoint

The two approved extra rounds are used. R8 retains one P1 for recovery after
a native edit commits but its tracking write fails. U113 stays unaccepted.

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Unit starting commit: `ca1a8aff4dbfdec6de63f1be29d69c535e37539c`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Root `de5d8f67` added normal-device media evidence without changing that code.

## Earlier checkpoint

`core-test/20261114T022826Z-67905` passes all 232 cases, with 308 filtered.
All nine original failing cases pass under their original names. The collision
case now applies and acknowledges the received type after moving the local one.
Both malformed-ordinal cases, the real 150k callback case, large rekey and all
three payload-memory cases pass with unchanged limits. The unit is not accepted.

Ordinary types use counters from retained payloads and validated incoming
images in durable intents. Part merges, conflict copies, unique names, dependent
rows and reopen now have real host coverage. The standalone generation ledger
is unchanged. Structural changes still wait; no unknown counter is seeded.

Lint `core-lint/20261114T023002Z-68357` passes, including layering and
feature-boundary checks. No lint rule or limit was suppressed.

Privacy `private/20261114T013744Z-55749` passes all 32 paths. Static checks
`20261114T013745Z-55949` pass. Similarity `20261114T014112Z-56840` passes
all 29 source files, skips 3 non-source files and uses no allowlist entry.
The ordinary-type follow-up passes privacy on all nine source paths in
`private/20261114T022628Z-66796` and similarity in
`20261114T023007Z-68634`, with no allowlist entry. This document and the
commit message require a fresh scan.

## Source and review

The producer records bad or oversized rows without inventing payloads. Failure
changes carry their own revisions and sequence numbers, remain in original
exchange accounting, block dependent sends and survive reopen. Accepted repair
or deletion clears them; an older ACK cannot clear a newer failure.

RuntimeV8 installs the classification tables and scan evidence transactionally.
Selected reads check their state and relevant coverage; full scoped semantic
validation remains at private admission and migration. Failure lookups use
indexes and keyed checks. Legacy absence stays unknown. Global attention
completeness remains false.

The observer uses actual journal presence metadata to avoid loading duplicate
large images. Protected writes retain paired-intent layout and digest checks.
Raw identity paging preserves malformed stored ordinal classes and bytes.

| Round | Independent receipt | Disposition |
| --- | --- | --- |
| 1 | `20261113T232901Z-39020` | Failure metadata, read cost and coverage findings repaired |
| 2 | `20261114T000723Z-42659` | Protected projection, scope and raw ordinal findings repaired |
| 3 | `20261114T003211Z-46246` | Compilation, routine read cost and refusal-boundary findings repaired |
| 4 | `20261114T005125Z-49063` | No new P0/P1 in its delta; lower findings retained |
| 5 | `20261114T011556Z-52353` | No new P0/P1 in memory delta; incomplete notetype path remains P1 |
| 6 | `20261114T022810Z-67438` | Two P1 findings: authority after a later edit, and unknown layout origins |
| 7 | `cleanroom20261114T062727Z-72443` | Original two P1s repaired; one new P1: native Undo/Redo loses the checked identity report |

The ordinary-type continuation starts at `7d43c5e7`. Independent round 6 reads
export `cleanroom/20261114T022244Z-65512`; its own export is byte-identical.
It finds no P0 and two P1s. A later ordinary edit before a new type's apply
settles can leave replay without layout authority. An external reorder of a
type without optional IDs can be mistaken for an ordinary edit and retain the
old counter. Both require fixes; passing tests do not close them. The original
unit, integration base and review cap remain unchanged. A blocked-red escalation
records the findings; two further focused rounds await Antoine's decision.

Initial focused run `20261114T021932Z-64427` passed 7/8; expanded run
`20261114T022038Z-65061` passed 231/232. The new card fixture copied a source
card's history value to a new ID without copying its revlogs. The correction
proves the new ID has no card or history rows, then derives its history value
from the canonical query. Every full equality, WAL, ACK and reopen assertion
remains. Focused run `20261114T022734Z-67115` passes 8/8, followed by the
current 232-case pass. The original collision fixture is unchanged.

Compile failure `20261114T022620Z-66470` executed no tests: an incremental
object was missing. Only that package's affected cache directory was retained
outside the active cache before the passing run. Privacy flagged one redundant
helper comment; removing the whole comment changed no code or guard.

Cleanup export `cleanroom/20261114T013112Z-54725` changes two complete test
interfaces, two comments and 12 mechanical lint forms. Root checks that reversing
the identifier mapping makes the collision fixture byte-identical. It still has
one test; all inputs, assertions and attributes are unchanged. The complete name
mapping stays in local ignored evidence. The fresh 225-case run retains the
same 224 passes and same failure. Earlier behavioral reviews cover unchanged
logic; cleanup was checked by root and is not a new independent acceptance round.

## Preserved failures and follow-up

Retain compilation failures `20261113T233236Z-39654` and
`20261114T003228Z-46732`; neither executed assertions. The first focused run
`20261114T000740Z-43161` passed 49/58. Expanded run
`20261114T005208Z-49553` passed 220/222 and exposed the rekey heap regression:
503966232 bytes against 300000000. Unchanged-master baseline
`20261114T010418Z-51560` passes the same limit. Repair execution
`20261114T011618Z-52854` and the current run pass it; no budget changed.
Earlier privacy failures remain recorded; final repair uses no guard exception.

Deferred P2: explicit positive selected-control status/provenance/digest;
complete index definitions; extra collision send/intent-content assertions;
mixed malformed ordinals across a page boundary.

Other open requirements include K19 generation claims and remote progress,
initial bad-notetype dependencies, structural/legacy removals, tolerated Config
values, the J6 integration audit and other attention producers. Actual historical
V7-reader execution against a valid V8 file remains outstanding. The independent
native quiet-point failure stays enabled. Normal-device media transfer evidence
is separate from this producer and does not complete the two-way review trial.

The signed Mac preview builds at `edd07eed` in
`build-app/20261114T023144Z-69984`. A normal quit and reopen of that exact
product retains 4 notes, 4 cards, 4 reviews and 4 media files. The local
replacement preview shows those counts and is cancelled without replacement.
Sync Now returns to Unknown. This is a native reopen check, not a new
two-way review trial; the phone still has the earlier verified build.

This commit is WIP: no Unit trailer, P24 closure, full gate, milestone, signed
lane or release acceptance. Required verification still uses the original
integration base.

## Final additional repair candidate for independent R8

Session `s-20261110T214025Z-20fa15fd`, filtered source `889384d3`, with
the original R7 own-code seed. This is the second and last additional repair
round authorized by `reviews-cloud-two-round-approval-20261114.md`. The six
earlier rounds, R7, original unit base `ca1a8aff` and integration base
`1416a520542a8c5daa38a23bfe398da704c3d6c9` remain in force. No cap resets.

Root's R7 regression run against old production,
`core-test20261114T062830Z-73018`, retains 2 passes and 3 failures across
5 cases (later edit, crash/reopen and external ID-less reorder failed).
The incoming module on the R7 candidate passes 12/12 in
`20261114T062944Z-73448`. Expanded storage/rekey/review/media/intents/recovery/
memory coverage passes 237/237 in `20261114T063059Z-74207`, including the
unchanged real 150k callback and 300000000-byte heap limit. These receipts
close neither native/signed verification nor the full gate.

R7 independent review found no P0 and one P1: a native save's journal keeps
checked identity maps, but `Entry::restore` previously restored raw rows without
carrying that witness into the Undo commit. Tracking could remain on the saved
image while the collection held its predecessor; reconcile and reopen refused.
The candidate captures admission for only the step's retained identity reports
before restoration, and completes them only after all row/collection restoration
succeeds. Identity maps invert to themselves. The restoring journal therefore
keeps the same proof for Redo. Earlier unreported writes still deny admission,
later raw writes still invalidate reports, and savepoints still roll back the
maps. The token API and all native callers were re-authored together. No layout
authority is inferred from optional IDs or name equality, and no counter is
created or advanced by this transfer.

The old-image reader now combines up to 250 journal rows with small live
pages. It keeps the old row order, removes new entries and restores deleted
ones. It no longer loads the full remaining aggregate at each call. Walking
journal keys for each page remains a P2 follow-up, along with earlier P2s.

Added selected tests, for root to execute:

- `incoming_notetype::ordinary_notetype_published_name_css_save_undo_redo_retains_counter_base_and_ack`
- `incoming_notetype::ordinary_notetype_published_legacy_name_swaps_undo_redo_reopen_keep_dependents`
- `incoming_notetype::ordinary_notetype_multipage_css_save_undo_redo_restores_complete_aggregate`
- `incoming_notetype::ordinary_notetype_multipage_name_swaps_undo_redo_restore_complete_aggregate`
- Storage `journal_images::checked_ordinal_reports_follow_savepoints_and_raw_write_invalidation`

The published-type cases exercise actual save, Undo and Redo, reconcile while
undone, and cold reopen after either the final Redo or a final Undo. They compare
the complete current aggregate, the original counter, exact remote base and ACK
fields, and dependent note/card images. Automatic WAL flush is paused around
each operation; an immediate coverage query must time out before an explicit
flush yields a matching file/seq receipt. Multi-page cases use a valid type
with 501 fields and 251 templates, including name swaps across page boundaries,
without adding timing or heap budgets. The storage case preserves raw-write
invalidation and both savepoint rollback directions.

The report API, its callers and the fixture helpers were rewritten together.
Every existing assertion and limit stays in place. No guard exception is used.
Fresh privacy and similarity results are required before committing.

This filtered repair executes no builds or tests and makes no commit. Root owns
focused and expanded verification, independent R8, scans and remaining gates.
The one source consistency pass completed: `git diff --check` passed; report
callers, restoration, paging and the added test paths were inspected together.
There is no acceptance claim. U114 quiet metadata/policy and Swift cancellation
are untouched. Original file/binding/key/digest/inbox checks, later-content
preservation, apply/WAL/ACK order, global `attention_known = false` and unfinished
K19 generation work remain unchanged. Any unresolved required repair after R8
needs another decision.

## Extension verification and remaining finding

R7 review `cleanroom/20261114T062727Z-72443` found the Undo/Redo report
transfer gap. R8 review `cleanroom/20261114T064349Z-81477` finds no P0,
but retains one P1: a native edit can commit while its observer tracking write
fails. The current journal report is temporary. On reopen, neither the old
tracked image nor an incoming intent proves the edited layout. Recovery needs
durable transition evidence tied to the file, binding and digests. Optional IDs
or names cannot supply that proof. No further behavioral repair is attempted
under the exhausted extension. Two P2s also remain: malformed-ordinal error
classification and journal-key scans during old-image paging.

The original R8 build `core-test/20261114T065047Z-87265` executes no tests.
Its new storage test called a nonexistent aggregate conversion. Export
`cleanroom/20261114T065416Z-88114` replaces those two comparisons with
complete stored head/field/template snapshots captured before each change.
All other production bytes and assertions stay unchanged.

`core-test/20261114T070151Z-92689` runs 307 checks: 303 pass and four
new fixture cases fail. The storage/journal, existing Undo, row/rekey, review,
media, recovery, real 150k and heap checks pass with their original limits.
Two new multi-page fixtures have duplicate template fronts; two reopened
positive cases omit the required fresh account read. These setup failures are
not product-error evidence. Retain that failed run as failed.

Fixture-only export `cleanroom/20261114T070644Z-93545` supplies unique valid
fronts for all 251 templates and establishes fresh synthetic account proof via
the real read/reply API. It keeps 501 fields, absent IDs, all assertions and
limits. The external-reorder fixture now proves the account gate is open before
testing its layout refusal. Root confirms that no production file changes.
`core-test/20261114T070909Z-95320` passes all five affected cases, including
the four corrected cases and the stronger external-reorder case. The earlier
303 passing checks cover unchanged inputs; no single fresh 307-case pass is
claimed. The two multi-page failures on the earlier R7 baseline likewise do
not count as behavioral red evidence because their fixture was invalid.

Privacy `private/20261114T070154Z-93002` passes all 16 applied paths.
`private/20261114T070917Z-95964` refreshes the sole changed fixture file.
Similarity `20261114T070913Z-95622` passes 13 source paths, no flags or
exceptions. Cleanup `cleanroom/20261114T064653Z-83418` removes one complete
redundant comment; all other file deltas match the reviewed candidate. Root
rewrites two evidence paragraphs without changing the recorded facts.

The first pre-integration scan at 07:01 preceded application because a patch
header comparison stopped early; it is not credited as candidate coverage.
The actual-source comparison and applied-path scan above replace it.

This remains WIP. No Unit trailer, P24 closure, full-gate or milestone pass,
signed SDK acceptance, real peer-delivery proof or release acceptance. Keep
the original integration base and every earlier failure. The verified U114
activation-order repair is separate; its result does not close this P1.
