# MCP SQLite collection download

Unit `mcp-sqlite-download`. Starting commit `b0ef4c69540b527c1b7b83126d0d0b46cc7cb2d5`.

Filtered implementation `20261123T095645Z-67877`. Review found that account reset could race stage creation, OAuth refresh retains a still-valid access token, and SQLite backup needs a destination page size that matches the source before starting WAL. Fixer exports `20261123T100943Z-17214` and `20261123T101352Z-58415` address these findings. The browser page now explains how to recover from a refusal. The new 65536-byte WAL fixture checks stand-alone SQLite integrity with the original schema collation and checks that source file bytes and sync stamps are unchanged.

Independent filtered review `20261123T101613Z-33352` found no remaining code defects in the combined candidate. It verified that the frontend assessment fingerprints identify source inputs and that the dependency lock, build policy, and trust roots are unchanged. Plan reviewer `/root/plan_review` classified the complete initial plan PUBLIC. Reviewer `/root/storage_review` reviewed the complete export and authority tests in filtered source view `20261123T101613Z-33352`. Session `s-20261123T100532Z-dfd9ce23` reviewed the complete protocol contract and four session records. Each reviewer classified their listed files PUBLIC. The orchestrator matched each repository path and digest before admission.

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

Fixer export `20261123T101856Z-23605` separates OAuth mutations into fresh generated harnesses and uses the supported browser form shape. Reviewer `/root/storage_review` classified these complete files PUBLIC in source view `20261123T101856Z-23605` and matched the browser test fingerprint to the source.

| Path | SHA-256 |
|---|---|
| `server/web/scripts/audit-assessment.json` | `e202d8efa1c0929ab386f274d27c8af6d764839a9d5464c67da664efdd26aafe` |
| `docs/process/provenance/sessions/s-20261123T101826Z-de158eb1.json` | `c13bb0e4148d289d79b90d39ec83327f1f4ca2e98e4c7ddfbbc16868f7665c71` |

The corrected Docker server selection passed all 25 cases in `sync-server-test/20261123T102054Z-90808`. Server lint passed in `sync-server-lint/20261123T101942Z-51901`. The staged private scan passed in `private/20261123T102130Z-15622`. The hosted pull request runs similarity against the pinned corpus.

`sync-web-verify/20261123T102055Z-91951` passed all 74 checks, including the real OAuth grant, MCP call and Chromium attachment save over the synthetic HTTPS server. Static checks passed all six checks in `static-checks/20261123T102207Z-36735`.

## Automatic download revision

Antoine requested a fully automatic agent download. Filtered implementation `20261123T102946Z-84648` delivers the file using a dedicated one-use bearer issued by the existing read tool. The agent downloads it directly from the fixed HTTPS endpoint. This revision removes the earlier browser handoff. Accounts using the ordinary OAuth consent page see the revised access description.

CodeRabbit on PR #156 asked for an exact replay rejection assertion, enforcement of the transfer deadline on buffered bytes, and named privacy reviewers. Fixer export `20261123T103621Z-13434` verifies that the competing request receives `capability_invalid` and terminates delivery at the common absolute deadline. It also fences stage admission on the original access grant and returns HTTP 409 if publication changes while copying.

Reviewer `/root/plan_review` classified the complete revised plan PUBLIC. Reviewer `/root/storage_review` reviewed the complete source and contract inputs in filtered source view `20261123T103925Z-280` and the three new session records. Each was classified PUBLIC, and the orchestrator matched the exact file and digest. The frontend assessment updates three source fingerprints and retains the dependency lock, graph, trust roots, and audit logic.

| Path | SHA-256 |
|---|---|
| `docs/plans/2026-11-23-feat-mcp-sqlite-download-plan.md` | `4a2022dfcd79da7351481e9bf42aa12f11bf9bfed747fbadd9aace968abcdc32` |
| `server/src/mcp_service/authority_tests.rs` | `ea295605b6ca19fdb94c5a836b46819ccf788554a446508cc0efbe201ded09f7` |
| `server/src/mcp_service/export.rs` | `977e84072894916f117bf0be798e94ccb28cae691943ea84873ed8bcf0143fd2` |
| `specs/servers/meguri-collection-mcp.md` | `e66ee71591d949fe299d4c744046c5528b83148ea10a2db7381e09496d2799d7` |
| `server/web/scripts/audit-assessment.json` | `17779a1e91c303db26256265114fbd015627340676ad9e56c957330e629b9077` |
| `docs/process/provenance/sessions/s-20261123T102845Z-df0595e8.json` | `133e0db5b62ca33da5fbb6b097361c951583405cf96df84580efc76b93328de9` |
| `docs/process/provenance/sessions/s-20261123T103546Z-044750ec.json` | `5c65d9f8d424732430c6515a4c2e044c41db37f7bc918dd4a605f5a67a7565bd` |
| `docs/process/provenance/sessions/s-20261123T103547Z-55ca5d89.json` | `6ec5827fae11e40b7aabf01f5a3f2917515d5fd2a9bc96d316af76ea87aae01d` |

Independent clean-room review `20261123T103925Z-280`, session `s-20261123T103547Z-55ca5d89`, found no actionable code findings. Docker server runs `20261123T103959Z-17899` and `20261123T104306Z-78749` passed 27 and 2 tests, including automatic HTTP redemption, SQLite integrity, replay, generic authentication and deadline handling. Server lint `20261123T104057Z-17892`, web verification `20261123T103958Z-17158` (69 checks), and staged privacy `20261123T104225Z-65599` passed. GitHub CI verifies the branch and similarity gate.

The review summary also identified missing WAL frame and index overhead in the temporary-byte reservation. Filtered fixer `20261123T104615Z-74065` added checked accounting for those bytes before staging. Docker run `sync-server-test/20261123T104809Z-40655` passed all 15 focused cases, including the new reservation proof. Reviewer `/root/storage_review` inspected the complete final files from the same filtered view and classified both PUBLIC.

| Path | SHA-256 |
|---|---|
| `server/src/mcp_service/export.rs` | `de4bcaceea675b072c5b13569ec3b347c838cd359af8866326b5900b3696c551` |
| `docs/process/provenance/sessions/s-20261123T104551Z-bf3cdc82.json` | `e2356f222776a75a84d4bb925cf1b4cc8d9bdaba07e6805d7e16a2a1d5a7e92c` |
