# Stock note types

Spec: notetypes/stock-kinds
Requirements: R908, R909, R910, R911, R912, R913, R916
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/records/ (`stock-*`, `io-on-demand`)

The six note types Meguri ships and puts in every new collection. Their structure (names, fields, templates, kinds, stored kind values, card requirements) is Anki's, so every Anki client recognizes them and Restore to Default works across clients (R909). Their template text and CSS are Meguri's own (R908, R1959): this file states what that text must do, never what it says. Records, stamps and normalization are `notetypes/records`.

## K1. The kinds

| Stock kind | `original_stock_kind` | Name | `kind` |
|---|---|---|---|
| Basic | 1 | `Basic` | standard (0) |
| Basic and reversed | 2 | `Basic (and reversed card)` | standard (0) |
| Basic with optional reverse | 3 | `Basic (optional reversed card)` | standard (0) |
| Basic with typing | 4 | `Basic (type in the answer)` | standard (0) |
| Cloze | 5 | `Cloze` | cloze (1) |
| Image Occlusion | 6 | `Image Occlusion` | cloze (1) |

The names, field names and template names are Anki's English ones; Meguri does not localize them (they are data in the collection, shared with other clients).

## K2. A new collection (R913)

- A collection Meguri creates holds all six, added in K1's order right after the default preset and the default deck (U28), so their ids are `now_ms`, `now_ms` + 1, …, `now_ms` + 5 by `storage/ids-usn-mtime` I2 (scenario `notetypes/records/stock-new-collection`).
- They are written as part of the new collection, not as local changes: `mtime_secs` 0 and `usn` 0 on the `notetypes` rows, `mtime_secs` 0 and `usn` 0 on the `templates` rows.
- `curModel` is set to the Basic note type's id as a local config change: `usn` −1, `mtime_secs` `now_s`, value the id as a JSON integer.
- A collection opened from a file keeps whatever note types it holds; Meguri never adds or rewrites stock kinds in an existing collection (R1590), except the on-demand Image Occlusion add (`notetypes/records` N9).

## K3. Values every stock kind holds

| Value | Content |
|---|---|
| `kind`, `original_stock_kind` | K1 |
| `sort_field_idx` | 0 |
| `css` | Meguri's own (K6) |
| `latex_pre`, `latex_post`, `latex_svg` | the defaults of `notetypes/records` N5.1 and N5.4 |
| `reqs` | K4, computed as `notetypes/records` N6 says |
| `original_id`, `target_deck_id_unused`, `other` | absent, 0, empty |
| fields | `notetypes/records` N5.2 defaults (font `Arial`, size 20, fresh random id) except `tag` and `prevent_deletion` as K4 gives |
| templates | `notetypes/records` N5.3 defaults (no browser formats, no deck override, fresh random id) with Meguri's own front and back (K5) |

## K4. Fields, templates and card requirements per kind

Fields in ordinal order; `tag` absent unless given; `prevent_deletion` false unless "protected".

| Kind | Fields | Templates | Card requirements |
|---|---|---|---|
| Basic | `Front`, `Back` | `Card 1` | `any [0]` |
| Basic and reversed | `Front`, `Back` | `Card 1`, `Card 2` | `any [0]`; `any [1]` |
| Basic with optional reverse | `Front`, `Back`, `Add Reverse` | `Card 1`, `Card 2` | `any [0]`; `all [1, 2]` |
| Basic with typing | `Front`, `Back` | `Card 1` | `any [0, 1]` |
| Cloze | `Text` (tag 0, protected), `Back Extra` (tag 1) | `Cloze` | `any [0]` |
| Image Occlusion | `Occlusion` (tag 0, protected), `Image` (tag 1, protected), `Header` (tag 2, protected), `Back Extra` (tag 3, protected), `Comments` (tag 4) | `Image Occlusion` | `any [0, 1, 2]` |

- Field tags: in a Cloze note type, 0 marks the cloze text field and 1 the back-extra field; in an Image Occlusion note type, 0 to 4 mark Occlusion, Image, Header, Back Extra and Comments. The image-occlusion code (U63, U68) finds its fields by these tags, not by name or position; Check Database restores missing Image Occlusion tags (R1644, U129).
- The card requirements follow from K5's field references; a Meguri stock kind whose templates give other requirements is wrong (the scenarios compare them with Anki's).

## K5. Template content (Meguri-authored, R908)

Meguri writes its own template text. Each template must reference exactly the fields below (with the filters and sections named), and no others, so that card generation, the card requirements and other clients' behavior match Anki's:

- Basic, `Card 1`: front shows `{{Front}}`; back starts with `{{FrontSide}}`, then an `<hr id=answer>` line, then `{{Back}}`.
- Basic and reversed: `Card 1` as Basic; `Card 2` front shows `{{Back}}`, back `{{FrontSide}}`, `<hr id=answer>`, `{{Front}}`.
- Basic with optional reverse: as Basic and reversed, but `Card 2`'s whole front is inside `{{#Add Reverse}}…{{/Add Reverse}}`, so the card exists only when `Add Reverse` is non-empty.
- Basic with typing: front shows `{{Front}}` and the type-in box `{{type:Back}}`; back starts with `{{Front}}` (not `{{FrontSide}}`, which would repeat the input box), then `<hr id=answer>`, then `{{type:Back}}` for the comparison.
- Cloze: front `{{cloze:Text}}`; back `{{cloze:Text}}` and then `Back Extra`. Meguri may show `Back Extra` inside `{{#Back Extra}}…{{/Back Extra}}`.
- Image Occlusion: K7.
- R910: every back starts with the question as above and an `<hr id=answer>` line (the reviewer scrolls to it, U66), except Cloze and Image Occlusion, whose backs repeat the cloze.
- Both sides of every template parse without error and pass `notetypes/validation` V3.

## K6. Styling (Meguri-authored, R908, R911)

- Every kind's CSS is Meguri's base card CSS (U66), which styles `.card` in light and dark appearance.
- Cloze adds cloze highlighting that is visible in light and dark appearance: rules for `.cloze` and for `.nightMode .cloze` (the reviewer puts `nightMode` on an ancestor in dark appearance, U66).
- Image Occlusion: K7.

## K7. Image Occlusion template contract (R912)

The template must work with Anki's image-occlusion runtime (so Meguri's Image Occlusion notes review on other clients) and with Meguri's own (U68). It holds, on the front:

- the `Header` field, shown only when non-empty (`{{#Header}}…{{Header}}…{{/Header}}`);
- an element hidden with `display: none` that holds `{{cloze:Occlusion}}` (the masks' data; card generation counts its cloze numbers);
- an element with id `image-occlusion-container` that holds `{{Image}}` and a `canvas` element with id `image-occlusion-canvas`;
- a script that calls `anki.imageOcclusion.setup()`; when the call throws, the card shows a message of Meguri's own wording instead of failing silently.

The back holds the same, then a `button` element with id `toggle` (the runtime makes it show and hide the masks; its label is Meguri's own wording), then `Back Extra`, shown only when non-empty. `Comments` is on no template.

The CSS sets the mask colors through custom properties that the runtime reads from the canvas's computed style:

| Property | Value |
|---|---|
| `--inactive-shape-color`, `--active-shape-color`, `--highlight-shape-color` | a CSS color |
| `--inactive-shape-border`, `--active-shape-border`, `--highlight-shape-border` | a width and a color separated by white space, for example `1px #212121` |

The runtime falls back to its own colors when a property is missing; Meguri defines all six, for light and dark appearance (U66, U68). Meguri ships no Anki runtime code (R1960).

## K8. Adding a stock kind later (R916)

- "Add: <stock kind>" adds the kind as K3 and K4 say, under the name the user gives (default: K1's name), through `notetypes/records` N5: local stamps, N4 step 7 (`Basic+` when `Basic` exists), `curModel` set to the new id, not a schema change, checked save.
- Restore to Default (R914) uses the same definitions (U34).

## K9. What is compared with Anki

- Compared (scenarios and goldens): names, field names, order, count, options, tags and protection, template names and count, `kind`, `original_stock_kind`, sort field, card requirements, ids and stamps of a new collection, and the cards that notes of each kind generate.
- Compared live (T1), as a status only: every card of a note of each kind gets, when Anki renders it for review from Meguri's templates, the status Anki's own templates give it (`ok` for every card of `stock-render-handoff`; `specs/conformance/render-ops.md` Q4). A Meguri template that references a field the kind lacks, leaves a section open, or whose front Anki finds blank where its own is not, fails there.
- Not compared: template text, CSS, the rendered question and answer (text and length) and the LaTeX values' effect. The CSS and templates are Meguri's by design (R908 deviation); goldens never hold Anki's (R1959). Meguri's own tests (`core/crates/meguri-engine/tests/stock_kinds.rs`) check K5–K7 on Meguri's text: the referenced fields and filters, `<hr id=answer>`, the Image Occlusion ids, setup call, toggle button and the six properties.

## K10. Scenarios

| Scenario | Covers |
|---|---|
| `notetypes/records/stock-new-collection` | K2 |
| `notetypes/records/stock-structure` | K1, K3, K4 through `notetype_structure`; K8 |
| `notetypes/records/stock-render-handoff` | live (T1): Meguri's new collection, handed over before any step; the oracle adds one note of each kind by its stored name (Basic (optional reversed card) with `Add Reverse` filled, Cloze with `c1` and `c2`, Image Occlusion with two masks, an image, a header, back extra and comments) and reports each card's render status, the cards each kind generated, and no Check Database problem: K2, K4, K5, K7 as Anki renders them |
| `notetypes/records/stock-handoff` | live (T1): notes of each kind that Meguri adds (U27), read by the oracle: the same structure, the cards Meguri generated, and no Check Database problem |
| `notetypes/records/io-on-demand` | `notetypes/records` N9 |
