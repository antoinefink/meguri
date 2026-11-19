# Meguri collection MCP review

## U1. Protocol and storage contract

Starting commit `d4c790116ad358f74645178d422ee5bd9d95f887`. Spec-writer session `s-20261119T143458Z-1266175d` used published protocol documentation and Meguri's neutral specs. The original ledger-only append is `2b81e5b0b27a36a69e1cfbb7edde3f0b2ecaaa55`.

- `workflow_review` independently reviewed the complete appended ledger record and confirmed it contains public operational metadata. The record SHA-256, including newline, is `2826cf52a0f6ac72dade5f3097f839df6653bc967d19b65a76f2b4cd5dd3abd1`. The complete candidate ledger SHA-256 is `1ee4687cf45a516700eb3223d534c841f1226b713a37f8846a20b6991c14e36e`. The parent ledger is an exact byte prefix.
- Independent plan and contract review found conflicting client metadata lookup instructions and a possible gap between grant invalidation and SQLite commit. The plan now requires exact reviewed local client records. M3 and W10 require a shared account publication barrier for grant revocation, valid credential replay and collection write commit. The plan includes the exact SPA route and a dedicated synthetic TLS fixture for hosted verification.
- The existing native merge rule matches notes by ID. C7 requires generated mixed-client collision and position evidence before creation can ship. Matching an offline note's ID while its GUID differs may require a native amendment. Hosted identity and release-package evidence remain pending.

The initial `sync-server-test` setup receipt `20261119T143609Z-19955` failed before tests because the worktree's dependency volume lacked the frontend lock. After pinned image setup and locked install, `sync-server-test/20261119T144003Z-34579` passed 9 existing auth and normal-sync tests. The existing browser checks passed 9 baseline tests. These runs cover the existing implementation while the documentation was in progress.

Public-artifact review covered each complete staged file. Reviewers found public protocol, product and test design only. The orchestrator verified the staged bytes against the reviewed SHA-256 values.

| Reviewer | File | SHA-256 |
| --- | --- | --- |
| `workflow_review` | `docs/plans/2026-11-19-1436-feat-meguri-sync-collection-mcp-plan.md` | `ece3faa526ba7a9002cdba8ef7e08845b64b6ca4c0a8cbc35e8fb36db06710fd` |
| `workflow_review` | `specs/servers/meguri-collection-mcp.md` | `b2e41383f691526431cbc231f4c1b9480868bfc70bdca4d02c861de016921e8a` |
| `mcp_agent_strategy` | `specs/servers/meguri-sync-web.md` | `c181fec0d6563e2a84897264435607d980b181492dc00f8602cc6015495e4b04` |
| `mcp_agent_strategy` | `specs/sync/server-collection.md` | `bf5f4248cb2a7126c58c4177873128c3b3d33ba2d0f155bac68ffe0dbe9d4eeb` |
