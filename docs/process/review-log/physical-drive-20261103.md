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

Phone verification uses the three new transport tests. An older parser test
requires files from the checkout and stays in its existing test lane.
D3 requires an initial capture before a later replacement action.
No phone drive has run yet.
