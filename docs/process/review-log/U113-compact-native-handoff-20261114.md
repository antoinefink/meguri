# U113 compact native handoff — 2026-11-14

Starting commit: cd082f34. Integration base:
1416a520542a8c5daa38a23bfe398da704c3d6c9.

On the physical phone, opening Connect from the compact sync-status sheet
closed that sheet without presenting Connect. Its retained modal request
then disabled controls until a normal reopen. Opening the same preview
through Settings worked. The navigation job executed all six steps but
failed its hierarchy checks; that failed result remains unchanged.

The compact layout now uses an explicit native sheet. Destination requests
wait for its onDismiss callback, with presentation, collection, navigation,
review and lifecycle checks before consuming the request once. Pending
handoffs retain input ownership. Account & Sync uses the same path, including
during a review. Regular popovers and Mac routing keep their prior behavior.
There are no timing workarounds or new styles, sizes or copy.

Filtered author 20261114T213622Z-53465 completed the candidate. Independent
review 20261114T214139Z-56303 found no P0, P1 or P2 source defect. Initial
Swift check 20261114T215259Z-64689 failed because the new test helper needed
access to an existing internal initializer. Filtered correction
20261114T215424Z-67114 changed only the test import to @testable. Every
production byte and assertion was compared with the reviewed patch and
remained unchanged. The failed compile receipt is retained.

Privacy 20261114T215311Z-66039 passed all seven files. Similarity
20261114T215314Z-66480 scanned all seven with no findings. The final test
import was checked again in privacy 20261114T215542Z-68280 and similarity
20261114T215636Z-70254. Static checks 20261114T215639Z-70650 passed.

Swift run 20261114T215628Z-68660 passed all 44 selected tests. Exact-build
native observations remain open.
The model tests manually deliver dismissal and use placeholder review
sessions. They do not prove native ordering or return to the same rendered
card. Keep the unresolved native count-row, paragraph identifier and audio
link findings. No gate, unit, P24 or visual acceptance is claimed here.

The subsequent filtered run 20261114T215950Z-72319 adds only a stable
identifier to the existing native Done button. Integrator inspection confirms
that its action, label and styling are unchanged. Privacy
20261114T220123Z-73245 and similarity 20261114T220127Z-73508 passed. The
physical check will use that button to return from Account & Sync to review.
