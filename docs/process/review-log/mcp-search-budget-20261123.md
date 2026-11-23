# MCP search budget

The unit started at `03b30d9420a97523fc5f54ee34b6cd6320ae113c`.
Cold unscoped search streams the text and tags it checks. The response marks
pages that reach their work limit before filling the requested page.

- Implementer `s-20261123T104331Z-127041dd` exported
  `cleanroom/20261123T104529Z-46154`. The initial change passed 396 server tests.
- Reviewer `s-20261123T104930Z-8cb19270` approved
  `cleanroom/20261123T105207Z-60172`. Fixer `s-20261123T105600Z-b83d2b21`
  verified error handling and updated continuation guidance in
  `cleanroom/20261123T105612Z-39476`. The same reviewer approved the final
  changes in `cleanroom/20261123T105744Z-90500`.
- The reviewer classified all 13 candidate paths and three session records as
  PUBLIC after complete-content review. The orchestrator verified all final
  digests. The reviewed records have these SHA-256 digests in session order.
  - `dd3796e117ffdca2742cf35326b1412b924108c27f5ac5a621876f307e9025eb`
  - `28ea36e22766d745e0b0a8517c791f93f796ae0cfeac09309da5eb99e35c709b`
  - `c3cda671c1996343085eeb2fe8486345c0293419bc76991f311d6339a250a1be`
- `private/20261123T105803Z-99718` flagged three exact public files. The scanner
  pins the reviewed SHA-256 digests. `private/20261123T105931Z-21815` passed.
  - `server/src/collection/mcp/read.rs`
    `f09f1ee57a1935968d52db4f26621afae69cb773d3a81375563d56070d48cd0d`
  - `server/src/collection/mcp/search_index.rs`
    `22ba70723d6e216a853814908300f82a1b8e7eecee8b90e82398b8dd92da27b8`
  - `specs/servers/meguri-collection-mcp.md`
    `98d13329bf646bb5eac4c637cd5bdc362e8ce89f5328c6e3b5b900d7507b4a8a`
- The final Docker server suite passed 397 tests in
  `sync-server-test/20261123T105805Z-1506`. Server lint passed in
  `sync-server-lint/20261123T105947Z-1501`. Hosted CI runs the similarity gate.
  The [verification register](../verification-status.md) tracks the inherited
  full integration failure outside this unit.
