# Prepared oracle batches 3 and 4

Starting commit: `346302b38115c567ca6dd892f3348b4671c9e8a3`.
Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Prepared top: `oracle/batch-4`, `56d35f38a8f8c897d2ae1700ddacb8607e5529ce`.
It includes the rebased batch 3; do not merge the separate batch 3 ref.

## Runner prerequisites

Filtered author `cleanroom/20261028T054907Z-16406` adds presence-aware
field IO options, separate legacy/latest export forms, seven observation
kinds, typed stats lists, recursive search requests and the precise Card Info
comparison paths. The CLI language contract follows all 64 observations;
the 87 step names are unchanged. No execution capability is activated.
The two schema files reuse the exact prepared top, rather than a second
reconstruction. Collection-dependent adapters remain pending.

Independent review 1 found schema metadata decoded through authored D2
limits, an empty-name validation gap and a partial schema-test helper.
Filtered repair `cleanroom/20261028T061029Z-27578` uses strict generic
metadata decoding, rejects the empty name and makes unsupported schema
assertions fail closed. Review 2 is clear. Two old fixture expectations
then failed against the canonical schema. Repair
`cleanroom/20261028T061651Z-36553` keeps empty stats maps omitted and reads
the nullable tag branch correctly. Independent review 3 is clear.
The authored input limits, other negative assertions and pending adapter
behavior remain checked.

- `core-test/20261028T060333Z-24479`: 121 passed, eight metadata failures.
- `core-test/20261028T061454Z-33688`: 129 passed, two fixture failures.
- `core-test/20261028T061834Z-38208`: all 131 passed, zero skipped.
- `private/20261028T060406Z-24936`: one new comment flagged; rephrased.
- `private/20261028T061457Z-34008`: all 17 inputs clear; final test-only
  changes clear in `private/20261028T061851Z-38636`.
- `similarity/20261028T060409Z-25225`: initial source clear; repairs clear
  in `20261028T061459Z-34329` and `20261028T061852Z-38843`.
- Formatting ran through MG before verification and final sealing.

Prepared-case parsing, merge, golden checks and dual-lane T0 remain open
at this commit. Whole-target lint still awaits the previously enabled
account harness; no unit, full batch or milestone gate is accepted here.

## Retained work

The six `sync/media/tracking` cases from `8e5cedc5` still lack driver support
and remain open and inactive. No frozen server work is authorized by this
intake. P24, O19, the R1881 data findings and the final acceptance gates
remain open. Batch 4 spec corrections have a separately reviewed proposal;
the four K7 recordings still require oracle-maintainer work.

## Prepared merge and spec follow-ups

`conformance/20261028T062006Z-39976` parsed all 89 affected prepared
cases before the merge: 89 pending, zero invalid, zero T0 failures.
Pending is not product conformance. `c5926bb3` merges only the prepared
batch 4 top and preserves all twelve commits. The sole conflict was
package vocabulary documentation: both prepared oracle coverage and
the verified typed-language availability were retained. D7.2 now agrees.

The spec-writer proposal based on the supplied handoff corrects search
parse-error order, recursive item details, saved names versus escaping,
and the filtered-home-deck Card Info failure. Independent review is clear.
No new oracle observation is claimed. K7 now names the four recordable
cases while keeping their recordings and goldens open. The typed Meguri
error remains unchanged. P24, O19 and all other retained gates stay open.

Golden checks, dual-lane T0 and the new-input R1881 recheck follow.

## Post-merge verification

- `goldens-check/20261028T062214Z-41651`: all 1,002 scenarios fresh;
  2,124 schema inputs checked with zero errors.
- `conformance/20261028T062248Z-42326`: **failed**. Docker has zero
  required-case failures and 638 passing product cases. The native testkit
  suite found the package inventory count still at 70 after 14 new package
  cases landed. Native `sched/day/config-values` also failed at step 15
  closing the collection (`suspend deadline exceeded: completion`).
  That native timeout remains unresolved; no full T0 pass is claimed.
- Filtered repair `cleanroom/20261028T063451Z-60722` changes only the exact
  package count to 84. Every file ID/hash assertion remains. Independent
  review 4 is clear.
- `core-test/20261028T064020Z-64392`: all 60 scenario-format checks and
  ten separate CloudKit host checks passed, zero skipped. This closes the
  count failure, not the native scheduling failure.
- `private/20261028T064024Z-64694` and
  `similarity/20261028T064024Z-64699`: pass for the final source.

The expanded search scan `similarity/20261028T062248Z-42310` fails with
four flagged files, including one newly landed generated builder result.
Its complete disposition is retained in the search-data review log.
No guard exception or unit acceptance was introduced. Focus the native
closing failure before running the next complete T0; preserve the failed
receipt and unchanged timeout.

## Native diagnosis and K7 recordings

`conformance/20261028T064413Z-68158` passes only the exact selected
scheduling case (`--lane native --jobs 1 sched/day/config-values`).
Correction: filtered runs omit native testkit-unit checks; the earlier
wording overstated that receipt. `--jobs` controls the live lane, not T0.
The full-workload timeout from `20261028T062248Z-42326` remains retained;
this focused run does not establish its cause or replace full T0.

The spec writer supplied the four missing K7 cases from K4/D7. Starting
commit `d5babca7` is recorded in `oracle-batch4/k7-start.json` under the
unit intake directory. Independent scenario review is clear. All 23
recorded values agree with the spec-derived packet; SQL result rows
remain empty, and no real IDs occur in observations. The fourth case
proves backend default reads, not Add-screen selection retention.

- `oracle/20261028T065613Z-76503`: all four recorded using the existing
  driver, with no driver or server changes.
- `oracle/20261028T070035Z-79062`: all four repeat comparisons pass.
- `conformance/20261028T070035Z-79034`: four parse and remain pending,
  zero T0 failures. No missing adapter was activated.
- `private/20261028T070106Z-80325`: two invented deck names flagged.
  Only those source strings were replaced; no golden was edited by hand.
- `oracle/20261028T070325Z-81422`: affected case re-recorded; independent
  equivalence/value review is clear.
- `oracle/20261028T070434Z-82756`: renamed case repeat comparison passes.
- `goldens-check/20261028T070432Z-82470`: all 1,006 scenarios fresh;
  2,133 schema inputs checked, zero errors.
- `private/20261028T070435Z-82957`: all nine scenario/golden inputs clear.

This closes the four K7 recording follow-ups. Their Meguri adapters and
UI evidence remain pending. P24, O19, four R1881 flags and the next full
T0/integration gate stay open. No Unit trailer or release acceptance.

## Second full workload

`conformance/20261028T070615Z-84638` fails only native
`sched/day/config-values`, now at closing step 43 with the same suspend
completion deadline. The native unit suite passes, including the corrected
package count. Docker again has zero required-case failures and 638 product
passes. No third unchanged full run was started: source analysis found
cross-directory sidecar serialization, recorded and repaired in the
native-close-sidecar review log. Full verification of that actual change
follows its passing seven storage and 32 native scenario checks.

## Full-workload result after the repair

`conformance/20261028T091118Z-43499` passes complete dual-lane T0 on
clean commit `11c2f23d`: native unit checks pass, all 1,006 scenarios are
considered on both lanes and there are zero required-case failures.
Docker has 638 product passes; native has 637. Inactive errors/failures,
live cases, pending adapters and reference cases retain their classifications;
they are not product passes. The earlier two failed receipts remain intact.

This verifies the sidecar repair against the full workload with unchanged
deadlines, sync flags and normal worker policy. It is not a full integration
or milestone receipt. The original batch base, missing CloudKit workflow
helpers, R1881 findings, six media-driver gaps and other open gates remain.

## Mechanical lint closure after Cloud integration

The all-target lint check reached style diagnostics in the prepared runner
work. Filtered repairs `cleanroom/20261028T102936Z-86793` and
`cleanroom/20261028T103153Z-88794` preserve capability branch priority, strict
recursive schema validation, unknown-keyword rejection and every assertion.
Independent delta reviews are clear. The single-input runner slice now uses
`from_ref`; no fixture, bound, pending classification or expected output changes.

`private/20261028T103048Z-87875` and
`similarity/20261028T103049Z-88104` pass for the model delta.
`private/20261028T103315Z-89662` and
`similarity/20261028T103316Z-90092` pass for the test-helper delta.
`core-test/20261028T103318Z-90400` passes all 38 affected tests, zero skipped.
The initial final lint run (`103316Z-89888`) retained only a formatter failure;
canonical formatting `103348Z-91076` resolves it. Full
`core-lint/20261028T103402Z-91405` passes all four checks.
This adds no adapter capability or unit acceptance.
