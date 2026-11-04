# Adoption capacity checks

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: 6eebd883.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Export 76603 adds four cases through the real CoreHost adoption path. A mutable
capacity provider belongs to one child device; its default delegates to statvfs.
It remains the same provider after reopen. No production or Host API changes.

Independent review finds one positive-case sequencing issue: background media
requires the floor plus its file bytes. Export 85464 holds the actual scan,
proves activation at exactly 512 MiB, then restores statvfs before media work.
All original convergence checks remain. No further P0–P2 issue is found.

Core 87942 finds a trait-object lifetime compile error in the assertion helper.
Export 88396 adds only the required static bound. Core 91817 passes all four
cases. The earlier failed receipt remains failed.

Private 92410 flags two comments and a test name. Export 93150 removes those
comments and renames the test. Core-format passes; Core 95984 passes four cases.
Private 95775 flags only the revised name. Export 97175 changes only that name;
all formatted test bodies and helper logic stay identical. Core 99962 passes
all four cases; Private 99888 passes all four source files. The separate child
entry is excluded by the selected filter, not counted as a passed case.

The cases cover floor refusal and exact-floor activation. They also exercise
a payload write, lower capacity after paging, abort cleanup and a new attempt
using statvfs.
The added sparse file belongs to the active test candidate.
The checks preserve old rows, media, authority and replacement evidence on
failure. No physical disk exhaustion, live-device success or unit acceptance
is claimed.

Core-lint 689 passes, including formatting, clippy and layering.
The required batch gate remains pending.
