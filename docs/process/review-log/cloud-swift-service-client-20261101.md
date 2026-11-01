# Headless Swift Service client

U113/U114 continuation. Start: `2bb28725faa74cb3037bc544596c5f23d160774a`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Neither unit is accepted by this component.

## Change

MG prepares a native developer CLI for headless MeguriKit package tests. It
selects the actual Cargo executable and publishes a run-owned copy, with source
identity, exclusive creation, bounded copying, hash, mode and receipt checks.
Cargo warnings, errors and invalid JSON stop publication and Swift execution.
The initial publication-fixture producer and existing failure accounting remain.

The macOS-only test client owns the original Process and pipes. It serializes
complete request/reply exchanges, drains a cancelled caller's original reply,
and joins process and I/O work on fixture shutdown. Fixed failures avoid raw
request/path diagnostics. The small-fixture input and reply cap is 1 MiB; it is
not a production sync limit. No request is retried automatically.

This uses the existing host headless Swift-package lane. It introduces no GUI
driving, network server, app/FFI dependency or production sync behavior. GUI,
app-hosted and device lanes remain in the VM. Full AppModel review/media exchange
and actual CloudKit execution remain separate requirements.

## Review and repair

Implementation `cleanroom/20261101T033253Z-32928` used session
`s-20261025T134357Z-235af9a2`. Independent review identified two P2 findings:
native Cargo warnings could escape verification, and the actual Swift frontend
rejected the compound sentinel expression before tests ran.

Fixer `cleanroom/20261101T034531Z-37924`, session
`s-20261025T162405Z-9dc656e4`, added diagnostic rejection with negative tests and
split the sentinel checks into typed values. Both negative assertions remain.
Independent round 2 confirms both findings resolved and no new P0–P2 findings.

## Verification

- `selftest/20261101T034215Z-34624`: initial 16 checks pass.
- `swift-test/20261101T034219Z-34890`: preserved compiler failure; native helper
  build/publication succeeded, but no smoke-test pass is claimed from this run.
- `selftest/20261101T034730Z-38644`: 18/18 pass, including warning/error and
  malformed Cargo JSON refusal before publication or execution.
- `swift-test/20261101T034732Z-38905`: all six real-process smoke tests pass.
  They cover actual save/read/conflict values, concurrent reply ownership,
  state after process reopen, fixed error diagnostics, EOF and cancellation.
  The package compiles without warnings; MG retains the native build/hash receipt.
- `private/20261101T034355Z-37121`: all seven files pass;
  `private/20261101T034849Z-40573`: final three-file repair passes.
- `similarity/20261101T034356Z-37367`: full component passes before the repair.
- `similarity/20261101T034853Z-40820`: final repair passes.

No Unit trailer is added. Original integration/milestone gates, runtime conflict
and generation work, media integration, signed setup and live delivery remain open.
