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
