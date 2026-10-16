# Search errors

Spec: search/errors
Requirements: R1075, R1778, R1085
Anki: 26.09.3
Scenarios: conformance/scenarios/search/errors/, conformance/scenarios/search/canonical/spec-examples.yaml

The closed list of search error kinds, the details each carries, the order in which errors are found, the range each covers and the message Meguri shows. The grammar that raises them is `grammar.md`. The browser (U85) and the AnkiConnect server (U141, R1778) return the same message text.

## E1. Kinds

Parse errors (raised while reading the text, so also by canonicalization):

| Kind | Raised when (`grammar.md`) |
|---|---|
| `misplaced_and` | an `and` operator word at the start or end of a sequence or right after another operator word (G2) |
| `misplaced_or` | the same for `or` |
| `empty_group` | `(` and `)` with nothing between them (G5) |
| `unopened_group` | a `)` with no open group (G5) |
| `unclosed_group` | a group still open at the end of the text (G5) |
| `empty_quote` | `""` as a term or as a partly quoted value (G3) |
| `unclosed_quote` | a `"` with no closing `"` (G3) |
| `missing_key` | a term whose first unescaped character is `:` (G6) |
| `unknown_escape` | an escape that is not allowed (G3, G7) |
| `invalid_state` | an `is:` value that is not a state word (G8) |
| `invalid_flag` | a `flag:` value that is not 0–7 (G8) |
| `invalid_property` | a `prop:` value that starts with no property (G8.1) |
| `invalid_operator` | a `prop:` property followed by no operator (G8.1) |
| `invalid_number` | a float operand that does not parse (G8.1) |
| `invalid_whole_number` | a signed whole number that does not parse (G8, G8.1) |
| `invalid_positive_whole_number` | an unsigned whole number that does not parse (G8, G8.1); 0 is allowed |
| `invalid_negative_whole_number` | a whole number that does not parse or is above 0 (G8.1) |
| `invalid_answer_button` | an answer button that is not 1–4 (G8, G8.1) |
| `invalid_id_list` | a `did:`, `nid:` or `cid:` value that is not digits separated by single commas (G8) |
| `invalid_dupe` | a `dupe:` value whose id has no comma after it (G8) |
| `malformed` | Anki's generic fallback for input its parser cannot classify. No input is known to reach it at 26.09.3; Meguri need not produce it, but the observation schema accepts it so that a differential run can record it. |

Run errors (raised when a parsed search runs, never by canonicalization):

| Kind | Raised when |
|---|---|
| `invalid_regex` | a regular expression does not compile (E3) |
| `search_failed` | a search that parses cannot run (E3) |

## E2. Details

Each error carries its kind, its range (E5) and, for some kinds, two texts that Anki reports in its message. Both are part of the observation (`specs/conformance/search-ops.md`), so they must be exact:

| Kind | `provided` | `context` |
|---|---|---|
| `unknown_escape` | the escape: a backslash and the next character, a lone trailing backslash, or (backslash before a space) the backslash and the rest of the text (G3) | |
| `invalid_state` | the `is:` value as written | |
| `invalid_property` | the whole `prop:` value | |
| `invalid_operator` | the property as matched (`ivl`, `cdn:abc`) | |
| `invalid_number`, `invalid_whole_number`, `invalid_positive_whole_number`, `invalid_negative_whole_number`, `invalid_answer_button` | the number text that failed | the text G8 names: `added:`, `edited:`, `introduced:`, `rated:`, `resched:`, `mid:` (fixed lower-case texts), or the key's whole value (`dupe:`, a `rated:` button, every `prop:` operand) |
| `invalid_id_list` | | the key as typed, then `:` (`NID:`) |

Every other kind has neither text. The offending text of an error is `provided` when the kind has it, otherwise the text its range covers.

Examples: `prop:ivl>x` → `invalid_positive_whole_number`, `x`, `ivl>x`. `rated:1:5` → `invalid_answer_button`, `5`, `1:5`. `added:x` → `invalid_positive_whole_number`, `x`, `added:`. `prop:ivl~1` → `invalid_operator`, `ivl`. `nid:1,a` → `invalid_id_list`, context `nid:`.

## E3. Run errors

- **`invalid_regex`** (raised by U81's matchers; specified in `text-matching.md`). A pattern of `re:`, `<field>:re:`, `tag:re:` or `w:` that does not compile. Meguri compiles every pattern while it builds the plan, so the error comes before any result and whatever the collection holds. Anki compiles a pattern only when a row reaches its test, so on an empty collection, or when other terms rule out every row first, it returns results instead; U81 records that as a deviation. Note that an unquoted `re:(` is not a regex error: the `(` ends the term, so it is `unclosed_group`; the invalid regex is `"re:("` or `re:[`.
- **`search_failed`**. Anki raises a database error instead of a search error for these searches, and the oracle records that as `search_failed`. Meguri raises `search_failed` for the same searches, before it runs anything:
  - the search holds a negated operator word (`-and`, `-or`, any letter case; `grammar.md` G4);
  - an id list of `did:`, `nid:` or `cid:` holds a digit other than ASCII `0`–`9` (G8 accepts any Unicode decimal digit).
- **Database limits.** When the database refuses the statement Meguri built for a valid search (too deeply nested, too many parameters), the search fails with `search_failed`. Anki already fails when about 1000 terms are joined by AND or OR in one sequence, because its statement nests one level per operator. Meguri's statement may reach that limit at a different size, so no scenario sits at the boundary.
- Property operands that are infinite or not a number (`prop:s>inf`, `prop:cdn:k<nan`) also make Anki's statement fail. Whether Meguri fails them the same way is for `properties.md` (U82).

## E4. Order

The text is read once, from left to right, one item at a time (`grammar.md` G2). The first error met is the one reported; nothing after it is examined.

1. **An item's own errors** are met when the reading reaches the item:
   - a term's form errors (G3: `empty_quote`, `unclosed_quote`, a backslash before a space or at the end), then its interpretation errors (G6–G8): `missing_key`; for a field search, the field name's `unknown_escape` before the value's; then the value's error. Within one value, the checks run in the order G8 lists them: `rated` days before the button, the `dupe` id before the missing comma, the `prop` property, then operator, then operand;
   - an operator word at the start of a sequence or right after another operator word: `misplaced_and` or `misplaced_or` at once.
2. **The end of a sequence** (a `)`, or the end of the text): a trailing operator word is reported first (`misplaced_*`). Then, for a group: `empty_group` when its `)` is there and it is empty, `unclosed_group` when there is no `)` (so `(` alone is `unclosed_group`). For the whole search: `unopened_group` when a `)` ended it.

Consequences, all pinned by `conformance/scenarios/search/errors/order.yaml`:

| Input | Error |
|---|---|
| `( is:foo` | `invalid_state` (the term comes before the unclosed group) |
| `(a and)`, `(a or` | `misplaced_and`, `misplaced_or` |
| `a or )` | `misplaced_or` |
| `a ) (b` | `unopened_group` |
| `(()`, `()a`, `(a)()` | `empty_group` |
| `((a)` | `unclosed_group` |
| `a"b`, `"a\"` | `unclosed_quote` |
| `"""`, `a""`, `"" a` | `empty_quote` |
| `"x":y` | `missing_key` (the quoted term `x`, then the term `:y`) |
| `- OR` | `misplaced_or` (`-` is a plain term, then a trailing `or`) |

## E5. Ranges

Meguri's errors carry a range: a half-open interval of character offsets (Unicode scalar values) into the text as the caller gave it, before `grammar.md` G1. Anki has no ranges; they are Meguri's, and no golden holds them. The FFI layer converts them to UTF-16 offsets for Swift (U85, U88).

| Kind | Range |
|---|---|
| `misplaced_and`, `misplaced_or` | the operator word |
| `empty_group` | from the `(` through its `)` |
| `unopened_group` | the `)` |
| `unclosed_group` | the `(` of the group left open |
| `empty_quote` | the two `"` |
| `unclosed_quote` | from the opening `"` to the end of the text (after G1 trimming) |
| `unknown_escape` | the characters of `provided` where they stand in the text (a lone trailing backslash: that backslash) |
| every other parse kind | the whole term: its key, colon and value, quotes included, without a negating `-` |
| `invalid_regex` | the whole term holding the pattern |
| `search_failed` | the negated operator word with its `-`, or the id-list term; the whole text for a database refusal |

A range is never empty and always lies inside the text.

## E6. Depth and size

- Groups and negations nest to any depth (R1025). Reading, canonical writing and plan building never fail or crash because of depth or length: no recursion that input can exhaust, no panic on any input (the fuzz target in U80 checks this). Anki itself aborts at about 10 000 nested groups; Meguri need not reproduce a crash.
- Running a search is subject to the database's limits (E3).

## E7. Messages

Meguri's own wording, in plain English, naming the offending text (R1075). Implementers take these strings only from here.

- A message is `Invalid search: ` followed by the text below for its kind, character for character. The block lists one kind per line as `<kind>: <text>`; the text starts after the first `: `.
- `{provided}` and `{context}` stand for the E2 texts, with each backtick (U+0060) in them replaced by an apostrophe (U+0027) so that the code spans stay intact. `{pattern}` and `{reason}` are defined in `text-matching.md` (U81).
- Backticks mark code spans: the browser may render them as code; AnkiConnect returns the string as it is.
- `search_failed` has one text per cause of E3.

```text
misplaced_and: an `and` must stand between two search terms. To search for the word, put it in double quotes: `"and"`.
misplaced_or: an `or` must stand between two search terms. To search for the word, put it in double quotes: `"or"`.
empty_group: there is nothing to search for between `(` and `)`. To search for brackets, put them in double quotes: `"( )"`.
unopened_group: a `)` closes a group that was never opened. To search for `)`, put it in double quotes or write `\)`.
unclosed_group: a `(` opens a group that is never closed. To search for `(`, put it in double quotes or write `\(`.
empty_quote: there is nothing to search for between the double quotes `""`. To search for double quotes, write `\"\"`.
unclosed_quote: a double quote `"` is never closed. To search for `"`, write `\"`.
missing_key: a colon `:` needs a keyword before it. To search for `:`, write `\:`.
unknown_escape: the escape `{provided}` does not exist. To search for a backslash, write `\\`.
invalid_state: `is:` does not accept `{provided}`.
invalid_flag: `flag:` needs a flag number: 1 red, 2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple or 0 for no flag.
invalid_property: `prop:` does not accept `{provided}`.
invalid_operator: `prop:{provided}` needs one of the comparisons `=`, `!=`, `<`, `>`, `<=` or `>=`.
invalid_number: expected a number in `{context}`, found `{provided}`.
invalid_whole_number: expected a whole number in `{context}`, found `{provided}`.
invalid_positive_whole_number: expected a whole number of 0 or more in `{context}`, found `{provided}`.
invalid_negative_whole_number: expected a whole number of 0 or less in `{context}`, found `{provided}`.
invalid_answer_button: expected an answer button from 1 to 4 in `{context}`, found `{provided}`.
invalid_id_list: `{context}` takes only ids separated by commas.
invalid_dupe: `dupe:` needs a note type id, a comma and the text.
malformed: check the search for typing mistakes.
invalid_regex: the regular expression `{pattern}` is not valid: {reason}
search_failed (negated operator word): a `-` cannot stand directly before `and` or `or`. To search for the word, put it in double quotes: `-"and"`.
search_failed (id list): `{context}` takes ids written with the digits 0 to 9 only.
search_failed (database refusal): this search is too large to run. Split it into smaller searches.
```

For `search_failed (id list)`, `{context}` is the key as typed, then `:`.
