# Normal Mac review trial — 2026-11-11

Role: fixer. Starting commit: `21e862df`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The normal signed preview used build `build-app/20261111T080705Z-52414`
from `b64295b3`. Later commits changed documentation only. Antoine approved
grading and Undo with the development collection while iCloud was paused.

Show Answer displayed the answer. Good advanced to the next question.
Study → Undo Answer returned to a question, disabled Undo and enabled Redo.
Back to Decks restored the starting counts. No Redo was performed.
iCloud was resumed. Sync Now updated the last-sync time and media downloads
continued. The restored counts and enabled sync setting remained after the
quit gesture and reopening the exact preview path.

One answer displayed an image. This does not establish complete image or audio
support. The app still reports media absent both locally and in iCloud.
A retained Core-next error disappeared on reopening; downloads continued before
and after that restart. Its original cause is not established.

## Focused checks

- `swift-test/20261111T084236Z-78442`: the native WebKit page-pair test passes
  (1/1). It covers question/answer replacement, appearance and scoped scripts.
- `core-test/20261111T085019Z-91583`: `cloud_media_roundtrip` passes (11/11).
  It uses real Core/storage and a fake cloud service across two processes.
  It does not prove a live CloudKit or physical-device round trip.
- `gate/20261111T084914Z-89396` is a plan only, not a passing gate.
- `ui-test/20261111T085902Z-10263` inventories no available physical device.

Live review delivery to another device, complete media delivery, independent
review and the integration gate remain open. No Unit or acceptance is claimed.
