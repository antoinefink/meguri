# MCP SQLite collection download

Unit `mcp-sqlite-download`. Starting commit `b0ef4c69540b527c1b7b83126d0d0b46cc7cb2d5`.

Filtered implementation `20261123T095645Z-67877`. Review found that account reset could race stage creation, OAuth refresh retains a still-valid access token, and SQLite backup needs a destination page size that matches the source before starting WAL. Fixer exports `20261123T100943Z-17214` and `20261123T101352Z-58415` address these findings. The browser page now explains how to recover from a refusal. The new 65536-byte WAL fixture checks stand-alone SQLite integrity with the original schema collation and checks that source file bytes and sync stamps are unchanged.

Independent filtered review `20261123T101613Z-33352` found no remaining code defects in the combined candidate. It verified that the frontend assessment fingerprints identify source inputs and that the dependency lock, build policy, and trust roots are unchanged. Separate non-author review of the complete filtered plan and files below classified them PUBLIC. The orchestrator matched each exact repository path and digest before admission.

| Path | SHA-256 |
|---|---|
| `docs/plans/2026-11-23-feat-mcp-sqlite-download-plan.md` | `7dc97a6d86b11ba4c9012945276e30b3589206b06a251fb5fba1819ba31fe0ca` |
| `specs/servers/meguri-collection-mcp.md` | `b73722161e727e78866ec9bb37aaa7498af8210f4ac0ce44972c037174da432a` |
| `server/src/mcp_service/authority_tests.rs` | `3867922e6a1a6fcc7b0a7847fa4fb281c5a720f1958d46d8290bf5a8c1cd1087` |
| `server/src/mcp_service/export.rs` | `ba83e50198cfd37d616d7aee17e65f378db3236cb0096a97558518ceef1c1d29` |
| `docs/process/provenance/sessions/s-20261123T095424Z-c39f9305.json` | `ccb2c429938eb8befd35780a752e3857c29feb34479f427355bfb144d1afd615` |
| `docs/process/provenance/sessions/s-20261123T100531Z-989dc708.json` | `e3b74ba82cfc68e48306be5f879818c5d964cb334196f261e155ba0c152041e9` |
| `docs/process/provenance/sessions/s-20261123T100532Z-dfd9ce23.json` | `5c3368283cd495139c31b2628f3a6b3143cb4d349064d23694404de24b064238` |
| `docs/process/provenance/sessions/s-20261123T101318Z-93c67d21.json` | `389931a83fb02b888c88624a3bd5ec800c8ea1136f0386fbb0c1d80fe4eddd3f` |

Baseline Docker runs passed 14 OAuth cases (`sync-server-test/20261123T095934Z-68273`) and 69 browser and unit checks (`sync-web-verify/20261123T101426Z-76630`). Candidate runs `sync-server-test/20261123T101136Z-83937` and `sync-server-test/20261123T101648Z-54413` exposed incorrect fixture assumptions about the registered collation, OAuth refresh, and the account attempt limit. `sync-web-verify/20261123T101648Z-54292` found an unsupported Playwright form argument type in the browser test.

Fixer export `20261123T101856Z-23605` separates OAuth mutations into fresh generated harnesses and uses the supported browser form shape. A non-author reviewed these complete final files as PUBLIC and verified the new browser test fingerprint against the source.

| Path | SHA-256 |
|---|---|
| `server/web/scripts/audit-assessment.json` | `e202d8efa1c0929ab386f274d27c8af6d764839a9d5464c67da664efdd26aafe` |
| `docs/process/provenance/sessions/s-20261123T101826Z-de158eb1.json` | `c13bb0e4148d289d79b90d39ec83327f1f4ca2e98e4c7ddfbbc16868f7665c71` |

The corrected Docker server selection passed all 25 cases in `sync-server-test/20261123T102054Z-90808`. Server lint passed in `sync-server-lint/20261123T101942Z-51901`. The staged private scan passed in `private/20261123T102130Z-15622`. The hosted pull request runs similarity against the pinned corpus.

`sync-web-verify/20261123T102055Z-91951` passed all 74 checks, including the real OAuth grant, MCP call and Chromium attachment save over the synthetic HTTPS server. Static checks passed all six checks in `static-checks/20261123T102207Z-36735`.
