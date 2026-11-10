# Paste filters (U197)

Spec-writer notes for `specs/editor/paste-filter.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-11-10 (session
s-20261110T085429Z-165d83d3). Both Anki editors run the same page filter;
only the host editor adds the pre-pass of X3. The X9 examples were worked
out from the rules, not taken from Anki's tests: the harvested filter tests
(`oracle/harvest/links.csv`, `ts/lib/html-filter/index.test.ts` and
`ts/tests/e2e/paste-filter.spec.ts` rows) stay deferred until the editor-page
oracle records goldens. Review on 2026-11-10 found six X9 rows whose literals
and outputs came too close to those tests (a bold word, an image with an
error handler, a div with a click handler, an unknown tag pair, a script with
an alert, a title); they were rewritten with other text, names, handlers and
shapes, keeping their rule coverage, and no row now reproduces a test's input
and output.

Meguri's own design, with no Anki source: X5's `javascript:` rule and dark
`font` colour (cited from `editor/paste` P15.2), X2's single-tree rule and
X6's encoding before the collapse (cited from `editor/paste` P14, P16, P25),
X8's direct insertion, X10's interim status of X9, and the proposed deviation
ids.

| Spec section | Anki files read | Read for |
|---|---|---|
| paste-filter X1, X2 | `ts/lib/html-filter/index.ts`, `ts/lib/html-filter/node.ts`, `ts/routes/editor/rich-text-input/data-transfer.ts` (pre-filter parse and serialization before the filter's own parse), `ts/lib/html-filter/element.ts` (`p` content re-parsed into a new element of the live document) | fragment parse into a template, depth-first order with children first, comment removal in every mode, one serialization in the filter; the extra parses of X2's Anki note |
| paste-filter X3 | `qt/aqt/editor_legacy.py` (paste pre-filter), `ts/routes/editor/rich-text-input/data-transfer.ts` (pre-filter) | the host pre-pass (lenient parser, removed tags, `p` renamed) versus the page pre-pass (images only) |
| paste-filter X4 | `ts/lib/html-filter/element.ts` (basic tag table, unwrap versus remove on empty content, `p` to `div`) | the strip-mode table |
| paste-filter X5 | `ts/lib/html-filter/element.ts` (extended tag table, attribute matching by upper-cased name), `ts/lib/html-filter/helpers.ts` (night mode) | the keep-mode table and attribute lists |
| paste-filter X5.1 | `ts/lib/html-filter/styling.ts`, `ts/lib/html-filter/element.ts` (span) | the allowed properties per appearance; the transparent background test on the CSS object model's serialized value; removal through the CSS object model, which rewrites the attribute only on removal |
| paste-filter X6 | `ts/lib/html-filter/index.ts` (output processors) | collapse of LF, TAB and space in strip mode; trim in every mode |
| paste-filter X7 | `ts/lib/html-filter/styling.ts` (internal exceptions), `ts/lib/html-filter/element.ts`, `ts/lib/html-filter/helpers.ts` (HTML elements only) | removed properties; image-only `vertical-align`; SVG left alone |
| paste-filter X8 | `ts/routes/editor/old-editor-adapter.ts` (insertion through the editing command) | that Anki inserts through the browser's insert-HTML command |
| paste-filter X9 | the files of X2 to X7 | the examples, derived from the rules |
| paste-filter X10 | none | Meguri's test plan |
