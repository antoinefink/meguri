# First-sync receipt references

Status: receipt validation WIP; U240, Q13 and first-sync acceptance remain open.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Spec-phase start: `288eca6c`. Implementation start: `817740f1`.
Source baseline: `d80773d8`.

Spec session: `s-20261101T234045Z-1a10cfde`.
Implementer session: `s-20261101T235351Z-264d9cd0`.
Fixer session: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-acceptance#Q13.1`.

The previous validator trusted copied receipt fields. The new path reads the
six exact declared MG receipts, checks their envelopes and source bindings,
and uses run definitions from the first-sync build's committed YAML. It rejects
path escapes, links, mismatched identities and copied fields that disagree.
Only the required declared evidence artifact is opened; optional logs are not
parsed as JSON. Historical YAML uses the existing PyYAML load phase. Evaluation
continues to use the host's standard Python library.

The actual lane evidence validators remain absent. Each missing validator
reports a failure and `evidence_validated:false`. A valid receipt reference or
artifact shape does not establish signed execution. No pending marker or
acceptance requirement is removed, and no unit is accepted by this change.

## Review and repair

Independent reviewer: `native_bounds_review`.
The test review found a P2: malformed signed-artifact cases could pass their
assertions through the generic missing-validator error. The repair requires
specific loader errors and observes actual loader/delegate calls. Positive
reference cases check identity and scope without producing a passing milestone.
The production review found no P0–P2 defects in the corrected candidate.

The first repair candidate used a separate historical YAML parser and opened
unrelated artifacts as JSON. It was rejected before testing. The corrected
candidate uses the existing YAML loader and the exact required declaration.
Filtered source: `cleanroom/20261102T002829Z-20993`.

## Verification

- Test-only RED: `selftest/20261102T000721Z-18213`, 171 tests, 93 assertion
  failures, zero errors. All failures were in the new receipt test class.
- Focused evaluator: `selftest/20261102T003601Z-21940`, pass.
- Gate tooling: `selftest/20261102T003934Z-22494`, 108 passed, zero failed or
  skipped.
- Similarity: `similarity/20261102T003939Z-22988`, pass; two source files
  scanned, two excluded inputs, zero findings.
- Privacy run `private/20261102T003936Z-22717`: fail, two source files.
  Filtered cleanup `cleanroom/20261102T004301Z-23768` removed redundant prose
  and renamed two test locals. Review found no behavior or assertion change.
  Final source scan `private/20261102T004428Z-24679`: pass, four files.
  Scanner rules and exceptions were not changed.
- Manifest validation: `milestone-gate/20261102T004429Z-24902`, pass.
- After the privacy cleanup, the focused evaluator and similarity checks passed
  again. Initial sandbox launches reported `docker-unreachable` before testing;
  the authorized Docker runs supply the results below.
- Final focused evaluator: `selftest/20261102T004455Z-25915`, pass.
- Final similarity: `similarity/20261102T004459Z-26172`, pass.
- Review-log privacy: `private/20261102T004516Z-26486`, pass. An earlier log
  scan failed on one newly written line; that prose was rephrased.

The shared full integration gate, actual lane producers and semantic evidence
validators remain required. U114's separate command-authority failure and
blocked-red escalation are unchanged by this tooling work. No VM or mobile
check was attempted while Antoine resizes the VM.
