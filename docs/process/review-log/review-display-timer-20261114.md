# Review display timer

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
U177 starting commit: `4dc3b8dbc34093c4a241e2795daa749c863107ce`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Scope: the R364 display-timer leaf, not acceptance of all U177 requirements.

## Change

Reuse the prepared review coordinator and its answer clock. The timer exposes
visibility, counting and capped states. Hidden, paused, frozen and capped values
have no display schedule. A lazy system TimelineView schedule ends after the
last cap update; a new clock or resume supplies a new schedule. Values still
come from the existing monotonic answer clock. The display schedule uses the
system date, independently of the app's injectable wall clock.

Keep existing platform placement, answer controls and card styling. At the cap,
a standard hourglass adds a cue beyond colour. The grouped accessibility value
uses the approved S17 text; the icon contributes no duplicate narration. This
choice still needs native design acceptance. The change adds no elapsed clock,
polling loop or persistence path and changes no answer, input or lifecycle gate.

R340 idle-display prevention remains with its trusted-input dependency. The
current CardHostReporting H10 does not install a production interaction producer.
This patch does not create a second input-authority path to bypass that gap.

## Source review

Implementation receipts: `cleanroom/20261114T031334Z-79705` and
`cleanroom/20261114T032653Z-84355`. The latter separates the view's update
dates from injected app time and adds the regression for fixed app clocks.

Independent round 1: `cleanroom/20261114T033121Z-87496`, reviewer session
`s-20261109T210639Z-47d06d5b`. No P0, P1 or P2 findings in the bounded
12-file change. The read-only export equals its seed byte for byte.

Two later mechanical changes preserve that functional review: reject a whole
two-line test comment (`cleanroom/20261114T033122Z-87769`) and add the missing
SwiftUI import to the snapshot test (`cleanroom/20261114T033431Z-91368`). Root
checks both patch differences. Neither changes an assertion or production code.
The final fixture correction (`cleanroom/20261114T033832Z-97563`) renders
the timer component directly. Root checks that production code is unchanged
and all preexisting screen fixtures return to their original bytes.

## Focused checks

- Baseline: `swift-test/20261114T031400Z-80129`, 37 passing tests.
- Corrected timer: `swift-test/20261114T033015Z-85457`, 49 passing tests,
  zero failures or skips. This includes seven view-schedule/resource tests
  and 42 review, focus, day-monitor and native media-reference tests.
- Model regressions live in ReviewTimerModelTests.swift as an extension of the
  selected ReviewCoordinatorTests suite. They cover freeze on both sides,
  retained answer time, cap and next-card reset, pause/resume, current preset
  changes and timer arithmetic through 7,200 seconds.
- `lint-ui/20261114T033139Z-88216`: 148 files, no findings.
- `static-checks/20261114T033140Z-88215`: three checks pass.
- `private/20261114T033536Z-91822`: all 12 paths pass after rejecting the
  whole explanatory comment. No privacy exception or guard change.
- `similarity/20261114T033541Z-92680`: all 12 files pass, none skipped,
  no allowlist entry. Root reads only the filtered status.
- After the fixture correction, `private/20261114T034021Z-99880` passes
  all 12 source paths. `similarity/20261114T034027Z-404` passes the two
  changed fixture files, with no skip or allowlist entry. The other ten
  source files retain the earlier evidence without changed inputs.

Earlier privacy outcomes: `20261114T033023Z-86739` is a usage error;
`20261114T033038Z-87176` rejects the new comment. Neither proves a pass.

## Native evidence and remaining acceptance

Fixed fixtures use the actual SwiftUI timer label for running, frozen and
capped appearances. They intentionally schedule no updates and cannot prove
live-view propagation or its placement in a real window toolbar.

`snapshots/20261114T033140Z-88792` fails to compile the new test because its
ViewBuilder needs an explicit SwiftUI import. After that import,
`snapshots/20261114T033539Z-92080` builds but cannot launch the team-signed
test host in the VM. It executes no timer test. Neither supplies rendered
evidence or a pass. The standard ad-hoc snapshot run is recorded separately
below; it cannot establish CloudKit or credential behavior.

`snapshots/20261114T033651Z-94753` renders three content-only screens.
Root inspects each and finds that none includes the timer: Mac toolbar chrome
is outside the renderer's captured content. These images do not establish
timer appearance or placement and are not recorded as baselines.

After the component-only correction,
`snapshots/20261114T034010Z-98094` records exactly three new component
references, without replacing an existing or approved baseline. Root inspects
all three: the ordinary and frozen labels show `00:12`; the capped label
shows `01:00` and the native hourglass, without clipping. These are agent
render checks and unapproved references, not user design agreement or a
passing comparison against an earlier reference.

The snapshot job still fails overall. Its Mac invocation runs other hosted
suites despite the snapshot-name filter. Retained failures are the cloud
callback fact expectation, cloud soak and scope probes without their dedicated
run context, settings search, credential access under ad-hoc signing and two
route-registry expectations. No timer case fails in the final record run.
This change does not repair, skip or waive those other checks; no full-suite
pass is claimed. Native render/fix rounds used: two, of the permitted three.

Outstanding: real-toolbar verification, accessibility and cap-cue agreement;
hosted live-view transitions; preset fallback and timebox integration; the
remaining U177 progress/chrome requirements; required mobile and integration
gates. No Unit trailer, P24 closure, full gate, milestone or release acceptance.
The held U113 repairs and native quiet-point failures remain unchanged.
