# Search: names and ids

Spec: search/names-and-ids
Requirements: R1050, R1051, R1052, R1053, R1054, R1055, R1056, R1057, R1058, R1059, R1060, R1061, R1074, R1268; covers AE49 (tag part)
Anki: 26.09.3
Scenarios: conformance/scenarios/search/tags/, conformance/scenarios/search/decks/, conformance/scenarios/search/notetypes/, conformance/scenarios/search/ids/, conformance/scenarios/search/dupe/

What the name and id terms match: `tag:`, `deck:`, `preset:`, `note:`, `card:`, `nid:`, `cid:`, `did:`, `mid:` and `dupe:`. The values each term holds are `grammar.md` G8, the scope of each term `query-model.md` Q3 and Q5, and how terms combine `query-model.md` Q1, Q4 and Q7. Every rule here was checked against the oracle; the scenarios each section names pin its examples, except those of N3.3, whose scenario is still to be written (N8).

## N1. Shared rules

- **Reading the value.** Each term starts from the value `grammar.md` G8 gives it (kinds U, Q, D, R of G7). Where this file says "NFC", the value is converted to NFC (`text/unicode-primitives#U3`) whatever the config key `normalize_note_text` says; only `dupe:` follows that key (N6).
- **Exact text.** Where a name is compared as exact text, every backslash in the value is removed together with its escaping role: `\\` becomes `\`, `\*` becomes `*`, `\_` becomes `_` (after U reading these are the only backslash pairs left).
- **Has wildcards** is `text-matching.md` T13.2: a `*` or `_` after an even number of backslashes.
- **Glob conversion** is `text-matching.md` T13.1 (`*` → `.*`, `_` → `.`, `\\` → `\\`, `\*` → `\*`, `\_` → `_`, every other unit escaped). Tags use the same conversion with `*` → `\S*` and `_` → `\S` (N2.2).
- **Expressions** are compiled as `text-matching.md` T11.1 (the pinned `regex` crate, default options) and tested unanchored unless the expression anchors itself. `(?i)` folds letter case with simple case folding (`case-folding.md` C3): `ß` never equals `ss`, `Ü` equals `ü`, fullwidth `Ｆ` equals `ｆ` but not `F`.
- **Unicase equality** is `text/unicode-primitives#U2` (full case folding): `straße` equals `STRASSE`.
- **When names are resolved.** Meguri may turn a name term into a set of note type, template or deck ids while it builds the plan (`query-model.md` Q9), as long as the result equals evaluating these rules on the collection state the statement reads. The plan therefore builds and runs on one read transaction, or re-resolves when the collection changed between the two.

## N2. Tags (R1050–R1053)

### N2.1 Stored text

`notes.tags` holds the empty string for a note without tags, otherwise a space, the note's tags separated by single spaces, and a space (`tags/tags` T4, U30): ` animal::mammal other `. A tag never contains U+0020, but it can contain other White_Space characters such as U+00A0. Matching reads the column as stored, whatever wrote it.

### N2.2 Normal mode

The value is G8's normal tag value (kind U), then NFC. Then, in this order:

1. Exactly `none`: the note matches when `notes.tags` is the empty string. Only the lower-case word is special; `tag:None` and `tag:NONE` search for a tag named `none`.
2. Exactly `*`: every note matches, untagged notes included.
3. The value contains U+0020 (possible only inside quotes, `"tag:a b"`): no note matches.
4. Otherwise the note matches when the expression `(?i).* ` + T(value) + `(::| ).*` matches `notes.tags`, where T is the glob conversion with `*` → `\S*` and `_` → `\S` (`\S`: one character that is not White_Space, in the `regex` crate's Unicode sense).

Read declaratively: the note matches when one of its tags has a prefix P, either the whole tag or the part before any occurrence of `::` in it, that the value matches as a whole, letter case folded, `*` matching any run and `_` any one character, both only over characters that are not White_Space. So:

- it matches the tag itself and its descendants, never a later level on its own: `tag:animal` matches `animal`, `Animal` and `animal::mammal`, not `animals`; `tag:mammal` does not match `animal::mammal`;
- `*` and `_` cross `::` but never a space between tags, nor a White_Space character inside a tag: `tag:x*i` matches `x::ani` but not the two tags `x` and `ani`; `tag:*ani` matches `ani` and `x::ani`;
- `tag:parent::*` matches descendants of `parent` only; `tag:_*` matches every note with a tag whose first level holds no White_Space character; `tag:` (empty) matches no note with ordinary tags;
- a tag holding U+00A0 (`r` U+00A0 `s`) is found by no normal-mode search: `tag:r`, `tag:r*` and `tag:r_s` miss it and `tag:_*` skips it; only `tag:re:` finds it (`tag:re:^r`);
- regular-expression characters are literal (`tag:c++`, `tag:a.b`); `tag:a\*b` matches only the tag `a*b`; `tag:a*b` and `tag:a_b` also match `axb` and `a.b`.

| Search | `animal::mammal` | `animals` | `Animal` | (no tags) | `x ani` | `x::ani` |
|---|---|---|---|---|---|---|
| `tag:animal` | yes | no | yes | no | no | no |
| `tag:animal*` | yes | yes | yes | no | no | no |
| `tag:ani*` | yes | yes | yes | no | yes | no |
| `tag:*ani` | no | no | no | no | yes | yes |
| `tag:x*i` | no | no | no | no | no | yes |
| `tag:none` | no | no | no | yes | no | no |
| `tag:*` | yes | yes | yes | yes | yes | yes |
| `tag:_*` | yes | yes | yes | no | yes | yes |

(`x ani` is a note with the two tags `x` and `ani`.) Scenario: `search/tags/hierarchy`.

### N2.3 `tag:re:`

- The value is G8's (kind Q of the text after `re:`), then NFC. The expression is `(?i)` followed by it. None of N2.2's special values apply: `tag:re:none` is an ordinary expression.
- The note matches when the expression matches at least one piece of `notes.tags` split at every U+0020. The pieces include the empty text before the first space and after the last one, and an untagged note has one piece, the empty text. So `^` and `$` anchor at the start and end of each tag, and an expression that matches the empty text (`tag:re:`, `tag:re:^$`, `tag:re:x*`) matches every note.
- `tag:re:^ani` matches `animal::mammal`, `animals` and `Animal`, not `x::ani`; `tag:re:a.b` matches `a.b`, `a*b` and `axb`; `"tag:re:(?-i)^Animal$"` matches only `Animal`.
- An expression that does not compile is `invalid_regex`, raised when the plan is built (`text-matching.md` T12.1, T12.2). Anki raises it only when a note reaches the test, so on a collection without notes Anki returns no ids (`text-matching.md` T12.3); the scenarios hold notes.

### N2.4 `tag:nc:`

- The value is G8's (kind U of the text after `nc:`), then NFC. N2.2's steps 1–3 apply to it as is: `tag:nc:none` matches untagged notes and `tag:nc:*` every note.
- Otherwise the value is put through the combining-free transform (`text-matching.md` T8.1) and then the tag glob conversion of N2.2 step 4; the note matches when that expression matches the combining-free transform of `notes.tags`. The conversion runs after the transform, so a character whose NFKD is `*` or `_` becomes a wildcard.
- `tag:nc:ubung` and `tag:nc:UBUNG` match `Übung`; `tag:nc:full` matches the fullwidth tag `ｆｕｌｌ`, which `tag:full` misses (`tag:ＦＵＬＬ` matches it by simple folding).

### N2.5 Letter case and normalization

All three modes fold with simple case folding, so `tag:ÜBUNG` matches `Übung`, `tag:straße` does not match `STRASSE` and `tag:strasse` does. A value typed decomposed (`u` U+0308 `bung`) is converted to NFC first and matches `Übung`. Scenario: `search/tags/regex-and-nc`.

## N3. Decks (R1054–R1058, R1268)

### N3.1 Stored names

`decks.name` holds a deck's full path with its levels joined by U+001F (`Lang` U+001F `French`); the human form writes `::` between levels (`storage/schema-upgrade#U7`). Filtered decks are rows of the same table.

### N3.2 Special values

The value is G8's (kind U), then NFC. These whole values are special, lower case only:

- `*`: every card matches, including cards whose deck no longer exists.
- `filtered`: cards whose `odid` is not 0.
- `current`: N3.4.

`deck:Filtered`, `deck:Current` and `deck:current::*` are ordinary names.

### N3.3 Names

Any other value V matches decks by name:

1. Glob-convert V (N1), giving an expression text E.
2. Cut E at every `::` and treat each piece as a deck name level: NFC, remove ASCII control characters, remove White_Space characters and `:` from both ends, and an empty piece becomes `blank` (the level rule of `storage/schema-upgrade#U7` step 2). Join the pieces with U+001F, giving E′.
3. A deck matches when `(?i)^` + E′ + `($|` U+001F `)` matches its stored name.

A card matches when its `did` is a matching deck, or when its `odid` is not 0 and is a matching deck (R1056). So:

- a deck matches together with all its subdecks, by the full path from the top: `deck:french` matches `French` and `French::Verbs`, not `Languages::French` (`deck:*french` and `deck:*::french` do);
- `deck:a -deck:a::*` keeps the parent without its subdecks;
- `*` and `_` can match U+001F, so they cross levels: `deck:a_b` matches `a::b` and `axb`; `deck:fr*` matches `French`, `French::Verbs` and `Frame`;
- level trimming applies to the value: `"deck:a :: b"` matches `a::b`, `deck:a:` matches `a` (and its subdecks), `deck:a::` searches `a::blank`, and `deck:` searches the deck named `blank`; a single colon is part of a level (`deck:x:y` and `deck:x\:y` match the deck `x:y`);
- letter case folds with simple case folding: `deck:ÜBER` matches `über`, `deck:straße` does not match `STRASSE`;
- `deck:**` matches the cards of every existing deck, which differs from `deck:*` only for cards whose deck row is missing.

The scenario `search/decks/names` is to pin these examples; it is not yet written, so no golden holds them today (`search/decks/current` pins only `deck:current` and `deck:default`).

### N3.4 `deck:current` (R1055, R1057)

1. The current deck id is the search context's (`query-model.md` Q8): the config key `curDeck` read as a 64-bit integer (`storage/config-table`); when the key is missing or does not read as an integer (a string such as `"301"` included), the id is 1.
2. When a deck with that id exists, its stored name N is used. Otherwise N is the name `Default`. It is a name, not deck id 1: after the default deck was renamed and another deck named `default`, `deck:current` with a missing current deck finds that other deck.
3. A deck matches when `(?i)^` + N with every expression metacharacter escaped + `($|` U+001F `)` matches its stored name; cards match as in N3.3.

So `deck:current` takes the current deck's subdecks and, through `odid`, the cards of filtered decks whose home deck lies in that tree. When the current deck is a filtered deck, it finds the cards in that filtered deck. Setting the current deck when the user opens a deck or starts studying belongs to the decks area (U28) and the screens that open decks; this file only reads it. Scenario: `search/decks/current`.

### N3.5 `did:` (R1061)

- The value is G8's id list. Each digit run is a decimal number, leading zeros allowed (`did:0400` is 400). A run beyond the signed 64-bit range matches no deck. A list holding a non-ASCII digit fails with `search_failed` before anything runs (`errors.md` E3).
- A card matches when its `did` is in the list, or when its `odid` is not 0 and is in the list. Subdecks are not included.
- So `did:<H>` finds the cards of deck H and the cards of filtered decks whose home deck is H, not the cards of H's subdecks; `did:<F>` for a filtered deck F finds the cards now in F.

### N3.6 `preset:` (R1058)

- The value is G8's (kind R: escapes kept exactly as written). It is not converted to NFC and never treated as a pattern: `preset:Fas*` looks for a preset named `Fas*`.
- The preset is the row of `deck_config`, taken in ascending `id` order, whose `name` first equals the value under unicase equality: `preset:fast` and `preset:FAST` find `Fast`, `preset:straße` finds `STRASSE`. When none does, no card matches.
- The decks using it are the normal decks whose stored preset id, the `config_id` of the normal-deck kind (`decks/deck-presets`), equals that row's id (filtered decks have none). The comparison is literal, with no fallback: a deck whose `config_id` names no existing preset uses the default preset for scheduling but matches no `preset:` search, `preset:Default` included.
- A card matches when its `did` or its `odid` is one of those decks.
- Because the value keeps its escapes, a backslash is part of the name: `preset:a\b` finds the preset `a\b`, and `"preset:Japanese \"Core\""` looks for the name `Japanese \"Core\"` with its backslashes, so it does not find a preset named `Japanese "Core"`. A preset whose name contains `"` cannot be found by `preset:`. Code that builds a `preset:` search for a name (U88, U46, U47) must keep this in mind; the oracle confirms it (`search/decks/presets`).

### N3.7 Cards in filtered decks (R1268)

Summary for a card in filtered deck F whose home deck is H: `deck:` and `did:` find it under H (through `odid`) and under F (through `did`); `preset:` finds it through H's preset; `deck:filtered` finds it. Search terms on the card's state and properties read its columns as stored; `properties.md` says where a term reads the original due (`odue`) instead.

## N4. Note types and templates (R1059, R1060)

### N4.1 `note:`

The value is G8's (kind U), then NFC.

- With wildcards: a note type matches when `(?i)^` + glob conversion of the value + `$` matches its `notetypes.name`. `note:vocab*` matches `Vocab` and `Vocab Extra`; `note:straß*` does not match `STRASSE` (simple folding); `note:*` matches every note type.
- Without wildcards: a note type matches when its name equals the exact text (N1) under unicase equality. `note:VOCAB` matches `Vocab`, `note:straße` matches `STRASSE`, `note:x\_y` matches only `x_y` while `note:x_y` also matches `xzy`. An empty value matches no note type.

A note matches when its `mid` is a matching note type.

### N4.2 `card:`

- **Number.** When G8 reads the value as a template number, the card matches when its `ord` equals the ordinal G8 gives (number − 1; 0 for `card:0`, so `card:0`, `card:1`, `card:01` and `card:+1` are the first template). On a cloze note type, ordinal n − 1 is cloze n: `card:2` selects cloze 2. `card:65535` looks for ordinal 65534; `card:65536` is a name.
- **Name.** Otherwise the value (kind U) is converted to NFC and compared with `templates.name`, with wildcards by `(?i)^` + glob conversion + `$`, without them by unicase equality of the exact text. A card matches when its note's `mid` and its `ord` are the `ntid` and `ord` of a matching template row.
- A cloze note type has one template row, at ordinal 0. Its template name therefore matches only the cloze 1 cards: `card:cloze` finds no cloze 2 or later card.
- In notes mode, `card:2` finds the notes that have a card at ordinal 1, and `-card:1` the notes that have a card at another ordinal (`query-model.md` Q4).

Scenario: `search/notetypes/names`.

## N5. `nid:`, `cid:` and `mid:` (R1061)

- `nid:` and `cid:` take G8's id list with the number rules of N3.5. `nid:` matches the notes (or the cards of the notes) whose id is listed; `cid:` matches the cards whose id is listed, and in notes mode their notes. Listing an id twice changes nothing.
- `mid:` takes one signed 64-bit id (G8); a note matches when its `mid` equals it. `mid:1,2` is `invalid_whole_number` (`grammar.md` G8).
- Scenario: `search/ids/lists`.

## N6. `dupe:` (R1074)

The term behind the editor's "Show duplicates". G8 gives a note type id and a text (kind D).

1. The text is converted to NFC when `normalize_note_text` is on (it is a context value, `query-model.md` Q8), then stripped keeping media file names (`text/strip-html#H5`). Call the result C.
2. K is the first 4 bytes, read as a big-endian unsigned 32-bit integer, of the SHA-1 digest of the UTF-8 bytes of C as it is: only steps 2 and 3 of `text/checksum-and-sort-field#C4`, with no second H5, no NFC and no control-character removal. H5 is not idempotent (H4 decodes entities, and a second pass removes the tags they produced), so a first field `&lt;b&gt;Cat` has the stored checksum of `<b>Cat`, and `dupe:<id>,&lt;b&gt;Cat` gives C = `<b>Cat` and finds it; hashing H5 of C (`Cat`) would miss it.
3. A note matches when its `mid` equals the id, its `csum` column equals K, and its stored first field (the text of `notes.flds` before the first U+001F), stripped by H5 without any NFC step, equals C exactly.

So letter case, spaces and sound tags count: `dupe:<id>,Cat` finds `Cat` and `<b>Cat</b>`, not `cat` or `Cat ` (trailing space); `dupe:<id>,Cat[sound:x.mp3]` finds only a first field holding that sound tag; `"dupe:<id>,<img src=\"x.jpg\">Cat"` finds `<img src="x.jpg">Cat` (both strip to ` x.jpg Cat`); `"dupe:<id>,Tom & Jerry"` finds `Tom & Jerry` and `Tom &amp; Jerry`. An empty text finds notes whose first field strips to nothing. Other note types, and text in other fields, never match. The note itself is not excluded (the editor excludes the note being edited when it shows the result, U27, U191). `csum` is read as stored: a note whose stored checksum does not fit its field is not found. Scenario: `search/dupe/first-field`.

Meguri reads the candidates through the index on `notes.csum` (`ix_notes_csum`), then compares the stripped first fields. It reuses the note save path's H5 strip and the save path's hash of an already stripped text (C4 steps 2–3, U20, U27), never a helper that takes a raw first field and strips or normalizes it again.

## N7. Plan

- **Resolved at build.** Note type, template, deck, preset and `dupe:` candidates may be resolved to id sets when the plan is built (N1). `deck:current` reads the current deck id; `dupe:` reads `normalize_note_text` (`query-model.md` Q8). Name terms resolved to id sets are not slow.
- **Slow.** Tag terms test an expression per note; they are slow terms (`query-model.md` Q9). Normal mode calls `meguri_regex(pattern, tags)` and `tag:nc:` calls `meguri_regex(pattern, meguri_transform(tags, 1))` (`text-matching.md` T14), both on the whole column; only `tag:re:` calls the tags function below.
- **Errors.** `invalid_regex` for `tag:re:` (N2.3); `search_failed (expression too large)` for any expression of this file over the compiled size limit, the glob and escaped ones included (`text-matching.md` T12.1, T16); `search_failed` for a non-ASCII digit in an id list (`errors.md` E3).
- **SQL function** (Meguri design, registered with the others of `text-matching.md` T14): `meguri_regex_tags(pattern, tags)` is 1 when the compiled expression matches at least one piece of `tags` split at U+0020 (N2.3), else 0; NULL `tags` gives NULL. Only `tag:re:` uses it.

## N8. Scenarios

| Scenario | Sections |
|---|---|
| `search/tags/hierarchy` | N2.1, N2.2 (AE49 tag part) |
| `search/tags/regex-and-nc` | N2.3, N2.4, N2.5 |
| `search/decks/names` (not yet written) | N3.2, N3.3, N3.7 |
| `search/decks/current` | N3.4 |
| `search/decks/presets` | N3.6 |
| `search/ids/lists` | N3.5, N5 |
| `search/notetypes/names` | N4 |
| `search/dupe/first-field` | N6 |

Filtered decks in these scenarios are rows written with `db_exec` as `specs/conformance/deck-ops.md` K13 (U28) describes (fixed id, deck common `X'08011001'`, the filtered kind blob given there); normal decks with fixed ids copy the default deck's `common` and `kind`. Note, card and note type ids come from the frozen clock (`storage/ids-usn-mtime#I2`): the scenarios pin them with an `ids` observation or a `db_exec` read before the searches that name them.
