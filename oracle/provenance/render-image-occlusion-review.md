# Image occlusion review runtime spec (U68)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write
`specs/render/image-occlusion-review.md`. No code was copied; every rule was
restated in the spec writer's words. The compiled reviewer bundle of the
installed Anki 26.9.2 app (`_aqt/data/web/js/reviewer.js`) was read to confirm
the API object's member order, the class field order of the shape classes, the
default `x`/`y` of polygon points, and that setup never throws synchronously
(U66's provenance found `ts/routes/image-occlusion` unchanged between 26.09.2
and 26.09.3). The shape markup in the M15 golden was recorded from the oracle
image `meguri-oracle:26.9.3-41befb49ed33574b`. Research notes 01 and 12 supplied
the user's IO and Tobira Occlusion note type structure.

| Spec section | Anki file | Read for |
|---|---|---|
| M2 | `ts/reviewer/index.ts`, `ts/reviewer/reviewer_extras.ts`, `ts/routes/image-occlusion/review.ts` | the `anki` members, the deprecated alias being the same function, the API object's members |
| M3 | `ts/routes/image-occlusion/shapes/from-cloze.ts`, `shapes/lib.ts`, `shapes/polygon.ts` | which elements are read, attribute conversions (`Number` check then `parseFloat`, angle steps, `oi`, ordinal), points parsing and the missing-points failure |
| M3, M15 | `rslib/src/cloze.rs`, `rslib/src/image_occlusion/imageocclusion.rs` | the attributes the core emits (`data-occludeInactive`, `data-font-size`, omitted empty and zero values) |
| M4 | `ts/routes/image-occlusion/shapes/base.ts`, `rectangle.ts`, `ellipse.ts`, `polygon.ts`, `text.ts`, `position.ts`, `tools/lib.ts` | constructor defaults, absolute and normal conversions, the 40-pixel label fallback, default colours |
| M5, M6, M10 | `ts/routes/image-occlusion/review.ts`, `canvas-scale.ts` | image wait, animation-frame scheduling, missing canvas and image behavior, aspect ratio, backing size and area cap, one-time key listener, toggle wiring and toggling |
| M7, M8 | `ts/routes/image-occlusion/review.ts`, `tools/lib.ts` | readings and filter, colour and border properties with defaults, hook calls, draw order and fills, shape drawing geometry, label layout constants |
| M9 | `ts/routes/image-occlusion/review.ts`, `rslib/src/image_occlusion/notetype.rs`, `ftl/core/notetypes.ftl` | failure paths, the stock template's `#err` handler, the English no-image string |
| M11, M13 D1 | `ts/routes/image-occlusion/review.ts` | the resize handler added on page `load` and what re-running setup repeats |
| M13 D2, D3 | `ts/routes/image-occlusion/review.ts` | the page-level hidden state and the options captured by the key listener |
| M13 D4 | `ts/routes/image-occlusion/shapes/*.ts` | the editor helpers on the shape classes |
| M14.1 | `rslib/src/image_occlusion/notetype.css` | the stock colour values (for MC9's defaults check only; the fixtures use their own CSS) |

No Anki test covers the IO review runtime at the tag (the image-occlusion
route has no unit tests; `ts/tests/e2e/io-mask-save-race.spec.ts` is the
editor's, U205/U206). No harvest rows were classified for U68.
