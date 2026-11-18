# Meguri Sync account storage and navigation

Starting commit `3519d1cf8`. Implementer session
`s-20261118T125429Z-84cae6e7`. First filtered export
`cleanroom/20261118T125430Z-52828` and independent full source review
`cleanroom/20261118T130316Z-61251`. The review returned clear and classified the
complete files and the 369-byte session record as public. Session row SHA-256
through LF `86d9a8edd349e24c41f19378f3f5703b3cf7d196a89fcf0b4a054b4c24bfcc03`.

Filtered refinement `cleanroom/20261118T130359Z-62611` keeps the original page
order and groups the account name with Sign out. Public-content review
`20261118T130715Z-65885` passed. Existing provisioning status covers new
accounts. Ready rows without a measurement show Storage unavailable.

Exact complete contents were independently classified public and verified before
refreshing existing exact-path privacy admissions.

| File | SHA-256 |
| --- | --- |
| `server/src/accounts/administration.rs` | `e79a9f333859f871966824e8decc9ea7f4eb1c7ecd28dc8aaf083d7d7ef516d0` |
| `server/src/accounts/mod.rs` | `ef5424f00837cb30fa88d85b41113f691d00cd941ad6b5ede3d3e70c71547b3c` |
| `server/src/media/index.rs` | `2b9b71b7037f080b67f555d5f87230c2cf94b42213464eddf742aa5edf01bf27` |
| `server/tests/accounts.rs` | `07099dc333f7beec973c5cb3856f362cf5097130124387ec11ba15d06b95aea8` |
| `server/tests/media.rs` | `819514b9d982d4062e7550645ae89e176dd47bcaa8083951ed3521aa1ace79b8` |
| `server/tests/provision.rs` | `cf31868be65159387c985c86cc6a07ea070febeff016406a0994c49ae0ef272e` |
| `server/tests/races.rs` | `98b2497f3c53602acef2aa8b5f9f4e4355da07409620577b2111066f747b12be` |
| `server/web/scripts/audit-assessment.json` | `7cf6314ddc6f5fd632a1524c7d62288cbd074e871392bd67c4d351b33880b62f` |
| `server/web/src/accounts.tsx` | `1f275369e73f55839205dd05016349add98d350e339e6e8bd3046064310a10f2` |
| `server/web/src/api.ts` | `e91452dfd81eadc35c905261d2d501c01e84da14c424ff6ec9085272aac47934` |
| `server/web/src/app.tsx` | `4d77e6929b7b62893dd6c436c86865bc53ce885bd32933fb58670b5a6a68b5f3` |
| `server/web/src/style.css` | `3fef8a9053c040b755bcc36ec2edd1fc8a159cd2b220ed4c6c6d480ccfa3b04d` |
| `server/web/tests/browser/accounts.spec.ts` | `430ae2c48ad6f123095322cc16f4c0dc877ff32985f118bb32a137096a786193` |
| `server/web/upstream-records.json` | `d01a9499a2330268d6c0a7aa495216e2163ed49946a12441c1e16574f8800c2f` |
| `specs/servers/interop-vocab.txt` | `eecb09a9a42d1b497d6b140775da919fffdb23983f7eb60180cf1f75d0d10743` |
| `specs/servers/meguri-sync-web.md` | `d9d3c5be056dd56eda6414ea3ed58a9e6eded6467c07c13e0f4cf80d08f333d6` |

This work awaits the full integration gate and design feedback.
