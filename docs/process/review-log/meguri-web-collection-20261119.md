# Meguri web collection and mobile navigation

Starting commit `6561d3456503750c1951b185df6882326a1d995d`. Implementation
session `s-20261119T070146Z-4b41f2e4`. Final source export
`cleanroom/20261119T085603Z-76572`. Antoine approved the rendered changes
and requested integration on master.

Collection leads the signed-in navigation and shows the current account's
collection database, media and total sizes. The Meguri header opens
Connection. The account header shows the username, and confirmation and
error banners use even padding without a status dot.

The initial banner check `sync-web-verify/20261119T085516Z-70812`
still required a dot. The revised check asserts its absence and keeps the
existing border, color, keyboard and accessibility checks.
`sync-web-verify/20261119T085738Z-81221` passed all ten checks against the
compiled server. Rendered 320px and 390px views supported the layout review.
`sync-server-test/20261119T090932Z-39480` passed all 194 cases on the
reviewed source in the main checkout.

Independent reviewer `registry_review` read the complete contents of the
21 changed source files, verified the frontend audit and upstream bindings,
and classified the files as public. The spec review aligned W1, W3 and W9
with the approved browser behavior. Staged scan
`private/20261119T091013Z-45268` identified the app shell, browser test
and spec. The following complete files were reviewed for exact public-artifact
digests.

| File | SHA-256 |
| --- | --- |
| `server/web/scripts/audit-assessment.json` | `df15913462bed471cf7ad98ea1a2ed59e098dc579d29525b17633a0c51448b02` |
| `server/web/src/app.tsx` | `ba611ea71b0c038c66cbd2d544120318e20fb37b6edcf164daa99f6328ec5128` |
| `server/web/src/style.css` | `6a0fdbdf6a892c051129c554b5b940271763bfda3641b0a1fecfa5f5b7583e9f` |
| `server/web/tests/browser/accounts.spec.ts` | `0beb4817bf0fe207d83cc018a3425c2b294a755d266ff1e43672c641cb74bd56` |
| `server/web/upstream-records.json` | `d58d484cde4905b07a9e188632341dbd03a9bd5a5b287d9f2b95461b27d233ea` |
| `specs/servers/meguri-sync-web.md` | `e10d6dd3c36b9f286d78576d72d059df0afadef296c5ec271ed180fca9f953eb` |

## Package screenshot evidence

Starting commit `97146fc1fe28e9a9bed4b96fcd9fe3eb8b07a195`. Package smoke
`sync-server-package-test/20261119T091456Z-71202` passed the browser and
native seed/check phases. Its full exact-image browser child
`sync-web-verify/20261119T091831Z-89015` passed the journey, then the
receipt check refused fifteen audited screenshots because it expected
nine. The first fix required each named nonempty regular file.
Verified the fifteen names against the retained exact-image captures.
Independent reviewer `registry_review` reviewed the complete tooling
export `cleanroom/20261119T092120Z-6816` and the final symlink guard.
The reviewed file on master has SHA-256 `ef5c3c4caa1ee5a6417fb6246cc7d0ed243cb40d98acfd1a0595ffbeaf6515d5`.

The first packaging attempt with the revised wrapper
`sync-server-package-test/20261119T092531Z-35737` stopped at the
frontend build-control audit. The assessment now binds the exact
revised wrapper bytes.

## Release-check simplification

The Playwright audit awaits screenshot writes directly to the retained
output directory. At Antoine's request, the packaging wrapper relies on
those writes and shares the existing report validation with the fixture
lane. The two lanes report the actual passing test totals from the
validated reports. The separate screenshot inventory and fixed test
totals are removed.

Starting commit `89935a9e7`. Filtered export
`cleanroom/20261119T093457Z-88457`. Independent reviewer `registry_review`
verified these complete public files.

| File | SHA-256 |
| --- | --- |
| `tools/mg.d/sync-web.sh` | `5eac87b061115351ba2287844f0c26425e20d1619c254ca573cf501ce1621914` |
| `server/web/scripts/audit-assessment.json` | `a712130b7db6600e3434f5473ba719d5c54d4d30b364c673fca743e60e44467f` |

## Collection as the signed-in home

Starting commit `87e2e8d9c`. Filtered export
`cleanroom/20261119T100016Z-8888`. Antoine chose Collection as the
destination for sign-in, the authenticated root URL and the logo.
The browser and package checks now open Connection explicitly when
checking its URL and username fields.

Independent reviewer `registry_review` checked the complete sources,
routing behavior and exact audit and upstream fingerprints. These six
files contain public material.

| File | SHA-256 |
| --- | --- |
| `server/tools/package-smoke.mjs` | `ce99c3df2d1353eb7a2edd155d8e1872f0de4e80c128384e55137d3c23821122` |
| `server/web/scripts/audit-assessment.json` | `5ec8f3b6e2e2201e69ccf8f91e05b2bcb86ad0b71662517b9aee39c2950b1bbd` |
| `server/web/src/app.tsx` | `af8641725a4c838aec5c45b516d6522d643c1880644a369025a3a634054747ee` |
| `server/web/tests/browser/accounts.spec.ts` | `cbac3ec97c1eb84b807a511964dd1e2049d8123f0d21f97f2dc949e4eca01b49` |
| `server/web/upstream-records.json` | `d7199181ece5972ea365c5086cb612f6f605fee2512af49d1667e304f3d1691c` |
| `specs/servers/meguri-sync-web.md` | `565b42fd6f92ddc57916e58f43dc46ea240efff00132b8d1bd6ab5334a664fe2` |

`sync-web-verify/20261119T100337Z-12986` passed all ten checks,
including signed-in landing, authenticated root and pointer and keyboard
logo navigation over private HTTPS.
