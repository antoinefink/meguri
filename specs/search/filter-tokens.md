# Filter tokens: Browse's search without syntax

Spec: search/filter-tokens
Requirements: R1078, R1080, R1084, R1092, R1102, R1104, R1105, R1106, R1107, R1110, R1111, R1114, R1115, R1158, R1165; supersedes in part R1076, R1077, R1086 to R1091, R1093 to R1101, R1103, R1108, R1109, R1112, R1159, R1168 to R1170 (the Browse and Note Types plan §5 lists the notes)
Anki: none (Meguri design, decided by Antoine 2026-11-20, round 8 option 2A; it reads and writes Anki search text by the 26.09.3 grammar)
Scenarios: none; tests in `core/crates/meguri-search/tests/` (T16)
Status: Approved 2026-11-20 (R1232). Approved by Antoine in chat (round 8, decided 2026-11-20, `docs/design/decisions.md` row `browser`), after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`). The lines that carry the marker "changed 2026-11-21" with "U88a review", in any letter case or punctuation, are in force: decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/builder-spec-gaps-20261121.md`). Every other section is in force and unchanged.

What Browse's search field holds and means (`docs/design/native-interface-contract.md` N21.2): plain words plus filter tokens, how they combine, how they become one Anki search that the existing search stack runs, how Anki search text typed, pasted or stored is read back into tokens, how the filter sidebar and the tokens stay in step, the suggestions, and the built-in saved searches. The grammar, the term meanings, the canonical writer and the builder stay `search/grammar`, `search/query-model`, `search/canonical-form` and `search/join-and-replace`; this file only says which built items a query makes. No syntax mode, no syntax reference and no canonical text are ever shown.

## T1. Terms

- **Query**: what the field holds: an ordered list of **tokens** and the **text** (the words the user typed after them). The query is the only state; the field, the sidebar's selected rows and the Filters sheet are views of it.
- **Kind**: what a token filters on (T3). Every token has one kind. **Value**: one thing of that kind (a deck, a tag, a state). A **value token** holds one or more values of its kind and an **is / is not** flag. An **advanced token** holds a piece of Anki search text that no kind can show (T6).
- **Built item** and `build`: `search/join-and-replace` J3 and J4. E(s) is J3's name escape.
- **Run text**: the Anki search text the query stands for (T4). It is what `browser/result-sets` H3 runs and what a saved search stores.

## T2. Combining (the whole rule)

- Two values in one token widen: the token is true when any of its values is (OR). "Tag: alpha or beta".
- Different tokens narrow: the query is true when every token is (AND), and the text too.
- A token set to **is not** is the negation of its whole OR: "Tag is not: alpha or beta" keeps rows with neither tag.
- One value token per kind: a second value of a kind is added to that kind's token, never as a second token (T9, T10). Field tokens are one kind per field name (T3), so "Back contains apple" and "Extra is empty" narrow; two tests of the same field widen.
- Advanced tokens narrow like other tokens; there may be several.
- The empty query (no token, no text) is the whole collection (`search/query-model` Q2).
- Order does not change the meaning. Tokens keep the order they were made in; the text always comes after them.

## T3. Kinds and values

| Kind (label) | Values (label: built item) | Sidebar section (T9) |
|---|---|---|
| Deck | a deck: deck by name with its full name, or J3.5's deck-ids item when J3 refuses the name (the deck and its subdecks either way); Current Deck: current deck | Decks |
| Tag | a tag: tag by name with its full name (it finds descendants, `search/names-and-ids` N2); Untagged: untagged | Tags |
| Note Type | a note type: note type by name, or by id when J3.5 refuses the name | Note Types |
| Card Type | a card type name s: parsed text `card:` + E(s) written by `canonical-form` F4 and F5 (it finds every card type of that name in any note type, `search/names-and-ids` N4.2); a name made only of ASCII digits would read as an ordinal and is not offered (as J3.3 refuses reserved names) | none |
| Card State | New `is:new`, Learning `is:learn`, Review `is:review`, Suspended `is:suspended`, Buried `is:buried` (J3 states); Leech: tag by name `leech` | Card State (the five states; Leech through the built-in saved search Leeches, T13) |
| Flag | Red to Purple, flags 1 to 7, labelled with their shown names (`tags/flags-and-marked` F4); No Flag `flag:0`; Any Flag (NOT `flag:0`) | Flags |
| Due | Today: due on day 0 (`prop:due=0`); Overdue: group AND (state due, NOT due on day 0), written `is:due -prop:due=0`; Next 7 Days: parsed text `prop:due>=1 prop:due<=7` | none |
| Added | Today `added:1`; Last 7 Days `added:7`; Last 30 Days `added:30` (J3 added) | none |
| Edited | Today `edited:1`; Last 7 Days `edited:7`; Last 30 Days `edited:30` | none |
| Studied | Today `rated:1`; Last 7 Days `rated:7`; Last 30 Days `rated:30` (answered in d days, any button) | none |
| Field f (one kind per field name; label "<f>") | contains t: the field contains item (T3.1); is empty: the field empty item (T3.1); has content: field present (`f:_*`) | none |
| Duplicates (made only by Find Duplicates, `browser/workspace` V10) | the note ids of the duplicate groups: J3 note ids | none |
| Studied On (made only by a heatmap day, `browser/workspace` V15; label "Studied on <date>") | the ids of the cards answered inside that heatmap day's bounds: J3 card ids | none |

- Day meanings are `search/history` H2 to H5 and `search/properties` P4. "Last 7 Days" is today and the six days before it.
- Labels are Meguri's (T15). A value whose built item J3 refuses (a negative id, a field name J3.3 refuses) is not offered.
- A flag value shows its colour; every other value shows text only.

### T3.1 Two new built items (amends `search/join-and-replace` J3)

| Item | Query item | Refused |
|---|---|---|
| field contains, field f, text t | field term, normal mode, name E(f), text `*` + E(t) + `*` | as field present; t empty |
| field empty, field f | field term, normal mode, name E(f), empty text (`f:`) | as field present |

J4 writes them like the other field items: `Back:*apple*`, `"Notes Field:*x*"`, `Extra:`. Their round trip is J8's first bullet.

## T4. The run text

1. Each value token gives one item: its value's built item when it has one value, otherwise the group OR of its values' items in their order. **is not** gives the negation of that item (J5).
2. Each advanced token gives the parsed-text item of its text (J3, `field_text` unset).
3. The text gives T5's items.
4. The query's item is the group AND of 1 to 3 in order: tokens in their order, then the text's items. The run text is `build` of that item (J4). The empty query gives the empty text, which runs the whole collection (`search/query-model` Q2). Changed 2026-11-21 (U88a review): so does any query for which 1 to 3 give no item (no token, and a text that T5 turns into no items, such as White_Space only); `build` is not called then, since a group of no items is refused (J3).

Examples:

| Query | Run text |
|---|---|
| Deck: Spanish | `deck:Spanish` |
| Deck: Spanish; Tag: alpha or beta; Card State is not: Suspended; text `word` | `deck:Spanish (tag:alpha OR tag:beta) -is:suspended word` |
| Tag is not: alpha or beta | `-(tag:alpha OR tag:beta)` |
| Due: Today or Overdue | `prop:due=0 OR (is:due -prop:due=0)` |
| Card State: Leech | `tag:leech` |
| Back: contains x; Extra: is empty | `Back:*x* Extra:` |
| advanced `prop:ivl>30`; text `two words` | `prop:ivl>30 two words` |

The run text is never shown. It is the text `browser/result-sets` H3 runs, and the text Save Search stores (T13).

## T5. The text

- The text is read by `search/grammar` after `search/join-and-replace` J2.
- When it parses and every item of its single AND-run (no OR at its top level) is a plain-text term, the negation of one, or a field term whose field name is not a field of any note type, the text stays text. Its items are: each plain-text term and each negation as read (Anki's plain-text meaning, wildcards included, `search/text-matching#T5`); each such field term as the literal item of its whole written text (J3, so `10:45` searches the characters `10:45`, not a field named `10`).
- Otherwise the text holds Anki syntax and T7 reads it when it is committed (T8); until then the live search runs T7's result without showing it as tokens.
- When the text does not parse, each run of non-White_Space characters is a literal item (J3) in its order. No error is shown (R1108 is superseded for the field: Meguri never shows a syntax error, `docs/design/native-interface-contract.md` N21.2).
- Empty text, or text of White_Space only, gives no items.

## T6. Advanced tokens

- An advanced token shows its text in a grey token, as written: the canonical text of its item (`search/canonical-form` F2 to F6). It is made only by T7.
- Clicking it puts its text back into the field for editing, in place of the token; committing (T8) reads the field again by T7, so the edit can give value tokens, advanced tokens or text.
- Its menu (T10) holds Edit and Remove Filter only.

## T7. Reading Anki search text into a query

Input: a text (typed and committed, pasted, a saved search's stored text, `defaultSearchText`, a search another screen hands to Browse, R1114). Output: a query. It never fails.

1. Apply J2, then read the text by `search/grammar`. On a parse error, the result is the text alone, treated by T5's last rule but one (literal words). An empty text, `deck:*` alone, or the whole-collection term gives the empty query.
2. If the top-level sequence has more than one AND-run (an OR at the top level), the whole text is read as one group OR and classified by step 3's rows (so the run text of a single OR token, which J4 writes without brackets, gives that token back). Only when no row but the last fits is the whole text one advanced token.
3. Otherwise take the items of the AND-run in order and classify each by the first row that fits:

Items are compared as follows. A `deck:`, `tag:` or `note:` term in normal mode with no wildcard is a value item when its name names exactly one existing deck, tag or note type by the equality `search/names-and-ids` N3.3, N2.2 and N4.1 use (letter case folded), so `deck:spanish` gives Deck: Spanish; its value is that object. Every other item is compared by its canonical text (`search/canonical-form` F2 to F6), so `Is:New` and `is:new` are the same item. A deck value is labelled with its last name component; its help tag and accessibility label give the path joined with " › ". A value's item may be a group (Overdue, Next 7 Days); at the top level a group's brackets are not written (J4), so first, in the AND-run, two adjacent items `is:due` and `-prop:due=0`, and two adjacent items `prop:due>=1` and `prop:due<=7`, in that order, are each taken as one item (the group of the two).

| Item | Becomes |
|---|---|
| an item equal to exactly one T3 value's item (`card:` with a name included); `tag:leech` alone | a value token of that kind with that value; `tag:leech` alone gives Card State: Leech |
| NOT of such an item | the same token, is not |
| a group OR whose items are all such items of one kind | one value token with those values in their order |
| NOT of such a group | the same token, is not |
| a group OR of Tag value items that holds `tag:leech` | a Tag token (leech stays a tag here) |
| a group OR of Card State value items and `tag:leech` | a Card State token, Leech among its values |
| a plain-text term, the negation of one, or a field term naming no field of any note type | text, as T5 |
| anything else (any other `prop:`, `re:`, `nc:`, `w:`, `sc:`, `dupe:`, ids, `preset:`, `rated:N:B`, other day counts, deeper groups, field terms in other modes) | an advanced token of that item |

4. A value token whose kind already has a token (an AND of two decks, say) becomes an advanced token instead, so meaning never changes.
5. Text items are joined with one space in their order as the query's text; tokens keep the order of their first item.
6. `nid:` lists become advanced tokens, so a Duplicates token's run text reads back as an advanced token (T16 lists it as a round-trip exception).

Examples:

| Text | Query |
|---|---|
| `deck:"Lang::Spanish" -is:suspended tag:alpha` | Deck: Spanish; Card State is not: Suspended; Tag: alpha |
| `tag:alpha or tag:beta word` | one advanced token `tag:alpha OR tag:beta word` (top-level OR, step 2) |
| `(tag:alpha or tag:beta) word` | Tag: alpha or beta; text `word` |
| `deck:A deck:B` | Deck: A; advanced `deck:B` (step 4) |
| `prop:ivl>30 is:review` | advanced `prop:ivl>30`; Card State: Review |
| `"deck:a` (unclosed quote) | text, literal word `"deck:a` |

### T7.1 Round trip

For every query the UI can make from T3 values and text that stays text (T5): reading its run text by T7 gives back the same tokens, values, flags and order, and text that runs the same (T16). The exceptions: a Duplicates or Studied On token (it comes back advanced); a Deck or Note Type value written by id (J3.5; it comes back advanced); a text that holds Anki syntax (it is converted on commit); a Tag token holding only `leech` (it comes back as Card State: Leech, which finds the same rows); a value whose object was renamed or deleted since the search was saved (it comes back advanced).

## T8. When the field changes the query

- **Live** (R1106): every edit of the text runs the query at once (debounce at most 60 ms, a newer run cancels the older, `browser/result-sets` H3). Return is never needed.
- **Incomplete text** (R1107): while the user is typing (before a commit) and `search/editing-services` D5 classifies the text at the cursor as incomplete (a key with no value, an unclosed quote or bracket, a dangling `-`, `and` or `or`), the last results stay and nothing changes.
- **Commit**: Return, a paste, accepting a suggestion, or the field losing focus. A commit reads the text by T7 and puts its tokens after the tokens already in the query, before the remaining text; text that stays text stays as typed. T7 step 4 counts the query's existing tokens too: with Deck: Spanish in the query, committing `deck:French` gives the advanced token `deck:French` (both must hold, as typed), never a second Deck token and never a widened one. To widen a token, the user adds a value from its menu or the suggestions (T10, T12), which add to the existing token. After a commit, text T7 turned into literal words (it did not parse) runs as those literal words; D5's incomplete rule no longer applies to it.
- A sidebar click, a token menu action, a Filters sheet change and an entry point (R1114) change the query directly and run it.

## T9. The sidebar and the tokens

Browse's sidebar (`browser/sidebar`, changed by `browser/workspace` V6) lists values of the kinds with a sidebar section (T3). Its rows are a view of the query:

| Input on a row of kind K, value v (Mac, iPad with a pointer) | Touch (iPad, the Duo opened) | Result |
|---|---|---|
| click, Return | tap | K's token becomes is {v}; every other token and the text stay |
| ⌘-click | | in K's token, v is added when absent and removed when present (a token left with no value is removed); with no K token, is {v} is made; the token's is / is not stays |
| ⌥-click | | K's token becomes is not {v}; on No Flag it becomes is {Any Flag} and on Any Flag is {No Flag} (T10's normal form, so the token reads back the same after a relaunch); on Untagged, which has no opposite value, it acts as a click |
| click on a saved search | tap | the whole query is replaced by T7's reading of its text |

- A row shows selected exactly when its value is in K's token and the token is **is**. Rows of an **is not** token are not selected; the token in the field says "is not". Removing a token deselects its rows; the clear button empties the query.
- A deck row stands for that deck with its subdecks; a tag row for that tag with its descendants (T3).
- A click never adds a second token of a kind (T2).
- Section headers fold their section only.
- Replaces `browser/sidebar` S7 and S8 (the Search and Select tools, the join and replace inputs, the multi-selection searches) for Browse: Browse's sidebar has no tools and no multi-selection (R1168 to R1170 superseded).

## T10. Editing tokens

- **Token menu** (a click on a value token; on touch a tap): Is and Is Not (one checked; Is Not is disabled while the token holds No Flag, Any Flag or Untagged, which are already negative or positive forms of one another: `flag:0` is read as No Flag, `-flag:0` as Any Flag, `-tag:_*` as Untagged, and "Flag is not: No Flag" is offered as Any Flag); the token's values, each checked, then the other values the kind offers in the suggestion order (T12) up to 8, then "Add Another <kind>…" (it opens the suggestions scoped to that kind); then Remove Filter (⌫). Checking a value adds it, unchecking removes it; unchecking the last removes the token. A Field token lists its tests instead of values ("Contains…", "Is Empty", "Has Content"); "Contains…" edits its text in place.
- **Keyboard** (Mac, iPad with a keyboard): ⌘F focuses the field and selects its text (`docs/design/native-interface-contract.md` N21.8); ← and → move over tokens as single units; ⌫ with the caret before the first character of the text selects the last token, a second ⌫ removes it; ⌘A then ⌫ empties the query; ⌘C copies the selected tokens and text as their run text (T4); ↓ moves into the suggestions; Return or Tab accepts the highlighted suggestion; Esc closes the suggestions, a second Esc empties the query (R1092).
- **Clear button** (the field's ✕): empties the query.
- No token edit asks anything; none is an undo step (the query is not collection data). Edit › Undo is the collection's, never the field's, except that a focused text field keeps its own text undo (N20.4).

## T11. Cards and Notes

The query is the same in both modes (R1080, `search/query-model` Q4): switching mode (`browser/workspace` V2.1) keeps every token and the text and runs the run text in the other mode. Counts (T12.3) count rows of the current mode.

## T12. Suggestions

The list under the field while it has focus (Mac, iPad: under the field; iPhone and the folded Duo: in place of the list, N21.10). The core computes it; Swift only shows it (as `search/editing-services` D6, whose name index, D6.7, it reads).

### T12.1 Sources

| Field state | Rows, in order |
|---|---|
| empty query, or empty text with the caret after the tokens | Saved Searches: every built-in (T13) then every user saved search (`browser/config-keys` K7.1, `browser/sidebar` S3.1's order); then Filter By: Deck, Tag, Note Type, Card Type, Card State, Flag, Due, Added, Edited, Studied, Field, each opening the list of that kind's values |
| text w (the word at the caret, T12.2) | first "Contains “w”" (it keeps w as text and commits); then, each under its section label: Saved Searches whose name matches; Card State, Flag, Due, Added, Edited, Studied values whose label matches; Decks, Tags, Note Types, Card Types and Fields whose name matches |
| a kind's value list (from Filter By, "Add Another…", or the Filters sheet) | that kind's values: fixed values in T3's order, then names in D6.5 step 3's order |

- A Field suggestion is a field name, with its note type as detail ("in My Type", or "in 3 note types"); accepting it shows the three tests, and "Contains…" takes w as its text when w is not the field's name.
- A Card Type suggestion is a card type name with the note types that have it as detail.
- Values already in an **is** token of their kind are not suggested again.

### T12.2 The word and matching

- w is the run of characters from the last White_Space before the caret to the caret, in the text part; with the caret right after White_Space or at the start of the text, w is empty. When the text holds Anki syntax at the caret (`search/editing-services` D6.2 finds a key before a colon), the suggestions are D6's for that term instead, so typed Anki syntax still completes; this is the only place D6's keyword rows appear, and they are never shown for an empty word.
- Names (decks, tags, note types, card types, fields, saved searches) match by D6.4: decks and tags by component (prefix and inner classes), the others by simple case-folded substring. Fixed labels (states, flags, day values, Leech) match by simple case-folded substring of the label, in the prefix class when at its start.

### T12.3 Order, limits and counts

1. "Contains “w”" first.
2. Then every prefix-class candidate before every inner-class one; within a class, by section in this order: Saved Searches, Card State, Flag, Due, Added, Edited, Studied, Decks, Tags, Note Types, Card Types, Fields; within a section, recently accepted first (D6.5 step 4's rule, newest first), then D6.5 step 3's name order or T3's fixed order.
3. Rows are shown under their section labels in the order their first row appears. At most 5 rows per section and 50 rows in all (R1104); a section cut short ends with "Show All <section>", which opens that kind's value list.
4. **Counts**: each value row (and each sidebar row, `browser/workspace` V6) shows the number of rows of the current mode that its value alone finds in the whole collection: cards in Cards, notes in Notes. They come from the core's filter counts read (T12.4) and are blank until known; a suggestion never waits for its count.
5. Timing (R1104): the list for a keystroke is returned within one frame (U88's budget, D6.7: 50 candidates in under 4 ms with 10,000 tags), counts excluded.

### T12.4 Filter counts (core)

- `filter_counts(mode, values)` returns, for each listed value, the number of rows of `mode` its built item finds (`search/query-model` Q4), or none when it could not be counted. It runs off the main thread, at lower priority than a search, and is cancelled by a newer call.
- Decks, tags, note types, states and flags are counted with one grouped read per kind over one snapshot (a deck's count includes its subdecks; a tag's its descendants). Day values and saved searches run their run text as a count-only search (changed 2026-11-21, U88a review: a saved search's run text is T4's run text of T7's reading of its stored text, so a saved `deck:*` counts what clicking it shows).
- Results are cached by (value, mode, change seq); a change set that touches cards, notes, decks or tags drops the affected entries (`engine/change-stream`). Budget: every count of the sidebar and the first suggestion screen within 200 ms at 150,000 cards (`browser/workspace` V17).

## T13. Saved searches

- **Built-in saved searches**, always listed first, in this order, never stored in the collection: Due Today (Due: Today), Added in Last 7 Days (Added: Last 7 Days), Leeches (Card State: Leech). They cannot be renamed or deleted in this version. A user saved search of the same name is listed after them under its own name.
- **Save Search…** (the last row of the suggestions while the query is not empty, and the sidebar section's context menu): asks for a name, then `browser/config-keys` K7.2 Save Search with the query's run text (T4). An existing name asks to replace it (Replace, Cancel). The step is "Save Search" (K10.2).
- **Rename… and Delete** of a user saved search are in its sidebar row's context menu (K7.2; `browser/sidebar` S9.4). Update to Current Search is not offered in this version.
- A saved search stores Anki text, so other Anki clients run it; reading it back is T7 (a search saved in another client becomes tokens where it can).
- Replaces: R1159's Today section (the three built-ins take its place), R1101's insertion of a saved search as a bracketed group (choosing one replaces the query, T9).

## T14. Opening Browse

- **First open on a device** (the first time Browse opens after the app is installed on this device, or after the collection is replaced): the query is T7's reading of `defaultSearchText` (`browser/config-keys` K6.4) when it is set, otherwise the empty query (the whole collection). R1112's `deck:current` is superseded.
- **Later opens**: the query, mode, selection and scroll Browse had when the user left it on this device, while the app runs (`browser/workspace` V1).
- **After a relaunch**: the query's run text (T4) and the mode are kept on the device and read back by T7; selection and scroll start at the top. `defaultSearchText` is not read again. A restored query that finds nothing shows the empty state with Clear Search (`browser/entry-points` E2's fallback to `deck:current` does not apply to Browse).
- **With a search** (R1114, `browser/entry-points` E3): T7's reading of the given search replaces the query. Stats' joined text (`stats/browse-queries` Q3) is read by T7 too; it usually gives advanced tokens.

## T15. Strings

| Text | Where |
|---|---|
| Search | the field's prompt |
| Contains “%@” | the first suggestion; %@ the word |
| Saved Searches, Filter By | suggestion section labels |
| Deck, Tag, Note Type, Card Type, Card State, Flag, Due, Added, Edited, Studied, Field | kind labels; also section labels of the suggestions (Decks, Tags, Note Types, Card Types, Fields for name sections) |
| Current Deck, Untagged, New, Learning, Review, Suspended, Buried, Leech, No Flag, Any Flag, Today, Overdue, Next 7 Days, Last 7 Days, Last 30 Days | value labels |
| Contains…, Is Empty, Has Content, contains %@, is empty, has content | Field token tests (menu, then token text) |
| is not | after a negated token's kind ("Tag is not") |
| or | between a token's values ("alpha or beta") |
| Is, Is Not, Add Another %@…, Remove Filter, Edit | token menu; %@ the kind's label in lower case ("Add Another tag…") |
| Show All %@ | a cut-short section; %@ the section label |
| Save Search…, Due Today, Added in Last 7 Days, Leeches | saved searches |
| Save Search, Name, Save, Cancel, A saved search named “%@” already exists. Replace it?, Replace | the Save Search sheet and its question |
| in %@, in %lld note types | Field and Card Type suggestion detail (plural) |

## T16. Tests

`core/crates/meguri-search/tests/filter_tokens.rs` (core: T4, T5, T7, T12) and Swift model tests (T8 to T10):

| Case | Required result |
|---|---|
| T4's table | the run texts exactly |
| T7's table; a text with a top-level OR; `deck:*`; empty | the queries exactly |
| The empty query, a query whose text is White_Space only, and the text `deck:*` read by T7, in Notes on a collection holding a note without cards (changed 2026-11-21, U88a review) | each runs the empty text (T4 step 4, T7 step 1), and the result holds that note (`search/join-and-replace` J8) |
| Property: random queries of T3 values (every kind, is and is not, one to four values, Unicode names, names J3.5 refuses) and plain text | T7.1: same query back, except the listed exceptions; and the run text returns the same ids as the query read by `search/query-model` on the fixture collection |
| Texts `10:45`, `a-b`, `-xyz`, `a_b`, a quoted pair of words, and `deck:` with nothing after it | T5: one literal item; a plain item; a negated plain item; a plain item with a wildcard; one quoted item; the last results kept |
| Sidebar: click, ⌘-click twice, ⌥-click, saved search click | T9's results; selected rows follow the tokens |
| Run texts of one OR token (Tag: alpha or beta; Due: Today or Overdue; two sidebar values by ⌘-click) read back by T7 | the same token (step 2) |
| `flag:0`, `-flag:0`, `-tag:_*`, `deck:spanish` for a deck Spanish, `tag:Alpha` for a tag alpha | No Flag; Any Flag; Untagged; Deck: Spanish; Tag: alpha |
| Token menu: uncheck the last value; Is Not; Add Another | the token removed; negated; scoped suggestions |
| Suggestions: empty field; `ch` with a deck Physics, tags part-1 and part-2, a field Part; 10,000 tags | T12's order and limits; under 4 ms for 50 rows |
| `filter_counts` on the 150k fixture | T12.4's budget; counts equal `search_ids` counts of each value's run text |
