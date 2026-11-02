# Signed CloudKit agent contract

Status: protocol specified and reviewed; executable producer and signed runs
remain required. No U248, signed-lane or milestone acceptance.
Spec-phase start: `2f5021d3bf4a5f4c9874bbf84318485adcf6d08a`.
Session record: `c811cabf02489565d8dcefa8c685e8d57a1c047d`.
Spec writer: `s-20261102T011456Z-f4a4e630`.
Contract: `conformance/cloudkit-agent#J1` through `#J7`, supporting
`conformance/cloudkit-acceptance#Q7.3`.

The protocol defines ordered commands, durable started/result records, actual
runtime binding checks and recovery after an app kill. It uses the existing
MG envelope and retains the required real adapter, coordinator and signed
CloudKit execution. Protocol tests, smoke and selected-leg results cannot pass
the whole lane. No product policy or user setup authority changed.

Independent reviewer: `native_bounds_review`.
Round 1 found three P2s: account lookup ordering, startup and failure callback
records, and loss of complete journal records. The repair permits only account
reads before verified scope, starts the launch record before core/engine work,
retains failed-run drain observations, and checks the exact journal prefix
already retained by the coordinator. Round 2 found one retained test-table
sentence that conflicted with the account-read rule; it was corrected.
The final review found no P0–P2 findings.

Source privacy initially flagged newly written prose. The spec writer rephrased
those passages without changing the guard, vocabulary exemptions or behavior.
Spec privacy: `private/20261102T012643Z-40114`, pass for all three spec files.
The final log scan and static checks are retained under `build/mg`.
No test run, build, VM action, account setup or signed network trial is implied
by this documentation change.
