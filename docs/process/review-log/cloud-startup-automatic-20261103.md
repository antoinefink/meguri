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

The rebuilt signed Mac check remains pending. These tests
prove Rust policy and FFI authority, not native lifecycle or cloud publication.
Other P24 requirements, real publication/adoption and full integration remain
open. U114's repair cap is unchanged.
