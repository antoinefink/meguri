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

## U2–U5. Server, browser and generated performance fixtures

The implementation starts from the reviewed contracts and is isolated to the `mcp` worktree. `tools/mg` recorded the working sessions and preserved each original append commit. Independent reviewer `workflow_review` verified their exact append boundaries and contents. The full ledger hash at `730f297ca` is `c64ec69f7fc2567a01626b5ccde2d82704a2d9569dbe4098b39b00585c215228`.

| Scope | Filtered implementation or fix receipts |
| --- | --- |
| OAuth and collection | `20261119T144627Z-64665`, `20261119T144626Z-63624`, `20261119T150334Z-42745` |
| Streamable HTTP and collection admission | `20261119T144815Z-75123`, `20261119T150116Z-34783`, `20261119T150452Z-47028`, `20261119T151250Z-76273` |
| Browser consent and account state | `20261119T144626Z-63763`, `20261119T145911Z-19753`, `20261119T152309Z-4735`, `20261119T153222Z-58357` |
| Protocol and collection review repairs | `20261119T152310Z-5587`, `20261119T153222Z-58357` |
| Generated performance fixtures | `20261119T152417Z-13118` |

The vendor SDK experiment `sync-server-deps/20261119T145950Z-22802` resolved the pinned `rmcp` release but failed policy on its unconditional `chrono` dependency. The custom adapter uses the existing HTTP and serialization dependencies. The lock was restored before the card-position dependency was resolved. Its subsequent locked graph and advisory review passed.

Independent clean-room passes `20261119T145357Z-93571`, `20261119T150618Z-52478` and `20261119T151249Z-75998` identified identity races, missing real route coverage, an unindexed sibling query path, omitted tags being rewritten, late token challenges, and legacy negotiation. The browser binds its display to the authorizing session. The final server routes are included in real loopback tests. The repair exports above address the indexed query, tag preservation, OAuth challenge, and negotiation findings. The browser's account-change errors follow the same restart policy as the server's refusal. Earlier browser failures remain recorded; the revised browser candidate passed the full existing account journey.

- `sync-server-test/20261119T152507Z-18975` passed 248 cases on the mounted server before the final review and browser feedback changes.
- `sync-web-verify/20261119T153414Z-64251` passed nine frontend checks and 42 browser cases using generated accounts and test-only client responses.
- `sync-server-mcp-bench/20261119T153614Z-73452` measured approximately 20 and 200 MB generated fixtures in the shared release Docker environment. The larger fixture's highest warm p95 across three workers was 0.44 ms exact lookup, 4.89 ms one-field edit and 4.13 ms note and card creation. The retained SQLite writer returned busy within 0.38 ms across both sizes. Warm read appended zero WAL frames. The p95 edit touched five frames on both sizes. Incremental worker memory was roughly 6 MiB on both. The run used a warm OS cache and shared Docker resources. Full M7 acceptance still needs the dedicated environment and transport measurements.

At this stage, the server blocked production writes that would create cards. Local tests used generated databases.

### Approved consent and final draft verification

Antoine approved the compact single-card consent flow, stacked permission choices and compact connection rows. The requested typography adjustment uses body-text size for the client line on the MCP sign-in card. Filtered run `20261119T154144Z-81677` applies that change and aligns the browser's session-change test with `403 capability_invalid`. `20261119T153840Z-78088` supplied the stale-consent handling and test-hook build separation for the opt-in benchmark.

- `sync-server-test/20261119T154356Z-91467` passed all 256 tests on the final source with no skips.
- `sync-server-lint/20261119T154528Z-91429` passed on the same server source.
- `sync-web-verify/20261119T154355Z-90093` passed all 51 checks on the final source, including 42 Chrome cases and nine frontend checks. The approved sign-in typography and other screenshots came from this build.

`workflow_review` and `mcp_agent_strategy` independently read the complete candidate files flagged by the local privacy scan. Both classified the scoped implementation, specification and generated test fixtures as public. The orchestrator matched the reviewed bytes to the whole-file SHA-256 entries in `tools/scripts/private-scan/private_scan.py`. This includes the browser specification digest `df95f9d7c15f66688c40b1890d115c30cf486ce1ab899adfc2a170714ceaacb4` and the normal test module digest `f025e206aa100c5bc0a62117c9d250dc9c74630c02b6fa2f38cc79f169e228bf`.

### Production note creation and card generation

Antoine approved the existing timestamp IDs and native conflict rules for rare offline ID overlaps. Starting commit `c7caf3301`. The C7 amendment enables creation and card-generating edits under those rules. It keeps the native wire format and verifies incremental delivery with distinct IDs.

- Filtered implementation `20261119T160905Z-70906` and independent read-only review `20261119T161347Z-77135` found no remaining review defects. The requests use the regular OAuth write scope and existing guarded transaction.
- `sync-server-test/20261119T161410Z-78657` verified creation through the mounted OAuth route and two native incremental rounds. The first delivered the new note and card. The second delivered the edited note and added card, excluded the unchanged sibling, and checked stable IDs, tag and counter state, and successful sync finish.
- `sync-server-lint/20261119T161609Z-80251` passed. The browser source is unchanged from the approved verification above. Live ChatGPT and Claude sessions, dedicated performance acceptance and the integration gate remain pending.

`workflow_review` independently classified the complete plan, amended specs and seven source files as public. The allowlist records the exact digests for the files with existing entries. The reviewer verified both ledger appends as public operational metadata. The record SHA-256 values, including newline, are `c9b266847f9f4d12351119a82a8057f807c4bac64616396c9bda6f6ae9a769ad` at `61c7a4521` and `b037b6a5e5826ecce02e4f3218e9b597978f7fc655d77154b35b7a963ef329e0` at `8775779ea`.
