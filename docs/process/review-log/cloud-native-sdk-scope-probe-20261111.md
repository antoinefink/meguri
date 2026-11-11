# Native Cloud fetch scope SDK diagnostic

- Fixer session `s-20261110T214025Z-20fa15fd`; original main starting commit
  `06ea6c30`; filtered runner starting commit
  `33da2ea7aa0cfe47ac66fa2460e31549a1472718`.
- User-authorized disposable-data diagnostic only, not a product fix, sync
  acceptance or signed-lane proof. No collection/media, account identity lookup,
  CloudKit setup/schema/container/account or owner approval machinery changes.
- Explicit public selector:
  `NativeCloudFetchScopeProbeTests/observeSavedStateFetchScopes` through
  `tools/mg mac-hosted-test --suite NativeCloudFetchScopeProbeTests/observeSavedStateFetchScopes`.
  Snapshot skips the whole new suite; CloudAgent's existing positive selection
  includes only its two old classes. UI, Unit and Performance do not select
  MeguriSnapshotTests. Existing selections/assertions/limits are preserved.
  The focused producer supplies an exact only-testing selector; native selection
  and its one-executed/zero-skipped receipt remain integration verification.
- One generated `mg-sdk-scope-probe-` UUID zone and one synthetic MGEntity;
  original exact per-item/terminal Saved checks; encrypted e changes from [1] to
  [2] on the returned server record with ifServerRecordUnchanged. One bounded
  zone inventory defines allExcluding(other current zones), excluding blob zones.
  Engine/delegate pairs are distinct, automaticallySync false, empty pending
  work, unchanged requested options and nil send batches. Seed's actual callback
  serialization is reused unchanged for limited/wide reopenings. Those value2
  outcomes are observations; seed value1/state and fresh nil-state value2 are
  required. Callback counts (including absent callbacks on A/B) are observations.
  Each fetch is explicitly invoked once, cancel/drained and joined
  before the next stage and exact generated-zone deletion, including failures.
- Probe-only request/resource budgets: 30/60 seconds; each engine fetch also has
  a 60-second structured deadline. No retries or detached/unjoined tasks. Cleanup
  joins its own bounded original operation even after caller cancellation; exact
  per-zone deletion failure remains a separate fixed Issue. A process crash
  cannot guarantee cleanup. Concurrent zones created after the
  single inventory are outside that inventory's exclusion snapshot.
  Per-item and final callbacks must report success before the probe advances. This diagnostic supplies no J2.1 or J3 evidence.
- Only one counts/booleans diagnostic JSON line, with fixed phase/error enums;
  no names, IDs, state bytes, payloads, assets, configuration or SDK descriptions.
  No CoreHost, production adapter, account source, defaults or app data access.
- Source-only filtered runner: no builds/tests, network, plugins, subagents,
  forbidden sources or raw similarity reports. One `git diff --check` at handoff;
  native compilation, independent review, provenance/privacy/similarity checks
  and the explicit authenticated VM diagnostic belong to integration. No live
  outcome or acceptance is claimed here.

## Round1 — independent review follow-up

- Session `s-20261110T214025Z-20fa15fd`; main starting commit `370860e2`;
  clean filtered baseline `83a0a5f1d60bc5e8aab1135d7ee51560a426f50e`.
  Review `cleanroom/20261111T210238Z-57120`: no P0/P1, one P2.
- Added numeric `signIns`; each new observer allows its first signIn without
  identity lookup or retaining account values. Repeated signIn, signOut,
  switchAccounts and unknown account cases remain unexpectedActivity; existing
  unexpectedActivity stays sticky. This is diagnostic classification only.
  All four stages use the original generated record. Seed and fresh-control
  bytes remain required. Existing assertions and budgets are unchanged.
- Compile-only CloudAgent build-app attempt `20261111T210510Z-58192` failed before
  retaining a build-result bundle; existing temporary cleanup removed scoped
  private diagnostics. No compiler diagnosis or SDK execution is established.
  After round2 review, the next native build/run uses the existing focused
  mac-hosted-test Snapshot producer; signing/injection infrastructure is unchanged.
  Selection remains unverified until its one-executed/zero-skipped receipt.
  This round is source-only, with one `git diff --check`; no cause, live outcome
  or acceptance is claimed.

## Compiler correction — source-only follow-up

- Session `s-20261110T214025Z-20fa15fd`; original main `370860e2`;
  filtered starting commit `d3b80bb1c65a8345cb7e85b3831875a82f50e5e1`.
  The supplied round1 signIn classification and comment corrections are preserved.
- Generated-data-only Snapshot attempt
  `mac-hosted-test/20261111T210945Z-60302` was a compile-only failure before
  any test or SDK operation ran; it is never a live SDK result. Supplied compiler
  findings identify MainActor-isolated configuration access at probe lines 16–19
  and MemberImportVisibility for the `.development` enum case.
- Added `import CoreBridge` for public enum type visibility only and annotated
  the new suite class `@MainActor`, matching existing app-hosted suites. No
  CoreHost or workflow calls were added. SDK helper/observer flow, record writes,
  scopes, timeouts, expected bytes, account classification and assertions are
  unchanged; no existing tests/plans, producer/signing/injection scripts, role
  boundaries or deployment settings were changed.
- Unrun here: no builds, tests or SDK operations. One `git diff --check` is the
  sole consistency check. Integration owns native compilation and independent
  round2 review before the live probe. No cause, fix or acceptance claim is made.

## Existing-profile scope repair — source-only handoff

- Main starting commit `7916d292`; filtered starting commit
  `078757a241c65726ba9044f12a2d9fb2a234904c`.
- Supplied integration evidence: the independently reviewed native SDK
  diagnostic compiled but never executed; VM launch failed in
  `mac-hosted-test` runs ending `63801` and `68903`. Read-only probes verified
  the actual Snapshot app signature. Its embedded profile covers three devices
  and excludes the private VM reference; the already approved local profile
  supplied through `MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE` covers four and includes
  the VM. These are supplied observations, not a live result or established
  launch-failure cause. No private settings, identities, paths or IDs are recorded.
- Extend existing app-only profile selection only to
  team/build-for-testing/mac/Debug/Snapshot/Meguri when
  `MG_JOB=mac-hosted-test`, using the existing validator and target-only settings.
  Preserve the CloudAgent allowlist, refusals and freeze/build invariants;
  widget/test signing remains unchanged. Clear stale binding state at build
  entry and recheck the selected Snapshot binding after build, before VM delivery.
  Add shell scope, stale-binding and pre-delivery recheck cases; existing tests
  remain enabled.
- No new grant, profile/certificate mutation, re-signing, entitlement/trust change,
  runtime override or host execution. Source-only: no builds/tests or live SDK
  result; one `git diff --check` at handoff. Integration owns focused tests,
  independent review and native verification. No unit acceptance is claimed.

## Closed SDK error categories — source-only follow-up

- Main starting commit `67c0caa0`; filtered starting commit
  `40c9e1e3ad7c7f4e00bab34d030d40eec53be0a8`.
- Supplied evidence: profile repair independently reviewed clear; generated-only
  test `72687` passed one/zero skips. Native probe
  `mac-hosted-test/20261111T213651Z-74589` executed exactly one/zero skips,
  failed at create with generic `sdkFailure`, and cleanup failed with the same
  category. Seed, limited, wide and fresh did not run. No cause is inferred.
- Extend only `ScopeProbeFailure` and `category(Error)` with fixed sdk-prefixed
  cases for the requested common CKError codes; preserve cancellation, existing
  categories and generic fallback. No descriptions, userInfo, identifiers,
  settings, payloads, arbitrary numeric fields or account API calls are added.
  Operations, resources, mutations, scopes, budgets, assertions, selection and
  cleanup are unchanged.
- Source-only: no builds/tests or SDK execution here; one `git diff --check`
  at handoff. Integration owns native compilation and verification. This change
  establishes no scope outcome or sync acceptance.

## Integration evidence

- Probe round2 independent review `cleanroom/20261111T211949Z-64336`: no P0–P2.
  Similarity `20261111T212921Z-68041` passed for base `06ea6c30`.
- Profile repair review `cleanroom/20261111T213457Z-72265`: no P0–P2.
  Focused Docker selftest `20261111T213435Z-71858`: 17 passed, zero skipped.
  Private scan `20261111T213431Z-71566` passed. Native generated-only selected
  test `mac-hosted-test/20261111T213514Z-72687`: one executed pass, zero skipped.
  It launches in the same VM where the previous profile failed.
- Explicit probe selection `mac-hosted-test/20261111T213651Z-74589`: one executed
  failure, zero skipped. It proves selection despite the default Snapshot
  exclusion; it provides no scope comparison because creation failed.
- Physical normal launch `run/20261111T212700Z-66327` passed. Read-only phone
  drive `20261111T212820Z-67680` completed all eight steps and verified process
  continuity; overall failed with 49 hierarchy issues. It still reports 2,389
  missing media files. No answer, replacement, Home cycle or manual sync was sent.
- One lease-aware dedicated VM restart retained both earlier launch failures.
  Doctor `20261111T213308Z-70969` warns, with no technical failures.
  No sync, unit, milestone or release acceptance is claimed.

## Classified native execution

- Independent mapping review `cleanroom/20261111T214012Z-77399`: no P0–P2
  in the scoped source pass. Swift lint `20261111T214016Z-77748`: 146 passed,
  zero findings or skips.
- `mac-hosted-test/20261111T214101Z-78442`: the new mapping compiled and the
  exact diagnostic executed once, failed once, zero skips. Both create and
  cleanup returned `sdkNotAuthenticated`. Every fetch-stage counter remained
  zero. No fetch-scope outcome, successful cleanup or sync acceptance follows.
- Do not repeat this VM SDK path without changed account readiness. Owner-only
  account setup remains reserved. The authorized Mac feedback path is a possible
  separate diagnostic, not an automatic fallback or VM acceptance substitute.
  The original integration batch base remains
  `1416a520542a8c5daa38a23bfe398da704c3d6c9`; full integration gates remain pending.
- Focused static checks `20261111T214307Z-80387`: three passed. Similarity
  `20261111T214311Z-80862` passed for base `06ea6c30`; seven unsupported or
  excluded paths remain scanner skips. These receipts do not replace a full gate.
