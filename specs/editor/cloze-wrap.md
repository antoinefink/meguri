# Cloze commands in the editor

Spec: editor/cloze-wrap
Requirements: R857, R858, R859, R860, R251; supports R833, R200, R1960
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/cloze/

The editor's two cloze commands (U192): Cloze Deletion (⇧⌘C), which wraps the selection in a cloze with a new number, and Cloze Deletion (Same Number) (⌥⇧⌘C), which reuses the highest number. This file says where they are offered, how the number is chosen, what text replaces the selection and what the field holds afterwards. How cloze markup is read for cards is `render/cloze`; the note's field checks are `notes/field-check`; the field model, bridge and save pipeline are `editor/field-html`; the toolbar, menus and key routing are `editor/formatting`.

## W1. Terms and sources

- Terms of `editor/field-html` E1 and `editor/formatting` F1, including the selection notation (`[`, `]`, `‸`).
- **Cloze field**: a field of a cloze note type named by a cloze reference in the front of its first template (`notes/field-check` K2). The host gets the set from the note type snapshot (U29) and gives it to the page per field (`editor/field-html` B4 `clozeField`).
- **Shown field**: a field of the editor's field list; image occlusion note types do not show their Occlusion and Image fields (`editor/field-html` F1, R833).
- **Session values**: for each field of the note, its target (`editor/field-html` H2) after a flush: the editor's current value for a shown field, the stored HTML for a field that is not shown.
- Sources and goldens: as `editor/formatting` F1. The goldens decide the exact markup of W9's scenarios.

## W2. Where the commands are offered (R857, R251)

- The toolbar's cloze group (Cloze Deletion, `editor.toolbar.clozeNew`; Cloze Deletion (Same Number), `editor.toolbar.clozeSame`) and the two Format menu items are shown when the note type is a cloze note type with at least one shown cloze field. Otherwise the group is hidden and the menu items are disabled. Decided 2026-10-27: a cloze note type with no shown cloze field shows no cloze group, where Anki shows it disabled for every cloze note type (W8). That covers an image occlusion note type, whose only cloze field in its stock templates is the hidden Occlusion field, and a cloze note type whose first template's front names no field with a cloze reference (for example a front of `{{Text}}`).
- The commands are enabled only while the focused rich field (`editor/formatting` F1) is a cloze field. In HTML mode, in a field that is not a cloze field, in the tag editor or with no focus they are disabled: the controls are dimmed, the menu items disabled, and ⇧⌘C and ⌥⇧⌘C do nothing to any field.
- Keys and context: `editor/formatting` F14 (`editor.richText`). Labels, with Anki's counterparts: `editor/formatting` F19.
- The group follows the note type: it appears, hides or changes enabled state when the Add screen changes the note type.

## W3. The number (R858, R859)

1. The host flushes the editor (`editor/field-html` S1).
2. In the session value of every field of the note, shown or not, cloze field or not, it finds every occurrence of `{{c`, then one or more ASCII digits, then `::`, reading the text from start to end; one occurrence ends before the next is looked for. Each occurrence gives the decimal value of its digits; leading zeros are allowed.
3. H is the largest value found, or 0 when there is none.
4. **Cloze Deletion** uses N = H + 1. **Cloze Deletion (Same Number)** uses N = H, or 1 when H is 0.

This is a text search, not `render/cloze`'s reading: occurrences count wherever they are, inside markup and attributes, in unclosed or nested clozes, and in image occlusion masks; a marker with several numbers (`{{c1,3::`) or a capital `C` counts for nothing.

| Session values | Cloze Deletion | Same Number |
|---|---|---|
| no markers | 1 | 1 |
| another field holds `{{c2::x}}` | 3 | 2 |
| `{{c0::x}}` | 1 | 1 |
| `{{c01::a}}` | 2 | 1 |
| `{{c1::a {{c4::b}}}}` | 5 | 4 |
| `{{c3::open` (unclosed) | 4 | 3 |
| `<img alt="{{c5::">` | 6 | 5 |
| `{{c1,3::x}}` | 1 | 1 |
| `{{C2::x}}` | 1 | 1 |
| a hidden Occlusion field with `{{c7::image-occlusion:rect:left=.15:top=.12:width=.2:height=.2}}` | 8 | 7 |

Decided 2026-10-27: digit runs are read as unsigned integers and saturate at 9007199254740991 (2^53 − 1), beyond which Anki's own arithmetic is inexact (a stated difference, W8); no scenario uses a run longer than 15 digits. N is written in decimal without leading zeros. A number above 65535 is written as computed, as Anki does, although `render/cloze` C2 does not read such a marker as a cloze.

## W4. The text that replaces the selection (R860)

The host sends the page the prefix `{{c` N `::` and the suffix `}}` (message `wrap`, `editor/formatting` F13). The page then:

1. Takes S, the serialization (`editor/field-html` E1) of the selected content. Elements cut by the selection's edges appear in S with only their selected part, closed at the edge; U+00A0 appears as `&nbsp;`.
2. Splits S into L, M and T: L is the longest run at S's start, and T the longest run at S's end, of whitespace characters and the entity text `&nbsp;` or `&#160;`. When S is whitespace only, L is S and M and T are empty. The whitespace characters are U+0009 to U+000D, U+0020, U+00A0, U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F, U+3000 and U+FEFF. Only the ends of the serialized text count: a space inside `<b>word </b>` is not at S's end.
3. Builds R = L + prefix + M + suffix + T. Every character of L, M and T is kept as it was (W8, `editor-cloze-nbsp-kept`).
4. Inserts R in place of the selection as `editor/formatting` F10 inserts HTML: one input and one history entry.
5. Places the caret: with a collapsed selection, between `::` and `}}` of the new cloze; otherwise after the inserted content.

| Before | Command, H | After |
|---|---|---|
| `a [word ]b` | Cloze Deletion, 0 | `a {{c1::word}} b` |
| `‸` | Cloze Deletion, 0 | `{{c1::‸}}` |
| `ab‸` | Cloze Deletion, 2 | `ab{{c3::‸}}` |
| `[word] ` | Same Number, 2 | `{{c2::word}} ` |
| `<b>[bold]</b>` | Cloze Deletion, 0 | `<b>{{c1::bold}}</b>` |
| `[<b>bold</b>]` | Cloze Deletion, 0 | `{{c1::<b>bold</b>}}` |
| `a[&nbsp;b&nbsp;]c` | Cloze Deletion, 0 | `a&nbsp;{{c1::b}}&nbsp;c` |
| `a[x&nbsp;y]b` | Cloze Deletion, 0 | `a{{c1::x&nbsp;y}}b` |

Selections across element or block edges (`<b>a[b</b>c]`, `<div>[a</div><div>b]</div>`) insert R as F10 does; the goldens of W9 give the markup.

## W5. Nested clozes

- A cloze command never inspects the clozes around or inside the selection. Wrapping text inside a cloze nests a new one: `{{c1::a [b] c}}` with Cloze Deletion gives `{{c1::a {{c2::b}} c}}`. Selecting text that holds a whole cloze wraps it: `[x {{c1::y}}]` gives `{{c2::x {{c1::y}}}}`.
- A selection that cuts a marker inserts its pieces as text, so the field may hold unbalanced markers; `render/cloze` C3 and C4 say how such text reads, and `notes/field-check` reports the state.
- Nesting depth is not limited here; `render/cloze` C4 drops markers beyond 10 open clozes when cards are rendered.
- Cloze hints are typed by the user; the commands add none.

## W6. Image occlusion

- In an image occlusion note type (`render/image-occlusion-format` F2) the masks' numbers come from the mask editor (`render/image-occlusion-format` F12, U206), never from these commands.
- With the stock templates no shown field is a cloze field, so the cloze group is hidden (W2). When a user's template makes a shown field such as Header a cloze field, the commands act there, and W3 still counts the hidden Occlusion field's numbers, so a new cloze never reuses a mask's number.

## W7. What the field holds

- The only change is the replaced selection (W4). The field is changed (`editor/field-html` H2) and saved by its pipeline; in the Add screen the draft takes it.
- Cloze commands touch no other field and no tag.
- The field check (`notes/field-check`, `editor/field-html` S3) runs after the commit, so a missing-cloze hint clears once a cloze exists.

## W8. Differences from Anki

Proposed `conformance/deviations.yaml` ids, to be entered with the goldens that show them:

| Situation | Anki 26.09.3 | Meguri | Deviation |
|---|---|---|---|
| U+00A0 in the selection | every `&nbsp;` in the selected markup, in L, M and T, becomes an ordinary space before wrapping | kept as it was (W4 step 3; R860 keeps outer non-breaking spaces as they are) | `editor-cloze-nbsp-kept` |
| A cloze note type with no shown cloze field: image occlusion, or a cloze type whose first front template names no field with a cloze reference | the cloze group is shown, always disabled (its visibility follows only the note type's kind) | hidden (W2) | none (no golden) |
| A digit run above 2^53 − 1 | the value is computed inexactly, and the new number can be written in exponent form such as `1e+21` | saturates at 9007199254740991 and is written in decimal digits (W3) | none (no golden) |
| Which fields are scanned | the page's current field contents, display-encoded media names included | the session values, with raw media names (`text/media-references` M7) | none (only a media name containing `{{c` differs) |

The NBSP change keeps text the user did not ask to change (data safety first).

## W9. Scenarios for the oracle maintainer

Requested in `conformance/scenarios/editor/cloze/`, with goldens in `conformance/goldens/editor/cloze/` recorded in the quarantined editor-page image as `editor/field-html` E2 and T1 say. None of the rows below is written or recorded: no runner step and no oracle driver op drives an editor page today. The scenarios that observe how Anki reads and saves the commands' results with existing ops are listed in W10.

**Setup.** One cloze note type `Gap` defined by the scenario (R1959): fields `Text`, `Extra` and `Note`; front template `{{cloze:Text}}`, back template `{{cloze:Text}}<br>{{Extra}}`, empty CSS. One note with the stored fields given (others empty). The maintainer opens Edit Current, focuses the field named, sets the selection in `editor/formatting` F1's notation and presses Anki's key (Ctrl+Shift+C, Ctrl+Alt+Shift+C). Each scenario ends with blur and probe: every field's stored HTML, and the caret's place for the collapsed cases (a `type "z"` before blur shows it).

| Scenario | Fields | Commands |
|---|---|---|
| `new-empty-note` | Text `‸` | Cloze Deletion, type `z` |
| `same-empty-note` | Text `‸` | Same Number, type `z` |
| `new-after-other-field` | Text `[word ]x`, Extra `{{c2::x}}` | Cloze Deletion |
| `same-after-other-field` | Text `[word ]x`, Extra `{{c2::x}}` | Same Number |
| `number-scan` | Text `[a]`, Extra `{{c01::b}} {{c1,9::c}} {{C8::d}} <img alt="{{c5::"> {{c3::open` | Cloze Deletion |
| `number-nested` | Text `{{c1::a {{c4::b}}}} [c]` | Cloze Deletion |
| `number-zero` | Text `{{c0::a}} [b]` | Cloze Deletion, then select `a`, Same Number |
| `whitespace-outer` | Text `a[ word ]b` | Cloze Deletion |
| `whitespace-only` | Text `a[ ]b` | Cloze Deletion |
| `nbsp-outer` | Text `a[&nbsp;b&nbsp;]c` | Cloze Deletion (W8) |
| `nbsp-inner` | Text `a[x&nbsp;y]b` | Cloze Deletion (W8) |
| `inside-bold` | Text `<b>a[bold]c</b>` | Cloze Deletion |
| `whole-bold` | Text `[<b>bold</b>]` | Cloze Deletion |
| `across-bold-edge` | Text `<b>a[b</b>c]` | Cloze Deletion |
| `across-blocks` | Text `<div>[a</div><div>b]</div>` | Cloze Deletion |
| `nest-inside` | Text `{{c1::a [b] c}}` | Cloze Deletion |
| `wrap-existing` | Text `[x {{c1::y}}]` | Cloze Deletion |
| `cut-marker` | Text `{{c1::a[b}}c]` | Cloze Deletion |
| `caret-in-bold` | Text `<b>ab‸</b>` | Cloze Deletion, type `z` |
| `not-cloze-field` | Extra `a[b]c` | Cloze Deletion (the field is unchanged) |
| `html-mode` | Text `a[b]c` in HTML mode | Cloze Deletion (the field is unchanged) |
| `two-in-a-row` | Text `[a] b` | Cloze Deletion, then select `b`, Cloze Deletion |

Meguri tests without goldens (plan U192): W2's shown, hidden and disabled states for a standard note type, a cloze note type, a cloze note type whose front is `{{Text}}`, an image occlusion note type and a note type change in the Add screen; W6 with a template that makes Header a cloze field; W3's saturation; and the keys in the registry test (`editor/formatting` F14).

## W10. Core scenarios

Written in `conformance/scenarios/editor/cloze/`, on W9's note type `Gap`, inactive until Meguri's adapters pass them. They use only existing ops (`edit_note`, `db_exec`, the `fields_check` and `cloze` observations) and observe what the Python-library oracle can: how the text the commands write is saved and read. The number itself (W3), W4's split and caret, W6 and W8 are computed in the editor page, so W9's goldens and Meguri's tests cover them. Each `edit_note` stands for the field's save after a command, with the text W3 to W5 give.

| Scenario | Covers |
|---|---|
| `save/new-and-same-number` | W3, W4 and W7: with `{{c2::x}}` in Extra, Cloze Deletion on `word ` writes `{{c3::word}}` with the space outside, and Same Number on `b` reuses 3. Extra's number already gives a card at setup, since a cloze note's numbers come from every field (`notes/card-generation` G4); the first save adds only the card of ordinal 2 and the second none (G7). The field check reports Extra's cloze (`notes/field-check` K3); the saved text's numbers and its card 3 front and back |
| `save/empty-note` | W3 and W4 with a collapsed caret on an empty note: `{{c1::}}` saved and read for card generation and the field check, then `{{c1::z}}` (the caret's place, W9's `new-empty-note`) |
| `output/read-back` | how `render/cloze` reads W3's table of session values, the results of W9's `number-zero`, `number-nested`, `two-in-a-row` and `cut-marker` rows, and W4's and W5's After column, front and back for each card. Where the numbers it reads differ from W3's H (a marker with several numbers, a capital `C`, an unclosed or attribute marker, 0, a number above 65535), the commands still use W3's rule |
