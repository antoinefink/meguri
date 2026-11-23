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

## Search budget and concurrent calls

Follow-up base `52a0131f4ca52e82ad8298f75127c40804236a93`. The cold scan
inspects up to 150,000 notes or 128 MiB with a 500 ms scan deadline. The
endpoint admits 12 calls. Collection work on one account has a separate
six-second MCP queue.

- Implementer `s-20261123T112905Z-2efe0c6e` exported
  `cleanroom/20261123T112922Z-95812`.
- Reviewer `s-20261123T104930Z-8cb19270` approved
  `cleanroom/20261123T114116Z-85625` and classified the complete contents
  of all 13 candidate paths and the new session record as PUBLIC. The
  orchestrator verified every digest. The session record SHA-256 is
  `9ad4db60ba1a9e2cc3bc85daadec6db753b56369ffd37d1c2f5afac396a13fbc`.
- `private/20261123T113702Z-61166` flagged two pinned paths. Their final
  independently reviewed SHA-256 digests are pinned.
  - `server/src/collection/mcp/read.rs`
    `9a46822ad1682fee5b4226c231efd779b9171e883e75610c1bb0276933c9536d`
  - `specs/servers/meguri-collection-mcp.md`
    `7dcfcbc8665087094f0fe9b7d6ec29d608b14bfda2a7696bee21eef464b087e5`
- `sync-server-test/20261123T113706Z-63818` passed all 400 tests, including
  ten simultaneous MCP HTTP calls, three durable creates, authority
  rechecks, queue timeout and cancellation. Server lint passed in
  `sync-server-lint/20261123T113835Z-98781`.
- The Docker release probe `sync-server-mcp-bench/20261123T113845Z-4584`
  completed 30 measured searches per case. At 126,340 generated notes,
  the late match and absent term each took one call. Their backend p95
  latencies were 67.82 ms and 71.82 ms. The server suite also checks
  cursor traversal when the production deadline ends the page.
