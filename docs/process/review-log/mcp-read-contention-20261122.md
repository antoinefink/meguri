# MCP read contention

The starting commit was `63ba8541e5d1d34436619c1b53568c43f9343859`.
Search and catalog reads now wait up to 500 ms for the per-account
publication barrier. They recheck authority after acquiring it. Contending
writes still fail immediately. Read contention uses the read-specific error
envelope, and fixed diagnostic labels identify the failing phase.

- Implementer session `s-20261122T181214Z-45749a24`. The red proof was
  exported in `cleanroom/20261122T181342Z-74322`. It failed with the
  expected `busy` in `sync-server-test/20261122T181546Z-86708`.
  The candidate was exported in `cleanroom/20261122T181644Z-89187`.
- Independent reviewer `s-20261122T182422Z-f23fc922` found no actionable
  issues and classified all 16 changed files and both session records as
  PUBLIC after complete-content review in `cleanroom/20261122T182449Z-17252`.
  The orchestrator verified all 18 digests against the candidate. The
  scanner refreshes only the three existing pins it flagged as stale.
- `sync-server-test/20261122T182505Z-18789` passed 358 tests, including
  overlapping reads, bounded waiting, queued credential rechecks, immediate
  write refusal, SQLite diagnostics and read/write error envelopes.
  `sync-server-lint/20261122T182603Z-23280` passed.
- Hosted similarity and CI remain the PR gates. The earlier full integration
  failure register remains open outside this unit.
