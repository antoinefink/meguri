# Oracle driver: the `type_answer` and `review_html` observations (U64)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind `driver/observe/type_answer.py`; meanings in
`specs/conformance/type-answer-ops.md` K1-K3, the spec writer's sources in
`render-type-answer.md`.

Unlike the other driver modules, this one carries Anki code: K3 asks for the
desktop reviewer's two type-in filters as the pinned release has them, and the
image holds only Anki's library. `_Reviewer` holds `typeAnsPat`,
`typeAnsQuestionFilter`, `typeAnsAnswerFilter` and `_contentForCloze` from
`qt/aqt/reviewer.py` at the tag with their bodies unchanged (AGPL-3.0-or-later,
like everything under `oracle/`); the dispatching `typeAnsFilter` is not
carried, since the driver calls each filter for its side. K3's changes live in
what the bodies call (`tr`, `gui_hooks`, `mw`), never in the bodies. A pin bump
diffs the carried bodies against the new tag's `reviewer.py`.

| Anki file | Read for |
|---|---|
| `qt/aqt/reviewer.py` (`typeAnsPat`, `typeAnsFilter`, `typeAnsQuestionFilter`, `typeAnsAnswerFilter`, `_contentForCloze`, `_mungeQA`, `_showQuestion`, `_showAnswer`, `_onTypedAnswer`, `__init__`) | the carried filters; the state they read and set (`typeCorrect`, `_combining`, `typeFont`, `typeSize`, `typedAnswer`) and its values before a question (`_combining` on, `typeCorrect` none, typed text `""` after reveal); question from `card.question()`, answer from `card.answer()` on the same card object; the type filter runs after display preparation |
| `qt/aqt/main.py` (`prepare_card_text_for_display`) | display preparation is media escaping, then play buttons (left out, K2) |
| `qt/tools/genhooks_gui.py` (`reviewer_will_compare_answer`, `reviewer_will_render_compared_answer`) | the two hooks' arguments and that, with nothing registered, each returns its first argument |
| `pylib/anki/cards.py` (`question`, `answer`, `render_output`, `note`, `note_type`) | a card renders once and keeps its output and note, so the driver reads a new card object per case |
| `pylib/anki/template.py` (`TemplateRenderContext.render`, `_partially_render`, `TemplateRenderOutput.question_and_style`, `answer_and_style`) | the existing-card render, the `<style>` prefix, FrontSide from the extracted question text, the `card_did_render` hook (LaTeX only; no scenario has LaTeX) |
| `pylib/anki/media.py` (`escape_media_filenames`) | the encoding direction is the backend's `encode_iri_paths` |
| `pylib/anki/collection.py` (`compare_answer`, `extract_cloze_for_typing`) | the comparison call and its `combining` flag; the cloze extraction the filter calls |

Probed in the oracle image (`driver/tests/test_type_answer.py`, and the
recorded goldens checked against the examples of
`specs/render/type-answer.md` A4.4, A5, A6.1, A7, A8,
`specs/render/media-display.md` D3 and
`specs/render/image-occlusion-render.md` I6): a re-record of all 20 goldens
gives identical bytes, and neither kind changes the scenario's collection
(`db_dump` before and after).
