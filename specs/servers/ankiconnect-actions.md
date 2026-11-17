# AnkiConnect collection actions

Spec: servers/ankiconnect-actions
Requirements: R1754 (except the `sync` round trip, U146), R1755, R1756, R1757, R1758, R1759, R1760, R1763, R1764, R1765, R1766, R1767, R1768, R1774, R1776, R1777, R1778, R1789, R1790 (one op per action), R1945 (client calls of these actions); supports R1775, R1779, R1780, R1948
Anki: 26.09.3
Scenarios: conformance/recordings/servers/ankiconnect/{decks,models,notes,tags,cards,search,stats}/ (`servers/transcripts` T2; ids `servers/ankiconnect/<dir>/<name>`): oracle transcripts recorded 2026-11-17, each golden next to its scenario, and the declared `stats/by-day-rollover-counted` and `stats/today-counted`. A20 gives them as request and reply tables, written with the entity tokens of `servers/transcripts` T10.1, and says which rows are recorded and which a transcript cannot express yet. `clients/` holds `servers/ankiconnect-clients` C7's scenarios.

The non-media, non-GUI actions that Meguri's AnkiConnect-compatible server (port 8765 by default) answers, with AnkiConnect's parameter names, result shapes and error strings. The interface this file targets is AnkiConnect API version 6 as served by the AnkiConnect add-on build of 2025-11-09 running in Anki 26.09.3, the build the U136 recorder pins. Its goldens are the authority on reply shapes; where Meguri differs on purpose, A18 lists the difference and its proposed deviation entry. That build was probed on 2026-11-10 (A21 Q6): the AnkiConnect replies stated in A4 to A14, A18 and A20 below are the observed ones, except where a section says a point is unverified. Meguri's own replies where it differs on purpose are specified, not observed: the A17 texts, the Meguri column of A18 (the A7.1 and A7.4 member errors among them) and the A20 rows marked (Meguri).

Every action here is a thin adapter: it decodes its parameters, runs one engine query on a server reader or one engine op of source `LocalApi` (`engine/op-model` O1), and shapes the reply. The collection behavior itself is the referenced specs' (notes, decks, note types, tags, scheduling, search, stats); this file states only what the action adds: parameter decoding, lookups by name, the checks AnkiConnect makes before calling the collection, result shapes and error strings.

Other files own the rest of the server:

- The transport, the request envelope (`action`, `version`, `params`, `key`), version wrapping of results, the error envelope, origins, CORS, the API key, `requestPermission`, `apiReflect`, `multi`, the read-only gate, the 30 s availability wait, the activity log, the agent snippet and the action registry: `servers/ankiconnect-envelope` (U141).
- `version`, `requestPermission`, `apiReflect`, `multi`: `servers/ankiconnect-envelope`. `sync` and the GUI actions: `servers/ankiconnect-gui` (U146).
- Media actions, the `audio`/`video`/`picture` attachments of note actions and the `getMediaDirPath` folder: `servers/ankiconnect-media` (U143).
- Binding, `Host` check, body limit and keep-alive: `servers/host-security` (U135).

## A1. Terms

- **Result**: the value an action returns. The envelope wraps it by version and turns an action **error** into the error envelope with the error's text (`servers/ankiconnect-envelope`). "Error `x`" below means the action fails with the text `x`, exactly, and writes nothing.
- **Read action** and **write action**: the registry flag `mutating` (A3). A write action runs as one `LocalApi` op; a read action runs on one server reader snapshot (`engine/lifecycle` L11) and writes nothing.
- **Human name** of a deck: `decks/decks#D1`. **Finding a deck by name**: `decks/decks#D2` (convert the given human name, then a stored name equal under `unicase`). The deck **of id 1** is the default deck, whatever its name.
- **Finding a note type by name**: the note type whose name is equal, under `unicase` (`text/unicode-primitives#U2`), to the given text as given, with no conversion or normalization. At most one matches (`notetypes/records` N4 step 7).
- **Field map of a note**: its note type's fields in field order, each with its value as stored.
- **Member errors**: a required member missing from an object, or a member of the wrong type, gives the envelope's texts `missing parameter: <path>` and `invalid parameter: <path>` (`servers/ankiconnect-envelope#E12.2`, `#E15.2`). The registry entry (A3.1) types an object parameter (`note`, an element of `notes`, of `cardTemplates`, an attachment) only as an object; it lists none of its members. Every member check is the action's, made at the point its section names, after the collection wait, so member checks and collection lookups interleave in the order the section gives (A7.1). `<path>` is the dotted path from the parameter (`note.modelName`). Inside an element of a list parameter (`notes`, `cardTemplates`, an attachment list), `<path>` is the member's own name within the element (`modelName`), so that one bad element fails only where the action says. AnkiConnect reports these cases with its language runtime's texts (the missing key in single quotation marks, `'modelName'`); transcripts compare them by shape (`servers/ankiconnect-envelope#E18`).
- **Lower-case form** of a text: the text after the Unicode default full lower-case mapping (Unicode Standard §3.13 toLowercase: SpecialCasing included, the Final_Sigma condition applied, no language tailoring). `Straße` and `STRASSE` have different lower-case forms (`straße`, `strasse`); `İ` becomes `i` U+0307. Two texts **match ignoring case** when their lower-case forms are equal code point for code point. This is AnkiConnect's rule for field names (A7.2) and tags (A10.1); it is not `unicase` (U2), which A1's name lookups use.
- **Python list form** of a list of messages: `[`, the messages in list order each written in Python string literal form, separated by `, ` (comma, space), then `]`. An empty list is `[]`. The **Python string literal form** of a text: it is enclosed in `'`, unless the text contains `'` and no `"`, in which case it is enclosed in `"`. Inside, `\` is written `\\`, the enclosing quote character is preceded by `\`, LF, CR and TAB are written `\n`, `\r`, `\t`, every other character that is not printable is written `\x` + 2 lower-case hex digits when below U+0100, else `\u` + 4 hex digits when below U+10000, else `\U` + 8 hex digits; printable characters, non-ASCII letters included, stay as they are. A character is printable unless its general category is Cc, Cf, Cs, Co, Cn, Zl, Zp or Zs; U+0020 is printable.

  | Messages | Python list form |
  |---|---|
  | `model was not found: x` | `['model was not found: x']` |
  | `deck was not found: Bob's`, `cannot create note because it is empty` | `["deck was not found: Bob's", 'cannot create note because it is empty']` |
  | `a'b"c` | `['a\'b"c']` |
  | `日本` TAB `x` | `['日本\tx']` |

- **PROFILE**: the single profile name Meguri reports (A4). Its value is `User 1` until A21 Q1 is decided.

## A2. Parameters and results

- Parameter names are exactly those of A3.1, case included. A required parameter that is missing, and a parameter of the wrong JSON type, are the envelope's errors (`servers/ankiconnect-envelope#E12.2`, no coercion: `0` is not a boolean); unknown extra parameters are ignored there (R1740). `null` is accepted only where A3.1's "Null" column says so.
- Ids (`cards`, `notes`, `card`, `note`, `deckId`, `modelId`, `modelIds`) are JSON integers. Results carry ids as JSON integers, except where a JSON object needs them as keys (A5.8), which are decimal strings.
- Lists of ids may name things that do not exist; each action says what happens. A list that names the same card or note twice is reduced to its first occurrences before a write op is called, so the op never sees a repeated id (`sched/manual-ops#M9`, `browser/bulk-actions#B7.2`). Read actions keep repeats and answer each position.
- Field values, tags, names and queries are JSON strings and are passed to the collection exactly as received: no trimming, no HTML change. The collection's own save normalization applies (`notes/note-records#N2`, `tags/tags#T4`, `decks/decks#D1`, `notetypes/records#N4`).
- Where the result is a list "in place" of an input list, it has one element per input element, in input order.
- Object members are written in the order the tables give. Members whose order follows the collection (fields, templates, decks) are written in that order.

## A3. Action table

`W` marks a write action (`mutating` in `servers/ankiconnect-envelope#E12.3`; refused in read-only mode, R1779). `Count` is what a successful write records as `affected` in the activity log (R1780, `servers/ankiconnect-envelope#E16`): the number of distinct objects of the kinds it names that the op inserted, changed or removed, each counted once. "+ media" adds the media files the action's attachments wrote into collection media (`servers/ankiconnect-media` X10; a reused identical file is not written and not counted). An op that changes nothing records 0. Reads, errors and refusals record 0, so the column is left empty for reads. Parameters are listed in A3.1.

| Action | Parameters | Result | Kind | Engine call | Count |
|---|---|---|---|---|---|
| `getProfiles` | none | list of names | read | none | |
| `getActiveProfile` | none | name | read | none | |
| `loadProfile` | `name` | boolean | read | none | |
| `reloadCollection` | none | null | read | none | |
| `deckNames` | none | list of names | read | deck names | |
| `deckNamesAndIds` | none | object | read | deck names | |
| `getDecks` | `cards` | object | read | card decks | |
| `createDeck` | `deck` | id | W | create deck (`decks/decks#D5`) | decks created (the named deck and its missing ancestors) |
| `changeDeck` | `cards`, `deck` | null | W | create deck, then Change Deck (`browser/bulk-actions#B7.2`) | cards whose row changed + decks created |
| `deleteDecks` | `decks`, `cardsToo` | null | W | delete decks (`decks/decks#D8`) | decks removed (descendants included) + notes removed + cards removed |
| `deckNameFromId` | `deckId` | name | read | deck by id | |
| `getDeckStats` | `decks` | object | read | deck tree counts (`sched/counts`) | |
| `modelNames` | none | list | read | note type names | |
| `modelNamesAndIds` | none | object | read | note type names | |
| `modelNameFromId` | `modelId` | name | read | note type | |
| `modelFieldNames` | `modelName` | list | read | note type | |
| `modelFieldDescriptions` | `modelName` | list | read | note type | |
| `modelFieldFonts` | `modelName` | object | read | note type | |
| `modelFieldsOnTemplates` | `modelName` | object | read | note type | |
| `modelTemplates` | `modelName` | object | read | note type | |
| `modelStyling` | `modelName` | object | read | note type | |
| `findModelsById` | `modelIds` | list | read | note types | |
| `findModelsByName` | `modelNames` | list | read | note types | |
| `createModel` | `modelName`, `inOrderFields`, `cardTemplates`, `css`, `isCloze` | note type object | W | add note type from scratch (`notetypes/records#N5.1`) | 1 |
| `addNote` | `note` | id | W | add note (`notes/note-records#A1`) | 1 + media |
| `addNotes` | `notes` | list of ids | W | bulk add (`notes/note-records#A5`) | notes added + media |
| `canAddNote` | `note` | boolean | read | field check | |
| `canAddNotes` | `notes` | list | read | field check | |
| `canAddNoteWithErrorDetail` | `note` | object | read | field check | |
| `canAddNotesWithErrorDetail` | `notes` | list | read | field check | |
| `updateNoteFields` | `note` | null | W | update note (`notes/note-records#U1`) | notes whose row changed (0 or 1) + media |
| `updateNote` | `note` | null | W | update note | notes whose row changed (0 or 1) + media |
| `updateNoteTags` | `note`, `tags` | null | W | update note | notes whose row changed (0 or 1) |
| `getNoteTags` | `note` | list | read | note | |
| `notesModTime` | `notes` | list | read | notes | |
| `deleteNotes` | `notes` | null | W | delete notes (`notes/note-records#D1`) | notes deleted |
| `findNotes` | `query` | list of ids | read | id search (`browser/result-sets#H13`) | |
| `notesInfo` | `notes` or `query` | list | read | id search, notes, cards | |
| `addTags` | `notes`, `tags`, `add` | null | W | add or remove tags (`tags/tags#T10`) | notes whose row changed |
| `removeTags` | `notes`, `tags` | null | W | remove tags (`tags/tags#T10`) | notes whose row changed |
| `getTags` | none | list | read | tag registry | |
| `clearUnusedTags` | none | null | W | Clear Unused Tags (`tags/tags#T11`) | tag registry rows removed |
| `replaceTags` | `notes`, `tag_to_replace`, `replace_with_tag` | null | W | update notes | notes whose row changed |
| `replaceTagsInAllNotes` | `tag_to_replace`, `replace_with_tag` | null | W | update notes | notes whose row changed |
| `findCards` | `query` | list of ids | read | id search | |
| `cardsInfo` | `cards` | list | read | cards, render, next states | |
| `cardsModTime` | `cards` | list | read | cards | |
| `cardsToNotes` | `cards` | list | read | cards | |
| `suspend` | `cards`, `suspend` | boolean | W | suspend or unsuspend (`sched/manual-ops#M3`, `#M4`) | cards whose row changed |
| `unsuspend` | `cards` | null | W | unsuspend (`sched/manual-ops#M4`) | cards whose row changed |
| `suspended` | `card` | boolean | read | card | |
| `areSuspended` | `cards` | list | read | cards | |
| `areDue` | `cards` | list | read | cards, review log, state search | |
| `getIntervals` | `cards`, `complete` | list | read | cards, review log | |
| `forgetCards` | `cards` | null | W | Reset (`sched/manual-ops#M6`) | cards whose row changed |
| `setDueDate` | `cards`, `days` | true | W | Set Due Date (`sched/set-due-date#S9`) | cards whose row changed |
| `getNumCardsReviewedToday` | none | integer | read | review log | |
| `getNumCardsReviewedByDay` | none | list | read | review log | |

- A write action whose op changes nothing still succeeds with its result; the op records no undo step and publishes no event (`engine/op-model` O4).
- No action here is recorded on the undo history; a write truncates it as `engine/undo` M7 says (R1795, R1796). Anki's own history after the same actions, and how it differs: `servers/api-undo-history` Y2 to Y4.
- Each write action is exactly one op, also where AnkiConnect makes several collection calls (`addNotes`, `updateNote`, `updateNoteTags`, `replaceTags`, `replaceTagsInAllNotes`, `changeDeck`, `deleteDecks`): it commits whole or not at all (R1790).
- A note add or update through this file never writes the Add-screen defaults (`notes/note-records#A4`: `update_add_defaults` false).

### A3.1 Parameters

The registry entry of each action (`servers/ankiconnect-envelope#E12.1`) holds exactly these parameters, in this order. "Registry" checks run before the collection wait with E12.2's texts; "action" checks are made by the action at the step its section names. `[int]` is a list of integers, `[string]` a list of strings. "Null" says whether `null` is accepted; an optional parameter that is `null` where null is accepted counts as absent.

| Action | Parameter | Type | Required or default | Null | Checked by |
|---|---|---|---|---|---|
| `loadProfile` | `name` | string | required | no | registry |
| `getDecks` | `cards` | [int] | required | no | registry |
| `createDeck` | `deck` | string | required | no | registry |
| `changeDeck` | `cards`; `deck` | [int]; string | required; required | no | registry |
| `deleteDecks` | `decks`; `cardsToo` | [string]; boolean | required; `false` | no; yes (as `false`) | registry |
| `deckNameFromId` | `deckId` | integer | required | no | registry |
| `getDeckStats` | `decks` | [string] | required | no | registry |
| `modelNameFromId` | `modelId` | integer | required | no | registry |
| `modelFieldNames`, `modelFieldDescriptions`, `modelFieldFonts`, `modelFieldsOnTemplates`, `modelTemplates`, `modelStyling` | `modelName` | string | required | no | registry |
| `findModelsById` | `modelIds` | [int] | required | no | registry |
| `findModelsByName` | `modelNames` | [string] | required | no | registry |
| `createModel` | `modelName`; `inOrderFields`; `cardTemplates`; `css`; `isCloze` | string; [string]; list of objects; string; boolean | required; required; required; `null`; `false` | no; no; no; yes; no | registry; members of `cardTemplates` elements: action (A6.2) |
| `addNote`, `canAddNote`, `canAddNoteWithErrorDetail`, `updateNoteFields`, `updateNote` | `note` | object | required | no | registry (object only); members: action (A7.1, A8) |
| `addNotes`, `canAddNotes`, `canAddNotesWithErrorDetail` | `notes` | list of objects | required | no | registry (a non-object element is `invalid parameter: notes`); members: action, per element |
| `updateNoteTags` | `note`; `tags` | integer; any | required; required | no; action decides (A8.3) | registry; `tags` type: action (A8.3) |
| `getNoteTags` | `note` | integer | required | no | registry |
| `notesModTime`, `deleteNotes` | `notes` | [int] | required | no | registry |
| `findNotes`, `findCards` | `query` | string | `null` | yes | registry |
| `notesInfo` | `notes`; `query` | [int]; string | `null`; `null` | yes; yes | registry; "neither given": action (A9.2) |
| `addTags` | `notes`; `tags`; `add` | [int]; string; boolean | required; required; `true` | no; no; no | registry |
| `removeTags` | `notes`; `tags` | [int]; string | required; required | no | registry |
| `replaceTags` | `notes`; `tag_to_replace`; `replace_with_tag` | [int]; string; string | required | no | registry |
| `replaceTagsInAllNotes` | `tag_to_replace`; `replace_with_tag` | string; string | required | no | registry |
| `cardsInfo`, `cardsModTime`, `cardsToNotes`, `unsuspend`, `areSuspended`, `areDue`, `forgetCards` | `cards` | [int] | required | no | registry |
| `suspend` | `cards`; `suspend` | [int]; boolean | required; `true` | no | registry |
| `suspended` | `card` | integer | required | no | registry |
| `getIntervals` | `cards`; `complete` | [int]; boolean | required; `false` | no | registry |
| `setDueDate` | `cards`; `days` | [int]; string | required; required | no | registry |

The actions not listed (`getProfiles`, `getActiveProfile`, `reloadCollection`, `deckNames`, `deckNamesAndIds`, `modelNames`, `modelNamesAndIds`, `getTags`, `clearUnusedTags`, `getNumCardsReviewedToday`, `getNumCardsReviewedByDay`) take no parameters. The media actions' parameters are `servers/ankiconnect-media` X2's.

## A4. Profiles and reload (R1754)

Meguri has one collection and reports it as one profile named PROFILE.

| Action | Result |
|---|---|
| `getProfiles` | `[PROFILE]` |
| `getActiveProfile` | `PROFILE` |
| `loadProfile` | `true` when `name` equals PROFILE code point for code point, otherwise `false`. Nothing else happens. |
| `reloadCollection` | `null`. Nothing happens: Meguri's readers always see committed state. |

`notesInfo` reports the same PROFILE (A9.2).

## A5. Decks (R1755)

### A5.1 `deckNames`

Every deck record's human name, in the order of `decks/decks#D17` (stored names under `unicase`), filtered decks and a hidden default deck included. Example: decks `Default`, `b`, `A::C`, `A` give `["A", "A::C", "b", "Default"]`.

### A5.2 `deckNamesAndIds`

A JSON object with one member per deck, in A5.1's order: the human name, then the id.

### A5.3 `getDecks`

`cards`: a list of card ids. The result is an object whose members are deck human names, each holding the listed card ids that are in that deck, in list order. A member appears at the position of the first card that names it. A card's deck is its current deck (`did`, so a card in a filtered deck is listed under the filtered deck). A card id that names no card, and a card whose deck does not exist, count as cards of the deck of id 1 and are listed under its name.

| `cards` | Result |
|---|---|
| c1 (deck `A`), c2 (deck `B`), c3 (deck `A`) | `{"A": [c1, c3], "B": [c2]}` |
| 999 (no such card) | `{"Default": [999]}` |
| (empty list) | `{}` |

### A5.4 `createDeck`

`deck`: a human name. Runs `decks/decks#D5` with it and returns the deck's id: the id of the deck that already has the same name (D2), writing nothing, or the id of the deck created last (the named deck itself, after its missing ancestors). D3's refusals (`filtered_parent`, `too_deep`) are errors with the Meguri texts of A17.

| Decks before | `deck` | Result |
|---|---|---|
| none but `Default` | `A::B::C` | the new id of `A::B::C`; `A` and `A::B` are created too |
| `A::B::C` | `a::b::c` | the id of `A::B::C`, nothing written |

### A5.5 `changeDeck`

`cards`: card ids; `deck`: a human name. The result is `null`.

1. Find the deck by name; when none exists, create it as A5.4 does, in the same op.
2. When the deck is filtered: error `cannot move cards into a filtered deck: <deck>` (A17), nothing written.
3. Move the listed cards with `browser/bulk-actions#B7.2` (cards already in the deck are skipped, cards in a filtered deck return home first, FSRS data and steps follow B7.2). Ids that name no card are skipped.

When `deck` names no deck and every listed card is skipped, the op still creates the deck.

### A5.6 `deleteDecks`

`decks`: human names; `cardsToo`: boolean, default `false`.

- When `cardsToo` is `false`, `null` or missing: error `Since Anki 2.1.28 it's not possible to delete decks without deleting cards as well`.
- Otherwise each listed name that equals a deck's human name code point for code point (no `unicase` matching, no conversion; `default` does not name `Default`) selects that deck; other names are skipped. The selected decks are deleted in list order by one `decks/decks#D8` op, which skips a deck already deleted as the descendant of an earlier one and keeps the default deck (D8 step 1). The result is `null`; nothing selected writes nothing.

### A5.7 `deckNameFromId`

`deckId`: an id. The result is the human name of that deck. When no deck has the id (0 included), the result is the human name of the deck of id 1. This action never fails for a missing deck.

### A5.8 `getDeckStats`

`decks`: human names, each found by name (A1, `decks/decks#D2`: converted, then `unicase`). The result is an object with one member per listed deck that exists and is a node of the deck list tree (`sched/counts` C1, C2: a hidden default deck and decks under a missing parent are not), in the tree's preorder (C1: each node before its children, siblings in `unicase` order). The member name is the deck id in decimal. Its value:

| Member | Value |
|---|---|
| `deck_id` | the deck id (integer) |
| `name` | the last component of the deck's human name |
| `new_count` | the node's displayed New (`sched/counts#C3`) |
| `learn_count` | the node's displayed Learn |
| `review_count` | the node's displayed Due |
| `total_in_deck` | the number of cards whose `did` is the deck, in any queue, children not included |

Counts are taken at the time of the request with the day of `sched/day-clock` D8, exactly as the deck list shows them (R1755). A listed name that finds no deck is left out (A18 `ankiconnect-getdeckstats-no-create`). A deck listed twice appears once.

Example: `Lang` (1 review due), `Lang::JA` (2 new, limits 20/200), `getDeckStats(["Lang::JA", "Lang", "Nope"])` gives `{"<id Lang>": {"deck_id": <id Lang>, "name": "Lang", "new_count": 2, "learn_count": 0, "review_count": 1, "total_in_deck": 1}, "<id Lang::JA>": {"deck_id": <id Lang::JA>, "name": "JA", "new_count": 2, "learn_count": 0, "review_count": 0, "total_in_deck": 2}}`.

## A6. Note types (R1756)

A note type that `modelName` does not find (A1) is error `model was not found: <modelName>`, with `modelName` as given; an id that names no note type is error `model was not found: <id>` in decimal.

| Action | Result |
|---|---|
| `modelNames` | every note type's name, in `unicase` order of the name (`text/unicode-primitives#U2`; names are unique under it, `notetypes/records#N4` step 7) |
| `modelNamesAndIds` | an object, name then id, in the same order |
| `modelNameFromId` | the name of the note type `modelId` |
| `modelFieldNames` | the field names in field order |
| `modelFieldDescriptions` | each field's description in field order (the empty string when it has none) |
| `modelFieldFonts` | an object, in field order: field name → `{"font": <font name>, "size": <font size>}` |
| `modelFieldsOnTemplates` | A6.1 |
| `modelTemplates` | an object, in template order: template name → `{"Front": <front>, "Back": <back>}`, the stored texts |
| `modelStyling` | `{"css": <the note type's CSS as stored>}` |
| `findModelsById` | for each id of `modelIds`, in order, the note type in schema-11 JSON form (`model/schema11-json#S4`, with its fields S5 and templates S6); the first id that names none is the error, and no list is returned |
| `findModelsByName` | the same for each name of `modelNames` |
| `createModel` | A6.2 |

### A6.1 `modelFieldsOnTemplates`

An object, in template order: template name → `[question names, answer names]`, two lists of strings, from the stored question text (`qfmt`) and answer text (`afmt`). The table gives the result for one template:

| Question / Answer text | Result |
|---|---|
| `{{Term}}` / `{{FrontSide}}<hr>{{Gloss}}` | `[["Term"], ["Gloss"]]` |
| `{{Term}}{{Term}}` / `{{Term}}{{Note}}{{Note}}` | `[["Term", "Term"], ["Note", "Note"]]` |
| `{{#Tags}}{{Tags}}{{/Tags}}` / `{{cloze:Body}}` | `[["Tags"], ["Body"]]` |
| `{{type:Term}}` / `{{text:cloze:Body}}` | `[["Term"], ["Body"]]` |
| `{{ Term }}` / `{{^Term}}x{{/Term}}` | `[[" Term "], ["^Term"]]` |
| `{{{Term}}}` / `{{a/b}}{{Term}}` | `[["Term"], []]` |
| `{{FrontSide}}` / `{{Gloss}}` | `[[], ["Gloss"]]` |
| `{Term}` / `{{}}` | `[[], []]` |

Every row was observed on the add-on. Anki's checked save refuses the templates of the `{{{Term}}}` and `{Term}` rows (`createModel` fails), so those two rows apply only to note types stored without the check (an import or a direct write).

The rules behind the table, in terms of the template syntax of `render/template-syntax`:

- Each side lists, in order of appearance, the **simple tags** of its text: a `{{`, a non-empty body, the first `}}` after it. A tag whose body holds `#`, `/` or `}` is not listed, so section openers and closers are skipped, while an inverted-section opener (`{{^Term}}`, whose body starts with `^`) is listed. The body may hold `{`, so the text `{{{Term}}` (a third brace before) is one tag. The text is read from the start and reading resumes after each tag.
- An entry is the tag's body with every `{` and `}` removed, then cut to what follows its last `:` (filters dropped): `{{type:Term}}` gives `Term`, `{{{Term}}` gives `Term`. Nothing is trimmed and no other prefix is removed (` Term `, `^Term`).
- An entry `FrontSide` is never listed. On the answer side, an entry that the question side lists is not listed. Repeats on one side are kept.

### A6.2 `createModel`

Parameters: `modelName` (string), `inOrderFields` (list of strings), `cardTemplates` (list of objects with `Front`, `Back` and optional `Name`), `css` (string, optional, default `null`), `isCloze` (boolean, default `false`).

Checks, in this order; the first that fails is the error:

| Check | Error |
|---|---|
| `inOrderFields` is empty | `Must provide at least one field for inOrderFields` |
| `cardTemplates` is empty | `Must provide at least one card for cardTemplates` |
| a note type's name equals `modelName` code point for code point | `Model name already exists` |
| a `cardTemplates` element lacks `Front` (then `Back`), or one is not a string | `missing parameter: Front` (then `Back`), `invalid parameter: Front` (member errors, A1) |

The order was observed on the add-on (empty `inOrderFields` before an existing name, an existing name before a missing `Front`). Its member texts are `'Front'`, `'Back'`, and for a non-string a backend JSON error text; they are compared by shape.

Then one op adds a note type from scratch (`notetypes/records#N5.1`): kind cloze when `isCloze` is true, else standard; the fields `inOrderFields` in order (N5.2); one template per element of `cardTemplates` in order (N5.3), named by its `Name` when present, else `Card <n>` with `n` its 1-based position in `cardTemplates` (so `[{Name: "X"}, {}]` gives `X` and `Card 2`); `css` when it is a string, else Meguri's base card CSS (N5.1). The add normalizes as `notetypes/records#N4` says, so a name equal to another note type's only under `unicase` gets `+` appended (N4 step 7; A18 `ankiconnect-createmodel-name-clash`), and runs the checked-save rules of `notetypes/validation#V1`–`#V3`. A refusal is an error with the Meguri text of A17 for its kind. The add sets `curModel` (N5).

The result is the stored note type in schema-11 JSON form (`model/schema11-json#S4`), as `findModelsById` would return it right after.

## A7. Adding notes (R1757–R1760)

### A7.1 The note object

| Member | Type | Required | Meaning |
|---|---|---|---|
| `deckName` | string | yes | the deck, by name (A1) |
| `modelName` | string | yes | the note type, by name (A1) |
| `fields` | object | yes | field name → value |
| `tags` | list of strings | no | the note's tags (default none) |
| `options` | object | no | A7.4 |
| `audio`, `video`, `picture` | object or list | no | attachments: `servers/ankiconnect-media` X10 |

A missing required member, or a member of another type, is a member error (A1): `missing parameter: note.modelName` for `addNote` and `canAddNote*`, `missing parameter: modelName` for an element of `notes`. A `fields` member value that is not a string is `invalid parameter: note.fields` (`fields` within an element of `notes`). The action makes these checks at the steps of A7.2, interleaved with the lookups.

Observed in the add-on: a missing member fails with the member name in single quotation marks (`'deckName'`, `'fields'`); a `deckName`, `modelName` or field value of another type, or a `tags` list holding a non-string, fails with `bad argument type for built-in operation`; a `fields` that is a string fails with `'str' object has no attribute 'items'`. These are compared by shape. A `tags` member that is a string is accepted, and each character of the string becomes one tag (`"pq rs"` gives ` p q r s `), where Meguri gives a member error (A18 `ankiconnect-note-member-types`, A21 Q9).

### A7.2 Building the note

The steps run in this order; the first that fails gives the note's error, and no later step runs:

1. `modelName`: missing, or not a string, is a member error.
2. Find the note type by `modelName`; none: error `model was not found: <modelName>`.
3. `deckName`: missing, or not a string, is a member error.
4. Find the deck by `deckName`; none: error `deck was not found: <deckName>`. A filtered deck: error `cannot create note in filtered deck: <deckName>` (R1758; A17, A18 `ankiconnect-add-filtered-deck`).
5. `tags`, when present: not a list of strings is a member error.
6. `fields`: missing, not an object, or holding a value that is not a string, is a member error.
7. Start from every field empty. For each member of `fields`, in object order, the first field of the note type whose name matches the member name ignoring case (A1, lower-case forms) takes its value; a member that matches no field is ignored. Two members matching one field: the later wins.
8. Append the attachments (`servers/ankiconnect-media` X10, including an attachment's own failure that fails the note, X10.2 step 3).
9. `options`, when present: not an object is `invalid parameter: note.options`; then A7.4's member type errors, in A7.4's row order.
10. Check the note (A7.3).

Example of competing errors: `modelName: "Nope"` with no `fields` member gives `model was not found: Nope`; a known `modelName` with no `deckName` and no `fields` gives `missing parameter: note.deckName`; `options: {allowDuplicate: "yes"}` on a note whose first field is empty gives the option error, not the empty error.

### A7.3 Checks and errors

The check depends on `options` (A7.4):

- **Default check** (`duplicateScope` is not `"deck"` and `checkAllModels` is not true): the field check of `notes/field-check` on the built note (K1 input: the note type, the built values, no note id). Its K3 state decides:

  | State | Outcome |
  |---|---|
  | `normal` | can add |
  | `empty` | error `cannot create note because it is empty` |
  | `duplicate` | error `cannot create note because it is a duplicate`; can add when `allowDuplicate` is true |
  | `missing_cloze`, `notetype_not_cloze`, `field_not_cloze` | error `cannot create note for unknown reason` |

  A note whose first field is filled but whose templates select no card is not refused: the add gives it its fallback card (`notes/card-generation#G6`), as Anki does (A21 Q7).

- **Scoped check** (`duplicateScope` is `"deck"`, or `checkAllModels` is true): no cloze checks, no card check. It looks at the **raw first field**, the built note's first value as built (not normalized, no HTML stripped), and its **checksum**: `text/checksum-and-sort-field#C4` steps 1 to 3, H5 applied to the raw first field with no NFC.

  Rules:

  - **Empty first.** When the raw first field is empty after removing white space from both ends (Unicode White_Space plus U+001C to U+001F), the error is `cannot create note because it is empty`, whatever the duplicate options say. A first field `<br>` is not empty here, unlike the default check.
  - **Duplicates are by checksum alone.** A note of the collection is a duplicate when its stored `csum` equals the checksum, and either it has the built note's note type or `checkAllModels` is true, and it lies in the deck scope (below). Field texts are not compared.
  - **Deck scope.** With `duplicateScope` other than `"deck"`, every deck is in scope. With `"deck"`, the scope is one deck and, when `checkChildren` is true, its descendants (`decks/decks#D3`); a note lies in the scope when at least one of its cards has its current `did` there. The one deck is the target deck, unless `duplicateScopeOptions.deckName` is present and not null; then it is the deck that name finds (A1). A `deckName` that finds no deck gives an empty scope, so no note is a duplicate.
  - A duplicate gives the error `cannot create note because it is a duplicate`, unless `allowDuplicate` is true.

  Outcomes for a note `x` of note type `Pair` added to deck `R`, when one other note `x` exists (same checksum) of note type `T` with its card in deck `P::Q`:

  | `T` | `duplicateScope` | `deckName` | `checkChildren` | `checkAllModels` | Outcome |
  |---|---|---|---|---|---|
  | `Pair` | absent | | | | duplicate (default check) |
  | `Pair` | `"deck"` | absent | false | false | can add (`R` holds no card of it) |
  | `Pair` | `"deck"` | `P` | false | false | can add (`P` itself holds none) |
  | `Pair` | `"deck"` | `P` | true | false | duplicate |
  | `Pair` | `"deck"` | `Nope` | true | false | can add (empty scope) |
  | other | absent | | | false | can add (default check: other note type) |
  | other | absent | | | true | duplicate (whole collection, every note type) |
  | other | `"deck"` | absent | false | true | can add (`R` holds none) |

Attachments are appended before the check (A7.2 step 8), so an attachment reference can make the first field non-empty or a duplicate.

### A7.4 `options`

| Member | Type | Default | Error when of another type |
|---|---|---|---|
| `allowDuplicate` | boolean | false | `option parameter "allowDuplicate" must be boolean` |
| `duplicateScope` | any | none | none; only the string `"deck"` has a meaning |
| `duplicateScopeOptions` | object or null | none | `invalid parameter: note.options.duplicateScopeOptions` (member error, A1) |
| `duplicateScopeOptions.deckName` | string or null | null (the target deck) | `invalid parameter: note.options.duplicateScopeOptions.deckName` (member error) |
| `duplicateScopeOptions.checkChildren` | boolean | false | `option parameter "duplicateScopeOptions.checkChildren" must be boolean` |
| `duplicateScopeOptions.checkAllModels` | boolean | false | `option parameter "duplicateScopeOptions.checkAllModels" must be boolean` |

Options are read only when present; `duplicateScopeOptions` is read even when `duplicateScope` is not `"deck"` (so `checkAllModels` alone selects the scoped check over the whole collection). `duplicateScopeOptions` `null` counts as absent. For an element of `notes`, the member-error paths start at `options` (`invalid parameter: options.duplicateScopeOptions`). AnkiConnect has no type check for `options`, `duplicateScopeOptions` and its `deckName`. Observed: an `options` or `duplicateScopeOptions` that is not an object is ignored, and the note added, only when it is a string that contains none of its member names (`"xyz"`) or a list that has none of them as an element (`[]`); any other non-object fails with a runtime text: a number or boolean (`argument of type 'int' is not iterable`), a string that contains a member name (`"allowDuplicate"`: `string indices must be integers, not 'str'`), a list that holds one (`list indices must be integers or slices, not str`). `duplicateScopeOptions` `null` fails with `argument of type 'NoneType' is not iterable`; a `deckName` of another type fails with `bad argument type for built-in operation`. Meguri keeps the member errors of this table and treats `null` as absent. The replies that differ in outcome (an ignored `options` or `duplicateScopeOptions`, and `null`) are A18 `ankiconnect-note-member-types`; the other errors are compared by shape (`servers/ankiconnect-envelope#E18`).

### A7.5 `addNote`

Builds and checks the note (A7.2, A7.3), then adds it in one op with `notes/note-records#A1`: the note type, the built values, `tags` (stored by `tags/tags#T4`), the deck's id, no GUID, `update_add_defaults` false. The cards go as `notes/deck-placement-and-position#D2` says. The result is the new note's id. The attachments' bytes are obtained before the op opens, off the writer (`servers/ankiconnect-media` X10.2); their files are named and written inside the same op (`media/media-files#M13`), so a failed add leaves none (A18 `ankiconnect-failed-add-media`).

### A7.6 `addNotes`

`notes`: a list of note objects. One op adds them all or none (R1760):

- The notes are built and checked in list order, each as A7.5, and each check sees the notes the same request added before it, so a second note that duplicates the first fails as a duplicate. Every failure of A7.2 and A7.3 is that note's error, member errors and an attachment failure that fails the note (`servers/ankiconnect-media` X10.2 step 3) included; it never fails the request by itself.
- When every note passes, the result is the list of new note ids in list order (`notes/note-records#A5` bulk add: one op).
- When at least one note fails, nothing is written (no note, card, grave, deleted-notes log line, position or media file) and the error is the Python list form (A1) of the failing notes' error texts, in list order. Passing notes contribute nothing to it.

| `notes` | Result |
|---|---|
| (empty list) | `[]` |
| A (ok), B (note type `Nope`), C (ok) | error `['model was not found: Nope']`, nothing added |
| A (ok), A again | error `['cannot create note because it is a duplicate']` |
| A (deck `X` missing), B (empty first field) | error `['deck was not found: X', 'cannot create note because it is empty']` |

(AnkiConnect adds the passing notes and then deletes them; A18 `ankiconnect-addnotes-rollback`.)

### A7.7 `canAdd*`

These four build and check notes exactly as A7.2 and A7.3 do, without step 8, and write nothing. Attachments are not fetched or stored: the check sees the fields without attachment references (A18 `ankiconnect-canadd-no-media`). Each note is checked on its own against the collection as it is (no note of the request counts for another).

| Action | Result per note |
|---|---|
| `canAddNote` | `true` when the note can be added, `false` for every error (lookups and member errors included) |
| `canAddNotes` | a list of those booleans, in place |
| `canAddNoteWithErrorDetail` | `{"canAdd": true}`, or `{"canAdd": false, "error": "<the error text>"}` |
| `canAddNotesWithErrorDetail` | a list of those objects, in place |

## A8. Updating and deleting notes (R1763)

A note id that names no note is error `Note was not found: <id>`, with a capital `N` as the add-on writes it (observed for `updateNoteFields`, `updateNote`, `updateNoteTags` and `getNoteTags`, 2026-11-10). R1738 writes `note was not found: <id>`; A21 Q2 must be decided before implementation, and this file changes with the decision. A missing `id` or `fields` member gives the add-on's `'id'` or `'fields'` (compared by shape).

### A8.1 `updateNoteFields`

`note`: an object with `id` (required, `missing parameter: note.id`), `fields` (required, `missing parameter: note.fields`) and the attachment members of A7.1.

1. Find the note; none: the error above.
2. For each member of `fields`, the field whose name equals the member name code point for code point (case-sensitive) takes the value; other members are ignored.
3. Append the attachments (`servers/ankiconnect-media` X10; their `fields` lists also match names case-sensitively).
4. One op updates the note with `notes/note-records#U1`–`#U3` (the note's tags unchanged). A save that changes nothing writes nothing (U2).

The result is `null`.

### A8.2 `updateNote`

`note`: an object with `id` and at least one of `fields` and `tags`; neither present: error `Must provide a "fields" or "tags" property.` With `fields`, the fields change as A8.1 steps 2 and 3; with `tags`, the tags become A8.3's result for that value. Both apply in one op (one note update). The result is `null`.

### A8.3 `updateNoteTags`

`note`: an id; `tags`: a string (taken as a list of that one string) or a list of strings; anything else, or a list holding a non-string: error `Must provide tags as a list of strings`. The note's tags become exactly the tags obtained by splitting each listed string at separators (`tags/tags#T1` tag text), stored by `tags/tags#T4`. The result is `null`.

| Stored | `tags` | After |
|---|---|---|
| ` a b ` | `["c"]` | ` c ` |
| ` a ` | `"x y"` | ` x y ` |
| ` a ` | `[]` | (empty) |

### A8.4 `getNoteTags`

`note`: an id. The note's stored tags as a list, in stored order (`tags/tags#T4`). Missing note: the error above.

### A8.5 `notesModTime`

`notes`: ids. In place: `{"noteId": <id>, "mod": <the note's mod>}`, or `{}` for an id that names no note.

### A8.6 `deleteNotes`

`notes`: ids. One op deletes the notes that exist with `notes/note-records#D1` (graves, the deleted-notes log). Ids that name no note are skipped. The result is `null`.

## A9. Querying notes (R1764)

### A9.1 `findNotes`

`query`: a search text, optional. Missing or `null`: the result is `[]`. Otherwise the result is A16's note search of the text: note ids in ascending order.

### A9.2 `notesInfo`

`notes` (ids) or `query` (search text). When `notes` is absent or null and `query` is absent or null: error `Must provide either "notes" or a "query"`. With a `query` that is not null, the ids are A9.1's result for it (and `notes` is ignored). The result is, in place of each id:

| Member | Value |
|---|---|
| `noteId` | the id |
| `profile` | PROFILE |
| `tags` | the stored tags as a list |
| `fields` | an object in field order: field name → `{"value": <stored value>, "order": <field index from 0>}` |
| `modelName` | the note type's name |
| `mod` | the note's `mod` |
| `cards` | the note's card ids in ascending template ordinal (`ord`) order; `[]` for a note that has no card (A18 `ankiconnect-notesinfo-no-cards`) |

or `{}` for an id that names no note. Repeated ids give repeated entries.

## A10. Tags (R1765)

| Action | Parameters | Effect | Result |
|---|---|---|---|
| `addTags` | `notes`: ids; `tags`: tag text; `add`: boolean, default `true` | with `add` true, `tags/tags#T10` Add of the text to the notes; with `add` false, T10 Remove | `null` |
| `removeTags` | `notes`, `tags` | T10 Remove of the text from the notes (matching per T5, so `foo` also removes `foo::kid`) | `null` |
| `getTags` | none | every registered tag name (`tags` rows), in `unicase` order of the name (`text/unicode-primitives#U2`) | list |
| `clearUnusedTags` | none | `tags/tags#T11` | `null` |
| `replaceTags` | `notes`, `tag_to_replace`, `replace_with_tag` | A10.1 on the listed notes | `null` |
| `replaceTagsInAllNotes` | `tag_to_replace`, `replace_with_tag` | A10.1 on every note | `null` |

`tags` must be a string: a list is the envelope's type error (the add-on answers `bad argument type for built-in operation`; compared by shape).

### A10.1 Replacing one tag

For each note that exists (missing ids skipped), when one of its stored tags matches `tag_to_replace` ignoring case (A1, lower-case forms), every stored tag that matches it is removed, `replace_with_tag` is added once as one item of the tag list, and the note's tags are stored by `tags/tags#T4`. So the replacement is split at separators (an item `x y` gives two tags; an empty or all-separator item gives none, which leaves the matching tags removed and nothing added), normalized, spelled and registered, and the list sorted. Only whole tags match: `a` does not match `a::b` or `ab`. A note without a matching tag is untouched. All notes change in one op.

| Stored | `tag_to_replace` → `replace_with_tag` | After |
|---|---|---|
| ` Old other ` | `old` → `new` | ` new other ` |
| ` old::kid ` | `old` → `new` | unchanged |
| ` a OLD old ` | `old` → `z` | ` a z ` |
| ` a old ` | `old` → `x y` | ` a x y ` |
| ` a old ` | `old` → (empty) | ` a ` |
| ` ΑΣ ` | `ας` → `z` | ` z ` (lower-case form of `ΑΣ` is `ας`) |

The rows were observed on the add-on. A save keeps one spelling of tags equal ignoring case (`tags/tags#T3`, `#T4`), so the stored ` a OLD old ` of row 3 is reached only by a direct database write. Through a save it is ` a OLD `, the first spelling, or the registered spelling when the registry already holds one (` a Old ` after a note tagged `Old`), with the same result.

## A11. Finding and reading cards (R1766)

### A11.1 `findCards`

`query`: optional, as A9.1: missing or `null` gives `[]`; otherwise A16's card search, card ids ascending.

### A11.2 `cardsInfo`

`cards`: ids. In place of each id that names a card:

| Member | Value |
|---|---|
| `cardId` | the id |
| `fields` | the note's field map as in A9.2 |
| `fieldOrder` | the card's `ord` |
| `question` | A11.3 question |
| `answer` | A11.3 answer |
| `modelName` | the note type's name |
| `ord` | the card's `ord` |
| `deckName` | the human name of the card's current deck (`did`); the deck of id 1's when it does not exist |
| `css` | the note type's CSS as stored |
| `factor`, `interval` | the card's `factor`, `ivl` as stored |
| `note` | the card's `nid` |
| `type`, `queue`, `due`, `reps`, `lapses`, `left`, `mod` | the card's `type`, `queue`, `due`, `reps`, `lapses`, `left`, `mod` as stored |
| `nextReviews` | A11.4 |
| `flags` | the card's `flags` as stored |

An id that names no card, or a card whose note does not exist, gives `{}`. The member order above is the add-on's observed order (`note` between `interval` and `type`).

### A11.3 Question and answer

The card is rendered in review mode (`render/card-render-pipeline#K4`, read only, K7) and the two texts are composed as `render/display-transform#D9`'s table, with T replaced by the identity: no replay buttons, no media-name encoding, no type-in box. So play markers (`[anki:play:q:0]`) and type-in markers stay as the pipeline wrote them, and each side starts with `<style>` + CSS + `</style>`:

| Outcome (K4) | `question` | `answer` |
|---|---|---|
| rendered | `<style>` css `</style>` question text | `<style>` css `</style>` answer text |
| blank | `<style>` css `</style>` question text + D7 notice | `<style>` css `</style>` D7 notice |
| template error | `<style></style>` D8 notice | `<style></style>` D8 notice |

LaTeX is not turned into images and nothing is written (K8; A18 `ankiconnect-cardsinfo-latex`). The D8 notice has Meguri's wording (A18 `ankiconnect-cardsinfo-template-error`). The three outcome rows, the CSS prefix and the empty `<style></style>` of a template error were observed on the add-on.

### A11.4 `nextReviews`

The four answer button labels (Again, Hard, Good, Easy, in that order) for answering the card now, as the review screen shows them: the next states of the card (U45, through U43) described by `sched/answer-labels` A1–A6. Always four strings, also for new, suspended and buried cards. The add-on wraps each amount in U+2068 and U+2069 (a new card's labels read `<`, U+2068, `1`, U+2069, `m` and so on); Meguri's labels carry no isolation marks (`sched/answer-labels`). The transcript comparison removes these two characters from `nextReviews` items before comparing (`servers/transcripts` T10.3), so the labels need no deviation entry.

### A11.5 `cardsModTime`

In place: `{"cardId": <id>, "mod": <the card's mod>}`, or `{}`.

### A11.6 `cardsToNotes`

The distinct note ids of the listed cards that exist, ordered by the smallest listed card id of each note, ascending. Missing ids are skipped.

| `cards` | Result |
|---|---|
| c3 (note n1), c1 (note n2), c2 (note n1) | `[n2, n1]` when c1 < c2 < c3 |

## A12. Card state (R1767)

A card id that names no card is error `Card was not found: <id>` where an action below says so (capital `C`).

### A12.1 `suspend`

`cards`: ids; `suspend`: boolean, default `true` (a parameter AnkiConnect accepts although its documentation does not list it).

- With `suspend` true, one op suspends the listed cards (`sched/manual-ops#M3`); with `suspend` false, it unsuspends them as `unsuspend` does (A12.2).
- When a listed id names no card: error `Card was not found: <id>` for the first such id in list order, nothing written (A18 `ankiconnect-suspend-unknown-card`).
- The result: `false` when `cards` is empty, or when it holds exactly one card and that card already has the requested state (suspended: `queue` −1; for `suspend` false: not suspended). Otherwise `true`, also when no card changed. This is AnkiConnect's result. It differs from R1767's "true if anything changed" for a list of two or more cards that all have the requested state already; A21 Q3 must be decided before implementation.

Observed on the add-on (2026-11-10):

| Cards before | Request | Result |
|---|---|---|
| (none) | `suspend {cards: []}` | `false` |
| c3 suspended | `suspend {cards: [c3]}` | `false` |
| c1, c2 not suspended | `suspend {cards: [c1, c2]}` | `true`; both suspended |
| c1, c2 suspended | `suspend {cards: [c1, c2]}`; with c3 also, `[c1, c2, c3]` | `true`; `true` |
| c1 suspended, c5 not | `suspend {cards: [c1, c5]}` | `true` |
| c4 not suspended | `suspend {cards: [c4], suspend: false}` | `false` |
| c1 suspended | `suspend {cards: [c1], suspend: false}` | `true`; c1 unsuspended |
| a buried card | `suspend` on it | `true`; suspended |
| c3 suspended; 7 no card | `suspend {cards: [c3, 7]}` | `true`, no error, nothing written |
| the same | `suspend {cards: [7, c3]}`, `{cards: [7]}`; c4 not suspended: `{cards: [c4, 7]}` | error `Card was not found: 7` |

The `[c3, 7]` row is where Meguri differs (A18 `ankiconnect-suspend-unknown-card`: Meguri fails it with `Card was not found: 7`).

### A12.2 `unsuspend`

`cards`: ids. One op runs `sched/manual-ops#M4` on the listed cards whose `queue` is −1; buried and unsuspended cards are untouched. Unknown ids: as A12.1. The result is `null`, as AnkiConnect's (A21 Q3). With two or more listed cards, AnkiConnect may also unbury a listed buried card; Meguri never does, and the scenarios list no buried card here.

Observed on the add-on: the result is `null` for an empty list, for unsuspended cards and for suspended ones. A listed id that names no card fails with `Card was not found: <id>` and nothing is restored (`[c3, 7]` with c3 suspended leaves c3 suspended). Whether a listed buried card (user or scheduler buried) is restored with the suspended cards depends on its position in the list: `[c1, c2, c4]` with c1 unsuspended, c2 buried, c4 suspended restores c2 and c4; `[c4, c3]` with c4 suspended and c3 buried restores c4 only.

### A12.3 `suspended`, `areSuspended`

- `suspended`: `card`: an id. `true` when its `queue` is −1, else `false`. Missing card: error `Card was not found: <id>`.
- `areSuspended`: `cards`: ids. In place: `true`, `false`, or `null` for an id that names no card.

### A12.4 `areDue`

`cards`: ids. In place, one boolean per id. The **latest row** of an id is its review-log row with the largest `id` (rows are looked up by card id, so a deleted card's rows still count).

- A card of type new (`type` 0, `search/state-and-flags#S5` `is:new`) is `true`, whatever its review log holds.
- Any other id is judged by its latest row:
  - A latest row whose `ivl` is −1200 or more (a review interval in days, or a learning step of at most 20 minutes) gives whether the card is due now: `search/state-and-flags#S1` with A16's context. An id that names no card is never due.
  - A latest row whose `ivl` is below −1200 (a learning step longer than 20 minutes, in seconds) gives `true` when the row's time in seconds (`id` / 1000, with its fraction) plus −`ivl` is at most now in seconds, compared as real numbers, else `false`.
- An id that is not a new card and has no review-log row fails the whole action with `list index out of range` (the add-on's text, A21 Q10).

| Card (now = T) | Result |
|---|---|
| new, never answered | `true` |
| new after a Reset, with old rows | `true` |
| review, `queue` 2, `due` today; latest row `ivl` 4 | `true` |
| review, `due` tomorrow; latest row `ivl` 4 | `false` |
| learning, `queue` 1, `due` T − 60 s; latest row `ivl` −600 | `true` |
| learning; latest row at T − 3,600 s with `ivl` −1800 | `true` (T − 3,600 + 1,800 ≤ T) |
| learning; latest row at T − 600 s with `ivl` −1800 | `false` |
| suspended review card; latest row `ivl` 4 | `false` (S1: never due) |
| learning card with no row (imported) | error `list index out of range` (A21 Q10) |
| id 7, no card, no row | error `list index out of range` (A21 Q10) |

Every row was observed on the add-on. With a latest row of `ivl` −600, a learning card due at now + 1,200 s is `true` and one due at now + 1,300 s is `false`: S1 includes the learn-ahead seconds (`collapseTime`, default 1,200).

### A12.5 `getIntervals`

`cards`: ids; `complete`: boolean, default `false`. In place, for each id:

| Card | `complete` false | `complete` true |
|---|---|---|
| of type new (`type` 0) | `0` | `0` |
| other, with review-log rows | the `ivl` of its latest row (A12.4) | the list of `ivl` of all its rows, in ascending row `id` order |
| other, with no review-log row (an unknown id included) | error `list index out of range` (the whole action, A21 Q10) | `[]` |

A negative value is a learning interval in seconds; a positive value is days (R1767). Observed on the add-on: a new card is `0` with `complete` true too, also after a Reset that left review-log rows; an unknown id gives `[]` with `complete` true.

## A13. Rescheduling (R1768)

- `forgetCards`: `cards`: ids. One op runs `sched/manual-ops#M6` Reset with "Restore original position" on, "Reset repetition and lapse counts" off and no context (no remembered keys written, M6.4), in list order. Missing ids are skipped. The result is `null`.
- `setDueDate`: `cards`: ids; `days`: the days text. One op runs `sched/set-due-date#S9` with the text and no context (S6: nothing remembered). Missing ids are skipped (observed: `setDueDate {cards: [7], days: "1"}` gives `true`). The result is `true`. An invalid text is an error with the Meguri text of A17 for `InvalidDays`; nothing is written.

Both write exactly the rows the UI's Forget and Set Due Date write on the same cards (review-log rows included). Observed on the add-on: each wrote one review-log row of `type` 4 per card.

## A14. Statistics (R1774)

Both actions read the review log with `stats/day-buckets` B1's inputs (N, the day clock's next day start; the device zone) at the time of the request. Following R1774 they count the rows that Meguri's studied-today count counts: every row whose `type` is neither 4 nor 5 (`stats/counting-rules#N6`), deleted cards' rows included (A18 `ankiconnect-review-counts-exclusions`; A21 Q4).

- `getNumCardsReviewedToday`: the number of such rows in the studied-today window (`stats/day-buckets#B2`: `id` > (N − 86,400) × 1,000).
- `getNumCardsReviewedByDay`: a list of `[date, count]` pairs, one per date that has at least one such row, newest date first. `date` is a `YYYY-MM-DD` text: the local date, in the device zone, of the instant `floor(id / 1,000) − H × 3,600` seconds, read with the offset the zone has at that instant (as `stats/day-buckets#B4.1`), where H is the hour of N's local time (the rollover hour, except where a time zone change at the rollover hour moved N). `count` is an integer.

Observed on the add-on (zone Europe/Paris, rollover 4): rows at 03:59 and 04:01 local on 2030-01-15 count for `2030-01-14` and `2030-01-15`, and only the 04:01 row counts today; rows of `type` 4 and 5 are counted by both actions, as are the `type` 4 rows that `setDueDate` and `forgetCards` write (A18 `ankiconnect-review-counts-exclusions`).

## A15. Writes and preservation (R1789, R1790)

- Every write is a `LocalApi` op through the same core ops as the UI (A3's engine calls), so mod times, usn, graves, the deleted-notes log, card generation, note and card normalization and the preservation of unknown data on touched rows are those ops' (`notes/note-records` U3: `data` kept; `model/card-data-json` C4; `notetypes/records`).
- Ops commit in arrival order with UI writes (`engine/op-model` O2). Reads run concurrently on server readers.
- A read action reads one snapshot: a `notesInfo` or `cardsInfo` reply never mixes rows from before and after a concurrent write.

## A16. Search (R1776–R1778)

`findNotes`, `findCards` and `notesInfo` with `query` run `browser/result-sets#H13` (notes mode or cards mode) on the text as given:

- The grammar, the search context (`search/query-model#Q8`: `deck:current` from `curDeck`, `decks/decks#D11`), case folding (`search/case-folding`), "ignore accents" and normalization are Browse's (R1776, R1777).
- The empty text matches every note or card.
- Results are ascending ids (A18 `ankiconnect-search-order`).
- A search error is an action error whose text is the message Browse shows (`search/errors#E7`) (R1778; A18 `ankiconnect-search-error-text`).

## A17. Meguri error texts

Errors that AnkiConnect does not have, or whose AnkiConnect text comes from Anki internals rather than an interface, have these Meguri texts. They are proposed wording (A21 Q5); implementers take them only from here.

| Where | Text |
|---|---|
| `addNote` and the `canAdd*` actions, filtered `deckName` (A7.2) | `cannot create note in filtered deck: <deckName>` |
| `changeDeck` into a filtered deck (A5.5) | `cannot move cards into a filtered deck: <deck>` |
| `createDeck`, `changeDeck`: `filtered_parent` (`decks/decks#D3`) | `cannot create deck under a filtered deck: <deck>` |
| `createDeck`, `changeDeck`: `too_deep` | `deck name has too many levels: <deck>` |
| `createModel`: `invalid_notetype` (`notetypes/records#N4`, `notetypes/validation#V2`) | `invalid note type: <reason>` |
| `createModel`: a V3 template check | `invalid card template: <reason>` |
| `setDueDate`: `InvalidDays` (`sched/set-due-date#S2`) | `invalid days value: <days>` |

`<reason>` is the engine refusal's own message (`notetypes/validation#V4`). `<deck>`, `<deckName>` and `<days>` are the parameter as given.

## A18. Deviations from AnkiConnect

Each row is a proposed `conformance/deviations.yaml` entry for the U136 replay. "Scenario" names the A20 (or `servers/ankiconnect-media` X14) row whose golden shows the difference and whose Meguri expectation asserts Meguri's behavior.

| Proposed id | AnkiConnect | Meguri | Scenario |
|---|---|---|---|
| `ankiconnect-search-order` | `findNotes`/`findCards` ids in the database's scan order (observed: `findCards {query: "deck:Lang"}` gave `[#c2, #c1]`; `findNotes {query: ""}` listed a later note type's note last) | ascending ids (R1777) | `search/empty` (`search/current`'s golden is ascending already) |
| `ankiconnect-search-error-text` | Anki's search error text: `Invalid search: ` and the message wrapped in U+2068/U+2069 (observed) | `search/errors#E7` text | `search/error` |
| `ankiconnect-add-defaults` | every add writes `curModel`, `_nt_<id>_lastDeck`, `_deck_<id>_lastNotetype` (observed); the note type's schema-11 `did`, `null` before the first add, then reads as that deck's id in `findModels*` | none written (`notes/note-records#A4`) | `notes/add-defaults` (the `did` only; the keys themselves are asserted by Meguri's tests of `notes/note-records#A4`, A20.3) |
| `ankiconnect-add-filtered-deck` | a filtered `deckName` is accepted and the cards go to Default (`notes/deck-placement-and-position#D2`); `canAddNoteWithErrorDetail` answers `{"canAdd": true}` (observed) | error (R1758) | none yet: `notes/add-filtered-deck` cannot be expressed in a transcript (A20.3); Meguri's tests assert A7.2 |
| `ankiconnect-addnotes-rollback` | passing notes are added, then deleted: graves, deleted-notes log lines, `nextPos` moved, their media files kept (observed: two passing notes left four graves and moved `nextPos` from 5 to 7; an attachment file stayed) | nothing written | `notes/add-notes` |
| `ankiconnect-failed-add-media` | attachment files of a failed add stay in the media folder (observed for a duplicate) | none left (`media/media-files#M13`) | `media/attach-rollback` |
| `ankiconnect-canadd-no-media` | `canAdd*` fetch and store attachment files (observed: `canAddNote` with a `data` attachment stored the file) | nothing fetched or stored | `notes/can-add-media` |
| `ankiconnect-changedeck-op` | every listed card stamped with the new `did` (also cards already there: their `mod` changes), no step remap, FSRS keys kept; a filtered target accepted (`did` the filtered deck, `odid` 0); a new name under a filtered deck fails with `Filtered decks can not have child decks.` (observed, except the step and FSRS points) | `browser/bulk-actions#B7.2`; filtered target refused | `decks/change-deck` |
| `ankiconnect-getdeckstats-no-create` | a listed name that finds no deck creates it and reports it with zero counts (observed) | left out, nothing created | `decks/deck-stats` |
| `ankiconnect-suspend-unknown-card` | an unknown id fails only where AnkiConnect happens to look it up (observed: `[c3, 7]` with c3 suspended gives `true`) | the first unknown id always fails, nothing written | `cards/suspend` |
| `ankiconnect-unsuspend-buried` | with two or more listed cards, a listed buried card may be unburied (observed, A12.2) | buried cards untouched | none yet: a transcript cannot bury a card with an action Meguri answers (A20.5) |
| `ankiconnect-review-counts-exclusions` | every review-log row counts (observed, A14) | types 4 and 5 left out (R1774) | `stats/by-day-rollover`, `stats/today` |
| `ankiconnect-createmodel-name-clash` | a name equal under `unicase` only gets `-` and five characters derived from the clock (observed: `pair` beside `Pair` gave `pair-6455a`) | `+` appended (`notetypes/records#N4` step 7) | `models/create` |
| `ankiconnect-createmodel-css` | Anki's stock CSS when `css` is missing or `null` (observed) | Meguri's base card CSS (`notetypes/records#N5.1`) | `models/create` |
| `ankiconnect-cardsinfo-template-error` | Anki's template error text, after `<style></style>` (observed) | Meguri's D8 notice | `cards/info-errors` |
| `ankiconnect-cardsinfo-latex` | `[latex]`, `[$]` and `[$$]` in `question` and `answer` replaced by LaTeX image tags, with images generated into media (or Anki's LaTeX error text) as a side effect of the read. Observed with the recorder's default preferences: `[$]x[/$]` became an `<img class=latex …>` tag naming a `latex-<40 hex>.png` file, followed by `LaTeX image generation is disabled in the preferences.`, and no file was written | left as stored, nothing written (`render/card-render-pipeline#K8`) | `cards/info-errors` |
| `ankiconnect-notesinfo-no-cards` | a listed note without cards fails the whole action with the runtime's missing-key text (observed: the note id in decimal, `1894701600005`) | `cards: []` | none yet: `notes/info`'s third row cannot be expressed in a transcript (A20.3); Meguri's tests assert A9.2 |
| `ankiconnect-repeated-ids` | a write list naming a card twice fails with a database error, or handles it twice. Observed: `setDueDate` fails with a backend database error text that reports a failed UNIQUE constraint, and writes nothing. `forgetCards` succeeds and resets the card once per occurrence: `[c, c]` writes two review-log rows of `type` 4 (`factor` 0) where `[c]` writes one | first occurrences only (A2) | `cards/repeated-ids`: pointers `/steps/2/db_dump/revlog` (Meguri: the golden's rows without its last one, one Reset row), `/steps/3/reply/body/json` (Meguri: `{"result": true, "error": null}`), `/steps/4/db_dump/cards/#c2` (Meguri: type 2, queue 2, `due` 1, `ivl` 0, `factor` 2500, `data` `{"pos":2}`, as `setDueDate {cards: [#c2], days: "1"}` writes) and `/steps/4/db_dump/revlog` (Meguri: its step-2 rows plus one type-4 row with `factor` 2500, `ivl` 0 and `lastIvl` 0). The exact error text stays in the golden and is replaced as a whole `/body/json` |
| `ankiconnect-error-text` | Anki's backend error texts for the A17 cases. Observed: an invalid `days` text gives the text itself (`abc`); a deck under a filtered deck gives `Filtered decks can not have child decks.`; a refused template gives `Card template ⁨1⁩ in note type '⁨<name>⁩' has a problem.<br>` and a reason (U+2068/U+2069 around the number and the name; observed for an unclosed section `{{#F}}`: `See the preview for more information.`); too deep is unverified | the A17 texts | `cards/reschedule`, `models/create`, `undo/create-model-refused` (pointer `/steps/1/reply/body/json/error`); `decks/create-nested`'s filtered-deck row cannot be expressed in a transcript yet (A20.1) |
| `ankiconnect-note-member-types` | a string `tags` in a note object is accepted and each character becomes one tag (`"pq rs"` gives ` p q r s `); an `options` or `duplicateScopeOptions` that is a string containing none of its member names, or a list holding none, is ignored and the note added (other non-objects fail on both sides and are compared by shape); `duplicateScopeOptions` `null` fails with `argument of type 'NoneType' is not iterable` (observed, A7.1, A7.4) | member errors; `null` counts as absent | `notes/add-errors` |

The search folding difference (R1777) and the envelope's differences are declared by U136 and `servers/ankiconnect-envelope`. `notes/note-records#U2`'s `note-save-same-after-normalizing` also applies to `updateNoteFields`.

## A19. Client call inventory (R1945)

The clients whose calls must replay green are `servers/ankiconnect-clients` C2's; their call shapes and sequences are its C3 and C4, which this file does not restate. Summary of their envelope traits and of the actions of this file they call:

| Client | Connection | Actions of this file it uses |
|---|---|---|
| Yomitan | extension origin, `version` 2 (bare results), key only when set, probes with `apiReflect` | `deckNames`, `modelNames`, `modelFieldNames`, `findNotes` (often inside `multi`), `findCards`, `notesInfo`, `cardsInfo`, `canAddNotes`, `canAddNotesWithErrorDetail`, `addNote`, `addNotes`, `updateNoteFields`, `suspend` |
| asbplayer | extension origin, `version` 6, optional key | `deckNames`, `modelNames`, `modelFieldNames`, `findNotes`, `findCards`, `notesInfo`, `notesModTime`, `cardsInfo`, `cardsModTime`, `areSuspended`, `addNote`, `updateNoteFields`, `addTags`, `createDeck`, `createModel` |
| mpvacious | no Origin (curl from Lua), `version` 6, optional key | `addNote`, `findNotes` (with `added:1`), `notesInfo`, `updateNoteFields`, `addTags`, `changeDeck`, `findModelsByName` |
| AI-agent sessions and scripts (Claude Code, the user's own scripts) | no Origin, `version` 6 or none, no key | read-only exploration: `deckNames`, `modelNames`, `findNotes` (`nid:`, `note:`, `is:new` queries), `findCards`, `notesInfo` (by `notes` and by `query`); the full set is `servers/ankiconnect-clients` C3's rows marked G |

Card templates that call AnkiConnect from a card (`servers/ankiconnect-clients` C2) use no action of this file successfully: they are refused by the envelope (R1679).

## A20. Scenarios

The scenarios, each a list of requests (`version` 6 unless a row says otherwise, no key, no Origin) with the replies the recorder must show. Setup uses the U4 DSL (`conformance/dsl`). The recorder runs only the setup sections its driver implements, and no `cards` or `revlog` section (`conformance/dsl` D6.6 and D6.7 are drafts the driver does not implement); filtered decks and direct database writes have no setup either. Card state therefore comes from exchanges of actions Meguri answers (`suspend`, `setDueDate`, `forgetCards`, `updateNoteFields`), never from `setSpecificValueOfCard`, `answerCards` or `insertReviews`, which Meguri refuses (`servers/ankiconnect-envelope` E12). Every `cards/` scenario sets `config: {fsrs: true}`, because Meguri always runs FSRS (R702, `sched/set-due-date` S1): with FSRS off, the recorder runs SM-2 Set Due Date and SM-2 answer labels, which Meguri never produces. The scenarios define their own note types (R1959): `Pair` is a standard note type with fields `Term`, `Gloss`, one template `Card 1` (question `{{Term}}`, answer `{{FrontSide}}<hr>{{Gloss}}`) and CSS `.card{}`; `Gap` is a cloze note type with fields `Body`, `Aside` and one template (question `{{cloze:Body}}`, answer `{{cloze:Body}}<br>{{Aside}}`). Ids are written with the tokens of `servers/transcripts` T10.1: `#n1`, `#c1`, `#d1` (decks numbered in ascending id order, so `#d1` is the first deck the scenario creates), `#m1`; `1` is the Default deck. Replies are shown unwrapped as `result` or `error`. A row marked "(Meguri)" asserts Meguri's side of the A18 entry it names; the golden records AnkiConnect's. The AnkiConnect replies of these rows were probed on the pinned add-on on 2026-11-10 and matched, except where a row says what was observed; not probed: the too-deep deck error. On 2026-11-17 the scenarios of A20.1 to A20.7 were recorded; each subsection says which rows are recorded. `search/current` was recorded then: with `curDeck` naming `B`, `findCards {query: "deck:current"}` gives the cards of `B` in ascending order. Rows that a transcript cannot express yet (`notes/preservation` among them) are named in their subsection, with where Meguri's side is asserted until then.

Every scenario that adds or updates a note (`notes/`, `models/create`, and `servers/ankiconnect-media` `media/attach*`) ends with `observe: [{db_dump: {tables: [notes, cards]}}, media_dir]` (the table form of `servers/transcripts` T18 and `servers/ankiconnect-clients` C5: a bare `db_dump` would hold the stock note types' rows, R1959), so the oracle's recording of the same requests carries the R1948 equivalence of note, cards and stored media (`servers/transcripts` T14).

### A20.1 `decks/`

| Scenario | Setup | Request | Reply |
|---|---|---|---|
| `create-nested` | none | `createDeck {deck: "A::B::C"}` | result `#d3`; decks `A::B` (`#d1`), `A` (`#d2`), `A::B::C` exist (missing ancestors are created nearest first, so `A::B` gets the lowest id, then `A`, then the named deck: `decks/decks#D3`, D5) |
| | | `createDeck {deck: "a::b::c"}` | result `#d3`, nothing written |
| | | `deckNames` | `["A", "A::B", "A::B::C", "Default"]` |
| | filtered deck `F` | `createDeck {deck: "F::x"}` | error `cannot create deck under a filtered deck: F::x` (Meguri, `ankiconnect-error-text`) |
| `delete` | deck `A` with one Pair note | `deleteDecks {decks: ["A"]}` | error `Since Anki 2.1.28 it's not possible to delete decks without deleting cards as well` |
| | | `deleteDecks {decks: ["a"], cardsToo: true}` | result null; nothing deleted (exact names) |
| | | `deleteDecks {decks: ["A"], cardsToo: true}` | result null; deck, note and card gone with graves |
| `names-ids` | decks `b`, `A::C` | `deckNamesAndIds` | `{"A": #d2, "A::C": #d3, "b": #d1, "Default": 1}` (creating `A::C` creates its parent `A` first; observed) |
| `get-decks` | c1 in `A`, c2 in `B`, c3 in `A` | `getDecks {cards: [#c1, #c2, #c3, 999]}` | `{"A": [#c1, #c3], "B": [#c2], "Default": [999]}` |
| `name-from-id` | deck `A` | `deckNameFromId {deckId: #d1}`; `{deckId: 12345}`; `{deckId: 0}` | `"A"`; `"Default"`; `"Default"` |
| `change-deck` | c1 in `A`, c2 in `New::Sub`; filtered deck `F` for row 3 only (not recorded, below) | `changeDeck {cards: [#c1, #c2], deck: "New::Sub"}` | result null; c1 in `New::Sub`; c2's row unchanged (Meguri, `ankiconnect-changedeck-op`) |
| | | `changeDeck {cards: [#c1], deck: "Brand::New"}` | result null; decks `Brand`, `Brand::New` created; c1 there |
| | | `changeDeck {cards: [#c1], deck: "F"}` | error `cannot move cards into a filtered deck: F`, nothing written (Meguri, `ankiconnect-changedeck-op`) |
| `deck-stats` | A5.8's example: decks `Lang`, `Lang::JA`; one Pair note in `Lang`, two in `Lang::JA` | `setDueDate {cards: [#c1], days: "0"}` (setup cannot set card state) | `true`; the `Lang` card is a review card due today |
| | | `getDeckStats {decks: ["Lang::JA", "Lang", "Nope"]}` | A5.8's object; no deck `Nope` afterwards (Meguri, `ankiconnect-getdeckstats-no-create`; recorded: AnkiConnect creates `Nope` as `#d3` with zero counts) |
| | | `getDeckStats {decks: ["lang::ja"]}` | the `Lang::JA` member only (lookup by name, A1) |

Recorded 2026-11-17 (goldens match): `create-nested` rows 1 to 3, `delete`, `names-ids`, `get-decks`, `name-from-id`, `change-deck` rows 1 and 2, `deck-stats`. The two rows with filtered deck `F` are not in the recorded `create-nested` and `change-deck` transcripts: a transcript setup (`servers/transcripts` T5, `conformance/dsl` D6) has no way to make a filtered deck (no direct database write, no card-state section), and AnkiConnect has no action that makes one. They wait for a setup that can; until then Meguri's unit tests assert them from A5.4, A5.5 and A17.

### A20.2 `models/`

| Scenario | Setup | Request | Reply |
|---|---|---|---|
| `reads` | `Pair`; `Grid`: fields `Term`, `Gloss`, `Note`, `Body`, one template per A6.1 row 1 to 5 and 7, named `R1` to `R5` and `R7` | `modelFieldNames {modelName: "pair"}` | `["Term", "Gloss"]` (`unicase` lookup) |
| | | `modelFieldsOnTemplates {modelName: "Pair"}` | `{"Card 1": [["Term"], ["Gloss"]]}` |
| | | `modelTemplates {modelName: "Pair"}` | `{"Card 1": {"Front": "{{Term}}", "Back": "{{FrontSide}}<hr>{{Gloss}}"}}` |
| | | `modelStyling {modelName: "Pair"}` | `{"css": ".card{}"}` |
| | | `modelFieldNames {modelName: "Nope"}` | error `model was not found: Nope` |
| | | `modelFieldDescriptions {modelName: "Pair"}` | `["", ""]` |
| | | `modelFieldFonts {modelName: "Pair"}` | `{"Term": {"font": "Arial", "size": 20}, "Gloss": {"font": "Arial", "size": 20}}` |
| | | `modelNameFromId {modelId: #m7}` | `"Pair"` |
| | | `modelNameFromId {modelId: 12345}` | error `model was not found: 12345` |
| | | `modelFieldsOnTemplates {modelName: "Grid"}` | `{"R1": [["Term"], ["Gloss"]], "R2": [["Term", "Term"], ["Note", "Note"]], "R3": [["Tags"], ["Body"]], "R4": [["Term"], ["Body"]], "R5": [[" Term "], ["^Term"]], "R7": [[], ["Gloss"]]}` (A6.1's rows) |
| `names` | `Pair`, then `Duo` created, then `alpha` | `modelNames` | the stock note types' and `["alpha", "Duo", "Pair"]` merged in `unicase` order of the name (not creation order) |
| | | `modelNamesAndIds` | `{"alpha": #m9, "Basic": #m1, "Basic (and reversed card)": #m2, "Basic (optional reversed card)": #m3, "Basic (type in the answer)": #m4, "Cloze": #m5, "Duo": #m8, "Image Occlusion": #m6, "Pair": #m7}`, in the same name order |
| `find` | `Pair` | `findModelsByName {modelNames: ["Pair"]}` | one schema-11 object (golden) |
| | | `findModelsById {modelIds: [1]}` | error `model was not found: 1` |
| | | `findModelsById {modelIds: [#m7]}` | the same object as `findModelsByName` |
| | | `findModelsByName {modelNames: ["Pair", "Nope"]}` | error `model was not found: Nope` (the first name that finds none is the error; no list) |
| `create` | `Pair` | `createModel {modelName: "Duo", inOrderFields: ["F", "B"], cardTemplates: [{Front: "{{F}}", Back: "{{B}}"}], css: ".x{}"}` | the stored object, template `Card 1` |
| | | the same request again | error `Model name already exists` |
| | | `createModel {modelName: "Trio", inOrderFields: [], cardTemplates: [{Front: "{{F}}", Back: ""}]}` | error `Must provide at least one field for inOrderFields` |
| | | `createModel {modelName: "pair", inOrderFields: ["F"], cardTemplates: [{Front: "{{F}}", Back: "{{F}}"}]}` | the stored object, named `pair+` (Meguri, `ankiconnect-createmodel-name-clash`); its `css` is Meguri's base card CSS (Meguri, `ankiconnect-createmodel-css`) |
| | | `createModel {modelName: "Quad", inOrderFields: ["F"], cardTemplates: [{Front: "{{#F}}", Back: "x"}]}` | error `invalid card template: <reason>` (Meguri, `ankiconnect-error-text`) |
| | | `createModel {modelName: "Trio", inOrderFields: ["F"], cardTemplates: []}` | error `Must provide at least one card for cardTemplates` |
| | | `createModel {modelName: "Pair", inOrderFields: [], cardTemplates: [{Front: "{{F}}", Back: "{{F}}"}]}` | error `Must provide at least one field for inOrderFields` (before the name check) |
| | | `createModel {modelName: "Pair", inOrderFields: ["F"], cardTemplates: [{Back: "{{F}}"}]}` | error `Model name already exists` (before the member check) |
| | | `createModel {modelName: "Trio", inOrderFields: ["F"], cardTemplates: [{Back: "{{F}}"}]}` | error `missing parameter: Front` (Meguri, `servers/ankiconnect-envelope#E18` parameter-error texts; the add-on's is `'Front'`) |
| | | `createModel {modelName: "Trio", inOrderFields: ["F"], cardTemplates: [{Front: "{{F}}"}]}` | error `missing parameter: Back` (Meguri, `servers/ankiconnect-envelope#E18` parameter-error texts; the add-on's is `'Back'`) |
| | | `createModel {modelName: "Trio", inOrderFields: ["F", "G"], cardTemplates: [{Name: "X", Front: "{{F}}", Back: "{{G}}"}, {Front: "{{G}}", Back: "{{F}}"}], css: ".t{}"}` | the stored object, templates `X` and `Card 2` |
| | | `createModel {modelName: "Gap", inOrderFields: ["Body", "Aside"], cardTemplates: [{Front: "{{cloze:Body}}", Back: "{{cloze:Body}}<br>{{Aside}}"}], css: ".g{}", isCloze: true}` | the stored object, `type` 1, `originalStockKind` 1 |

Recorded 2026-11-17: every row of `reads`, `names`, `find` and `create`; the goldens match the AnkiConnect side of each row. The (Meguri) rows of `create` (the `pair` and `Quad` rows, steps 3 and 4) still need their `conformance/deviations.yaml` entries.

### A20.3 `notes/`

| Scenario | Setup | Request | Reply |
|---|---|---|---|
| `add-match` | `Pair`, deck `D` | `addNote {note: {deckName: "D", modelName: "Pair", fields: {term: "x", GLOSS: "y", Other: "z"}}}` | result `#n1`; fields `x`, `y` |
| `add-errors` | `Pair`, deck `D` | `addNote` with `modelName: "Nope"` | error `model was not found: Nope` |
| | | with `modelName: "Nope"` and no `fields` | error `model was not found: Nope` (order, A7.2) |
| | | with `deckName: "Nope"` | error `deck was not found: Nope` |
| | | with `fields: {Term: "", Gloss: "y"}` | error `cannot create note because it is empty` |
| | | with `fields: {Term: "x"}` twice | second: error `cannot create note because it is a duplicate` |
| | | the same with `options: {allowDuplicate: true}` | result `#n2` |
| | | with `options: {allowDuplicate: "yes"}` | error `option parameter "allowDuplicate" must be boolean` |
| | | with `fields: {Term: "t1"}` and `tags: "pq rs"` | error `invalid parameter: note.tags`, nothing written (Meguri, `ankiconnect-note-member-types`) |
| | | with `fields: {Term: "t2"}` and `options: "xyz"` | error `invalid parameter: note.options`, nothing written (Meguri, `ankiconnect-note-member-types`) |
| | | with `fields: {Term: "t3"}` and `options: 5` | error `invalid parameter: note.options` (the add-on's runtime text, compared by shape) |
| | | with `fields: {Term: "t4"}` and `options: {duplicateScopeOptions: null}` | result `#n3` (Meguri, `ankiconnect-note-member-types`) |
| | | `modelName: "Gap"`, `fields: {Body: "no cloze"}` | error `cannot create note for unknown reason` |
| `add-no-card` | note type `Rev`: fields `Term`, `Gloss`, one template (question `{{Gloss}}`, answer `{{Term}}`); deck `D` | `addNote` with `fields: {Term: "x"}` | result `#n1`; one card of ordinal 0 (A7.3, A21 Q7) |
| `add-defaults` | `Pair`, then a second note type (so `curModel` names it), deck `D` | `findModelsByName {modelNames: ["Pair"]}`; `addNote` as `add-match`; `findModelsByName` again | result `#n1`; Pair's schema-11 `did` is `null` before and `#d1` after in the golden, and stays `null` in Meguri (Meguri, `ankiconnect-add-defaults`). A transcript cannot observe `curModel` or the `_nt_`/`_deck_` keys themselves: they hold real ids, and `servers/transcripts` T10.6 keeps `config` out of transcript dumps; Meguri's tests of `notes/note-records#A4` assert that they are unchanged |
| `add-filtered-deck` | `Pair`; filtered deck `F` | `addNote` with `deckName: "F"` | error `cannot create note in filtered deck: F`, nothing written (Meguri, `ankiconnect-add-filtered-deck`) |
| `dupe-scope` | `Pair`; decks `P`, `P::Q`, `R`; note `x` in `P::Q` | `canAddNotesWithErrorDetail` for `x` in `R` with: no options; `duplicateScope: "deck"`; `"deck"` + `deckName: "P"`; `"deck"` + `deckName: "P"`, `checkChildren: true`; `"deck"` + `deckName: "Nope"`, `checkChildren: true` | `[{"canAdd": false, "error": "cannot create note because it is a duplicate"}, {"canAdd": true}, {"canAdd": true}, {"canAdd": false, "error": "cannot create note because it is a duplicate"}, {"canAdd": true}]` |
| | | the same with a second note type and `checkAllModels: true` | `false` without scope; `true` with `duplicateScope: "deck"` in `R` (A7.3's table) |
| `can-add-media` | `Pair`, deck `D` | `canAddNote` with `audio: {filename: "a.mp3", data: "YQo=", fields: ["Term"]}` | `true`; no media file (Meguri, `ankiconnect-canadd-no-media`) |
| `add-notes` | `Pair`, deck `D` | `addNotes` with A7.6's rows | A7.6's results; no note, grave or log line after a failure (Meguri, `ankiconnect-addnotes-rollback`) |
| `update` | note n1 `x`/`y`, tags ` a b ` | `updateNoteFields {note: {id: #n1, fields: {gloss: "q"}}}` | result null; nothing written (case-sensitive) |
| | | `updateNoteFields {note: {id: #n1, fields: {Gloss: "q"}}}` | result null; Gloss `q`, `mod` now |
| | | `updateNote {note: {id: #n1, tags: ["c"]}}` | result null; tags ` c ` |
| | | `updateNote {note: {id: #n1}}` | error `Must provide a "fields" or "tags" property.` |
| | | `notesModTime {notes: [#n1, 5]}` | `[{"noteId": #n1, "mod": <now>}, {}]` |
| | | `updateNoteFields {note: {id: 5, fields: {}}}` | error `Note was not found: 5` (A21 Q2) |
| `info` | n1 as above; note n2 whose card row was removed outside Meguri (setup writes the database directly) | `notesInfo {notes: [#n1, 5]}` | `[{"noteId": #n1, "profile": "<PROFILE>" (the golden holds the recorder's profile name, A21 Q1), "tags": ["c"], "fields": {"Term": {"value": "x", "order": 0}, "Gloss": {"value": "q", "order": 1}}, "modelName": "Pair", "mod": <mod>, "cards": [#c1]}, {}]` |
| | | `notesInfo {}` | error `Must provide either "notes" or a "query"` |
| | | `notesInfo {notes: [#n2]}` | `[{…, "cards": []}]` (Meguri, `ankiconnect-notesinfo-no-cards`) |
| `delete` | n1 | `deleteNotes {notes: [#n1, 5]}` | result null; card and note graves; a deleted-notes log line |
| `preservation` | n1 whose `data` column and note type hold unknown values | `updateNoteFields` changing Gloss | `data` and the note type blob byte-identical (R1789) |

Recorded 2026-11-17 (goldens match): `add-match`, `add-errors`, `add-no-card`, `add-defaults`, `dupe-scope`, `can-add-media`, `add-notes`, `update`, `info` rows 1 and 2, `delete`.

The recorder cannot express `add-filtered-deck` (a filtered deck), `info`'s third row (a note with no card row) or `preservation` (unknown `data` and note type values). Transcript setup is `conformance/dsl` D6, which has neither filtered decks nor direct database writes, and no action that both sides serve makes these states. They wait for a transcript setup or step that writes the database (an oracle-maintainer change to `servers/transcripts` T5 or T7). Until then, Meguri's side of `ankiconnect-add-filtered-deck` and `ankiconnect-notesinfo-no-cards`, and R1789 for this server, are asserted only by Meguri's own tests from A7.2, A9.2 and A15.

### A20.4 `tags/`

| Scenario | Setup | Request | Reply |
|---|---|---|---|
| `add-remove` | n1 ` a `, n2 ` b::c ` | `addTags {notes: [#n1, #n2], tags: "x  y"}` | null; n1 ` a x y `, n2 ` b::c x y ` |
| | | `removeTags {notes: [#n2], tags: "b"}` | null; n2 ` x y ` |
| | | `getTags` | `["a", "b::c", "x", "y"]` (rows stay) |
| | | `clearUnusedTags`; `getTags` | null; `["a", "x", "y"]` |
| `replace` | A10.1's rows | `replaceTags` / `replaceTagsInAllNotes` | A10.1's results |
| `note-tags` | n1 ` a ` | `updateNoteTags {note: #n1, tags: "x y"}`; `getNoteTags {note: #n1}` | null; `["x", "y"]` |
| | | `updateNoteTags {note: #n1, tags: 5}` | error `Must provide tags as a list of strings` |

Recorded 2026-11-17: `add-remove`, `replace` and `note-tags` (source `oracle`); the three goldens match the rows as written.

### A20.5 `cards/`

Every scenario here sets `config: {fsrs: true}` (A20).

| Scenario | Setup | Request | Reply |
|---|---|---|---|
| `info` | two Pair notes in deck `A` (`x`/`y`, then `p`/`q`) | `setDueDate {cards: [#c2], days: "3"}` | `true`; c2 becomes type 2, queue 2, `due` 3, `interval` 0, `factor` 2500 |
| | | `cardsInfo {cards: [#c1, #c2, 7]}` | two objects (question `<style>.card{}</style>x`, answer `<style>.card{}</style>x<hr>y` for c1; `nextReviews` `["<1m", "<6m", "<10m", "9d"]` for c1 and `["<10m", "1d", "2d", "9d"]` for c2, after `servers/transcripts` T10.3) and `{}` for 7 |
| `info-errors` | note type `Bad` with fields `Term`, `Gloss`, question `{{Term}}{{#FrontSide}}z{{/FrontSide}}` and answer `{{Gloss}}`; note type `Tex` (`{{Term}}` / `{{Term}}`); one note of each, `Tex`'s Term `[$]x[/$]` | `cardsInfo {cards: [#c1, #c2]}`; then `media_dir` | below |
| `suspend` | three Pair notes (c1 to c3) | `suspend {cards: [#c3]}` | `true`; c3 suspended |
| | | `suspend {cards: [#c3]}` | `false` |
| | | `suspend {cards: [#c1, #c2]}` | `true`; both suspended |
| | | `areSuspended {cards: [#c1, #c3, 7]}` | `[true, true, null]` |
| | | `unsuspend {cards: [#c1]}` | `null`; c1 restored |
| | | `unsuspend {cards: [#c1, #c2]}` (c1 unsuspended, c2 suspended) | `null`; c2 restored |
| | | `suspend {cards: [#c3, 7]}` | error `Card was not found: 7`; nothing written (Meguri, `ankiconnect-suspend-unknown-card`; the golden gives `true` and writes nothing; pointer `/steps/7/reply/body/json`) |
| | | `suspended {card: 7}` | error `Card was not found: 7` |
| | | `suspended {card: #c3}` | `true` |
| `intervals` | three Pair notes (c1 to c3) | `setDueDate {cards: [#c2], days: "1"}`, then `days: "4"`; `setDueDate {cards: [#c3], days: "0"}` | `true` each; c2 has two type-4 rows of `ivl` 0; c3 is type 2, queue 2, `due` today |
| | | `getIntervals {cards: [#c1, #c2, #c3]}`; `{cards: [#c2], complete: true}` | `[0, 0, 0]`; `[[0, 0]]` |
| | | `areDue {cards: [#c1, #c2, #c3]}` | `[true, false, true]` |
| | | `getIntervals {cards: [7]}`; `{cards: [7], complete: true}`; `areDue {cards: [7]}` | error `list index out of range`; `[[]]`; error `list index out of range` |
| | | `forgetCards {cards: [#c2]}`; then `getIntervals {cards: [#c2]}`, `{cards: [#c2], complete: true}`, `areDue {cards: [#c2]}` | `null`; `[0]`; `[0]`; `[true]` |
| `reschedule` | two Pair notes | `setDueDate {cards: [#c2], days: "3"}` | `true`; c2 type 2, `due` 3, `factor` 2500, one type-4 row |
| | | `forgetCards {cards: [#c2]}` | `null`; `due` back to position 2, a type-4 row with `factor` 0 |
| | | `setDueDate {cards: [#c2], days: "5"}`, then `days: "1!"` | `true`; `true`; `ivl` stays 0, because `!` changes nothing with FSRS on (`sched/set-due-date` S4.2) |
| | | `setDueDate {cards: [#c2], days: "abc"}` | error `invalid days value: abc`, nothing written (Meguri, `ankiconnect-error-text`; the golden's error is `abc`; pointer `/steps/7/reply/body/json/error`) |
| | | `setDueDate {cards: [7], days: "1"}`; `forgetCards {cards: [7]}` | `true`; `null`; nothing written |
| `repeated-ids` | two Pair notes | `setDueDate {cards: [#c2], days: "3"}` | `true` |
| | | `forgetCards {cards: [#c2, #c2]}` | `null`; rows as for `[#c2]` (Meguri, `ankiconnect-repeated-ids`; the golden writes two Reset rows) |
| | | `setDueDate {cards: [#c2, #c2], days: "1"}` | `true`; c2 due 1 as for `[#c2]` (Meguri, `ankiconnect-repeated-ids`; the golden is a database error that writes nothing) |
| `to-notes` | note type `Both`: fields `Term`, `Gloss`, two templates, the second with front `{{Gloss}}`; n1 (`Both`) added with Gloss empty (card c1 only); Pair note n2 owns c2 | `updateNoteFields {note: {id: #n1, fields: {Gloss: "b1"}}}` | `null`; c3 generated for n1 (`notes/note-records#U1`), so c1 (n1) < c2 (n2) < c3 (n1) |
| | | `cardsToNotes {cards: [#c3, #c2]}` | `[#n2, #n1]` (n2's smallest listed card c2 < n1's c3; ascending note id or list order would give `[#n1, #n2]`) |
| | | `{cards: [#c3, #c1, #c2]}` | `[#n1, #n2]` |
| | | `{cards: [#c2, 7]}` | `[#n2]` |
| | | `{cards: [#c3, #c3]}` | `[#n1]` |
| | | `{cards: []}` | `[]` |

`info-errors`: a checked save accepts `Bad` (FrontSide is a special field name, `notetypes/validation#V3` rule 2.3), and its render fails because FrontSide is not in the field map (`render/special-fields` P4: `unknown_conditional`), so no direct database write is needed. In the golden, `question` and `answer` of the `Bad` card are each `<style></style>` followed by Anki's template error text, which links to its docs; Meguri gives its D8 notice after `<style></style>` (Meguri, `ankiconnect-cardsinfo-template-error`). For `Tex`, the golden's question and answer are each the CSS prefix, a LaTeX image tag naming a `latex-<40 hex>.png` file, and Anki's text saying that LaTeX generation is disabled; Meguri's are `<style>.card{}</style>[$]x[/$]` as stored (Meguri, `ankiconnect-cardsinfo-latex`). `media_dir` is empty on both sides. The deviation pointers are `/steps/0/reply/body/json/result/0/question`, `.../0/answer`, `.../1/question` and `.../1/answer`.

The `reschedule` dumps after each write compare without deviations. A11.6's prose and example stay as written; the `to-notes` golden confirms them.

Recorded 2026-11-17: `info`, `info-errors`, `suspend`, `intervals` (FSRS-on `setDueDate` rows only), `reschedule`, `repeated-ids` and `to-notes`. Three parts cannot be recorded in a transcript yet:

- A card buried by the user: no action Meguri answers buries a card, so `ankiconnect-unsuspend-buried` needs another scenario (a declared transcript, or recorder support for `conformance/dsl` D6.6 card setup).
- Learning rows in `intervals` (`ivl` −600, and −1800 back-dated): the recorder has no D6.7, and Meguri refuses `insertReviews` and `answerCards`. Until the recorder supports review-log setup, A12.4's below −1200 branch and the latest-row reading of A12.5 have only their A12 tables as evidence.
- The comparison of `forgetCards` and `setDueDate` with the UI's Reset and Set Due Date on the same collection (A13): a transcript runs no DSL ops, so it belongs in a `conformance/dsl` sched-ops scenario (`conformance/sched-ops`). It uses fixed day texts, because the two executors draw a range's day differently.

### A20.6 `search/`

| Scenario | Setup | Request | Reply |
|---|---|---|---|
| `folding` | a note `Über` | `findNotes {query: "über"}` | `[#n1]` (declared U136 deviation R1777; the golden is `[]`: the add-on finds `Über` neither with `über` nor with `Term:über`, observed) |
| `current` | decks `A`, `B`, `curDeck` = `B`, cards in both | `findCards {query: "deck:current"}` | the cards of `B`, ascending |
| `error` | any | `findNotes {query: "("}` | error: E7's `unclosed_group` text |
| `empty` | 3 notes | `findNotes {query: ""}`; `findNotes {}` | all 3 ascending (Meguri, `ankiconnect-search-order`; the golden is `[#n1, #n3, #n2]`); `[]` |
| `notes-info-query` | 2 notes | `notesInfo {query: "Term:x"}` | the matching note's object |

Recorded 2026-11-17: all five rows; each golden matches the row and the A18 entries it names.

### A20.7 `stats/`

| Scenario | Setup | Request | Reply |
|---|---|---|---|
| `by-day-rollover` | zone Europe/Paris, rollover 4, clock 2030-01-15 03:59; `setDueDate {cards: [#c1], days: "1"}`, `advance 2m`, `setDueDate {cards: [#c2], days: "1"}` (type 4 rows at 03:59 and 04:01 local: no action Meguri serves writes rows of other types) | `getNumCardsReviewedByDay` | `[]` (Meguri, `ankiconnect-review-counts-exclusions`; the golden is `[["2030-01-15", 1], ["2030-01-14", 1]]`) |
| `by-day-rollover-counted` (declared) | zone Europe/Paris, rollover 4, setup review-log rows (`conformance/dsl` D6.7) of type 1 at 03:59 and 04:01 local on 2030-01-15 and one of type 4 at 04:02 | `getNumCardsReviewedByDay` | `[["2030-01-15", 1], ["2030-01-14", 1]]`: two dates, newest first, the type 4 row left out |
| `today` | zone Europe/Paris, rollover 4, clock 2030-01-15 03:59:59; `setDueDate` on `#c1`, `#c2`, `#c3` with `advance 1s` between them (type 4 rows at S − 1 s, S and S + 1 s, S the day start 04:00:00) | `getNumCardsReviewedToday` | `0` (Meguri, `ankiconnect-review-counts-exclusions`; the golden is `1`: the row at S is outside the window) |
| `today-counted` (declared) | zone Europe/Paris, rollover 4, setup review-log rows of type 1 at S − 1, S and S + 1 ms, and one of type 5 inside the window | `getNumCardsReviewedToday` | 1 |

Recorded 2026-11-17: `by-day-rollover` and `today`, with type 4 `setDueDate` rows. The declared `by-day-rollover-counted` and `today-counted` keep the counted-row assertions; their `revlog` setup is `conformance/dsl` D6.7, a draft.

### A20.8 `clients/`

The client scenarios are `servers/ankiconnect-clients` C7's (`yomitan/`, `asbplayer/`, `mpvacious/`, `agent/`), replaying green (R1945). U142 also replays `servers/ankiconnect-envelope` E19.3 `ae75` and E19.2's key rows (AE75–AE77) with `deckNames` and `addNote` in place of the stand-in actions.

## A21. Open questions

Probe of 2026-11-10 on the pinned add-on (provenance in `oracle/provenance/servers-ankiconnect-actions.md`): Q6 is settled; Q1 to Q5 and Q7 to Q10 are product decisions and stay open, each with what the add-on was seen to do. Until a decision, each question's interim rule applies, and this file is written to it: Q2, Q3, Q7, Q8 and Q10 follow the add-on, so that the goldens replay, Q2 and Q3 against the wording of R1738 and R1767; Q4, Q5 and Q9 use Meguri's rule, with the A18 entries they name; Q1 uses its proposed value.

- Q1. (Open, product.) The PROFILE name. Anki names a first profile `User 1`; Meguri has no profiles. Proposed: `User 1`, so scripts written against a fresh Anki keep working. Observed: the en_US name of Anki's first profile is `User 1`; the recorder names its profile from its configuration (`oracle` by default), and `getProfiles`, `getActiveProfile` and `notesInfo`'s `profile` report that name, while `loadProfile` is `true` only for that name exactly (`Oracle` gives `false`). If `User 1` is chosen, the recorder runs with its profile named `User 1` so the goldens carry it.
- Q2. (Open, product; blocks implementation of A8.) R1738 lists `note was not found: <id>` in lower case. The add-on's text for a missing note is `Note was not found: <id>` (and `Card was not found: <id>`), which this file uses so that the goldens replay. Either the Product Contract text is amended to the add-on's, or Meguri uses R1738's text and a deviation `ankiconnect-note-not-found-case` is added (golden `notes/update`). `guiEditNote` (U146) is not part of this decision: the add-on answers an unknown id there with Anki's own not-found message, so Meguri uses R1786's lower-case `note was not found: <id>` there in every case (`servers/ankiconnect-gui` G7, G11). Observed: `Note was not found: 5` from `updateNoteFields`, `updateNote`, `updateNoteTags` and `getNoteTags`; `Card was not found: 7` from `suspend`, `unsuspend` and `suspended`. The fact is settled; the choice between amending R1738 and the deviation is not.
- Q3. (Open, product; blocks implementation of A12.1 and A12.2.) R1767 says `suspend` and `unsuspend` return true when anything changed. The add-on returns A12.1's result for `suspend` (`true` for a list of two or more cards that are all suspended already) and nothing (`null`) for `unsuspend`. This file follows the add-on so that the goldens replay. The plan's U142 test "suspend on already suspended cards → false" passes under this file only for a one-card list; with the add-on's rule it must be restated as "a one-card list of an already suspended card → false". Following R1767 literally instead needs deviations `ankiconnect-suspend-result` and `ankiconnect-unsuspend-result` (golden `cards/suspend`). Observed: A12.1's table and A12.2's results (`null` in every case, also when cards were restored). The fact is settled; whether to amend R1767 is not.
- Q4. (Open, product.) R1774 asks for Meguri's stats exclusions, and this file uses the studied-today rule (types 4 and 5 left out) for both actions. The add-on counts every row. Confirm, or choose the add-on's rule and drop `ankiconnect-review-counts-exclusions`. Observed: both actions counted inserted rows of `type` 4 and 5, and the `type` 4 rows of `setDueDate` and `forgetCards`.
- Q5. (Open, product.) The A17 texts are Meguri's proposal; no source fixes them. Observed add-on texts for the same cases are in A18 `ankiconnect-error-text`; a filtered `deckName` in `addNote` and a filtered `changeDeck` target are accepted by the add-on, so it has no text for them.
- Q6. (Settled, 2026-11-10.) U136's `ADDON_PIN` build (25.11.9.0) was probed through the recorder image. Its replies confirmed this file except for the points now written into A7.1, A7.4, A10.1, A11.2, A11.4, A12, A13, A14, A18 and A20; where a later recorded golden differs, the golden still decides and this file changes.
- Q7. (Open, product.) R1758 lists `cannot create note because it is empty` also "when no card would be generated". In Anki every added note gets a card (`notes/card-generation#G6`), so the add-on never refuses a note for that, and this file follows it (scenario `notes/add-no-card`). The add-on's error for an add that generates no card (`The field values you have provided would make an empty question on all cards.`) is therefore never produced. Confirm, or decide that Meguri refuses such a note (a deviation entry and a card-generation check before the add). Observed: the `notes/add-no-card` note was added with one card of ordinal 0.
- Q8. (Open, product.) The cloze states (`missing_cloze`, `notetype_not_cloze`, `field_not_cloze`) give the add-on's `cannot create note for unknown reason`. R1758 names no error for them. Confirm that the add-on's text stays. Observed: a cloze note type with `Body: "no cloze"` gives `cannot create note for unknown reason`.
- Q9. (Open, product; new 2026-11-10.) The add-on accepts a string `tags` in a note object and makes each character of it one tag (`"pq rs"` gives ` p q r s `). It ignores an `options` or `duplicateScopeOptions` that is a string containing none of its member names or a list holding none; other non-objects fail there too, with runtime texts. A7.1 and A7.4 give member errors (A18 `ankiconnect-note-member-types`). Proposed: keep the member errors. A client that sends a string `tags` already gets letter tags from the add-on, so no working client depends on the add-on's reading, and an error shows the mistake where silent letter tags would not; accepting the string as tag text, as `updateNoteTags` does (A8.3), would be a third behavior that matches neither. Interim: the member errors of A7.1 and A7.4, with the deviation.
- Q10. (Open, product; new 2026-11-10.) For an id that is not a new card and has no review-log row, `areDue` and `getIntervals` (`complete` false) fail the whole action with `list index out of range` (A12.4, A12.5). That text is the add-on's language-runtime message, not an authored interface string. This file uses it so that the goldens replay exactly. Alternative: an A17 Meguri text (for example `card has no review history: <id>`) with a deviation `ankiconnect-no-revlog-text`, whose scenario would add a card without review-log rows to `cards/intervals`. Interim: the add-on's text. Observed: the text for a learning card with no row and for an unknown id in `areDue` (A12.4's rows).
