# U114 typed callback rejection and cleanup

Repair base: `3ce2ae24`. Typed bridge checkpoint: `407a0cb1`. Original gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Unit acceptance remains pending.

Fresh account proof can end SDK authority after callback validation. Core correctly rejects the old mutation. The bridge now preserves that typed result, while real storage and SDK failures keep their existing error path. See [the deterministic reproduction](U114-root-loss-deterministic-reproduction-20261115.md).

Author `cleanroom/20261115T030735Z-53432` added the typed mapping. Review `031651-54547` found that three benign returns could retain an invalid original. Fix `032427-56411` schedules its existing asynchronous stop, including the standalone saved-removal path. Final independent review `033308-61324` has no required P0–P2 findings. The original owner is cancelled and released without joining shutdown inside a callback or accepting rejected work.

Verification:

- `core-test/20261115T031804Z-55051`: all 312 selected FFI tests pass; 85 were filtered.
- `swift-test/20261115T032433Z-56787`: two methods pass, including 27 error-mapping cases.
- `swift-test/20261115T032648Z-58297`: six app-model checks pass, including retained review changes after close/reopen and diagnostic privacy.
- `swift-test/20261115T033011Z-60184`: five root-loss and real storage-failure methods pass.
- `swift-test/20261115T033335Z-61867`: new cleanup tests fail on the old behavior, with no stop task, retained owner and no cancellation. Setup checks complete.
- `swift-test/20261115T033556Z-63033`: all nine selected methods pass after the fix, including pending take/ACK, batch, both late-event variants and real storage errors.
- `core-lint/20261115T033929Z-64256`: all three checks pass.

A private test-phase label was renamed by `cleanroom/20261115T034120Z-65260` after a scanner collision. Inverse byte comparison proves that only nine identifier occurrences changed. An optional two-line process comment was omitted during integration. `swift-test/20261115T034402Z-66104` passes all four selected methods on the renamed code. No guards, assertions, limits or privacy rules changed.

The saved-removal subpath shares the cleanup helper; its individual race was not separately exercised. Broader Core, integration, signed peer and milestone evidence remain required. No Unit trailer or acceptance claim is added.
