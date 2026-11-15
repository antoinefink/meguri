# Adoption waiting for review completion

Started from `20cddcab4b269f06b3cae68219755406f8398bb3`; integrated on `0746512b705b870979a6e1701bb5f3dbf4ecced8`. This is additive U161/U114/U113 work, not completion of those units. The pending full-gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The status now distinguishes a live Ready adoption whose pending activation is held by an active review. It displays the approved T9.1 explanation and Cancel, then continues through the existing session-end path. Resume remains for transport waits. Recovered Cancel-only adoption retains precedence. Publication behavior and execution authority are unchanged.

The core reads the review state outside Runtime, store and owner locks. The status is descriptive; execution still validates the current operation. The typed fact crosses FFI and CoreBridge into the existing native status controls. No layout or style override was added.

Author: `cleanroom/20261115T140903Z-49790`. Independent review: `cleanroom/20261115T141635Z-58280`, no P0–P2 findings; source and native-control usage only. The integrator read the full ten-file patch.

Verification:

- `swift-test/20261115T142003Z-62269`: assertion-only baseline RED. The old presentation offers Resume when transportWait is false.
- `core-test/20261115T142128Z-64986`: PASS, two real adoption workflow tests. They cover exact pending activation, session end, renewed review, Cancel and transport wait. Excluded tests are not passes.
- `swift-test/20261115T142207Z-65370`: PASS, the presentation regression with resolved English catalog text and invalid-flag precedence. Native bindings regenerated.
- `swift-test/20261115T142357Z-69529`: PASS, two original-job ownership tests, including both payload-lease cases and explicit Resume drain.
- `core-lint/20261115T142442Z-71113`: PASS, correctness, layering and release-name checks.
- `private/20261115T142300Z-67447`: PASS, ten source files.
- `similarity/20261115T142303Z-67747`: PASS, ten files, zero flags; filtered implementer view read. Earlier malformed file-list invocations scanned no valid source set and do not count.

Native rendering, actual Apple peer review exchange, integration gates and final acceptance remain required. No Unit trailer or signed-pass claim.
