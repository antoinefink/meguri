# Search grammar

Spec: search/grammar
Requirements: R1021, R1022, R1023, R1024, R1025, R1026, R1027, R1028, R1029, R1030, R1031, R1032
Anki: 26.09.3
Scenarios: conformance/scenarios/search/lexing/, conformance/scenarios/search/structure/, conformance/scenarios/search/canonical/spec-examples.yaml

How search text becomes a query: whitespace, items, terms, keys and values. What each term means as data is `query-model.md`, the errors named here (in `code` font, such as `unclosed_quote`) are `errors.md`, and the text written back is `canonical-form.md`. Every rule here was checked against the oracle; the scenarios pin the examples.

## G1. Whitespace

1. Before anything else, every character with the Unicode White_Space property becomes U+0020 SPACE. This happens everywhere in the text: between terms, inside double quotes and inside raw values. The set has 25 code points: U+0009–U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000–U+200A, U+2028, U+2029, U+202F, U+205F and U+3000 (what the `regex` crate's `\s` matches).
2. Leading and trailing spaces are then removed.
3. If nothing remains, the search is the whole collection (`query-model.md` Q2). Its canonical text is `deck:*`.

- Characters outside the set are ordinary term characters, among them U+200B ZERO WIDTH SPACE, U+FEFF, U+180E and the separators U+001C–U+001F (U+001F, the field separator, included): `a<U+001F>b` is one term.
- So a term can never hold a tab, a line break or a no-break space: `"a<U+00A0>b"` is the same search as `"a b"`, and `re:a<TAB>b` is `re:a` AND `b`.
- Step 1 replaces one character by one character, so offsets after it are offsets in the input. Step 2 shifts them by the number of leading spaces removed (`errors.md` E5).
- Anki's desktop search field also turns U+001C–U+001F into spaces before it calls the parser. That is a rule of the search field (U85, U88), not of this grammar.

Every later rule reads the text after G1. "Space" below means U+0020.

## G2. Items and sequences

The search, and the inside of every group, is a sequence of items. Spaces between items are optional wherever the boundary is clear: `p"q"(r)` is the three items `p`, `"q"` and `(r)`, and `(x)(y)` is two groups.

An item is one of:

| Item | Form |
|---|---|
| operator word | an unquoted term (G3) whose whole text is `and` or `or` in any ASCII letter case: `and`, `AND`, `aNd`, `Or` |
| group | `(`, a sequence, `)` (G5) |
| negation | `-` followed at once by a group or a term (G4) |
| term | G3 |

Sequence rules:

- Two neighbouring items that are not operator words are joined by an implicit AND.
- An operator word must stand between two other items. One at the start of a sequence, at its end, or right after another operator word is an error: `misplaced_and` for `and`, `misplaced_or` for `or`. For two operator words in a row, the second one is the misplaced one: `a or and b` gives `misplaced_and`, `a and or b` gives `misplaced_or`.
- AND binds tighter than OR: `a b or c d` means (a AND b) OR (c AND d), and `x or y and z` means x OR (y AND z). An explicit `and` means the same as the implicit AND.
- There is no NOT keyword: `not` is a plain word.
- Quoted, escaped or longer words are never operator words: `"and"`, `"OR"`, `and:x`, `andy` and `-and` (G4) are not.

## G3. Terms

A term is one of three forms. At a position where an item can start, the forms are tried in this order and the first that applies is used.

1. **Quoted term.** `"`, then the inside, then `"`. Inside, `\` followed by any character (a space or `"` included) is kept as those two characters and does not end the term; any other character except `"` is kept. The first `"` not taken by a backslash ends the term.
   - `""` (nothing inside) is `empty_quote`.
   - No closing `"` before the end of the text is `unclosed_quote`. So is a text whose last character is the backslash of a pending escape: `"abc\`.
   - The term's text is the inside, still escaped: `"a\"b"` has the text `a\"b`.
2. **Partly quoted term.** A key, `:`, then a quoted term exactly as in form 1, with nothing between the key, the colon and the opening quote. The key is one or more characters other than `"`, `(`, `)`, `:`, space and `\`, where `\` followed by any character but a space is also allowed (as two characters). The quote must come right after the colon.
   - The key is the part before the colon and the value is the inside of the quotes, both still escaped. `key:"value with spaces"` is the key `key` with the value `value with spaces`.
   - `key:""` is `empty_quote` and `key:"abc` is `unclosed_quote`.
   - When the text after the colon does not start with `"`, the form does not apply and form 3 is tried: `w:3:30` is an unquoted term.
3. **Unquoted term.** A run of characters that ends before a space, an unescaped `"`, `(` or `)`, or the end of the text. A backslash takes the next character into the term whatever it is (`a\"b`, `\(`, `a\)` stay in one term), except a space.
   - A backslash followed by a space is `unknown_escape`. The offending text is the backslash and everything after it up to the end of the text (G1 applied): `foo\ bar baz` reports `\ bar baz`.
   - A backslash as the last character of the text is `unknown_escape` with the offending text `\` (`abc\`).
   - Spaces need not separate terms from quotes and brackets: `a"b"` is `a` then `"b"`, `"k l m"no` is `"k l m"` then `no`, `a(b)` is `a` then the group `(b)`.

At a position where none of the three forms and no group can start, the reading of the sequence stops there. That position holds `)` or is the end of the text (G5).

A term is then interpreted from its text (G6). Partly quoted terms skip the key search of G6 step 1: their key and value are already split.

## G4. Negation

- `-` followed immediately (no space) by a group or a term negates that one item: `-a b` is (NOT a) AND b; `-(a or b)` negates the group; `-"x y"` and `-deck:a` negate the quoted term and the key term.
- The negated item may itself start with `-`, which is then part of its text: `--a` is NOT (the plain text `-a`), and `-\-` is NOT (the plain text `-`).
- A `-` that no group or term follows directly (a space, `)` or the end comes next) is not a negation. It starts the plain term `-`: `- a` is `-` AND `a`; `-` alone searches for `-`; `-)` is the term `-` followed by a stray `)` (`unopened_group`).
- Inside a term, a hyphen is an ordinary character: `a-b` is one term. A term whose text must begin with `-` is written `\-term` or `"-term"` (R1030).
- **Negated operator word.** `-` directly followed by an operator word (`-and`, `-OR`) is accepted and negates the operator word itself. It has a canonical text (`canonical-form.md` F3), but running such a search fails with `search_failed` (`errors.md` E3). It joins its neighbours like any other item: `a -and b` is three items.

## G5. Groups

- `(` opens a group wherever an item can start. The group's sequence follows, and the next `)` that ends the sequence closes it. Groups nest to any depth (R1025); `errors.md` E6 covers depth.
- An empty sequence between the brackets (`()`, `( )`) is `empty_group`.
- A group whose sequence reaches the end of the text without `)` is `unclosed_group`, raised for the outermost group left open: in `(a (b)` the first `(`.
- A `)` met where no group is open is `unopened_group`: `)`, `a ) b`, `(a) b)`.
- Before a group is closed, and before a stray `)` or the end of the text is reported, the sequence's last item is checked: a trailing operator word is reported first (`(a or)`, `(a or` and `a or )` are all `misplaced_or`).
- `errors.md` E4 gives the full order in which errors are found.

## G6. Term text: plain text or key and value

Steps for a quoted or unquoted term's text:

1. Find the first colon that no backslash escapes. Read from the left: a backslash and the character after it form one unit, so `\:` is skipped; every other character is a unit of its own.
2. No such colon: the term is plain text (`query-model.md` Q4), whose text is the term's text unescaped (G7, kind U). `3\:30` is the plain text `3:30`.
3. The colon is the first character: `missing_key` (`:x`, `:`, `::`, `":foo"`).
4. Otherwise the key is the part before the colon and the value is everything after it, more colons included: `w:3:30` has the key `w` and the value `3:30`; `rid:1:2` has the key `rid` and the value `1:2`. Colons after the first need no escaping (R1029).

Quoting does not stop the key search: `"deck:a b"` is a deck term with the value `a b`, and `"a:b c"` is a field search. A colon that must be text is written `\:` (`"a\:b c"` is plain text).

Keys:

- A key is reserved when its text, with ASCII letters lowered, is one of: `deck`, `note`, `tag`, `card`, `flag`, `resched`, `prop`, `added`, `edited`, `introduced`, `rated`, `is`, `did`, `mid`, `nid`, `cid`, `re`, `nc`, `sc`, `w`, `dupe`, `has-cd`, `preset`. So `DECK:`, `Tag:` and `Has-CD:` are reserved keys (R1031). Only ASCII letters are lowered.
- The comparison uses the key as typed, before unescaping. `has\-cd:x` is therefore not reserved: it is a field search on the field named `has-cd`. No other reserved key can be reached through an escape.
- Any other key is a field search on the field of that name (R1032), never an error: `rid:1:2` searches the field `rid` for `1:2`; `and:x` and `or:x` search the fields `and` and `or`.

The values of reserved keys are read by G8. The values themselves are case-sensitive where G8 compares words (R1031): `is:NEW` is `invalid_state`.

## G7. Escapes and value kinds

Each value (and each field name) is read as one of these kinds:

| Kind | Reading |
|---|---|
| U (unescaped) | Check first: a backslash that is not itself escaped, followed by a character other than `\`, `"`, `:`, `*`, `_`, `(`, `)`, `-`, or by nothing, is `unknown_escape`; the offending text is that backslash and the character after it (the first such pair from the left). Then `\"`, `\:`, `\(`, `\)` and `\-` become the bare character, while `\\`, `\*` and `\_` stay as the two characters (they are wildcard escapes that `text-matching` reads later). |
| Q (quotes only) | Every `\"` becomes `"`. Nothing else changes and nothing is an error: `re:\d` keeps `\d`, `re:a\x` keeps `a\x`. |
| D (dupe text) | Every `\"` becomes `"`, then every `\\` becomes `\`, each replacement left to right without overlaps. Nothing is an error. |
| R (raw) | Kept exactly as written, escapes included. Nothing is an error. |

- "Not itself escaped" counts backslashes: a run of an odd number of backslashes ends with an escaping one; `\\x` is a literal backslash then `x` (valid), `\\\x` is invalid (`\x`).
- The valid escapes of plain text are therefore exactly `\\`, `\"`, `\:`, `\(`, `\)`, `\-`, `\*` and `\_` (R1028). `\x`, `\%`, `\f`, a backslash before a space and a trailing backslash are errors; the U check reports `\x` for `\x\y`, `\f` for `\fern` and `\x` for `a\\\x`.
- Raw kinds still go through the term rules of G3, so `re:a\ b` is `unknown_escape` (`\ b`) and an unquoted `re:a\` is `unknown_escape` (`\`).
- The field-name kind is U; the U check runs on the field name before the value.

## G8. Values of reserved keys

"Unsigned", "signed" and "float" are G9's number forms. Contexts are the texts errors report (`errors.md` E2).

| Key | Value | Result | Errors (`provided` / `context`) |
|---|---|---|---|
| `deck` | U | deck pattern | |
| `note` | U | note type pattern | |
| `tag` | value starts with `re:`: Q of the rest; with `nc:`: U of the rest; otherwise U of the value | tag pattern in regex, nc or normal mode | |
| `card` | an unsigned 16-bit number: template ordinal; anything else: U | ordinal: the number minus 1, but 0 when the number is 0 (`card:0` and `card:1` are the first template); name otherwise (`card:65536`, `card:-1`, `card:` are names) | |
| `flag` | unsigned 8-bit, at most 7 | flag 0–7 | `invalid_flag` (no details) |
| `resched` | unsigned 32-bit | days, kept as given (0 allowed) | `invalid_positive_whole_number`: value / `resched:` |
| `added`, `edited`, `introduced` | unsigned 32-bit | days, 0 raised to 1 | `invalid_positive_whole_number`: value / `added:`, `edited:` or `introduced:` |
| `rated` | split at the first `:`: days, then an optional button | days unsigned 32-bit, 0 raised to 1; button unsigned 8-bit in 1–4 | days: `invalid_positive_whole_number`: days text / `rated:`; button: `invalid_answer_button`: button text / the whole value (`rated:1:5` gives `5` / `1:5`; `rated:1:2:3` gives `2:3` / `1:2:3`) |
| `prop` | G8.1 | property comparison | G8.1 |
| `is` | R, compared exactly with `new`, `review`, `learn`, `due`, `buried`, `buried-manually`, `buried-sibling`, `suspended` | card state | `invalid_state`: the value as written (`is:NEW` gives `NEW`, `is:` gives empty, `is:ne\w` gives `ne\w`) |
| `did`, `nid`, `cid` | R; one or more decimal-digit runs separated by single commas, nothing else | id list text, kept as written | `invalid_id_list`: context is the key as typed plus `:` (`NID:1,a` gives `NID:`) |
| `mid` | signed 64-bit | note type id | `invalid_whole_number`: value / `mid:` |
| `re` | Q | regex over all fields | |
| `nc`, `sc`, `w` | U | text | |
| `dupe` | split at the first `,`: note type id, then the text | id signed 64-bit; text D | id: `invalid_whole_number`: the id text / the whole value (`dupe:` gives empty / empty, `dupe:x,a` gives `x` / `x,a`); no comma: `invalid_dupe` |
| `has-cd` | U | custom data key | |
| `preset` | R | preset name, escapes kept | |
| any other key (field search) | field name U of the key; value starts with `re:`: Q of the rest; with `nc:`: Q of the rest; otherwise U of the value | field term in regex, nc or normal mode | |

- Mode prefixes are lower-case and checked on the value as written: `f:RE:x` is a normal field search for `RE:x`; `tag:re\:x` and `f:re\:x` are normal searches for `re:x`.
- In an id list, a "decimal digit" is any Unicode character of category Nd, not only `0`–`9`: `nid:١٢` (Arabic-Indic digits) is accepted here and fails when run (`errors.md` E3). Leading zeros and digits beyond the 64-bit range are accepted (`nid:007`).
- Reading `card`, `flag` and the others as numbers uses the value as written, before any unescaping: `flag:+1` is flag 1, `card:+1` and `card:01` are the first template.
- An empty value is allowed wherever the table gives no error for it: `deck:`, `tag:`, `card:`, `note:`, `re:`, `w:`, `nc:`, `sc:`, `has-cd:`, `preset:`, `f:`, `f:re:`, `tag:nc:` all parse.

### G8.1 `prop:`

The value is a property, an operator and an operand, with nothing between them.

1. **Property.** The value must start with one of these, tried in this order, the first that matches being taken: `ivl`, `due`, `reps`, `lapses`, `ease`, `pos`, `rated`, `resched`, `s`, `d`, `r`, `cdn:` followed by one or more ASCII letters or digits (as many as there are), `cds:` followed by the same. Matching is case-sensitive and takes a prefix only: `prop:dueish>1` takes `due`, `prop:rx>1` takes `r`, `prop:stability>1` takes `s`. No match is `invalid_property` with `provided` the whole value (`prop:foo>1` gives `foo>1`, `prop:IVL>1` gives `IVL>1`, `prop:cdn:é=1` gives `cdn:é=1`, `prop:` gives empty).
2. **Operator.** The rest must start with `<=`, `>=`, `!=`, `=`, `<` or `>`, tried in that order. Otherwise `invalid_operator` with `provided` the property as matched (`prop:ivl~1` gives `ivl`, `prop:cdn:abc_d=1` gives `cdn:abc`, `prop:pos` gives `pos`). The operator is never doubled: in `prop:ivl==1` the operator is `=` and the operand `=1`.
3. **Operand.** Everything after the operator. The context of every operand error is the whole value (after `prop:`), and `provided` is the operand text:

   | Property | Operand | Error |
   |---|---|---|
   | `ease`, `s`, `d`, `r`, `cdn:<key>` | float | `invalid_number` |
   | `due` | signed 32-bit | `invalid_whole_number` |
   | `ivl`, `reps`, `lapses`, `pos` | unsigned 32-bit | `invalid_positive_whole_number` |
   | `rated` | split at the first `:`: days signed 32-bit and at most 0, then an optional button unsigned 8-bit in 1–4 | days: `invalid_negative_whole_number` with the days text; button: `invalid_answer_button` with the button text (`prop:rated=-1:` gives empty, `prop:rated<=-1:4:` gives `4:`) |
   | `resched` | signed 32-bit and at most 0; a colon makes it invalid | `invalid_negative_whole_number` (`prop:resched=-1:2` gives `-1:2`) |
   | `cds:<key>` | the rest as written (R), possibly empty | none |

## G9. Number forms

Numbers are read from the value text exactly as written. These are the forms of Rust's `FromStr` for the named type; Meguri parses with it.

- **Unsigned N-bit**: an optional `+`, then one or more ASCII digits; the value must fit (8-bit ≤ 255, 16-bit ≤ 65535, 32-bit ≤ 4294967295). Leading zeros are fine. A `-` (even `-0`), a space, `_`, a fraction or an empty text is invalid: `flag:-0` and `flag:1.0` are `invalid_flag`.
- **Signed N-bit**: an optional `+` or `-`, then one or more ASCII digits, within the type's range (32-bit: −2147483648 to 2147483647). `prop:due=2147483648` is `invalid_whole_number`.
- **Float** (32-bit): an optional `+` or `-`, then either `inf`, `infinity` or `nan` in any ASCII letter case, or a decimal with at least one digit and an optional fraction (`1`, `1.`, `.5`, `1.5`), optionally followed by `e` or `E`, an optional sign and one or more digits. The value is rounded to the nearest 32-bit float (ties to even); beyond the range it becomes ±infinity, below it ±0. Hex (`0x10`), `_`, two dots and an empty text are invalid.

## G10. Examples

| Input | Items |
|---|---|
| `dog cat or  fish` | (dog AND cat) OR fish |
| `a AND b`, `a Or b` | a AND b; a OR b |
| `"and"` | the plain text `and` |
| `-(a or b)` | NOT (a OR b) |
| `key:"value with spaces"` | field `key`, value `value with spaces` |
| `key: "value"` | field `key` with the empty value, AND the plain text `value` |
| `3\:30`, `w:3:30`, `\-term` | plain `3:30`; `w:` with `3:30`; plain `-term` |
| `rid:1:2` | field `rid`, value `1:2` |
| `DECK:x`, `Tag:y` | deck `x`; tag `y` |
| `a:"b"c` | field `a` with `b`, AND plain `c` |
| `a:b:"c"` | field `a` with `b:`, AND plain `c` (the quote does not follow the first colon) |
| `a<U+3000>b`, `a<TAB>b`, `a<LF>b`, `a<U+00A0>b` | a AND b |
