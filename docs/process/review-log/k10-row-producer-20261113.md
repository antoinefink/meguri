# K10 row tracking: WIP checkpoint

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Unit starting commit: `ca1a8aff4dbfdec6de63f1be29d69c535e37539c`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Root `de5d8f67` added normal-device media evidence without changing that code.

## Current result

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

The ordinary-type continuation starts at `7d43c5e7`. Independent round 6 is
running on export `cleanroom/20261114T022244Z-65512`; no review pass is claimed.
The original unit and integration bases above remain unchanged.

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

The signed Mac preview builds at `67ff77b9` in
`build-app/20261114T015345Z-58539`. A normal quit and reopen of that exact
product retains 4 notes, 4 cards, 4 reviews and 4 media files. The local
replacement preview shows those counts and is cancelled without replacement.
The sync status remains Unknown. This is a native reopen check, not a new
two-way review trial; the phone still has the earlier verified build.

This commit is WIP: no Unit trailer, P24 closure, full gate, milestone, signed
lane or release acceptance. Required verification still uses the original
integration base.
