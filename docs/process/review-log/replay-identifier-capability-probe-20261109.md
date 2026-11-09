# Replay identifier capability probe

Status: experiment; native attempt not established. Start: `fa203c29`.
Session: `s-20261103T133750Z-f276f58e`.
Spec: render/display-transform D4, R1964, R1965.

Phone `63748` completes 13 functional steps. Its replay links have names,
but no native IDs. Package `58335` passes localization and finds no links
with its in-process tree walker. Neither result is waived.

Export `65823` adds one synthetic test-hooks route. Review finds two P2
issues; export `70089` closes both. Admission follows the current-page and
geometry checks, just before the native hit. The existing Probe action is
disabled after admission. Both leaf branches require the public link trait.

Only the documented UIAccessibilityIdentification setter is attempted, on
an existing native leaf. Four booleans report leaf, conformance and readback
facts. Enabled control: no native attempt. Disabled control: attempt admitted.
A disabled action proves admission, not OS identifier propagation. The OS
snapshot must show the expected ID and original name before activation.

Independent review has no remaining P0–P2 finding. Native execution remains
pending. Existing tests, checker rules and production controls are unchanged.
No private API, overlay, tree replacement, timer or retry is added.

## Native result

Export `77637` adds the missing CoreBridge import. Drive `78819` builds,
but its sheet closes before the Probe action. It makes no native attempt.
Drive `82971` removes those two scroll actions; all 11 steps complete.
The Probe action stays enabled and all four values stay false. Admission
is not established, so this result cannot judge the public setter. The
OS link still has its name and lacks its ID; the drive remains failed.

Stop this experiment here. Keep the failed receipts and enabled checks.
This test-only route supplies no production fix or acceptance evidence.
