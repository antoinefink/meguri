# MCP search candidates

The unit started at `132d81a75fb1876408494eb8c45ba9638b38c1b7`.
Deck filters now select notes by home deck before applying other predicates.
A bounded in-memory index also supplies candidates for tag and text filters.

- Implementer session `s-20261122T200713Z-4b3b3967`. Two cases in
  `sync-server-test/20261122T201407Z-8487` reproduced empty pages on
  the global scan. The deck-first proof passed 21 cases in
  `sync-server-test/20261122T202618Z-42307`.
- The integrated candidate and test-parameter correction came from
  `cleanroom/20261122T202800Z-47280` and
  `cleanroom/20261122T203930Z-86296`. The initial server run passed 384 cases.
- Reviewer session `s-20261122T203646Z-33affc53` found an empty-page path
  that could bypass a required index error, in
  `cleanroom/20261122T203709Z-76643`. Fixer session
  `s-20261122T204304Z-c2f6bd79` preserved the legacy refusal and added
  native incremental sync and cursor fit tests in
  `cleanroom/20261122T204305Z-94992`. The reviewer approved the revised
  change in `cleanroom/20261122T204857Z-4237`.
- The two review receipts classify 19 distinct paths as PUBLIC after
  complete-content review. The orchestrator verified all final digests.
  `private/20261122T204921Z-5392` flagged these three exact public files.
  The scanner now pins their reviewed hashes.
  - `server/src/collection/mcp/search_cache.rs`
    `f0f5d92a2f3d79056c19d1d4953d2d9376c4633f3ab198380db38c5ba48d82be`
  - `server/src/collection/mcp/search_index.rs`
    `eceacc44bfa21be963bb4744c75c52bd694da92ec3b77405339c754621807e58`
  - `specs/servers/meguri-collection-mcp.md`
    `6471bd285f1a03e536d22bc2fa4986a76029c9b3302b36f08483ce94aadc3874`
- Final server tests use `sync-server-test/20261122T204911Z-4849`.
  Server lint passed in `sync-server-lint/20261122T205008Z-6495`.
  The unchanged dependency inputs passed `sync-server-deps/20261122T204122Z-89247`.
  Hosted CI and similarity are the PR gates. The full integration register
  remains open outside this unit.
