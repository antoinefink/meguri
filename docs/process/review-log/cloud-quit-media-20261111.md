# Quit while media is running — 2026-11-11

Role: fixer. Starting commit: `caa0b030`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contract: `sync/policy#P11.4`.

The normal Mac preview's quit did not finish while media was running.
`ReviewCollection.performClose` calls `quitAfterLocalFlush` before
`cancelAndDrain`. The first method waited for every operation task, including
a media round that only the second method cancels. A completed quit command
could therefore wait for the whole media backlog before reaching shutdown.

The coordinator now tracks operation lists that contain an actual issued
QuitNow or QuitFlush. Quit joins those original tasks and their result ACKs.
The set includes QuitFlush's separate child task; completion removes each
original ID. Other operations remain owned by the existing cancel-and-drain
path. Deadlines, failure handling, required shutdown effects, and backend
retention on close failure are unchanged.

## Verification

- Red `swift-test/20261111T080012Z-31345`: all four new scheduler cases time
  out with media held after the quit command returns. Cases cover QuitNow and
  QuitFlush, each with success and failure.
- Green `swift-test/20261111T080217Z-35232`: all four cases pass. They check
  original quit ACK, preserved failure behavior, unfinished media before
  shutdown, then owned cancellation and joined close.
- `swift-test/20261111T080331Z-40130`: six scheduler and existing shutdown
  groups pass. The new owner test fails at setup: its fixture defaults to
  automatic sync off, so Foreground issues no ResumeMedia to hold.
- `swift-test/20261111T080505Z-45995`: the owner test passes after explicitly
  enabling automatic media in its fixture. It runs the production collection
  owner, policy and backend over Service, observes quiescence while media is
  still held, then releases that original command and verifies joined close.
  Assertions and the existing timeout are unchanged.
- Lint `20261111T080628Z-50792`, similarity `20261111T080623Z-50117`, and
  privacy `20261111T080626Z-50436` pass.

Direct diff review checked task registration/removal, the nested quit task,
missing-command and failed-command returns, and required result admission.
No independent agent was used. Independent review, the integration gate and
the normal signed Mac trial remain open. No Unit or acceptance is claimed.

## Normal Mac trial

Signed standalone build `build-app/20261111T080705Z-52414` passes on clean
`b64295b3`, including normal signature, cloud configuration and executable hash.
The older preview was paused and replaced through an exact-process SIGTERM;
that replacement is not evidence for the repaired normal-quit path.

The new preview resumed media with a backlog still present. An initial window
observation did not establish whether quit had completed. The controlled check
then retained the exact preview PID before the normal quit gesture and observed
that original PID exit, with zero preview processes and no successor during its
15-second observation window. It did not send a termination signal in this check.
The PID check confirms normal quit. Timing and signed-lane acceptance await
their required checks.

The same preview was opened again. The collection and enabled sync setting
remain, and media is still pending. Error-category inspection was scoped to the
original preview PID to exclude expected errors from test processes. The preview
is left open on its deck page. Missing cloud media, two-device review delivery,
independent review and the integration gate remain open.
