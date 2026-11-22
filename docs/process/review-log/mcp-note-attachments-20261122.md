# MCP note attachments

Unit base `cb7aea87be6dac7685e7cc640bc433bfe313853f`.

Independent clean-room reviewer `s-20261122T122552Z-08b55794`, run `20261122T122609Z-31480`, reviewed each complete file without private inputs. The reviewer classified the plan, the protocol contract and both session records as PUBLIC. The orchestrator verified the exact repository paths and hashes before admission.

| File | SHA-256 |
|---|---|
| `docs/plans/2026-11-22-1323-feat-mcp-note-attachments-plan.md` | `1b50dcfe066bc5ea38249c6497213d673548660320dd70adf558b681c0a8ba69` |
| `specs/servers/meguri-collection-mcp.md` | `b00adf15f47480f696314faa426bd7463d42b1a8beb99aab808d89d6df662599` |
| `docs/process/provenance/sessions/s-20261122T122224Z-c1dc9d57.json` | `37935ece653e881d255cf60945827dd7c26682f6a1a6c2549b67e39685d705fc` |
| `docs/process/provenance/sessions/s-20261122T122552Z-08b55794.json` | `0f84a9fbcec02da7cb5f34019c3fd19647e19867e725ce5b5491e27e0943508f` |

The clean-room implementation exported run `20261122T122521Z-27788`. The baseline MCP selection passed 42 cases (`sync-server-test/20261122T122757Z-43707`). The independent code review in `20261122T124535Z-92718` found the old raw HTTP argument parser, the unresolved Rust TLS graph, and missing account-switch recovery on the upload page. It reviewed its whole session record `s-20261122T124521Z-bab36064` as PUBLIC, SHA-256 `cfaf9db864912bc7501f82afc028c1d313ec21783e852f75501ccaae5e1d8f15`.

Retained red receipts on the first candidate include `sync-server-test/20261122T124642Z-97010` (50 passes, 3 HTTP argument failures), `sync-server-deps/20261122T124641Z-96532` (license policy for `webpki-root-certs`), and `sync-web-verify/20261122T124642Z-96725` (exact frontend build input assessment). No hosted attachment or offline app result is established by these runs.

Independent privacy reviewer `s-20261122T124916Z-5424be51`, run `20261122T124928Z-7063`, reviewed the complete staged candidate source files below. The reviewer classified each as PUBLIC. The two additional session records were also PUBLIC.

| File | SHA-256 |
|---|---|
| `server/src/accounts/oauth/policy.rs` | `802fd74612b1ae0d70b3ed32106b873b5e6dbd1824795eddbd40fa846ab16e46` |
| `server/src/mcp_service/authority_tests.rs` | `b48e4c5f7d8b2ad2e744391bac51acc03e70366c66a6b80f1d66f9edff343f22` |
| `server/src/mcp_transport/inputs.rs` | `35fcbf999f5235196af4ae965cd2bf75b9482e5bfc83f16a566260f35d577522` |
| `docs/process/provenance/sessions/s-20261122T124809Z-a711e9e6.json` | `2f0e9c9b031bbefa73c9b9a0c302b34f1cfd15ce9de9edc1cd681a048b7cfc82` |
| `docs/process/provenance/sessions/s-20261122T124916Z-5424be51.json` | `24e0e2c54554da6b2a744a2b149ac611ccaaf0f1c9e09c1928b1bab9840a2d60` |
