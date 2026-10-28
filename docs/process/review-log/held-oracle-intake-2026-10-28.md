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
