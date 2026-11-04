# Adoption filesystem work

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: 470a2205.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

The live phone made progress before its separate record-validation failure.
Source review finds whole-spool enumeration repeated during each row write.
Export 91364 adds owner-local byte accounting and real entry-visit tests.
Independent review clears ownership, replacement/orphan bytes, temporary write
space, fresh SQLite sizes, path checks and final full reconciliation.

The isolated baseline patch keeps the old algorithm and the same assertion.
Core 3609 fails that assertion with 3,360 visits at N and 12,864 at 2N. The
test counts actual filesystem entries, not elapsed time. Core 4117 compiles
the optimization but fails all eleven selected cases at initial ledger read.
Both failed receipts remain failed.

Export 7173 makes the existing staging constructor request owner-only mode
at directory creation and adds a real-constructor privacy test. The old
constructor did not request this mode. No chmod window or weaker ledger guard
is introduced. Review confirms the pinned tempfile implementation applies the
mode during creation. The original failing mode was not captured.

Core 10195 passes all sixteen selected ledger, capacity and epoch checks.
Core 11450 passes 107 adoption, publication, recovery and storage checks.
The linearity assertion now passes; device throughput remains unmeasured.
Every capacity read still uses fresh free-space data and root/SQLite sizes;
all known spool bytes and temporary writes count toward the same floor and
twice-size requirement. Failed publication invalidates the ledger, and final
reconciliation checks actual files before validation.

Private 11179 flags three comments and an assertion message. Export 12628
removes only those text fragments; the comparison operands remain unchanged.
Independent review confirms all other source bytes. The final source scan
and Core-lint 13692 pass. Native 14656 passes all 46 selected diagnostic and
probe tests against the rebuilt core. The signed phone run follows.
This change does not repair the observed record-validation failure or close
the batch gate.
