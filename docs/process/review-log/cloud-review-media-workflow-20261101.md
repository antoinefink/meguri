# Review and media workflow

U113/U114 continuation; neither unit is accepted by this component.
Start: `cb3434d0be1806a2c24a60a648dcc77d06b69ad7`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Coverage

The new test publishes a generated collection and media through the production
publication driver. A second CoreHost adopts it, records a real review, releases
the session hold and sends the answer with another media file. Both clients
reopen their files and retain the exact Card, Revlog and media bytes. Existing
notes, history and other cards stay intact. The service remains unchanged while
the review hold is active. No binding or acknowledgement is seeded.

The transport is the deterministic Service, not Apple's network. Media byte
checks do not prove playback or rendering. Collection projections read physical
rows with tracking-provided layout and identity metadata. Signed multi-device
CloudKit runs and the full app composition test remain required.

## Review and repair

Implementation: `cleanroom/20261101T025423Z-7782`, session
`s-20261025T134357Z-235af9a2`. The independent correctness reviewer found no
P0–P2 findings. No spec change was needed.

Fixer: `cleanroom/20261101T025859Z-10626`, session
`s-20261025T162405Z-9dc656e4`. Removed two complete redundant comment regions
reported by the privacy guard. No exceptions changed. Applied the formatter's
exact remaining whitespace diff during integration. A second independent review
confirmed unchanged behavior and no findings requiring repair.

## Verification

- `core-test/20261101T025723Z-8761`: all three tests pass, including the new
  combined workflow. Later edits only remove comments and format whitespace.
- `core-lint/20261101T025750Z-9250` and `20261101T025959Z-11578`: formatting
  failures retained; compiler and layering checks passed.
- `core-lint/20261101T030020Z-12243`: pass after the exact formatter correction.
- `private/20261101T025800Z-9572`: failed on the removed comments;
  `private/20261101T030000Z-11836`: pass after removal.
- `similarity/20261101T025801Z-9802`: pass, one file, no flags or exceptions.

No unit trailer is added. Original integration and milestone gates remain open;
this focused test does not close their previously recorded failures.
