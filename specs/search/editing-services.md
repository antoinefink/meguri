# Search-field editing services

Spec: search/editing-services
Requirements: R1086, R1087, R1088, R1089, R1094, R1095, R1096, R1097, R1098, R1099, R1100, R1101, R1102, R1103, R1104, R1107, R1108, R1109; AE42 (core part)
Anki: none (Meguri design; it reads text by the 26.09.3 grammar)
Scenarios: conformance/scenarios/search/errors/ae42.yaml (the commit error kind only); tests in `core/crates/meguri-search/tests/edit_services.rs`
Status: Under review (R1232; round 8, decided 2026-11-20) for the lines of D2, D3, D5, D6 that begin "Changed 2026-11-20 (round 8)": do not implement them until approved. Every other section is in force and unchanged.

The core services behind Browse's search field (U88): canonical text for a commit, token spans for chips, edits of one token, classification of the text at the cursor while the user types, and suggestions. Anki's search field has none of these; the grammar they read is `grammar.md`, the errors `errors.md`, the text they write `canonical-form.md` and `join-and-replace.md` J3–J5. The field itself is U214, the suggestion list U215, the query pipeline U210. Swift never reads search text by its own rules: every token, tail and suggestion comes from here.

## D1. Services and shared rules

| Service | Input | Output |
|---|---|---|
| `canonicalize(text)` | field text | D2 |
| `spans(text)` | field text | D3 |
| `edit(text, target, through, action)` | field text, span indexes, an action | D4 |
| `extract(text, first, last)` | field text, span indexes | D4.4 |
| `classify_tail(text, cursor)` | field text, cursor offset | D5 |
| `suggest(text, cursor, limit, recent)` | field text, cursor offset, a count, recently accepted texts | D6 |

- **Field text.** Each service applies `join-and-replace.md` J2 first (U+001C–U+001F become spaces). J2 and `grammar.md` G1 step 1 replace one character by one, so every offset below is an offset in the text as the caller gave it.
- **Offsets.** Inside the core, offsets and ranges are character offsets (Unicode scalar values), half-open, as in `errors.md` E5. Across the FFI they are UTF-16 code units, both ways. A cursor or range from Swift that lies past the end of the text or splits a surrogate pair is refused with `invalid_offset`.
- **Purity.** All services except `suggest` read no collection, config or clock, and the same input always gives the same output. `suggest` reads the name index of D6.7 and its arguments only. No service raises a run error (`errors.md` E3); those come from running the search (`browser/result-sets` H3).

## D2. Canonical text

Changed 2026-11-20 (round 8): Browse's field no longer shows or commits canonical text: a commit reads the text into filter tokens (`search/filter-tokens` T7, T8). `canonicalize` stays for recent-search keys and for tests.

`canonicalize(text)` gives the canonical text of `canonical-form.md` F1–F6, or the parse error (kind of `errors.md` E1, details of E2, range of E5). U210 calls it to commit (R1109: Return, an accepted suggestion, a sidebar or chip action) and for searches that other screens hand to Browse (R1114), except Stats' joined text, which Browse shows and runs exactly as `stats/browse-queries` Q3 writes it (Q5); for that text U210 calls `canonicalize` only to record its canonical form in recent searches (`browser/entry-points` E3 rule 5, E6), never to show or run it. `dog cat or  fish` gives `dog cat OR fish`; an empty text, or one of spaces only, gives `deck:*` (F2.1). A committed empty search shows `deck:*` in the field, runs `deck:*` and records it, as Anki's browser does (`browser/entry-points` E5 step 3).

## D3. Token spans (R1086, R1087)

Changed 2026-11-20 (round 8): Browse's tokens are filter tokens (`search/filter-tokens` T1 to T6), not spans of Anki text; D3 and D4 are not used by Browse's field. They stay specified for later use.

`spans(text)` reads the text as `grammar.md` does and returns a list of spans in text order, and the parse error when there is one. An empty text has no spans.

### D3.1 Kinds

| Kind | Items (`query-model.md` Q3, `grammar.md` G2) |
|---|---|
| `plain` | plain text |
| `field` | a field term, any mode |
| `text` | `re:`, `nc:`, `sc:`, `w:` |
| `tag` | `tag:`, any mode |
| `deck` | `deck:`, `did:` |
| `preset` | `preset:` |
| `notetype` | `note:`, `mid:` |
| `template` | `card:` |
| `flag` | `flag:` |
| `state` | `is:` |
| `property` | `prop:`, every property |
| `date_range` | `added:`, `edited:`, `introduced:`, `rated:`, `resched:` |
| `ids` | `nid:`, `cid:` |
| `duplicates` | `dupe:` |
| `custom_data` | `has-cd:` |
| `operator` | an operator word (`and`, `or`), and a negated operator word (G4) |
| `group_open`, `group_close` | the `(` and the `)` of one group |

An implicit AND has no span. Saved searches are not a kind: the text does not record that a group came from one. Decided 2026-11-17: R1086 lists a saved-search icon among the chip kinds, and no span carries one. A saved search accepted from the suggestions (D6.6) enters the text as a bracketed group of its own items, which U214 shows as a token group like any other (R1087); U214 does not guess an origin by comparing group texts with saved searches. R1086's saved-search icon is therefore not provided, and the plan's R1086 needs the matching amendment.

### D3.2 Fields of a span

| Field | Value |
|---|---|
| `kind` | D3.1 |
| `range` | A term: from its first character to its last, quotes included. An operator word: the word. `group_open`: the `(`, or `-(` for a negated group. `group_close`: the `)`. A negated term, group or operator word starts at its `-`, so for a negated group only the `group_open` range holds the `-`. |
| `negated` | true when a `-` (G4) negates the item; for a negated group, true on its `group_open` and false on its `group_close` |
| `key` | for a term with a reserved key, the key with ASCII letters lowered (`DECK:x` gives `deck`); none for every other span |
| `key_range` | the key as written, for terms with a key (the field name for a field term); none otherwise |
| `value_range` | for a term with a key, the text after its first unescaped colon (G6), inside the quotes for a quoted or partly quoted term; for plain text, the text inside its quotes, if any. It can be empty: an empty range at the position after the colon |
| `canonical` | for a term, its canonical text (F5), without the negating `-`; for an operator word, `AND` or `OR` (F3's `- ` or `- OR `, trailing space included, when negated); none for brackets |
| `flag` | for a `flag:` term, the flag 0–7; none otherwise |
| `depth` | the number of groups whose `(` stands before the item and which hold it, closed or not |
| `partner` | for `group_open` the index of its `group_close`, and the reverse; none otherwise |

### D3.3 Texts with a parse error

With a parse error, the list holds the term and operator-word spans whose `range` ends at or before the start of the error's range (`errors.md` E5), and the bracket pairs whose `)` ends there too. A group still open has no bracket spans; the items inside it keep their depth. So `deck:Japanese tag:"wk-lev` gives the deck span and the error `unclosed_quote`, and U214 shows the rest as plain text (R1091).

### D3.4 Examples

`-deck:a (b or tag:c)`:

| Index | Kind | `range` | `negated` | `key` | `key_range` | `value_range` | `canonical` | `depth` | `partner` |
|---|---|---|---|---|---|---|---|---|---|
| 0 | `deck` | 0..7 | yes | `deck` | 1..5 | 6..7 | `deck:a` | 0 | |
| 1 | `group_open` | 8..9 | no | | | | | 0 | 5 |
| 2 | `plain` | 9..10 | no | | | 9..10 | `b` | 1 | |
| 3 | `operator` | 11..13 | no | | | | `OR` | 1 | |
| 4 | `tag` | 14..19 | no | `tag` | 14..17 | 18..19 | `tag:c` | 1 | |
| 5 | `group_close` | 19..20 | no | | | | | 0 | 1 |

- `"deck:a b" "x"`: a `deck` span 0..10 with `key_range` 1..5 and `value_range` 6..9, then a `plain` span 11..14 with `value_range` 12..13.
- `deck:"a b"`: a `deck` span 0..10, `key_range` 0..4, `value_range` 6..9.
- `deck:😀x`: the `deck` span is 0..7 in characters and 0..8 in UTF-16 code units; its `value_range` is 5..7 and 5..8.
- `a -and b`: `plain`, an `operator` span 2..6 with `negated` and `canonical` `- `, `plain`.
- `-(a b)`: a `group_open` span 0..2 with `negated`, depth 0 and partner 3; `plain` spans 2..3 and 4..5 at depth 1, not negated; a `group_close` span 5..6, not negated, depth 0, partner 0.

## D4. Token edits (R1088, R1089)

`edit(text, target, through, action)` changes one item, or one run of items, of the parsed text and returns the canonical text of the result, written as a whole search (`canonical-form.md` F2–F6), as `join-and-replace.md` J7 step 3 does. A text that does not parse gives its parse error and nothing else.

### D4.1 Targets

- `target` is the index of a span in `spans(text)` of the same text. A term span or a `group_open` span names that item (a negated one with its negation). An `operator` span names its joiner, or, for a negated operator word, that item. A `group_close` span, or an index outside the list, is refused with `invalid_target`.
- `through` is none, or for `remove` only, the index of a later item of the same sequence: the run from `target` to `through` is then removed as one item. A `through` in another sequence is `invalid_target`.

### D4.2 Actions

| Action | Target | Result |
|---|---|---|
| `toggle_negation` | an item that is not an operator word | the item negated by `join-and-replace.md` J5: a negation gives the item it negates, anything else its negation |
| `set_joiner(AND or OR)` | a joiner, or an item that is not the first of its sequence (then the joiner before it) | that joiner set; an explicit `and` and an implicit AND are the same joiner |
| `replace_with(item)` | a term, negated or not | the term replaced by the built item (J3, written by J4's rules), the negation above it kept; a built group is bracketed (F3). When the term is negated and the built item is itself a negation (J3's "any flag", "untagged", or a negated item), the two negations combine by J5: the target becomes the item the built item negates, never a negation of a negation, which F3 would write as `--` and `grammar.md` G4 would read back as another term. J3's refusals are `invalid_item` |
| `remove` | an item or a run (D4.1) | D4.3 |

Any other combination is `invalid_target`: a negated operator word can only be removed.

### D4.3 Removing

The item (or run) is removed with one joiner next to it:

- the first item of its sequence: the joiner after it;
- the last item: the joiner before it;
- otherwise, when exactly one of the two joiners is OR, the other one (the AND); otherwise the joiner before it.

So the OR structure stays: removing `b` from `a OR b c` gives `a OR c`, not `a c`. A group left with no item is removed in turn by the same rule, from its own sequence. When the whole search is left with no item, the result is the empty text (the whole collection, `query-model.md` Q2), not `deck:*`: it is the only result of this file not written by F2.

### D4.4 Copy

`extract(text, first, last)` gives the canonical text, written as a whole search (F2–F6), of the items from span `first` to span `last` of one sequence and the joiners between them. Both must be term, `group_open` or negated-operator spans of one sequence, `first` not after `last`; otherwise `invalid_target`. ⌘C and ⌘X of selected tokens copy it (R1089); ⌘X then calls `remove` with the same run.

### D4.5 Examples

| Call | Result |
|---|---|
| `deck:French tag:verbs`, `toggle_negation` on 1 | `deck:French -tag:verbs` |
| `deck:French -tag:verbs`, `toggle_negation` on 1 | `deck:French tag:verbs` |
| `-(a b)`, `toggle_negation` on 0 | `a b` |
| `a OR b`, `set_joiner(AND)` on 1 (the operator) | `a b` |
| `a b`, `set_joiner(OR)` on 1 | `a OR b` |
| `a OR b c`, `remove` 2 | `a OR c` |
| `a b OR c`, `remove` 1 | `a OR c` |
| `(a) b`, `remove` 1 | `b` |
| `deck:a`, `remove` 0 | (empty text) |
| `a b c d`, `remove` 1 through 2 | `a d` |
| `-deck:a is:due`, `replace_with(deck B)` on 0 | `-deck:B is:due` |
| `-flag:1`, `replace_with(any flag)` on 0 | `flag:0` |
| `flag:1`, `replace_with(any flag)` on 0 | `-flag:0` |
| `a OR b`, `toggle_negation` on 1 | `invalid_target` |
| `a (b c) OR d`, `extract` 1 to 6 | `(b c) OR d` |
| `a (b c) OR d`, `extract` 1 to 1 | `b c` |

## D5. Classifying the text at the cursor (R1107, R1108)

Changed 2026-11-20 (round 8): in Browse's field D5 is used only while the user types, before a commit (`search/filter-tokens` T8); Browse shows no parse error (T5), so this section's references to the pipeline showing the error (R1108) and to Return showing it (AE42) do not apply to Browse.

`classify_tail(text, cursor)` tells U210's live search (R1106) whether the text can run now. It returns one of:

- `Valid`: the text parses and no form of D5.1 holds. The pipeline runs it.
- `Incomplete{form, range}`: a form of D5.1 holds. The pipeline keeps the last valid results and shows no error.
- `Error{error}`: the parse error, as `canonicalize` gives it. The pipeline shows it (R1108).

`Valid` means only that the text parses: run errors come when it runs. A commit never uses this service: it calls `canonicalize`, so Return on an incomplete text shows its error (AE42).

### D5.1 Incomplete forms

An item **ends at the cursor** when its `range` (D3.2) ends exactly at the cursor. Only the error the text reports (`errors.md` E4) is considered: an earlier error elsewhere is `Error` whatever the cursor.

| Form | Holds when | `range` |
|---|---|---|
| `empty_value` | the text parses, and a term with a key, reserved or a field, ends at the cursor, or the cursor stands just before its closing `"`, and its value is empty or ends in an empty part. An **empty part** is nothing after a mode prefix (`re:`, `nc:`, lower case as `grammar.md` G6 reads it) of a `tag:` or field term, nothing after the comma of `dupe:`, or nothing after the operator of `prop:cds:<key>`; nothing else is one, so `deck:a::` and `tag:a::` are `Valid` (D5.2). The forms: `deck:`, `tag:`, `note:`, `card:`, `re:`, `has-cd:`, `preset:`, `f:`, `f:re:`, `f:nc:`, `tag:re:`, `tag:nc:`, `dupe:1,`, `prop:cds:k=`, `"Extra Info:"` | the term |
| `dangling_minus` | the text parses, and the plain term `-` written as a bare `-` (G4) ends at the cursor | the `-` |
| `unclosed_quote` | the error is `unclosed_quote` and the cursor stands after its opening `"` | the error's range |
| `empty_quote` | the error is `empty_quote` and the cursor stands between its two `"` | the error's range |
| `unclosed_group` | the error is `unclosed_group` and the cursor stands after the `(` it names | from that `(` to the end of the text |
| `operator_word` | the error is `misplaced_and` or `misplaced_or`, and either the word ends at the cursor (more letters can make it a plain word: `or` → `orange`), or the word follows an item of its sequence that is not an operator word, is the last item of its sequence, and only spaces stand between its end and the cursor | the word |
| `trailing_escape` | the error is `unknown_escape` for a lone trailing backslash (G3), which ends at the cursor | the backslash |
| `partial_value` | the error is a value error of a term with a key (`invalid_state`, `invalid_flag`, `invalid_property`, `invalid_operator`, `invalid_number`, `invalid_whole_number`, `invalid_positive_whole_number`, `invalid_negative_whole_number`, `invalid_answer_button`, `invalid_id_list`, `invalid_dupe`), the term is unquoted and ends at the cursor, and some characters other than space, `"`, `(` and `)` added at the cursor would make the term parse | the term |

### D5.2 Examples

With the cursor at the end of the text unless shown by `|`:

| Text | Result |
|---|---|
| `deck:Japanese tag:"wk-lev` | `Incomplete{unclosed_quote}`, range from the `"` (AE42) |
| `deck:Japanese tag:"wk-lev` with the cursor before `deck` | `Error{unclosed_quote}` |
| `deck:` | `Incomplete{empty_value}` |
| `deck: ` | `Valid` (the deck named `blank`, `names-and-ids.md` N3.3) |
| `deck:a::`, `tag:a::`, `f:RE:` | `Valid` (`deck:a::` searches `a::blank`, N3.3; `f:RE:` is a normal field search for `RE:`, G6) |
| `f:re:`, `tag:nc:`, `"dupe:1,\|"`, `prop:cds:k!=` | `Incomplete{empty_value}` |
| `deck:\| is:due` | `Incomplete{empty_value}` |
| `a -` | `Incomplete{dangling_minus}` |
| `(a b`, `deck:x (\|is:due` | `Incomplete{unclosed_group}` |
| `a or`, `a or `, `or`, `a and or`, `(a or \|)` | `Incomplete{operator_word}` |
| `or `, `a and or `, `a or )` | `Error{misplaced_or}` |
| `abc\` | `Incomplete{trailing_escape}` |
| `is:`, `is:ne`, `is:buried-`, `flag:`, `flag:+`, `added:`, `rated:1:`, `mid:-`, `nid:1,`, `dupe:12` | `Incomplete{partial_value}` |
| `prop:`, `prop:iv`, `prop:re` (to `reps`), `prop:ivl`, `prop:ivl!`, `prop:ivl>`, `prop:due=-`, `prop:ease>1e`, `prop:r<in`, `prop:cdn:`, `prop:rated=-1:` | `Incomplete{partial_value}` |
| `is:NEW`, `is:x`, `flag:8`, `flag:1.`, `rated:1:5`, `added:-1`, `prop:IVL`, `prop:ivl>1.`, `prop:ivl>-`, `prop:rated=1`, `nid:1,,`, `dupe:x` | `Error` with the value's error |
| `is:x deck:` | `Error{invalid_state}` |
| `is:\| flag:9` | `Incomplete{partial_value}` (the later error shows once `is:` is complete) |
| `:` | `Error{missing_key}` |
| (empty) | `Valid` |

## D6. Suggestions (R1094–R1104)

Changed 2026-11-20 (round 8): Browse's suggestions are `search/filter-tokens` T12. D6 still gives the rows when Anki syntax is being typed at the caret (T12.2), and D6.4, D6.5 and D6.7 are the matching, order and name index T12 reuses. Keywords are never suggested for an empty word.

Decided 2026-10-27: suggestions are computed by this service in the core, as U88 states; U215 shows them, adds recent searches (R1102) and handles the keys (R1105), and needs no reading of search text of its own.

This replaces these parts of the plan's U215: `CursorContext.swift` and `CursorContextTests.swift` (the current term, its key and the note type scope are D6.2 and D6.4), the matching and ranking of `SuggestionEngine` (D6.4, D6.5), the `SearchVocabulary` snapshot and its prepared match keys (the core's name index, D6.7), and the budget of 8 ms ranking on the main thread (D6.7's core budget). U215 keeps the list view, recent searches, the keys, and one `suggest` call per keystroke off the main thread. Its plan test `note:Kaishi Mea` gives `as_typed`, then the field key `Meaning:` (D6.3, row W); the forms `Meaning:_*`, `Meaning:`, `Meaning:re:` and `Meaning:nc:` come only once `Meaning:` is typed (R1098, D6.6). The owner of U215's plan text updates those lines.

### D6.1 The call

`suggest(text, cursor, limit, recent)` returns candidates in display order.

- `limit`: at most this many candidates; 0 gives none, and values above 50 count as 50 (R1104).
- `recent`: insert texts (D6.6) of suggestions the user accepted, newest first. U215 keeps them on the device (never in the collection); they only rank (D6.5).
- A candidate has: `kind` (`as_typed`, `keyword`, `deck`, `tag`, `notetype`, `template`, `field`, `flag`, `state`, `property`, `operator`, `value_hint`, `days`, `button`, `preset`, `saved_search`); `label`, the text to show (a name as stored, with `::` between levels); `detail` (the filtered-deck marker, a flag's shown name, a property's unit, a day count or a button number, by kind); `insert_range`, `insert_text` and `cursor_after` (D6.6), absent for `as_typed` and `value_hint`; and `completes` (D6.6).

### D6.2 The current term

- The **current term** is the term of the text (`grammar.md` G3 forms) whose range holds the cursor or ends at it, its negating `-` left out. When no term does (the cursor at the start, or after a space, `(` or `)`), or the term is the bare plain term `-` ending at the cursor, the current term is empty at the cursor. A term with an unclosed quote ends at the cursor. When G3 cannot read the term at the cursor (a backslash before a space in it), there are no candidates.
- Its **typed part** T is its text from its start to the cursor, without its opening `"` for a quoted term, with `\"` read as `"`. T is split at its first unescaped colon (G6): no colon gives a **word** W = T; otherwise a **key** K (ASCII letters lowered) and a **partial value** V, the rest of T (inside the quotes for a partly quoted term).
- W and V are read as G7's kind U, without its errors (a backslash pair it does not know stays as written), then as `names-and-ids.md` N1's exact text: `\_` gives `_`, and a typed `*` or `_` is matched as that character, not as a wildcard.
- K, with ASCII letters lowered, picks a reserved key's context of D6.3. Any other K is a field key: it is read like W and V (so `my\:field` gives `my:field`) before it is compared with field names.

### D6.3 Sources

The field is **empty** when the text is empty or only spaces. Otherwise the context comes from the current term:

| Context | Candidates, by source |
|---|---|
| empty field | every saved search (R1102; U215 puts recent searches first) |
| W empty | every keyword; every saved search |
| W | `as_typed`; keywords whose name begins with W, ASCII case ignored; field keys of the scoped note types (D6.4) matching W; saved searches matching W (R1094, R1098, R1101) |
| `deck:` | the specials `current`, `filtered` and `*` that begin with V, ASCII case ignored; decks matching V, each deck with subdecks followed by `Name::*`; filtered decks carry the marker (R1095) |
| `tag:` | `none` when it begins with V, ASCII case ignored; registered tags (`tags` rows) matching V (R1096). Accepting `animal` puts `tag:animal` at the cursor, whose candidates then hold `animal::mammal` |
| `tag:re:`, `tag:nc:` | none |
| `note:` | note type names matching V (R1097) |
| `card:` | template numbers 1 to N, N the largest template count of a scoped note type, whose decimal text begins with V; template names of the scoped note types matching V (R1097) |
| a field key K that names a field of a scoped note type (unicase) | the forms `K:_*`, `K:` (empty), `K:re:`, `K:nc:` whose value part begins with V (R1098) |
| any other field key | none |
| `flag:` | flags 0–7 whose number begins with V, and flags 1–7 whose shown name (`tags/flags-and-marked` F4) matches V: `flag:imp` gives flag 3 when it is named "Important" (R1099) |
| `is:` | the eight state words of `grammar.md` G8 that begin with V, ASCII case ignored, in G8's order |
| `prop:` | D6.3.1 |
| `added:`, `edited:`, `introduced:`, `resched:`, and `rated:` before a second colon | days 1, 7, 30 and 365 whose decimal text begins with V (R1100) |
| `rated:N:` | buttons 1–4 whose digit begins with the text after the second colon |
| `preset:` | preset names (`deck_config` rows) matching V that D6.6 can write |
| `did:`, `nid:`, `cid:`, `mid:`, `dupe:`, `re:`, `nc:`, `sc:`, `w:`, `has-cd:` | none |

Every context with a non-empty current term starts with `as_typed`, which commits the text as it is (R1094). Keywords, in this order: `deck:`, `tag:`, `note:`, `card:`, `flag:`, `is:`, `prop:`, `preset:`, `added:`, `edited:`, `rated:`, `introduced:`, `resched:`, `dupe:`, `nid:`, `cid:`, `did:`, `mid:`, `re:`, `nc:`, `sc:`, `w:`, `has-cd:`. So `de` gives `as_typed` then `deck:`, and `pr` gives `prop:` then `preset:`.

#### D6.3.1 `prop:`

With V the partial value:

1. Properties of `grammar.md` G8.1 (`ivl`, `due`, `reps`, `lapses`, `ease`, `pos`, `rated`, `resched`, `s`, `d`, `r`, `cdn:`, `cds:`, in that order) that begin with V, ASCII case ignored, other than V itself.
2. When V is a property p (with at least one key character after `cdn:` or `cds:`) followed by nothing or by the start of an operator: the operators `<`, `<=`, `>`, `>=`, `=`, `!=` that begin with what follows p.
3. When V is a property p, an operator and an operand: one `value_hint` for p, whose `detail` is its unit: days (`ivl`, `s`), days from today, negative when overdue (`due`), a count (`reps`, `lapses`), an ease factor such as 2.5 (`ease`), a position (`pos`), days ago, 0 or less (`rated`, `resched`), 0 to 1 (`d`, which `properties.md` P7 maps to the stored 1–10 scale, and `r`), a number (`cdn:`), a text (`cds:`). U215 writes the words (R1100).

So `prop:r` gives `reps`, `rated`, `resched`, then the six operators for `r`.

### D6.4 Matching and scope

- **Decks and tags** match by `tags/complete` C2 and C3, with V as the input after C5's removal of one single leading and one single trailing `:`. A deck's components are the levels of its name with `::` between them (`names-and-ids.md` N3.1). C3's prefix matches are the **prefix** class, its inner matches the **inner** class: with the decks `Japanese::Lesson1` and `Lessons`, `deck:lesson` gives `Lessons` (prefix) before `Japanese::Lesson1` (inner).
- **Other names** (note types, templates, fields, presets, saved searches, flag names): V matches when it occurs in the name, both compared character by character under simple case folding (`case-folding.md` C3), without other normalization. It is in the prefix class when it occurs at the start, otherwise in the inner class.
- **Scoped note types.** The note types named by the non-negated `note:` and `mid:` terms of the current term's AND-run (a sequence is the OR of its runs of AND-joined items, `query-model.md` Q1), and of each AND-run that holds a group around the current term. `note:` resolves by `names-and-ids.md` N4.1 and `mid:` by id. Only items read before the text's first parse error count, and the current term never does. When there are no such terms, every note type is in scope. When there are some and they name no note type (a typo, a deleted id), the scope is empty: `card:` and a field key then give only `as_typed`, and a word W gets no field keys. `note:"Basic" card:` gives only Basic's templates.

### D6.5 Order and limit

1. `as_typed`, when the context has it.
2. The fixed candidates of the context, in the order D6.3 lists them: keywords, specials, `none`, template numbers, field forms, flags in flag-number order, whether they match by number or by shown name (a flag that matches both ways is listed once; D6.4's classes do not reorder flags), states, properties, operators, days, buttons, the value hint.
3. The name candidates of the prefix class, then those of the inner class. In a context with several name sources, each class lists field keys before saved searches. Within a class, each source's names are in the order `ORDER BY name COLLATE unicase, name` (`storage/schema-18`): stored deck names (levels separated by U+001F, so `a::b` comes before `a b` and `a-b`), tags, note type, template, field and preset names, saved search names. A deck's `Name::*` follows the deck.
4. Recent use reorders only inside one group: within step 2, and within each class of step 3 (prefix, inner) on its own, a candidate whose `insert_text` is in `recent` comes first, newest first; the others keep their order. Decided 2026-10-27: R1104 names both orders, and the class wins, so a recent inner match never comes before a prefix match, and no candidate moves across steps.
5. The list is cut after `limit` candidates.

### D6.6 Insertion (R1101, R1103)

- **`insert_range`.** The current term, from its start (after a negating `-`, which stays) to its end, which can lie after the cursor. An empty current term gives an empty range at the cursor. A saved search accepted in an empty field replaces the whole text.
- **`insert_text`** is the canonical text of the candidate's built item (`join-and-replace.md` J3 and J4), so names are escaped and quoted there and nowhere else:

  | Candidate | `insert_text` |
  |---|---|
  | keyword k | `k` (`deck:`) |
  | deck d; tag t; note type n | J3's deck, tag and note type items by name (`deck:Current` for a deck named `current`, J3.1); for a deck or note type that J3.5 refuses, its exact item: `did:` with the deck's id and its descendants' ids, or `mid:` with the note type's id |
  | specials | `deck:current`, `deck:filtered`, `deck:*`, `tag:none` |
  | `Name::*` | Q(`deck:` + E(name) + `::*`), with E of J3 and Q of `canonical-form.md` F4 |
  | template number k; template name s | `card:` + k; Q(`card:` + E(s)) |
  | field key f | Q(f′ + `:`), with f′ = E(f) with every `:` written `\:` (F5) |
  | field forms | J3's field present item (`f′:_*`); Q(f′ + `:`); Q(f′ + `:re:`); Q(f′ + `:nc:`) |
  | flag n; state w; days d; button b | `flag:n`; `is:w`; key + `:` + d; `rated:` + the typed days text + `:` + b |
  | property p; operator o | `prop:` + p; `prop:` + p + o |
  | preset p | Q(`preset:` + p) |
  | saved search with text s | c in an empty field, otherwise `(` + c + `)`, where c is `canonicalize(s)` (D2) |

- **Saved search texts.** The stored text s is never inserted as it is: bracketing a text that does not parse can change where its items end (`abc\` in brackets becomes `(abc\)`, whose `\)` is part of the term). A saved search whose s does not parse (`canonicalize` gives an error) is not offered. One whose s parses but fails when it runs (`errors.md` E3) is offered, and its error shows when the accepted search runs.
- **Names that cannot be written.** A template or preset name is offered only when its `insert_text` reads back (`grammar.md`) as a term of that kind holding exactly that name: so no preset name holding `"` (`names-and-ids.md` N3.6) or White_Space other than U+0020, and no template name that reads back as a number. Field names that `join-and-replace.md` J3.3 refuses are not offered. A deck that J3.5 refuses has no `Name::*` candidate (its subdecks are offered each with its own exact item), and a deck or note type whose exact item has a negative id is not offered.
- **`completes`** is true when the inserted text is a whole term or a saved search: names, specials, template numbers, `K:_*` and `K:`, flags, states, days, buttons, presets, saved searches, and `as_typed`. It is false for keywords, field keys, `K:re:` and `K:nc:`, properties and operators, after which the user goes on typing. Decided 2026-10-27: accepting a candidate commits the search (R1109) only when `completes` is true. Committing `prop:` would show an error the user did not make.
- **`cursor_after`**: the end of the inserted text; when `completes` is false and `insert_text` ends with a closing `"`, just before that `"`, so the value is typed inside the quotes.

Examples:

| Text, cursor at the end | Accepted | Field text after it |
|---|---|---|
| `de` | `deck:` | `deck:` |
| `-de` | `deck:` | `-deck:` |
| `deck:fre` | deck `Languages::French` | `deck:Languages::French` |
| `deck:a_` | deck `a_b* (x)` | `"deck:a\_b\* (x)"` |
| `Ext` | field key `Extra Info` | `"Extra Info:"`, cursor before the last `"` |
| `note:Kaishi Mea` (candidates: `as_typed`, then field key `Meaning`) | field key `Meaning` | `note:Kaishi Meaning:`; its candidates are then `as_typed`, `Meaning:_*`, `Meaning:`, `Meaning:re:`, `Meaning:nc:` |
| `flag:imp` | flag 3 | `flag:3` |
| `deck:x lee` | saved search `Leeches` (`tag:leech`) | `deck:x (tag:leech)` |
| (empty) | saved search `Leeches` | `tag:leech` |

### D6.7 The name index

- `suggest` reads an in-memory index, never the collection: deck names with their ids, filtered state and whether they have subdecks; registered tags; note types with their ids, template names by ordinal and field names; preset names; saved searches as `browser/config-keys` K7.1 reads `savedFilters`; flag shown names as `tags/flags-and-marked` F4 reads `flagLabels`.
- The engine builds it when the collection opens and rebuilds the affected part from the change stream after each change set that touches decks, tags, note types, presets or those two keys. A call reads the index as last built and never waits for a write or a reader. A candidate can therefore lag a very recent change; accepting it only puts search text in the field and changes no data.
- R1104: a call returns within one frame on a 150k-card collection with thousands of tags; U88's micro-benchmark checks at most 50 candidates in under 4 ms with 10,000 tags.

## D7. Tests

No oracle scenario applies to the services themselves: Anki has none of them. `core/crates/meguri-search/tests/edit_services.rs` covers every example table of D3–D6 and these cases:

- `spans` over every kind of D3.1, negated and grouped, with error texts of D3.3; FFI offsets in UTF-16 for text holding characters above U+FFFF; `invalid_offset` for a cursor inside a surrogate pair.
- `edit`: each action and each removal rule of D4.3, on terms, groups, runs and negated operator words; `replace_with` of a negation item (any flag, untagged, a negated item) on a negated and on a plain target; every result parses, reads back (`grammar.md`) to the query it was written from, and equals its own canonical text, except the empty text. Like `join-and-replace.md` J8, these checks exclude inputs holding one of `canonical-form.md` F7's first three shapes, and for its fourth shape check the meaning only.
- `classify_tail`: each form of D5.1 at, before and after the cursor; AE42 (core part): the D5.2 row, then `canonicalize` of the same text gives `unclosed_quote` with the range from the `"`. The error kind itself (not its range) is Anki's too, pinned by `conformance/scenarios/search/errors/ae42.yaml` (`canonical_search` and `search`).
- `suggest`: each context of D6.3, the scope of D6.4 (an AND-run, an OR between runs, an enclosing group, a negated `note:`, and `note:Kaishx card:` with no note type `Kaishx`, which gives only `as_typed`), the order of D6.5 with and without `recent` (decks `a::b`, `a b` and `a-b` for `deck:a`), the cut at `limit`, the insertions of D6.6, an escaped field key (`my\:field:` gives the forms of field `my:field`), and `deck:Cur` offering `current`. For each deck and note type candidate, and each tag candidate whose name J3 writes exactly (in NFC), the field text after acceptance, run on the fixture collection, finds that object and none of its look-alikes (`a_b*` next to `axb`; `a` U+00A0 `b` next to `axb`, inserted as `did:`; R1103). For the other tag names (`join-and-replace.md` J10), the expected rows are those `names-and-ids.md` N2 gives for the inserted text; the fixture holds at least one such name.
- The micro-benchmark of D6.7.
