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

Fixer export `20261122T124852Z-4821` corrected the raw parser, pinned the original media directory, moved blob work out of the shared metadata writer, and rechecked authority before note commit. It added recovery for a signed-in member using the wrong account. The staged privacy scan passed in `private/20261122T125801Z-42397` after matching the final candidate bytes to the reviewed hashes. The locked dependency gate passed in `sync-server-deps/20261122T125738Z-39783`, server lint in `sync-server-lint/20261122T130257Z-67871`, and static checks in `static-checks/20261122T130256Z-67479`.

The independent follow-up `20261122T130312Z-69493` confirmed the parser, publication, and account-switch repairs. It checked the complete fixed frontend input inventory and found no new path from an uploaded file or account input to the build-time matcher. It found the exact consent locator and the package-specific license validation still needed repair. Retained receipts `sync-server-test/20261122T125738Z-40033` and `sync-web-verify/20261122T125737Z-39423` record the OAuth fixture rate limit and stale consent description. Fixer export `20261122T130236Z-61847` preserves the checks with independent grants and the new exact description. Full server `sync-server-test/20261122T130402Z-72483` passed 295 cases including exact native note and media delivery. Web `sync-web-verify/20261122T130401Z-72215` passed 65 checks, including all 55 browser cases.

On clean `c6304477c`, package receipt `sync-server-package/20261122T130445Z-76610` records that package context rejected the crate-specific notice accepted by Cargo's dependency gate. Hosted attachment handoff and offline Apple study playback remain separate verification items.

On clean `f5215305ff`, `sync-server-package/20261122T131012Z-93993` passed the exact release-image assembly and inventory. `sync-server-package-test/20261122T131012Z-93187` passed all four smoke cases and the packaged browser run passed all 55 cases. `meguri-provider-real-server/20261122T131643Z-19995` passed six phases on the same packaged image, including review on the sender, exact media and review-record equality on the receiver, and cold reopen.

CodeRabbit on PR #124 identified two valid format checks that refused JPEG application segments and MP3 ID3 metadata beyond 64 KiB. Fixer export `20261122T132515Z-47396` keeps the file-size, marker, fill-run and frame checks while allowing metadata to precede those headers. `sync-server-test/20261122T132702Z-54149` passed all 298 cases, including the three regressions for large metadata and truncated inputs.

## Expanded attachment formats

Independent complete-file privacy review `20261122T133721Z-98983` by `s-20261122T133702Z-023bbfaa` classified the updated plan and contracts and the new session record as PUBLIC. The orchestrator matched the exact bytes before refreshing the existing plan and MCP contract pins.

| File | SHA-256 |
|---|---|
| `specs/servers/meguri-collection-mcp.md` | `027bf623ffb399d420722072fbcbde82a0feb7c2d5eaaf44675b8d787bf5ca26` |
| `specs/render/card-behaviors.md` | `7e9c28e1886e1d250f21c859a6b5b4eb38c34acc66a8d7b2704793af2e513b90` |
| `specs/render/media-serving.md` | `5d8eac20f1081ab18f64578914726bf45d916cfae0fe85b852fb859239f18264` |
| `docs/plans/2026-11-22-1323-feat-mcp-note-attachments-plan.md` | `81191321d7af0a18c682b5bff490ea26beea3ccfa4ce1ad28b53f38dcc5f1adb` |
| `docs/process/provenance/sessions/s-20261122T133702Z-023bbfaa.json` | `bd5ff072c55e8847ff96e6e2686d1e5bb884e5a67c74f2181cbc5ce78ec095b5` |

The server export `20261122T133414Z-84582` expanded format validation and the picker. Native export `20261122T133518Z-89872` added a bounded local PDF viewer. Independent reviews `20261122T134358Z-27613` and `20261122T134813Z-41324` flagged case aliases for synced PDFs and valid PDF opening destinations and stream syntax. Fixer exports `20261122T134646Z-35849` and `20261122T135321Z-62504` cover those cases. The scanner still refuses controlling PDF actions and the native reader still requires a trusted click and current card binding.

The first expanded server run `sync-server-test/20261122T134937Z-50671` passed 305 of 306 tests. Its short-prefix loop included a complete AAC frame. Fixer export `20261122T135245Z-59585` corrected the fixture and tested both a complete short frame and a partial frame. `sync-server-test/20261122T140040Z-91866` and `sync-server-test/20261122T140924Z-29433` both passed all 312 cases. The second run followed the fixture module cleanup, which corrected a duplicate-module finding from `sync-server-lint/20261122T140418Z-10113`. A final import adjustment places the integration fixture only in its consumer. The expanded browser run `sync-web-verify/20261122T140039Z-91748` passed all 59 browser cases.

The native compiler found a weak local variable that was never mutated. Fixer export `20261122T140351Z-8607` uses a weak task capture. `swift-test/20261122T140512Z-13726` passed seven offscreen PDF attachment cases, including cancellation, tracked reads and forged clicks. It exercises a recording presenter. The iOS Debug build `build-app/20261122T140515Z-15086` passed without warnings. The iPhone simulator run `unit-sim/20261122T134150Z-19536` queued because the test VM is unconfigured. A hosted file handoff and hands-on native PDF presentation still need verification.

Independent non-author `s-20261122T133702Z-023bbfaa` reviewed each complete file below as PUBLIC. The orchestrator matched all exact candidate digests to the reviewed files.

| File | SHA-256 | Review |
|---|---|---|
| `server/src/mcp_service/attachments/format/audio.rs` | `0950666b1b175bd5839f5ff9d830e5e048d44bfcbdec13804b52796ef408d9b3` | `20261122T140020Z-90436` |
| `server/src/mcp_service/attachments/format/isobmff.rs` | `b337b94d622f4e69e0f3851b76cc76342a3ca9a52f1c556236a083ce3d702e2c` | `20261122T140020Z-90436` |
| `server/src/mcp_service/attachments/format/tests.rs` | `1fb734dd0b26e06e4048f28f7ddb802bb2aadcbb0bdf12e504c6431f26649d28` | `20261122T140849Z-28060` |
| `server/tests/support/attachment_formats.rs` | `ddf1bbee4b1a5515b011ff3c8d9b1957424ad552faf99f23b5dddb5b4754bbb0` | `20261122T140020Z-90436` |
| `server/src/mcp_service/authority_tests.rs` | `7714d0522108e89ef351884762e6557f17ad1184e46cbf0a06eb7e68be35453c` | `20261122T140849Z-28060` |
| `server/src/mcp_transport/backend.rs` | `4a0708c96150f2a28f5c68a493898163eee4c51bd0d52088b3c74f751e07ae62` | `20261122T140335Z-7608` |
| `server/tests/mcp_oauth.rs` | `f24e5bf89ad95dd53b90d300bebe3d1b6318c41c082171361f90a5c49441db75` | `20261122T141226Z-39714` |
| `server/web/scripts/audit-assessment.json` | `5333a1f1e2980c2c8559cc912026c2774aa68aef86df15df246a0ac84a1893ee` | `20261122T140849Z-28060` |

## CodeRabbit follow-up

PR #124 review on `2b299778b` raised eight inline findings. The native viewer now supplies a retained PDFKit delegate that suppresses URL and remote-document actions, and the scheme handler denies reserved PDF filename aliases containing default-ignorable scalars. Each viewer retains its internal PDF navigation. The render vocabulary includes the PDF signal. Independent source review `20261122T143224Z-22035` confirmed the delegate boundary. `swift-test/20261122T144543Z-70283` passed all nine focused tests, and `build-app/20261122T144541Z-69973` passed the iOS build without warnings.

The server checks cover unique Ogg chain serials, bounded WebP animation geometry and extension chunks, AVIF item-specific codec properties, and self-contained M4A sample tables that resolve to the uploaded bytes. `sync-server-test/20261122T143528Z-33469` passed all 22 focused tests at the Ogg/WebP stage. `sync-server-test/20261122T144742Z-78304` passed all eight final ISO tests, including long audio tables; `20261122T144628Z-74838` reviewed the final ISO file. Exact PDF stream boundaries now use declared cross-reference lengths, including incremental revisions and compressed length objects. The intermediate run `sync-server-test/20261122T143823Z-42948` passed 324 of 325 tests. Its wrong-length case counted a tolerated separator, so the final fixture also exercises genuinely truncated or excessive lengths. The next run `sync-server-test/20261122T145122Z-90094` passed 331 of 332. Its object stream fixture now distinguishes valid leading whitespace from a truncated header. The full follow-up checks are tracked on the PR.

Review runs `20261122T143743Z-41455` and `20261122T144628Z-74838` checked the complete applicable source files. The following are the final candidate digests for the format repairs.

| File | SHA-256 |
|---|---|
| `server/src/mcp_service/attachments/format/audio.rs` | `1eca9c96b3c638ad66723119d54ed0537246a3fedcd3fe6fd9acb5e8fa8043a9` |
| `server/src/mcp_service/attachments/format/images.rs` | `988ce9459cf9172fe1c71f5b92914c53f1da7d55b9a513a40b466e935cc422be` |
| `server/src/mcp_service/attachments/format/isobmff.rs` | `86d7f0acb3ad4ebead3c807876593a2ecd166fcaf59e33c8127ff6778bb393e9` |
| `server/src/mcp_service/attachments/format/pdf.rs` | `45ff26c7b441c3ce02c09a6a62ed1a4278e6b1cd91259ee50860023bf49097c2` |
| `server/src/mcp_service/attachments/format/tests.rs` | `21da443e4142d955f93c2cb387194a7858d7fbb5064afe920fe495506238d659` |
| `server/tests/support/attachment_formats.rs` | `ff6d752781665d9c0a80403079e653681629b52c1e384c834df4b3952fa76522` |
