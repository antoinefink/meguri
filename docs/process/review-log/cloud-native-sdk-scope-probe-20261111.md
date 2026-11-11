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
  per-zone deletion failure remains a separate fixed Issue. SDK cancellation and
  terminal callback draining still depend on Apple's completion contract; a
  process crash cannot guarantee cleanup. Concurrent zones created after the
  single inventory are outside that inventory's exclusion snapshot.
  Direct operations retain their original terminal results and join their
  operation completionBlock before advancing or attempting cleanup.
- Only one counts/booleans diagnostic JSON line, with fixed phase/error enums;
  no names, IDs, state bytes, payloads, assets, configuration or SDK descriptions.
  No CoreHost, production adapter, account source, defaults or app data access.
- Source-only filtered runner: no builds/tests, network, plugins, subagents,
  forbidden sources or raw similarity reports. One `git diff --check` at handoff;
  native compilation, independent review, provenance/privacy/similarity checks
  and the explicit authenticated VM diagnostic belong to integration. No live
  outcome or acceptance is claimed here.
