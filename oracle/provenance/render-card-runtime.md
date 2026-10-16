# Card runtime spec (U66)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/render/card-runtime.md`.
No code was copied; every rule was restated in the spec writer's words. Exact
token values and compiled selectors were read from the compiled stylesheets of
the installed Anki 26.9.2 app (`_aqt/data/web/css/webview.css`, `reviewer.css`)
after checking that `git diff 26.09.2 26.09.3` is empty for `ts/lib/sass`,
`ts/reviewer`, `ts/routes/image-occlusion`, `ts/lib/tslib`,
`qt/aqt/data/web/css`, `qt/aqt/reviewer.py`, `qt/aqt/webview.py` and
`qt/aqt/theme.py`. The jQuery version was confirmed in the compiled
`reviewer.js`. Research note 12 (template compatibility) supplied the
AnkiMobile and shared-deck context.

| Spec section | Anki file | Read for |
|---|---|---|
| C2.1, C2.3 | `qt/aqt/reviewer.py`, `qt/aqt/webview.py`, `qt/aqt/main.py` | page skeleton, `reviewExtra` placement, base URL, document classes and attributes, page title |
| C2.4 | `qt/aqt/webview.py` | when the GUI starts evaluating page calls (document-ready bridge handshake) |
| C3.1 | `qt/aqt/theme.py`, `qt/aqt/reviewer.py` | body class list and order, when it is applied |
| C3.2, C10.1, C10.2 | `ts/reviewer/index.ts`, `qt/aqt/reviewer.py` | flag and mark drawing, call sites (drawn right after the question call, before its update runs) |
| C4.1, C4.2 | `ts/lib/sass/_vars.scss`, `_color-palette.scss`, `_functions.scss`, `_root-vars.scss`, `buttons.scss`; compiled `webview.css` | token names, light and dark values, focus colour |
| C4.3, C4.4 | `qt/aqt/data/web/css/webview.scss`, `ts/reviewer/reviewer.scss`, `ts/routes/image-occlusion/review.scss`, `qt/aqt/webview.py` (standard CSS); compiled `reviewer.css` | base rules, selector specificity of the flag and mark offsets |
| C5 (live switch) | `qt/aqt/webview.py` (theme change handler) | classes added at the end, removed in place, body order `night_mode`, `nightMode`, `macos-dark-mode` |
| C4.5 | `ts/lib/sass/_root-vars.scss`, `buttons.scss`, `_button-mixins.scss`, `scrollbar.scss`, `elevation.scss`, `qt/aqt/webview.py` | rules left out: root font size, button chrome, scrollbars, `fancy` |
| C6.1–C6.5, C6.7 | `ts/reviewer/index.ts`, `ts/reviewer/index_wrapper.ts` | entry points, queue, update order, hooks, script re-creation and waiting, failure behavior; script text copied from the old element's serialized markup and attributes copied by name, the basis of C15 D21 and D22 |
| C7.1–C7.3 | `ts/reviewer/preload.ts`, `ts/reviewer/images.ts`, `ts/reviewer/index.ts` | preload kinds, budget selection, font URL memory, answer image preload |
| C9.1, C9.2 | `ts/reviewer/index.ts`, `ts/reviewer/index_wrapper.ts`, `ts/reviewer/reviewer_extras.ts`, `ts/lib/tslib/runtime-require.ts`, `qt/aqt/webview.py` | globals and how they are defined (assignment versus `var`) |
| C10.3, C10.4 | `ts/reviewer/index.ts`, `qt/aqt/reviewer.py` | typed answer capture (`null` when there is no value), Enter handling, type box markup |
| C10.5 | `qt/aqt/webview.py`, `qt/aqt/reviewer.py` | bridge function, return value, callback argument (JSON null), reviewer command set |
| C10.6 | `ts/reviewer/answering.ts`, `ts/reviewer/index.ts` | state mutation API shape |
| C10.7 | `ts/lib/tslib/runtime-require.ts`, `ts/lib/tslib/bridgecommand.ts`, compiled `reviewer.js` | packages registered in the reviewer bundle, error text, list order |
| C10.8 | `ts/reviewer/index.ts` | MathJax lazy loading (left out) |
| C15 | all of the above, `qt/aqt/data/web/js/webview.ts` | differences and behaviors not reproduced |

Harvest catalog rows for the reviewer's TypeScript tests (state transforms,
MathJax loading) were classified out of scope in the same change.

Second spec-writer session (s-20261016T052335Z-a3394ee3) re-checked every
C4.1 and C4.2 token against the compiled `webview.css` of the installed app
(all 61 match) and added C3.2 timing, the C5 live-switch order, the C6.5
element, text and attribute rules, and the C10.3 non-control case from the
files above. C2.1 (origin form), C6.4 (R534 on iOS) and C6.6 (what settled
waits for) follow U65's spike records `docs/design/decisions/s1-occluded-page-swap.md`
and `s2-custom-scheme-media.md` (Meguri documents, not Anki).
