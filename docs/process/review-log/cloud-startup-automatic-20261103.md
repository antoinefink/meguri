# U113 startup automatic preference

Status: component WIP; no unit acceptance.
Component start:355e67d5246b98120f8423ff9299cae63465f3a6.
Original U113 start:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Integration base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer session:s-20261103T092052Z-6d1c3590.

The signed Mac caller trace identified ConfigureAutomatic reaching runtime
preparation before account verification. Restoring automatic-on emitted this
action despite the closed policy gate. P10.1 requires an open gate.

The Rust transition now retains the preference without emitting automatic-on
work while closed. Admission preserves open-gate scheduling intent, including
an original SDK that is stopping. Closed-gate off configuration requires a
current non-stopping SDK during a scan or ordinary intent hold. Original guards
are checked first. No native adapter, Resume or SDK ownership code changed.

Filtered export37275 supplied the initial implementation and tests. Root first
applied only its test files. RED core-test47381 compiled and ran10 cases:
5 failed at policy assertions and5 passed. Queue callback reentry probes also
printed their expected caught panics; those were not the failing assertions.

Independent R1 found one P2: changing Automatic after reopening but before the
old SDK released could discard the successor intent. Filtered repair52038
admits that open-gate intent. The real Host/FFI test rejects the stale start,
admits the latest configuration, refuses early registration, then registers
once with the latest preference after release. R2 found no P0–P2 findings.

| Check | Evidence |
| --- | --- |
| Focused policy model and FFI suites | core-test56729:50 passed;510 filtered |
| Privacy | private57271:5 files; no matches |
| Rust lint | core-lint57772:4 checks passed |
| Similarity | similarity57801:5 files; no flags or exemptions |
| Static integration | static-checks58885:3 checks passed |

These tests
prove Rust policy and FFI authority, not native lifecycle or cloud publication.
Other P24 requirements, real publication/adoption and full integration remain
open. U114's repair cap is unchanged.

## Signed Mac check

Team build91867 passed on cleanb1a62f3e. The freshly launched process used the
exact standalone Debug product. Executable SHA256:
06c002699873c354b8a88791e90da0925d18a804c1d12ff8b32d391ff1d40c75.

Settings shows a signed-in account, Offline and a generic failure. The retained
operation's Resume control is absent. No publication completion or remote
round trip is proved. Independent source audit identifies missing U151
attachment discovery: startup never loads recoveryStatus. V15.3 requires restart
to continue an active publication after matching account proof.

A later startup probe stopped at an ambiguous inline catch location. Its
expression failed and restored the process; all breakpoints were removed and
the debugger detached. It supplies no new error classification. The ordinary
app reopened its collection and remains running. No user values were exported.
