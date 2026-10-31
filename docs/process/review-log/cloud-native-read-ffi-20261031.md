# Owned native asset-read ports

Source work starts at `590ab2b9`. Integration is on `ad2b6e29`.
The original batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Opaque passes and work retain the original host, runtime, collection, SDK and
issued command. Queue callbacks run outside core locks. Refusal does not consume
an attempt; accepted work survives accounting errors and pass cancellation.
Completion uses normal incoming writer and WAL handling. Generic native retry
admission remains blocked.

- `core-test/20261031T214327Z-72612`: 7 pass, 4 fail.
- Review identifies fixture errors in handler entry, successor refresh ordering
  and replay of completed or refused work. These are repaired without changing
  production validation.
- `core-test/20261031T215350Z-76064`: 10 pass, 1 fails.
- `private/20261031T215353Z-76343`: pass.
- `core-lint/20261031T215035Z-74751`: pass before the fixture-only corrections.

The conflict fixture now starts with an actual Saved ACK, then changes separate
local and remote fields. It verifies the merged values, remote ACK and pending
local resend. A second test completes two entries through the same retained pass
and normal writer/WAL calls. No production scheduling change was needed.

- Filtered fixer export: `cleanroom/20261031T220027Z-77609`.
- Independent round 3 review: no findings requiring repair.
- `core-test/20261031T220542Z-79455`: all 12 asset-read FFI tests pass.
- `core-test/20261031T220652Z-79972`: all 184 FFI library tests pass; none skipped.
- `private/20261031T220657Z-80261`: pass.
- `core-lint/20261031T221007Z-81007`: pass, including formatting and layering.

Native Swift transport and scheduling, generic conflict receipt handling,
cross-device proof and the batch gate remain outstanding. This is WIP;
U113 and U114 remain unaccepted.
