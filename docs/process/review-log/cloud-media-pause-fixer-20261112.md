# Media Pause normal-control repair, round 1

- Role: fixer; permitted own-code source only. Requested upstream base:
  `ae2b31e4`. The filtered runner exposes synthetic baseline
  `5ebdc901feaef8e62adbda427fbbff356056a196`, without upstream history.
- Input evidence: `swift-test/20261112T072339Z-9664`, both automatic settings
  fail Pause with the original SDK's revoked token (`mismatchMask=4`).
- Production repair: skip network SDK preparation only for `PauseMedia` in
  `Runtime::policy_begin_media_with_work`. Preserve Swift queue ownership and
  Core paused/epoch/pending-update effects. No guard or SDK validator changes.
- Retain command owner, original Host, generation, policy/binding and Runtime
  guard checks before effects. All other media actions keep preparation,
  validation and their existing `PolicyMediaPrepared` boundary; Pause retains
  `PolicyMediaSelected` at its actual effect.
- New Rust tests in `cloud_policy_execution_tests.rs`:
  `settings_pause_with_media_succeeds_with_revoked_or_released_original_sdk`
  (automatic false/true, StopEngine retire/release before/after Pause), and
  `settings_pause_with_media_rejects_stale_policy_runtime_guards_and_wrong_owner`.
  Cover old SDK/network rejection, no Pause preparation, actual Pause effects,
  unchanged collection rows (including cards/revlog), sequence and idle review.
- App regression seed is untouched, SHA-256
  `f463098cd595fd20ec63c15c4bbc9ec46ba8805d4d591b0d0d2f64fd70bc2aa4`.
- Filtered-runner handoff only: one source consistency check; no native build
  or test. Integrator still owns compilation/formatting, focused Rust and app
  regressions, independent review and provenance/privacy/similarity gates.
  This round makes no acceptance claim and changes no existing expectations.

## Integrator verification

- Confirmed the upstream starting commit as `ae2b31e4`.
- `core-test/20261112T073233Z-14733` selected no tests and failed. The module
  name is `policy_execution`; the corrected selector is recorded below.
- `core-test/20261112T073305Z-17744`: 47 tests passed, including both new
  regressions and existing native job, cancellation and owner checks.
- `swift-test/20261112T073331Z-18137`: the same app regression that failed
  before the repair passed for automatic sync off and on. Actual Pause,
  Resume, successor exchange and retained collection/review facts were checked.
- MG formatting and the unchanged staged privacy guard passed.
- Independent scoped source review `cleanroom/20261112T074755Z-21723` found no P0-P2 issues in this repair. This is not the final review of the earlier recovery checkpoint.
- `similarity/20261112T074831Z-23310` passed. The signed Mac build `build-app/20261112T074802Z-22077` passed.
- In that exact Mac app, Pause no longer produced the original media SDK preparation error. Older UpdatePending and ResumeMedia commands still produced a persistent warning; their refusal classification remains a separate open repair.
- The Mac recorded an answer after Resume. Undo restored the card and Redo advanced it. Three collection records were sent; pending and in-flight counts reached zero. A subsequent manual sync succeeded. After a normal quit and relaunch, the answer remained saved.
- These observations do not establish receipt on the other device or complete media coverage. The signed two-device round trip, retained failed stress cases, final review and full batch gate remain open.
