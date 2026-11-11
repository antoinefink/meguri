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
