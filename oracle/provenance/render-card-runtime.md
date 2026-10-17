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
AnkiMobile and shared-deck context. The completion pass below derives these
values again from the pinned tag and records the lockfile check.

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

## Completion pass (s-20261017T180648Z-983edde5, 2026-10-17)

Answers review finding 5 of `docs/process/review-log/U66.md` (values taken
from the installed 26.9.2 app instead of the pinned tag) and settles C19
item 1.

### Lockfile check, 26.09.2 against 26.09.3

`git -C refs/anki diff --name-only 26.09.2 26.09.3` (tags at bb0dd6d1 and
29bb700b) lists 34 files. None of them is under `ts/`, `qt/aqt/data/` or
`build/`, and `package.json`, `yarn.lock`, `.yarnrc.yml`, `tsconfig.json`,
`Cargo.lock` and `rust-toolchain.toml` are unchanged. The changed files are
docs, the two `ftl/` submodule pointers (translations), pylib and installer
tests, `qt/aqt/__init__.py`, `main.py` and `preferences.py` (safe mode and
video driver start-up), the Windows and Linux installers, `uv.lock` (Python
tooling), and Rust files in `rslib/src/` (card rendering service, scheduler,
stats, tags). None of them feeds the web bundles or stylesheets. So the
26.9.2 app's compiled `webview.css`, `reviewer.css` and `reviewer.js` are
what the 26.09.3 sources and lockfiles build.

### Values derived again from `refs/anki-26.09.3`

- C4.1 and C4.2 tokens. A throwaway evaluator (kept out of the repo) read the
  `colors` and `props` maps of `ts/lib/sass/_vars.scss` with the palette of
  `_color-palette.scss`, applied the naming of `_functions.scss` and
  `_root-vars.scss`, and computed `color.scale` as Sass documents it (alpha
  scaled linearly, lightness and saturation scaled in HSL). All 51 colour
  tokens in both appearances and the 8 prop tokens equal the spec's numbers.
  - 85 colour values are palette literals or the keywords `white` and
    `black`. Sass emits a literal colour as written when it is interpolated,
    so their text comes from the tag.
  - 17 values (11 tokens) are computed by `color.scale`: `--canvas-glass`,
    `--button-disabled`, `--highlight-bg`, `--selected-bg` (alpha, both
    appearances); `--button-bg`, `--button-gradient-start`,
    `--button-gradient-end` (dark); `--button-primary-bg`,
    `--button-primary-gradient-end` (both); `--button-primary-gradient-start`,
    `--button-primary-disabled` (dark). Their numbers come from the tag.
    Their text, the `rgba(r, g, b, a)` comma form and the `rgb(p%, p%, p%)`
    form with ten decimals and trailing zeros dropped, is the serialization
    of the Sass compiler (`sass` 1.102.0 in `yarn.lock`), which is not in the
    tag. Only compiled output shows it.
- `--focus-color` (C4.2): `ts/lib/sass/buttons.scss`. `#6366f1` is the
  interpolated `shadow-focus` palette literal. The Mac value is a custom
  property's raw text, which Sass emits unchanged apart from interpolation,
  so it comes from the tag. The rule is nested in `:root`, so Anki's compiled
  selector has a descendant combinator (0,2,0). The spec now describes the
  matched elements and the specificity instead of quoting that selector
  (U66-E1, resolved 2026-10-17 with a rewrite and no allowlist entry).
- Compiled selectors (C4.3, C4.4): they follow from Sass nesting in
  `qt/aqt/data/web/css/webview.scss`, `ts/reviewer/reviewer.scss` and
  `ts/routes/image-occlusion/review.scss` (for example a `[dir="ltr"] &`
  block inside `#_flag`). The compressed output only respells values
  (`black` as `#000`, `yellow` as `#ff0`, `0.5em` as `.5em`, unquoted
  attribute values). The computed values are the same either way, and
  custom property values are not respelled.
- jQuery (C9, GS11): `yarn.lock` resolves `jquery` to 3.7.1 at the tag, and
  `ts/reviewer/index.ts` exports jQuery's dist build as both `$` and
  `jQuery`.
- C10.7: `build/configure/src/web.rs` bundles `reviewer.js` from
  `ts/reviewer/index_wrapper.ts`. Its import graph (23 source modules,
  followed with the `@tslib` alias) has three registrations:
  `ts/lib/tslib/runtime-require.ts` (`anki/packages`),
  `ts/lib/tslib/bridgecommand.ts` (`anki/bridgecommand`, which imports
  runtime-require) and the body of `ts/reviewer/index.ts` (`anki/reviewer`).
  The generated modules (`backend`, `post`, `ftl`) register none. ES module
  evaluation order gives the listing order. `qt/aqt/reviewer.py` (`_initWeb`)
  and `qt/aqt/webview.py` (`stdHtml`) load only `js/webview.js` and
  `js/reviewer.js` in the review page, and `qt/aqt/data/web/js/webview.ts`
  registers nothing. The error text, `hasPackages` with no names, and plain
  objects (no deprecation proxy for these three) come from
  `runtime-require.ts`.

### C10.5 and C19 item 1: the callback receives `null`

`qt/aqt/webview.py`, `_create_bridge_script`: the page's `pycmd` and
`bridgeCommand` call the channel slot, and the slot's reply goes through
`JSON.parse` before the callback runs, and only when the callback is truthy.
`Bridge.cmd` returns `json.dumps` of the handler's result. `_onBridgeCmd`
returns nothing for `domDone`, `close` and commands that arrive after the web
view or collection is gone, and otherwise returns what
`webview_did_receive_js_message` or the page's handler returns.
`qt/aqt/reviewer.py`, `_linkHandler`, returns nothing on every branch. So the
reviewer callback gets JSON `null`. Antoine's decision of 2026-10-17 keeps
`null` when the tag shows it, so C10.5 and GS5 are unchanged.

One small difference seen in passing: Anki also calls a truthy callback that
is not a function, which throws in a later task. C10.5 ignores a callback that
is not a function, and C15 does not list this.

### Open question for Antoine

Only the text form of the 17 computed values cannot come from the pinned
tag. It now rests on the 26.9.2 app's compiled stylesheet, which the
lockfile check ties to 26.09.3. Allowed sources that would close the gap,
for `docs/process/roles/spec-writer.md`:

1. the installed app's compiled assets, when the app's tag and the pinned tag
   have the same web sources and lockfiles (recorded as above);
2. the pinned sources compiled in Docker with the `sass` version that
   `yarn.lock` pins (one npm download, checked against its `yarn.lock`
   checksum);
3. the compiled assets of the `aqt` 26.9.3 wheel (a PyPI download).
