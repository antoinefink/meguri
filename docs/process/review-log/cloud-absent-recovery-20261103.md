# U113 absent recovery discovery

Status: WIP; no unit acceptance.
Start:53ea0f35; source:f5abf023.
Original U113:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T104237Z-a8c7fb0a.

Recovery discovery now distinguishes absent tracking from a failed read.
Both operation queries use the same accessor. Without an installed store,
only the live NoCollection owner and exact NotFound result permit None.
Present entries remain errors. Reads create no collection, sidecar or store.
Populated-store queries and FFI generation checks retain their behavior.

R1 and R2 reviews are clear of P0–P2 findings. Export56703 supplies the change.
Compiler correction65687 uses storage's existing SQLite reexport. No dependency
or native/Resume change was made.

| Receipt | Result |
| --- | --- |
| core-test64154/core-lint64428 | Test import compile error; retained |
| core-test66810 | Tests-only RED:2 failed,2 passed |
| core-test68045 |29 connection cases passed |
| core-lint69826 |4 checks passed |
| private68418 |3 files passed |
| similarity70116 |3 files passed; no flags/exemptions |
| swift-test70049 |20/21 test functions passed; older Resume case failed |

Swift evidence includes retained startup/unpause, fresh adoption with a review,
and coordinator/presentation cases. Prior-coordinator45964 also fails the older
Resume assertions. Preserve that failure and U114's cap. No failure is waived.

Unrun: metadata I/O faults, older-format migration sentinel, added cross-scope
cases, signed adoption and complete media convergence. Full gate remains open.
