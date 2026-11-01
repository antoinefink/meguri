# Persistent synthetic Service bridge

U113/U114 continuation. Start: `38020b8c5396754f06d54ec7225dfbea4beba43e`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Neither unit is accepted; no Unit trailer is added.

## Change

The internal CLI command retains one existing Service while it reads JSON frames
from stdin. It returns the original Service responses for zone creation,
conditional mutations, reads and database/zone pagination. No scheduler, sync
policy, acknowledgement endpoint or tracking mutation was added. Service keeps
its existing persistence and opaque envelope, asset, token and system-field
representations.

Input frames are limited to 1 MiB before allocation beyond that bound. This is
a small-fixture transport limit, not a production sync limit. Responses are
streamed. Protocol failures stop further dispatch and emit fixed classifications
without request bodies or paths. A failure after dispatch may follow a commit;
callers must not retry it automatically. EOF closes normally.

The helper stays in the developer CLI. FFI, app dependencies and ordinary
CoreBridge sources are unchanged. MG helper publication and Swift test adapters
remain to be implemented. Synthetic bytes and service attribution do not prove
native asset lifetimes, SDK authority, full app sync or live CloudKit delivery.

## Review and verification

- Implementation: `cleanroom/20261101T032130Z-28111`, session
  `s-20261025T134357Z-235af9a2`.
- Independent correctness/clean-room review: no P0–P2 findings.
- `core-test/20261101T032524Z-29190`: all five real-process tests pass. They cover
  durable reopen, conditional conflict, atomicity, exact envelope/assets,
  pagination across other requests and reopen, malformed/unsupported/oversized
  refusal, fixed diagnostics and no rejected-input mutation.
- `core-lint/20261101T032558Z-29638`: formatting failed; compiler and layering
  checks passed. `private/20261101T032609Z-30215`: one redundant comment failed.
- Fixer `cleanroom/20261101T032659Z-30705`, session
  `s-20261025T162405Z-9dc656e4`, applied the exact formatter output, removed the
  entire comment and reduced the guidance. No privacy exception changed.
- Independent follow-up confirms unchanged behavior and assertions; prior five
  passing tests and similarity evidence remain applicable.
- `core-lint/20261101T032801Z-31290`: pass.
- `private/20261101T032804Z-31550`: pass on all four files.
- `similarity/20261101T032608Z-29951`: pass on the three code files.

Original integration/milestone gates and remaining runtime, generation, media,
signed setup and end-to-end app requirements remain open.
