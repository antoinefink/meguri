# Tag completion

Spec: tags/complete
Requirements: R883; supports R882, R884, R1190
Anki: 26.09.3
Scenarios: conformance/scenarios/tags/complete/ (listed in C7, recorded on the Linux oracle)

The suggestions the tag editor shows while the user types a tag (R883): which registered tags match a partial name, in which order, and how many. U191 provides the query. The tag editor (U193) and Browse's tag entry (U218, R1190) call it. Tag terms (components, registered, implicit parent) are `tags/tags` T1.

## C1. The query

A read-only query on the query lane (KTD9). It writes nothing.

- **Input.**
  - `input`: the partial tag as text, possibly holding `::`. It is used as given: no NFC, no trimming, no removal of control characters or separators.
  - `limit`: the most tags to return, an integer of at least 0.
  - A cancel token (`engine/jobs` J4).
- **Output.** A list of registered tag names, each spelled as its `tags` row, at most `limit` long, in C4's order. Implicit parents are never in it, because only rows are read. A registered tag that no note uses is in it (until Clear Unused Tags removes its row, `tags/tags` T11).
- **Cancellation.** The token is checked between rows. A cancelled query returns `Cancelled` and no list. The tag editor cancels its previous query at each keystroke (C5).
- `limit` 0 returns an empty list and reads nothing.

## C2. Filters

`input` is split into **filters** at each `::`, left to right without overlap, as `tags/tags` T1 splits a name into components. Every piece is a filter, an empty one included.

| `input` | Filters |
|---|---|
| `jap::aud` | `jap`, `aud` |
| `jap` | `jap` |
| `jap::` | `jap`, (empty) |
| (empty) | (empty) |
| `::` | (empty), (empty) |
| `a:::b` | `a`, `:b` |

## C3. Which tags match

- A filter **fits** a component when the component contains the filter as a run of consecutive characters, where two characters are equal when their simple case foldings are equal (`search/case-folding` C3). An empty filter fits every component. There is no other normalization: `e` U+0301 does not fit `é` (U+00E9).
- A row's tag is split into components (`tags/tags` T1). The filters are taken in order. Each filter takes the first component, after the one the previous filter took, that it fits. A component that the filter does not fit is skipped. The tag **matches** when every filter takes a component. Components after the last one taken do not matter.
- A match is a **prefix match** when filter k took component k for every k, and each of those components begins with its filter. Every other match is an **inner match**. So a skipped component, or a filter found after the first character of its component, makes an inner match.

The search takes the first component that fits, not the best one. With `a`, the tag `Japanese::Audio` is an inner match: `a` fits `Japanese` (at its second character) before `Audio` is tried.

Examples, with the rows `German::Verb::modal`, `japan` and `Japanese::Audio`:

| `input` | `German::Verb::modal` | `japan` | `Japanese::Audio` |
|---|---|---|---|
| `jap` | no | prefix | prefix |
| `jap::aud` | no | no (one component) | prefix |
| `jap::` | no | no | prefix |
| `JAPANESE::AUDIO` | no | no | prefix |
| `verb::mod` | inner (`German` skipped) | no | no |
| `verb` | inner (`German` skipped) | no | no |
| `ger::modal` | inner (`Verb` skipped) | no | no |
| `GERMAN::V` | prefix | no | no |
| `erman` | inner | no | no |
| `pan` | no | inner | inner |
| `a` | inner | inner | inner |
| `audio::jap` | no | no | no (order counts) |
| (empty) | prefix | prefix | prefix |
| `::` | prefix | no | prefix |

Letter case uses simple folding, not the registry's full folding (`text/unicode-primitives` U2):

| Row | `input` | Result |
|---|---|---|
| `Über` | `über` | prefix |
| `Straße` | `STRAẞE` (U+1E9E) | prefix |
| `Straße` | `strasse` | no |
| `Σοφία` | `σοφ` | prefix |
| `Ｇｒａｍｍａｒ` (fullwidth) | `ｇｒａ` | prefix |
| `Ｇｒａｍｍａｒ` (fullwidth) | `gra` | no |

## C4. Order and limit

1. Rows are read in the registry's order: `tag` under the `unicase` collation (`storage/schema-18`). The `tags` key is unique under that collation, so no two rows tie.
2. Matching rows are collected in that order. Reading stops when `limit` rows are collected.
3. The result is the collected prefix matches, in registry order, followed by the collected inner matches, in registry order.

The cut of step 2 comes before the reordering of step 3. So a prefix match that comes late in registry order can be missing while earlier inner matches are present.

Example, with the rows `Grammar::Japan`, `Japan` and `Japanese` and the input `jap`:

| `limit` | Result |
|---|---|
| 1 | `Grammar::Japan` |
| 2 | `Japan`, `Grammar::Japan` |
| 3 | `Japan`, `Japanese`, `Grammar::Japan` |
| 500 | `Japan`, `Japanese`, `Grammar::Japan` |

Anki's request takes `match_limit` and returns at most `match_limit` + 1 tags. Meguri's `limit` is Anki's `match_limit` + 1, so the two give the same list. The oracle op passes `limit` − 1 (C7).

## C5. The tag editor's request (U193)

- The editor queries for the tag being typed, with its displayed separators written as `::`, and with `limit` 500 (R883).
- Before it sends the text, the editor removes one leading `:` when the text begins with a single colon (not `::`), and one trailing `:` when it ends with a single colon (not `::`). So `verb:` asks for `verb` while the user is typing a separator. An empty result sends no query and shows no suggestions, and so does an empty tag.
- Each change of the typed text cancels the query in flight and sends a new one; moving the highlight through the suggestions sends none, and no query is sent while the input holds an input method's marked text (`editor/tag-editor` G4, G6). Only the reply to the latest query is shown.
- Suggestions are shown with `::` displayed as `∷` (R882). Tags already on the note are not removed from the list. Choosing one replaces the tag being typed and ends it once; the highlight cycle and acceptance by Return, Space and click are `editor/tag-editor` G6 (R884). The tag's spelling on save is `tags/tags` T3.
- Browse's tag entry (U218) uses the same query, the same `limit` and the same preparation.

## C6. Differences from Anki

- Anki's editor asks with `match_limit` 500, so it shows up to 501 suggestions. Meguri shows up to 500 (R883). The query itself gives Anki's list (C4).
- Anki's editor removes the outer single colons only when the text between them is at least two characters long and neither begins nor ends with a colon. So it sends `a:`, `:a` and `::ab:` unchanged. Decided 2026-10-27: Meguri applies C5's rule, so a tag being typed keeps its suggestions while its separator is half typed. This changes only what the editor sends. The query is Anki's.
- Anki's Browse tag entry (the Add Tags and Remove Tags prompts) sends the tag being typed unchanged, with no colon removal, also when it is empty, and with `match_limit` 500, so it also shows up to 501 suggestions. Meguri's Browse entry (U218) applies C5's preparation and `limit`.

None of these differences is visible to a scenario, which calls the query directly, so none has a `conformance/deviations.yaml` entry.

## C7. Scenarios

Status (2026-11-17): the oracle driver has the observation kind, and the six scenarios below are recorded on the Linux oracle.

The observation kind `tag_complete`, modeled on `fields_check` (`conformance/note-ops` N4). It reads the collection and changes nothing, and may be requested once per observe step.

- **Request** `{tag_complete: {cases: [{input: <text>, limit: <integer from 1 to 2^32>}, ...]}}` (Anki's `match_limit` is a 32-bit unsigned integer).
- **Value** a list of results, one per case in case order. Each result is the list of tag names, in the order returned.
- **Oracle.** The backend's tag completion with `input` as given and `match_limit` `limit` − 1.
- **Meguri.** The C1 query with the same input and limit.

Rows are set up through `add_note` tags, or through `db_exec` for spellings that save normalization would change.

| Scenario (`tags/complete/`) | Covers |
|---|---|
| `components` | C2, C3: the first example table, with `limit` 500 |
| `folding` | C3: the case table, plus a raw `db_exec` row holding U+212A KELVIN SIGN (`k` fits it) and a decomposed `e` U+0301 row |
| `order-limit` | C4: the `Grammar::Japan` example at each limit; rows whose `unicase` order differs from code-point order (`_x`, `A`, `b`, `ä`) with an empty input |
| `separators` | C2: inputs `::`, `jap::`, `::aud`, `a:::b`, and rows with single colons (`ratio:a`, `a:::b`) |
| `registry` | C1: an implicit parent (`Bar` beside the row `Bar::x`) is not suggested; a tag whose last note was deleted still is |
| `filter-sequence` | C3, C4: inputs of two, three and four filters against tags of up to six components, and a two-filter input reversed. Components may be skipped before and between the taken ones, and further components may follow the last one taken. A tag with fewer components than filters never matches, nor one whose components hold the filters only in another order. A skipped component, or a filter found after the first character of its component (first or middle), makes an inner match, listed after the prefix matches; components after the last one taken do not. |
