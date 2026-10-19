# Type-in, media display and image-occlusion render specs (U64)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/render/type-answer.md`,
`specs/render/media-display.md`, `specs/render/image-occlusion-render.md` and
`specs/conformance/type-answer-ops.md`. No code was copied; every rule was
restated in the spec writer's words. The backend parts were checked against the
oracle image's Anki (`meguri-oracle:26.9.3-6f7ed31e55eb0839`) as a black box:
every scenario input of `render/type-answer/compare` and the 3,000 corpus pairs
through the answer comparison (a scratch model of A6 agreed on every pair whose
expected text is shorter than 200 characters, and differed only on long pairs,
which is the pinned matcher's long-text behavior), the shape texts of
`render/image-occlusion/render/divs` through the cloze renderer, and the media
names through the collection's media escaping. The reviewer filters (A4, A5)
were checked by running the scenarios' notes and cards through the backend with
a temporary copy of the two reviewer functions before the isolated adapter was committed.

| Spec section | Anki file | Read for |
|---|---|---|
| type-answer A2-A5, A8 | `qt/aqt/reviewer.py` | marker pattern, first-marker parsing (`cloze:` then `nc:` split at the second colon), field lookup by exact name, font and size keys, empty versus missing expected text, warning choice, box markup, re.sub replacing every marker, answer filter with the exact `<hr id=answer>` removal and the unchanged-answer branch, wrapper markup, typed text defaulting to empty |
| type-answer A1, A3 | `qt/aqt/reviewer.py`, `qt/aqt/main.py` | order: card question/answer, media escaping, play buttons, then the type filter; the answer pre-sent with the question escaped too |
| type-answer A6 | `rslib/src/typeanswer.rs`, `rslib/src/card_rendering/mod.rs`, `rslib/src/card_rendering/parser.rs`, `rslib/src/card_rendering/writer.rs`, `rslib/src/text.rs`, `Cargo.lock` | expected stripping (AV removal, line-break regex, strip_html, trim), empty typed output, NFC versus NFKD-without-marks sequences, expected split for display, equality shortcut, opcode-to-token table, leading-mark isolation, minimal escaping, output frame; crate versions difflib 0.4.0, htmlescape 0.3.1, unicode-normalization 0.1.24, unic-ucd-category 0.9.0 |
| type-answer A7 | the oracle | every row recorded through the comparison call |
| type-answer A8 | `ftl/core/studying.ftl`, `rslib/i18n/src/lib.rs` | English text of the two warnings, no isolation marks around the field name |
| type-answer A9 | none at the tag | Meguri's own design (R964); the catalog's supplementary rows show case-insensitive comparison tests on Anki's main branch, not read |
| type-answer A11.1 | `qt/aqt/clayout.py` | preview simulation: the `[[type:` substring test, rule removal on both sides, first marker only, fixed `example`/`sample` inputs, no font wrapper, later markers as a warning |
| type-answer A11.2 | `qt/aqt/browser/previewer.py` | browse preview removes markers with a `[^]]+` spec before display preparation |
| type-answer A12 | `rslib/src/template.rs`, `rslib/src/template_filters.rs` | `{{type:Unknown}}` is an unknown-field template error, specials give markers |
| media-display D1-D4 | `qt/aqt/main.py`, `qt/aqt/reviewer.py`, `qt/aqt/clayout.py`, `qt/aqt/browser/previewer.py`, `pylib/anki/media.py`, `rslib/src/text.rs` | which sides are escaped and when; the encoding is U20's `text/media-references#M6` |
| image-occlusion-render I1-I4 | `rslib/src/cloze.rs`, `rslib/src/image_occlusion/imageocclusion.rs` | element frame and class rule, attribute order and skipping rules per key, `data-occludeInactive` and `data-font-size` names, raw values except the encoded label, points rewrite, nothing for a shape text without a colon |
| image-occlusion-render I6 | `rslib/src/cloze.rs`, `rslib/src/card_rendering/writer.rs` | attribute-encoded front content hides sound tags; answer play markers |
| type-answer-ops K3 | `pylib/anki/collection.py`, `pylib/anki/media.py`, `pylib/anki/cards.py`, `qt/aqt/reviewer.py` | the calls the driver uses, and the two reviewer functions it must carry |

Third-party crate source read for exact behavior (not Anki code): `difflib`
the pinned version 0.4.0 matcher (its candidate filter retains only elements
occurring more than len/100 + 1 times once the second sequence has 200 or more,
the two extension passes, opcode generation), `htmlescape` 0.3.1
`encode_minimal`, `unicode-normalization` 0.1.24 (`is_combining_mark` is
General_Category M, Unicode 16.0.0). The Unicode 10.0.0 version of
`unic-ucd-category` 0.9.0 was confirmed in the oracle: a typed token starting
with U+0335 gets a no-break space, one starting with U+08D3 (Unicode 11) or
later marks does not.

Harvest rows classified for U64: the `rslib/src/typeanswer.rs` tests at the tag
(in scope, `render/type-answer/compare`) and its two supplementary
case-insensitive tests (not applicable at the tag). Left for after U63 merges,
because U63's branch edits neighbouring catalog lines: two `typeanswer.rs` rows
(`haccda9311c93`, `hafc40dbe3447`), and the image-occlusion rows
`hae0105ed752c` and `h1f6151e74934`, which `render/image-occlusion/render/divs`
covers.
