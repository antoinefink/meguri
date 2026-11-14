# Sync stabilization extension — 2026-11-14

Antoine approved the proposed bounded scope:

> I approve carr on with with your best judgment

Allow at most two more repair/review rounds for U113 and two for U114.
Do not reset prior counts. Source before approval: 03372c03.
Gate base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.

U113 covers only durable native transition proof for the P1 from review
cleanroom/20261114T064349Z-81477. A native edit can commit before its observer
saves the transition evidence. A new process cannot use the lost in-memory
report. Tie retained proof to the exact file, account binding and old/new digests.
Cover a failed tracking write and process exit, later edits, Undo and Redo.
Keep broad generation work out. Missing contracts require specs.

U114 covers only the eleven failing functions from
swift-test/20261114T074553Z-36446. Establish causes before repairs; fix test
isolation only with evidence. Keep every assertion. That run passes 143/154
functions with 30 issues; the original paired cancellation, five dependent-fetch
and four import-drain cases pass. Rust run core-test/20261114T074355Z-32198
passes 307 cases. Diagnostic swift-test/20261114T075600Z-46766 fails all six
selected adoption functions. It proves no cause or repair. Keep all receipts.

Use the approved contracts and filtered author plus independent reviewer workflow.
Keep every time and memory bound, failed run and escalation. Keep file/account/
generation checks, callback identity and order, real errors, and WAL-before-ACK.
Add no retries or privacy exceptions. Add no Unit trailer or acceptance claim.
At the new limit, report what remains and stop further source repair.

Run focused checks before integration. Both units still need their full gates,
P24 evidence and final acceptance. Server work stays frozen.

The separate generated-data trial still needs its exact replacement/export approval and the phone. This request grants no data replacement, export or account setup. Server work stays frozen.
