# Note type validation

Spec: notetypes/validation
Requirements: R921, R942, R943, R930 (core side), R937 (core side)
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/records/validation.yaml, conformance/scenarios/notetypes/records/names.yaml

The checks a note type save runs, the order Anki runs them in (the first failure is the refusal), and the check of a field name typed by the user. Template parsing and analysis are `render/template-syntax` (U60): "parses" means no structural error of T6, "references" means T11.1. Name normalization at save is `notetypes/records` N4.

## V1. Checked and unchecked saves

Every save applies the structural rules of V2 and the normalization of `notetypes/records` N4. A **checked** save also runs the template checks of V3; an **unchecked** one skips them. As in Anki:

| Save | Checked |
|---|---|
| adding a note type (from scratch, stock, clone) | yes |
| renaming a note type (R917) | yes |
| the template editor's save: front, back, styling, deck override, browser appearance, add, remove, reorder and rename of templates (R937) | yes |
| the Add screen's field pin (`sticky`, R805) | yes |
| Restore to Default (U34) | yes |
| the field editor's save: add, remove, rename, reorder, sort field, field options (R920–R927) | no |
| rows arriving by sync or import (U107, U120) | no |

So a field-editor save of a note type whose templates already fail V3 succeeds, and a rename of it fails, as in Anki.

## V2. Structural rules (every save)

A save is refused with `invalid_notetype`, writing nothing, when:

- the note type has no field, or no template (R942);
- `notetypes/records` N4 refuses a name: a note type name empty after removing `"`, a field name empty or empty after its fixes, a template name empty or made only of `"`.

## V3. Template checks (checked saves)

Run on the templates as they will be stored (after `notetypes/records` N4 and, for a U34 save, after field renames rewrote the references). Front and back are checked; browser formats are not. The checks run in this order, and the first failure is the result:

1. **Identical fronts** (`duplicate_front`): two templates whose front texts are equal code point for code point, unless the front contains `{{`, optional White_Space, `Card`, optional White_Space, `}}` (the `Card` special field makes otherwise identical fronts differ). Equal texts either both contain it or neither does. The failure names the later template and the earlier one it repeats.
2. For each template in order, the first of:
   1. **Parse error** (`template_parse_error`): the front or the back has a structural error (`render/template-syntax#T6`).
   2. **No field on the front** (`no_front_field`): the front's field references (T11.1: replacement keys and section keys, sections included, comments excluded, `c<number>` section keys excluded) are empty. Special names and the empty key count as references here: `{{Tags}}`, `{{FrontSide}}`, `{{#Tags}}x{{/Tags}}`, `{{^F}}x{{/F}}` and `{{}}` pass; plain text, `<!--{{F}}-->` and `{{#c1}}x{{/c1}}` alone fail.
   3. **Unknown field** (`unknown_field`): a name referenced on the front or the back (T11.1) that is not empty, not a special name (`FrontSide`, `Card`, `CardFlag`, `Deck`, `Subdeck`, `Tags`, `Type`, `CardID`: `render/special-fields` P1) and not the exact name of a field of the note type. Names compare code point for code point, case included: `{{front}}` with a field `Front` is unknown. Section keys count (`{{#Nope}}…{{/Nope}}`); `c<number>` section keys do not.
3. **Cloze** (`missing_cloze`, R943): for a cloze note type, the first template's front and its back each hold at least one cloze reference (T11.1: a replacement whose filter chain contains a `cloze` piece, so `{{cloze:Text}}`, `{{text:cloze:Text}}` and `{{cloze:text:Text}}` all count). Other templates of a cloze note type are not checked for clozes.

Examples, each a whole check:

| Templates (front / back) | Result |
|---|---|
| A `{{F}}` / `{{B}}`; B `{{F}}` / `{{B}}` | `duplicate_front` (B repeats A) |
| A `{{F}}{{Card}}` / `{{B}}`; B the same | passes |
| A `{{Nope}}` / ``; B `{{Nope}}` / `` | `duplicate_front` (before `unknown_field`) |
| A `plain` / ``; B `{{#F}}` / `` | `no_front_field` (A comes first) |
| A `{{#F}}x` / `{{F}}` | `template_parse_error` |
| A `{{F}}` / `{{/F}}` | `template_parse_error` |
| cloze: `{{cloze:T}}{{Nope}}` / `x` | `unknown_field` (before `missing_cloze`) |
| cloze: `{{cloze:T}}` / `{{T}}` | `missing_cloze` |

## V4. Results for the editor (R930, R937)

- Meguri's validation returns every failure V3 defines, in V3's order: each template whose front repeats an earlier one, then per template the first of 2.1 to 2.3, then the cloze failure. Each failure carries its kind, the template's position, the side when it applies (front or back), the offending name for `unknown_field` (the first in the template's text), and the earlier template's position for `duplicate_front`. The template editor shows them live (R930, deviation: Anki shows only the first, on save).
- A checked save is refused with the first failure in V3's order and writes nothing; the editor stays open (R937).
- The kinds are an interface for tests and scenarios (`specs/conformance/notetype-ops.md` T7). Meguri's messages are its own wording.

## V5. A field name typed by the user (R921)

The check the field editor (U201) runs on a typed name before it changes its draft. Input is the typed text:

1. every `"` is removed and White_Space is trimmed at both ends;
2. empty: no change (the editor ignores it, as Anki does);
3. the first character is `#`, `^` or `/`: refused, `bad_first_character`;
4. it contains `:`, `{` or `}`: refused, `bad_character`;
5. it equals under unicase (`text/unicode-primitives#U2`) the name of another field of the note type: refused, `duplicate`. The field being renamed does not count, so a case-only rename is accepted;
6. otherwise accepted, with the text of step 1.

The editor explains a refusal inline with its own wording (R921). The save's own normalization (`notetypes/records` N4) still runs and would make any name that slipped through valid; V5 is what the user sees. Anki's field editor ignores a case-only rename of a field; Meguri applies it (no scenario observes the editor).

Template and note type names typed by the user are not checked beyond N4: blank input (after trimming) is ignored by the editors (U200), and anything else is saved as typed, with `"` removed. The prompts' prefilled values and blank handling are `notetypes/editor-actions` A3, A4, A8 and A10.

## V6. Scenarios

- `notetypes/records/validation`: every V3 kind through `add_notetype`, the order rules, and the accepted edge cases above.
- `notetypes/records/names`: V2's `invalid_notetype` for empty names.
- The unchecked field-editor save (`skip_checks`) and V5 are covered by Meguri's tests (`core/crates/meguri-engine/tests/notetype_records.rs`); U34's structure scenarios use `skip_checks` where the field editor would.
