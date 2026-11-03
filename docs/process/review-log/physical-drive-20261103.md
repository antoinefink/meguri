# Physical-device drive transport

Status: WIP; no unit or signed-cloud acceptance.
Component start:2c0ee80ea020957da9c3874ce3049ff653a4b82a.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer:s-20261103T150638Z-42b1ad5b.
Contract:tools/drive-scripts/physical-device D1-D4, approved27bca9e5.

Export40805 adds private inline script transport to the existing driver. The
physical route verifies the selected paired device and Debug team build. It
retains actual app state and refuses launch overrides and motion. Host and runner
bound decoded input to16,384 bytes. Fixed input errors do not echo the payload.
The result requires the exact test, no skips, successful steps, captures and
hierarchy checks. Independent review has no P0-P2 findings; live transport and
captures still require verification.

Private44072 passes all four changed source paths. Selftest43683 passes six of
seven cases. The positive case's command expectation incorrectly requires an
extra argument between Debug and destination; the actual command is correct.
Keep all signing, destination and exact selected-test assertions while repairing
that one expression. Preserve this failed receipt.

Export44671 changes that expression only. Independent review is clear.
Selftest45704 passes all seven physical-drive cases. Selftest45029 passes all17
existing physical-UI cases. Similarity45975 passes the four source files.
Runner compilation and actual phone captures remain pending.

Physical UI52357 compiles the signed runner and passes all three transport tests
on the available phone, with no skips. Drive56799 runs the exact selected driver
test and both inspection steps successfully. Captures and manifest are present;
hierarchy checks report no issues. The actual screenshot shows the empty
collection screen. This proves signed app launch and direct inspection only.

Drive61191 opens the iCloud panel: all three steps and the selected XCTest pass,
but the drive receipt fails its hierarchy check. Four elements inherit the outer
popover identifier; the native sheet grabber also lacks an identifier. The first
is an app bug; the native-control classification is a separate unresolved issue.

UI correction start:d281f8f502cec016b726806dab80b89f9c0a7f54.
Export65556 adds one explicit accessibility container to the status panel, keeping
its children separate. Independent technical review has no P0-P2 findings.
Private66647 and Similarity66646 pass. Actual phone recapture remains pending.
Keep the native grabber and its failed evidence; no audit exception was added.

Phone67343 confirms each app identifier appears once and preserves the native
layout; all three actions pass. Its drive receipt still fails for the grabber.
The separate native-sheet-chrome log records that classification repair.
Phone69967 reaches Account & Sync and72513 opens Connect; all requested steps
pass, while hierarchy findings keep those receipts failed. The initial preview
capture is immediate: a generic message can reflect a prior operation while a
fresh request remains active. It is not proof of that request's final outcome.

Export82207 adds leaf identifiers to existing preview progress and error origins.
Independent review is clear; state, copy, guards and timing are unchanged.
Private84233 and Similarity84891 pass. The next actual-phone script will use the
existing wait-until-gone action to observe readiness before judging the result.

Phone86370 completes all six requested steps, including direct Home entry and
progress disappearance, with no skips. Its final hierarchy contains the generic
operation-error element and neither the local entry-error nor typed-failure
element. The preview is absent and Try Again is visible. This establishes the
settled UI outcome but does not identify the discarded underlying error.
The receipt remains failed for six identifier issues. Preserve those findings;
do not repeat unchanged requests as proof of repair.

Phone verification uses the three new transport tests. An older parser test
requires files from the checkout and stays in its existing test lane.
D3 requires an initial capture before a later replacement action.
No phone drive has run yet.
