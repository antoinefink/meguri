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

The conflict case needs an actual initial Saved ACK and edits to separate fields.
The next test also covers two entries in one pass.

Native Swift transport and scheduling, generic conflict receipt handling,
cross-device proof and the batch gate remain outstanding. This is WIP;
U113 and U114 remain unaccepted.
