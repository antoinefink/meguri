# Built searches, joining and replacing

Spec: search/join-and-replace
Requirements: R1078, R1079; supports R1076, R1077, R1109, R1114, R1153, R1322 (AE43, core part; AE60 only for J11's test that its text parses and that its canonical form equals a golden)
Anki: 26.09.3
Scenarios: conformance/scenarios/search/builder/, conformance/scenarios/search/join-replace/

The search text Meguri writes for UI actions: searches built from structured items (a deck, a tag, a flag, ids), a built item joined to the current search with AND or OR, and a built item put in place of the current search's terms of the same kind. Every Meguri-generated search except Stats' joined text goes through these services (U88): sidebar and chips, Home "Browse", editor duplicates, Select Notes, AnkiConnect GUI actions. Stats "Show in Browser" is not a caller: its text is the literal join of `stats/browse-queries` Q3, which is not J6's canonical join, and its Deck scope text (`deck:"..."`, `stats/browse-queries` Q2.2) is not the form J3 builds for a deck. U88 tests only that AE60's text parses and that its canonical form equals a golden (J11). The text is written by `canonical-form.md`; what it means is `query-model.md`. The token spans, incomplete-tail classification and suggestions of U88 are Meguri's own and live in `search/editing-services` (not this file).

## J1. The services

| Service | Input | Output |
|---|---|---|
| `build(item)` | one built item (J3) | its search text (J4) |
| `join(existing, item, joiner, negate)` | the search field's text, a built item, AND or OR, a flag | J6 |
| `replace(existing, item)` | the search field's text, a built item | J7 |

- They read no collection, config or clock: the same input always gives the same text.
- Errors: a parse error of `existing` (`errors.md` E1) or of a parsed-text item (J3), and `invalid_item{reason}` for a built item J3 refuses. No run error (E3) is raised here; running the result may raise one.
- **Parse-error order.** `join` and `replace` read `existing` first and return its parse error before reading a parsed-text part of the built item. After that, or for `build`, parsed-text parts are read in item order: a group visits its items in list order, fully descending into each before the next; a negation visits its child. The first parse error is returned. This specifies parse-error selection, not a new precedence among `invalid_item` refusals.
- A parse error carries a `source`: `existing`, or the path of the parsed-text item in the built item, which is the list of steps from the top item down to it (in a group, the item's index; under a negation, 0). Its range lies in that text: U214 highlights it in the field only when `source` is `existing`, or a parsed-text item with `field_text` set (J3).
- Ranges cross the FFI in UTF-16 code units (`errors.md` E5).

## J2. Search-field text

Text taken from the search field (typed, pasted, picked from history) has every character U+001C–U+001F replaced by U+0020 before anything reads it, as Anki's search field does; `grammar.md` G1 then handles the other white space. The services that take field text apply this rule: `join` and `replace` to `existing`, U210's commit and live search to the field text, and entry points that open Browse with a search to that search (R1114). The canonical text observation (`canonical_search`) and the local servers' searches do not: Anki's own search reads those characters as term characters (`grammar.md` G1).

## J3. Built items

Each built item makes one query item (`query-model.md` Q1, Q3). "E(s)" below is the name escape:

- **E(s)**: s with a `\` put before every `\`, `*` and `_`, so the name has no wildcard (R1078). Then every character with the White_Space property other than U+0020, and every character U+001C–U+001F, is replaced by `_` (G1 and J2 would otherwise turn it into a space and split the term; `_` matches that one character, and any other there).

| Item | Query item | Refused (`invalid_item`) |
|---|---|---|
| deck by name n | deck term with E(n) (J3.1) | n empty; n not writable exactly (J3.5) |
| current deck | deck term `current` | |
| any deck | deck term `_*` | |
| tag by name n | tag term, normal mode, E(n) (J3.1); n holding White_Space other than U+0020 or U+001C–U+001F: J3.2 | n empty |
| any tag | tag term `_*` | |
| untagged | NOT (tag term `_*`) | |
| note type by name n | `note:` with E(n) | n empty; n not writable exactly (J3.5) |
| any note type | `note:_*` | |
| note type by id | `mid:` with the id | |
| template by ordinal k | `card:` with ordinal k (0-based) | k > 65534 |
| field present, field f | field term, normal mode, name E(f), text `_*` | f empty; f is a reserved key (J3.3) |
| field text, field f, text t | field term, normal mode, name E(f), text E(t) | as field present |
| literal text t | plain text E(t) | t empty |
| flag n (1–7) | `flag:` n | n outside 1–7 (0 is the "no flag" item) |
| no flag | `flag:0` | |
| any flag | NOT `flag:0` | |
| state | `is:` with one of the 8 states | |
| added, edited, first review in d days | `added:`, `edited:`, `introduced:` with d, 0 raised to 1 | d above 4294967295 |
| answered in d days, any button or button b (1–4) | `rated:` with d (0 raised to 1) and b | as above; b outside 1–4 |
| rescheduled in d days | `resched:` with d (0 allowed) | as above |
| due on day i | `prop:due=` i | i outside signed 32-bit |
| due within i days | `prop:due<=` i | as above |
| note ids, card ids, deck ids | `nid:`, `cid:`, `did:` with the ids in the given order, joined by `,`, in decimal | an empty list; a negative id |
| duplicates of text t in note type m | `dupe:` with m and t | t holding White_Space other than U+0020, or U+001C–U+001F (J3.4) |
| whole collection | the whole-collection term | |
| parsed text s, with a flag `field_text` | the items `grammar.md` reads from s (J2 first when `field_text` is set): one item when s has one, otherwise a group of all of them | a parse error of s |
| group | AND or OR over 1 or more built items | no items |
| negation of an item | J5 | |

- A group of one item is that item, with no brackets. A group of two or more items is a group (`query-model.md` Q1) of the items joined by its joiner.
- Days, ids and numbers are those of `grammar.md` G8 and G9; the item stores them as numbers, so `canonical-form.md` F5 and F6 write them.
- `field_text` is set by a caller whose text comes from a search field (typed, pasted or picked from history, J2). Saved searches, AnkiConnect texts and other stored texts leave it unset. `existing` is always field text (J2).

Save normalization and search escaping are separate. A note type save removes `"` from its name; a field save removes `:` (and the other characters of `notetypes/records` N4). The builder does not apply that save normalization: it writes the name it is given. A caller building a search for a saved object uses the stored name returned by the save. An escaped query for a name that the save changed need not find that object.

### J3.1 Names that are special values

`deck:` gives `current` and `filtered`, and `tag:` gives `none`, a special meaning when the value is exactly that lower-case word (`names-and-ids.md` N3.2, N2.2). Decided 2026-10-27 (data safety first: generated searches feed bulk actions): a deck named `current` or `filtered`, or a tag named `none`, is written with its first letter in upper case (`deck:Current`, `deck:Filtered`, `tag:None`), which `names-and-ids.md` reads as the ordinary name, with letter case folded, so it matches that deck (and its subdecks) or tag (and its descendants) only. Anki writes `deck:current`, `deck:filtered` and `tag:none` for them, and keeps a deck, tag or note type name, or the field name of a "field present" item, of exactly `_*` unescaped (`deck:_*`, `_*:_*`: every deck, any field); E escapes it (`deck:\_\*`, `\_\*:_*`), and the "any" items above give `_*` instead. These differences are J10's.

"Any tag" and "untagged" are Anki's own terms (`tag:_*` and `-tag:_*`), kept for parity. `tag:_*` skips a note whose every tag holds White_Space in its first level (`names-and-ids.md` N2.2), so "any tag" misses such a note and "untagged" returns it, as in Anki; notes whose tags column is empty are exactly `tag:none` (N2.2 step 1).

### J3.2 Tags holding other white space

A tag can hold White_Space characters other than U+0020 (`names-and-ids.md` N2.1), which no normal-mode tag search finds. Such a tag is written as a regex tag term (`names-and-ids.md` N2.3) whose expression is `^`, the tag with each character of `\ . + * ? ( ) | [ ] { } ^ $ # & - ~` preceded by `\` and each White_Space character and each character U+001C–U+001F written as `\x{` + its code point in lower-case hexadecimal without leading zeros + `}`, then `(::|$)`. The expression matches the tag and its descendants, with letter case folded, as a normal-mode term would: `r` U+00A0 `s` gives `"tag:re:^r\x{a0}s(::|$)"`. Anki writes such a tag unchanged, and its search text splits there (J10).

### J3.3 Reserved field names

A field whose name, with ASCII letters lowered, is a reserved key of `grammar.md` G6 (`deck`, `tag`, `is`, `has-cd` and the others) cannot be searched by a field term: `Deck:_*` is a deck term. The builder refuses such a field (`invalid_item{reason: reserved_field_name}`), and the UI offers no search for it. Anki writes the reserved term and searches decks, tags or states instead (J10).

### J3.4 Duplicates

The `dupe:` text is compared exactly (`names-and-ids.md` N6), so no character may change on the way back: text holding White_Space other than U+0020, or U+001C–U+001F, is refused, and the caller (the editor's Show Duplicates, R807) builds a note-ids item from the duplicate notes it already found.

### J3.5 Deck and note type names that cannot be written exactly

E puts `_` in place of some characters, and `_` matches any character, so the built search would also find look-alikes: deck `a` U+00A0 `b` would give `deck:a_b`, which also finds `axb`. Decided 2026-10-27 (data safety first, as J3.1): the builder refuses such a name with `invalid_item{reason: unrepresentable_name}`, and the caller builds the exact item instead.

- **Deck by name n** (n is the full name with `::` between levels) is refused when a level of n, split at `::`, holds White_Space other than U+0020 or an ASCII control character (U+001C–U+001F included), is not in NFC, is empty, or starts or ends with White_Space or `:`. This is the list of `stats/browse-queries` Q2.2. The caller builds the deck-ids item with the deck's id and the ids of all its descendants, which finds that deck and its subdecks, as the deck term would (`names-and-ids.md` N3.5). A caller holding a deck one of whose stored levels holds `::` (which n cannot show, N3.3) builds the same deck-ids item without calling the name item.
- **Note type by name n** is refused when n holds White_Space other than U+0020 or a character U+001C–U+001F, or is not in NFC (`names-and-ids.md` N4.1 puts the value in NFC). The caller builds the note-type-id item (`mid:`).
- A negative id cannot be written (J3), so a deck or note type that needs these items and has a negative id gets no built search: the UI offers none for it, as for J3.3.

These callers are the sidebar, Home "Browse", Select Notes and the suggestions of `search/editing-services` D6.6. The deck-ids text does not follow a later rename; a built search is used at once.

## J4. Writing a built item

`build(item)` writes the item as a whole search with `canonical-form.md` F2 to F6. So a group at the top is written without its brackets (F2.2: `is:due -prop:due=0`), a group inside another is written in brackets (`(a OR b) c`), and every name is quoted and escaped by F4 and F5 after E.

## J5. Negation

Negating an item that is already a negation gives the item it negates; negating anything else gives its negation. So negating twice gives the item back, and negating "any flag" (NOT `flag:0`) gives `flag:0`.

## J6. Joining (R1079)

`join(existing, item, joiner, negate)`:

1. Read `existing` (J2, then `grammar.md`). An empty text is the whole collection (`query-model.md` Q2). A parse error is the result.
2. E is the existing search as one item: its only item when it has exactly one, otherwise a group of all its items.
3. N is the item, negated (J5) when `negate` is set.
4. The result is the sequence E, `joiner`, N, written by `canonical-form.md` F3 to F6. It has three items, so F2.2 does not apply: E is written in brackets whenever it is a group, whatever the joiner, and so is N.

So the existing side keeps its meaning without the writer looking at precedence: an existing search of one item is never bracketed (`-is:new` gives `-is:new -flag:0`), one of two or more items always is (`deck:French is:due` with a negated tag `verbs` gives `(deck:French is:due) -tag:verbs`), and an existing group stays one (`(x y)` gives `(x y) OR is:due`). An empty field gives `deck:* deck:Spanish`.

## J7. Replacing (R1079)

`replace(existing, item)`:

1. Read `existing` as J6 step 1.
2. If the item is one term (not a negation, not a group), every term of `existing` of the same kind, at any depth (inside groups and negations), is replaced by the item's term. Negations above a replaced term stay. If the item is a negation or a group, nothing is replaced.
3. The result is the query, changed or not, written as a whole search (`canonical-form.md` F2 to F6). With nothing replaced it is the canonical text of `existing`.

Kinds (each line is one kind; every term of `query-model.md` Q3 is in exactly one):

| Kind | Terms |
|---|---|
| plain text | plain text |
| field | every field term, whatever its field name and mode |
| tag | every `tag:` term, every mode |
| deck | every `deck:` term, `deck:*` and `deck:current` included |
| deck ids | `did:` |
| note type | `note:` |
| note type id | `mid:` |
| template | `card:`, by ordinal or by name |
| state | `is:`, every state |
| flag | `flag:` |
| answered | `rated:` and `resched:` together |
| added, edited, first review | `added:`; `edited:`; `introduced:` (three kinds) |
| ids | `nid:`; `cid:` (two kinds) |
| duplicates | `dupe:` |
| property | every `prop:` term, whatever its property (`prop:rated` and `prop:resched` included) |
| text terms | `re:`; `nc:`; `sc:`; `w:` (four kinds) |
| custom data | `has-cd:` |
| preset | `preset:` |
| whole collection | the whole-collection term |

Negated operator words are not terms and are never replaced. Consequences, all Anki's:

- An empty field holds only the whole-collection term, so replacing in it gives `deck:*` for any item but the whole collection. A caller that wants the item alone calls `build`.
- "Any flag", "untagged" and every negated item replace nothing; nor does a group such as a template item grouped with its note type.
- One `prop:` item replaces every property term: "due on day 0" put into `rated:2 prop:ivl>10` gives `rated:2 prop:due=0`.

## J8. Round trips

- **Built items other than parsed text** hold none of F7's four shapes (`canonical-form.md` F7): J3 makes no negated operator word, no `preset:` or `prop:cds:` item, no field term for a field named `has-cd` (J3.3), and a group of one item is no group. Every output made only of such items, and of an `existing` free of those shapes, is canonical text: writing its canonical text again gives it back.
- **Parsed text** (J3), wherever it stands in an item, and `existing` in `join` and `replace`, can hold any of F7's shapes. The output keeps the first three shapes' text as F7 writes them, and reading it again gives what F7's table says: `a -and b` joined with `c` gives `(a -  b) c`; `replace` with a parsed-text item `has\-cd:x` puts `has-cd:x` in place of every field term.
- **The fourth shape.** When the item `build` writes, or the query `replace` writes, is one group whose only item is again a group, the output loses one level of brackets (F2.2) and its canonical text loses the next one, as F7's fourth row says: `build` of parsed text `((a b))` and `replace(((a b)), deck X)` both give `(a b)`, whose canonical text is `a b`. `join` never gives this shape: its result has three items (J6 step 4).
- **Property tests.** U88's round-trip property tests (`tests/builder.rs`) run U80's checks over `build`, `join` and `replace` outputs o: canonical(o) = o, and o returns the same ids on the fixture collection as the items and `existing` it was made from. Like F7's tests, they exclude exactly the cases whose `existing` or parsed-text items hold one of F7's first three shapes, and for the fourth shape they check the meaning only (o parses and returns the same ids) and do not compare canonical(o) with o.

## J9. Examples

| Call | Result |
|---|---|
| build deck `French::Verbs` | `deck:French::Verbs` |
| build deck `My Deck` | `"deck:My Deck"` |
| build deck `a_b*` | `deck:a\_b\*` (it finds only that deck and its subdecks) |
| build deck `C++ (old)` | `"deck:C++ (old)"` |
| build deck `say "hi"` | `"deck:say \"hi\""` |
| build deck `x\y` | `deck:x\\y` |
| build tag `x y` | `"tag:x y"`, which matches no note (`names-and-ids.md` N2.2 step 3, R1051) |
| build note type `Basic (and reversed card)` | `"note:Basic (and reversed card)"` |
| build field present `Extra Info` | `"Extra Info:_*"` |
| build field present `my:field` | `my\:field:_*`; it does not find the field saved from `my:field`, whose stored name is `myfield` (J3) |
| build field present stored name `myfield` | `myfield:_*` |
| build literal `and`, `-x`, `3:30`, `50%_off` | `"and"`, `"-x"`, `3\:30`, `50%\_off` |
| build group AND (note type `Basic`, template ordinal 0) | `note:Basic card:1` |
| build group OR (state new, state learn) | `is:new OR is:learn` |
| build group AND (state due, negated due on day 0) | `is:due -prop:due=0` |
| build any flag; no flag; flag 1 | `-flag:0`; `flag:0`; `flag:1` |
| build added in 0 days; answered in 1 day with button 1; rescheduled in 0 days | `added:1`; `rated:1:1`; `resched:0` |
| build note ids 5, 3 | `nid:5,3` |
| build duplicates of `a\b "c"` in note type 1600000000000 | `"dupe:1600000000000,a\\b \"c\""` |
| join `deck:French is:due`, tag `verbs`, AND, negated | `(deck:French is:due) -tag:verbs` |
| join `deck:French`, tag `verbs`, OR | `deck:French OR tag:verbs` |
| join `a or b`, literal `c`, AND | `(a OR b) c` |
| join (empty), deck `Spanish`, AND | `deck:* deck:Spanish` |
| join `-is:new`, any flag, AND; the same negated | `-is:new -flag:0`; `-is:new flag:0` |
| join `a`, group AND (note type `Basic`, template ordinal 0), AND | `a (note:Basic card:1)` |
| replace `deck:French is:due`, deck `Spanish` | `deck:Spanish is:due` |
| replace `deck:A or (deck:B -deck:C)`, deck `D` | `deck:D OR (deck:D -deck:D)` |
| replace `is:new added:3`, state review | `is:review added:3` |
| replace `resched:1 is:due`, answered in 1 day with button 1 | `rated:1:1 is:due` |
| replace `front:x back:y`, field present `Notes` | `Notes:_* Notes:_*` |
| replace `deck:French`, any flag | `deck:French` |
| replace (empty), deck `X` | `deck:*` |
| replace `(a  b)`, deck `X` | `a b` |

AE43 (core part) is the first join and the first replace above. In 26.09.3 the join brackets the existing two-item search, as the golden of `search/join-replace/ae43` records; the plan's AE43 text without brackets predates this check.

## J10. Differences from Anki

Meguri writes Anki's text for every built item except these, where Anki's text would match other rows than the named one or split into other terms. Each is a deliberate deviation (R1078 over byte parity); a scenario that records one of these inputs needs its `conformance/deviations.yaml` entry.

| Input | Anki | Meguri |
|---|---|---|
| deck `current`, `filtered`; tag `none` | `deck:current`, `deck:filtered`, `tag:none` (special values) | `deck:Current`, `deck:Filtered`, `tag:None` (J3.1) |
| a deck, tag or note type named `_*`; a "field present" item for a field named `_*` | `deck:_*` (every deck); `_*:_*` (any field) | `deck:\_\*`; `\_\*:_*` (J3.1) |
| a deck or note type name that J3.5 lists (other White_Space, control characters, not NFC; for a deck, also empty levels and levels with outer White_Space or `:`) | the name as written; the search splits there, normalizes the name, or finds look-alikes | refused (J3.5); the caller builds `did:` or `mid:` |
| a tag, field name or text holding White_Space other than U+0020, or U+001C–U+001F | the character kept; the search splits there | `_` (J3), or J3.2 for tags |
| a field named like a reserved key | the reserved term (`deck:_*`) | refused (J3.3) |
| duplicates text holding such a character | the character kept | refused (J3.4) |
| an empty id list, a negative id, an empty name or literal text | text that fails to parse or searches something else | refused (J3) |
| 0 days for added, edited, first review, answered | `added:0` (read back as 1) | `added:1` (canonical at once) |

## J11. Scenarios

Both directories use the observation kinds `search_build`, `search_join` and `search_replace` (`conformance/search-ops` S6), each taking built items in the neutral JSON form of `conformance/search-ops` S5 (`{deck: "<name>"}`, `{tag: ...}`, `{flag: "any" | "none" | 1–7}`, `{group: {joiner, items}}`, `{negated: ...}`, `{text: "<search>"}` and so on) and returning the text or the parse error. The oracle maps each item to Anki's own structured search item. These J3 items have no structured counterpart in Anki and stay out of the oracle scenarios: note type by id (`mid:`), card-id and deck-id lists, and the states `buried-manually` and `buried-sibling`. U88's `tests/builder.rs` covers them from this text. Recorded at the current oracle pin:

| Scenario | Covers |
|---|---|
| `search/builder/names` | J3, J4: deck, tag, note type and field names with spaces, brackets, quotes, `\`, `*`, `_`, `:`, a leading `-`, `::` and non-ASCII; each built search also run with `search` on a collection holding the named object and look-alikes (`a_b*` next to `axb`), so the built text finds only the named one (R1078) |
| `search/builder/items` | J3: flags, the six states Anki's item has, day items, due items, note-id lists, duplicates, template ordinal, groups of one and of several items, nested groups, double negation |
| `search/join-replace/join` | J6: existing searches of one item, of several items, of one group, empty; AND and OR; negated items, negated groups; F7's shapes in `existing`; parse errors of `existing` |
| `search/join-replace/replace` | J7: every kind of the table, replacements at depth and under negation, negated and grouped items (nothing replaced), the empty field, `existing` with nothing of the kind |
| `search/join-replace/ae43` | AE43 (core part): the two calls of J9 |

J10's inputs stay out of these scenarios; Meguri's builder tests (U88 `tests/builder.rs`) cover them from this text. The same tests hold AE60's check: the joined text of `stats/browse-queries` Q3's AE60 row parses, and its canonical text (`canonical-form.md` F2–F6) equals the golden `(deck:Japanese) (is:review -is:learn) prop:ivl<21 -(is:buried OR is:suspended)`. They never write the Stats text through `build` or `join`. That golden is the `canonical_search` observation of `stats/browse-queries/parse-all`, which already pins AE60's text; U88 compares with it and adds no scenario.
