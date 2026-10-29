# U113 unbound publication driver

Status: tests-first contracts are enabled and RED; production remains pending.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Phase start: `af2cee18745c64d2cf36ef1dfbf5c7ede88428b0`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Producer `cleanroom/20261029T073650Z-86717` exports a real unbound child driver,
shared SDK scheduling and generation-specific restore receipts. Its two new
contract files are extracted before production. Independent R1 clears the tests:
unbound discovery draws no keys or baseline; the second failed key draw leaves
no publication; automatic publication does not change the saved preference;
prepared and submitted Active differ durably; retained package rows stay
independent of live rows; restore receipts belong to the actual generation,
including successive restores and missing provenance.

- `core-format/20261029T080043Z-8465`: pass.
- `core-test/20261029T080114Z-8955`: expected compile RED on missing
  `PublicationRig`, `Point` and `replacement_evidence_for`. No case is counted
  as passing. Both new targets are selected.
- `private/20261029T080115Z-9210`: first run fails on custom test identifiers.
  Fixer `cleanroom/20261029T080301Z-10482` changes only a test name and a local
  variable. Independent equivalence review is clear; assertions stay enabled.
- `private/20261029T080415Z-11432`: final two files pass.
- `similarity/20261029T080415Z-11231`: two files, no findings or exemptions.

The original nine publication contracts are unchanged and enabled. Production
review has identified a held transport/close defect to repair before integration.
Adoption, nonempty media and intent execution remain missing in the exported
phase. No delivery, Unit, P24 or integration-gate acceptance is claimed.
