# U114 policy fixtures, R17 — 2026-11-14

Starting commit: cd082f34. Integration base:
1416a520542a8c5daa38a23bfe398da704c3d6c9.
This repair changes positive test setup only; production code is unchanged.

Fifteen retained policy failures used a baseline whose epoch was unknown.
They expected collection writes or quiet-point proofs, which the approved
policy closes until a complete matching epoch has been accepted. The positive
callers now opt into the existing R15 helper before their measured work.
It takes the actual record through the original SDK and checks the resulting
facts. The default unknown-epoch setup remains unchanged.

The repair preserves every assertion and limit. Setup does not create a
quiet-point proof. Three pending-start cases still need valid setup before
the registration they measure; this patch leaves them unchanged, including
their requirement that no engine exists at that point.

Filtered author run 20261114T215051Z-62702 exported the candidate. Independent
review 20261114T215452Z-67527 found no P0, P1 or P2 issue. Focused core test
20261114T215539Z-67998 passed all fifteen selected cases. Rust lint
20261114T215632Z-69839 passed all three checks. Privacy
20261114T215542Z-68280 and similarity 20261114T215636Z-70254 passed for this
file and the separate presentation-test import correction. Only the safe
implementation similarity report was read. Static checks
20261114T215639Z-70650 also passed.

These results cover the fifteen selected cases, not the whole FFI suite.
The original failed receipts, the three pending-start cases, other fixture
families and required integration gates remain open. No unit or P24 closure
is claimed.
