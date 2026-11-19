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
