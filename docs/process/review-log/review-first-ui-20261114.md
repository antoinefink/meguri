# Review UI checks — 2026-11-14

App source: 6e5737ce. Test input: clean 56faa013, which adds only an
escalation record. No app or core code changed in this verification pass.
The sync repair limit remains in force.

## Builds

- Mac signed Release preview: build-app/20261114T100501Z-60029, pass on
  clean 6e5737ce. The build does not launch the app.
- Shared iOS simulator app: build-app/20261114T101337Z-89552, pass on
  clean 56faa013. This is a build, not physical-device or iCloud evidence.
- Doctor/20261114T101608Z-1770: dedicated VM running, SSH ready, matching
  Xcode 27.2 and simulator runtime available. No reported failures.

## Existing UI journeys

All runs use generated data in the dedicated VM. No assertion, limit,
fixture, permission or production code changed.

| Platform | Run | Passed | Failed |
| --- | --- | --- | --- |
| iPhone | ui-test/20261114T101734Z-3943 | 1 | 0 |
| iPad | ui-test/20261114T102004Z-8533 | 1 | 0 |
| Mac | ui-test/20261114T102923Z-25081 | 1 | 0 |
| Mac | ui-test/20261114T103207Z-30746 | 1 | 1 |
| iPhone | ui-test/20261114T103651Z-37323 | 2 | 0 |
| iPad | ui-test/20261114T104210Z-43761 | 2 | 0 |

The first three rows select testStudyRevealFourRatingsUndoWaitAndRestart.
The fourth selects testInitialKeyboardReviewAndNativeCloseBackActivation and
testAllEasyExhaustionAndRestart. The final two select
testAllEasyExhaustionAndRestart and testTerminalActionsAtLargestAccessibilitySize.

Totals: eight pass, one fail, zero skipped. The Mac keyboard failure reports
AXReadError(-25211) and trusted=false. It cannot establish the Close control's
native focus. Keep the failure. The VM-only helper needs explicit setup inputs;
none were created. No permission was changed and no test was run again.

Viewed generated-test captures: waiting and Home on iPhone and Mac, waiting
on iPad. Kept full Mac captures and smaller inspection copies. These views
supply neither design approval nor a full accessibility result.

## Ordinary Mac observation

Reopened the signed preview normally. Sync Now entered Syncing Media and then
returned to Unknown; its last-sync time advanced. The existing test deck showed
one saved answer and no available cards. No new answer, replacement or collection
export was performed. This observation proves no fresh peer delivery or global
convergence. Host controller screenshots were unavailable; VM captures remain.

## Open work

Sync runs 095102Z-33657 and 095753Z-47707 remain failed. The scope request and
escalation E-20261114T101046Z-660e remain open. The read-only account-outcome
reading cites P12.6: an accepted account transition requires durable failed,
while ordinaryCancel remains cancelled. Five new fixture expectations disagree
with that rule. No assertion or source was changed, and the actual stored enum
was not printed. Preserve the separate original Swift error and ACK checks.

The native structural-recovery draft remains Under review. Its bounded follow-up
writes no approved contract and authorizes no implementation. The generated-data
replacement/export request is separate and still pending.

No Unit trailer, P24 closure, full gate, signed peer trial or release acceptance.
