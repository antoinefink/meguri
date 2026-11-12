# Dependent fetch refusal repair — 2026-11-12

Role: fixer, implementer view. Clean sourcebase: `1604ae40`.
Seed baseline: `5042e3a247a7ba45c535176dd0764dedefe32931`.
Current filtered runner base: `9c4517ab0eec0ea830ee57801047aafaa4695b65`.
Scope: RequestFetch, StopEngine, retained errors and input ACKs.
New cases cover a failed stop followed by a completed stop.
App five-case setup/assertions and unrelated failures remain unchanged.

Previous receipts:
- `swift-test/20261112T113618Z-49052`: failed.
- `swift-test/20261112T115758Z-83006`: failed43.
- `cleanroom/20261112T115753Z-82643`: P2 repair requested.
- `swift-test/20261112T115919Z-84328`: app selection failed.

Source consistency: one diff/seed-preservation check. No native builds or tests
run here. Integrator compilation, focused tests, independent review and
provenance/privacy/similarity checks remain pending. No unit or full-gate claim.


Integration: `cleanroom/20261112T120837Z-90908` found no new P0-P2 issue.
`swift-test/20261112T120842Z-91597` ran3 groups with all assertions passing;
MG remains failed for a compiler warning at test line1744. The optional storage
context makes this require expression redundant. Bind its optional input and
required value before assignment; keep the required foreign-command check.
No broad pass claimed. Remaining app errors stay open.


## Final checks for this repair

- `cleanroom/20261112T121508Z-15706`: no P0/P1 or P2 finding in the typed require fix.
- `swift-test/20261112T121513Z-16106`: PASS3 groups,14 cases, no compiler warning.
- `swift-test/20261112T121622Z-18269`: FAIL;8 groups pass, the existing five-case
  group remains red. Account cases1/2 now retain only A cancellation; B stale
  refusal is qualified. Deadline/replacement/reopen failures remain enabled.
- `similarity/20261112T121626Z-19417`: PASS for all8 changed Swift files.

The earlier Core91597 receipt stays failed. No test limit, required check,
assertion, originalGuardRefusal rule or private-data guard was disabled. The
package AGENTS note records the compiler trap. These checks close B's repair,
not the full unit or integration batch. Base1416 remains in force for that gate.
