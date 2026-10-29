# U114 native receive and secure-key composition

Status: compiled WIP, not accepted.
Phase base: `6b136035`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Export `cleanroom/20261029T173801Z-35348` connects durable fetched records
to CoreHost apply, without a second receive. Root delivery skips collection
apply. Native configuration retains a synchronous secure-byte provider;
Swift uses Security, Rust accepts exactly 32 bytes, and open stays lazy.

- `core-format/20261029T174645Z-42233`: pass.
- `core-test/20261029T174706Z-42700`: all 85 FFI library tests pass.
- `core-test/20261029T174936Z-45064`: the real feature-gated FFI opener
  and provider test passes. The separate incoming fixture remains red.
- `private/20261029T174711Z-43201`: all nine paths pass.
- `similarity/20261029T174709Z-42961`: all nine paths pass.

Independent R1 requires two P1 repairs: a rolled-back ordinary apply must
keep the native SDK, and positive progress must continue beyond one 500-entry
operation. The existing per-operation bound remains. A P2 coverage repair
must retain a real queued-inbox snapshot comparison; stale applied redelivery
correctly compares equal. Secure-key composition has no review finding.

Next add real regressions and repair typed apply outcomes and continuation.
Swift compilation and execution remain unrun. No native app, Unit, P24,
batch or milestone acceptance is claimed.

## Swift checkpoint

`swift-test/20261029T175311Z-47516` builds the updated bindings and CoreBridge
and passes all 68 selected EventTests and CloudKeyByteProviderTests. This
includes actual native receive and Security provider checks. The two R1 P1
cases and queued-content coverage are not accepted by this passing subset;
their fixer export and targeted regressions remain pending.

## Native apply safety: tests first and review

Start: `2d8f53d3`. Export `cleanroom/20261029T175210Z-46800` adds typed
Deferred only after a successful rollback, keeps legacy drain errors, and
continues bounded native work. Root extracts tests before production.
`core-test/20261029T180020Z-53479` runs five new tests: three pass; the real
502-record delivery stops at 500 and ordinary rollback still throws.

R2 adds mixed-queue and rollback-commit tests. Both fail on R1 production:
`core-test/20261029T181135Z-58036` runs eight, with six passing. Export
`cleanroom/20261029T180250Z-54986` repairs both cases.

Independent R3 finds no P0/P1 and no new P2. The review covers SDK authority,
the 500-entry limit, actual SQL failures, post-COMMIT errors and retry grants.

- `core-format/20261029T181227Z-58549`: pass.
- `core-test/20261029T181256Z-58951`: 126 execute, 125 pass, one fails.
  Every native apply regression and FFI check passes. The separate deleted-card
  reconciliation case remains red and enabled.
- `similarity/20261029T181302Z-59289`: all ten paths pass.
- `private/20261029T181301Z-59229`: only one new test name and two synthetic
  key labels are flagged. A mechanical fixer preserves every assertion.

The added rollback fault seam does not establish complete F5 catalog coverage.
Swift execution of the new native cases and final privacy checks follow.
App connection, media, deleted entities, P24 and the original gate stay open.

## Final native verification

`swift-test/20261029T181538Z-61384` passes all 70 selected bridge tests.
This includes real native deferred failure/cancellation and queued-content
snapshot cases. The production source and Swift tests match the R3 export.

Mechanical export `cleanroom/20261029T181523Z-60960` changes only one Rust
test name and two synthetic labels. Exact comparison of all ten paths confirms
the other nine are byte-identical; no assertion changes. Targeted Rust checks
and final privacy verification cover those edits before the commit.

- `core-format/20261029T181844Z-64246`: pass.
- `core-test/20261029T181854Z-64692`: both affected tests pass.
- `private/20261029T181858Z-64958`: all eleven paths pass.
- `similarity/20261029T181919Z-65317`: final changed test path passes.

This accepts neither full U114 nor iCloud delivery. Retain the original batch
base and the deleted-card failure; complete app connection and media next.
