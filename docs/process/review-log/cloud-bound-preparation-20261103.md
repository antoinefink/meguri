# U113 publication preparation

Status: WIP; no unit acceptance.
Start:e5cc84a1; source:ec3d7294.
Original U113:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T101859Z-043ac3fa.
Contract:V15.2 step3, V15.3, V2.4, P6.

Swift25331 reports runtimeTrackingFailed at Bound before any own operation.
The FFI preparation call tried to reconcile an unseeded publication binding.
V15.2 puts that scan at Zones. Existing reconciliation correctly refuses
new structural rows outside that phase.

Export24746 adds prepare_binding. The FFI delegates to it.

| State | Preparation |
| --- | --- |
| Current new publication binding, Bound | Settle intents; validate; defer scan |
| Zones and other cases | Existing reconciliation |

No early entities, coverage, engine or send admission. No native/Resume edits.

R1 independent review found no P0–P2s. Tests-only RED33537 compiled: two cases
failed with the runtime-tracking error; account mismatch passed. Caught
callback reentry probes also print expected panics, not the failed assertions.

New real FFI tests:

- publication_bound_preparation_defers_initial_scan_until_control_acks
- publication_bound_preparation_reopen_preserves_partial_control_acks
- publication_bound_preparation_does_not_bypass_account_mismatch

Coverage: initial/reopened Bound, partial ACKs, stale handles, wrong account,
SDK refusal, actual scan/WAL receipts, entity ACKs.
Unrun: outstanding-intent and mismatched-op cases through the new entry.
Device media and signed app checks await verification.

Rust/Swift results and full integration remain pending. No Q11/P24 item closes.

Rust34990:52 passed;2 failed at stored.whole_scan after successful preparation
and current-scan proof. R2 confirms that field marks clean drain, not current
scan completion. The repair must assert both stored drain flags remain false
and retain the scan, WAL, coverage, admission and ACK checks. Lint36485 passes.

| Receipt | Outcome |
| --- | --- |
| Export39326 | One test assertion |
| Rust44629 | Pass:54 cases |
| Swift39682 | Fail: initial adoption |
| Swift45964 | Fail: prior coordinator |

Saved SHA256 values verify exact coordinator restoration. Missing-file discovery
and earlier native findings remain open. No signed publication or gate pass.
