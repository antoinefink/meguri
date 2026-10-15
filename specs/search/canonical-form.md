# Canonical search text

Spec: search/canonical-form
Requirements: R1021, R1076, R1077, R1919
Anki: 26.09.3
Scenarios: conformance/scenarios/search/canonical/, conformance/scenarios/search/structure/, conformance/scenarios/search/lexing/

The text a committed search is replaced with (R1076). It is byte-identical to Anki's normalized search text for the same input, which the `canonical_search` observation records (`specs/conformance/search-ops.md`). The goldens are the reference: compare with them, never with Meguri's own output.

## F1. Scope

- Canonical text is a function of the search text alone. It reads no collection, no config and no clock; it never raises a run error (`errors.md` E3), only the parse errors of `errors.md` E1.
- It is written from the parsed query (`query-model.md`), so every input that parses has one, and an input that does not parse has none (the error is the result).
- The writer works on the parsed items, never on the input text: quoting and escaping are produced anew, so redundant quotes and escapes disappear and needed ones appear.
- The same writer serves searches that Meguri builds from UI actions (U88); the rules below apply to any query, parsed or built.

## F2. The whole search

1. A search that is the whole collection (empty or only whitespace, `grammar.md` G1) is written `deck:*`.
2. A search that is exactly one item, and that item is a group, is written as the group's inside, without its brackets. Only one level goes: `(a b)` → `a b`, `((a b))` → `(a b)`, `(a)` → `a`. A negated group is not a group: `-(a b)` stays.
3. Any other search is its items written in order (F3).

## F3. Items and joiners

| Item | Written |
|---|---|
| AND, implicit or explicit | one space |
| OR | ` OR ` (space, `OR`, space) |
| group | `(`, its items, `)` |
| negation | `-` followed by the negated item |
| negated `and` | `- ` (`-` and a space) |
| negated `or` | `- OR ` |
| term | F5 |

Items are written back to back with exactly these strings: the input's spacing never survives. `dog cat or  fish` → `dog cat OR fish`; `1 and 2 or 3` → `1 2 OR 3`; `a(b)` → `a (b)`; `"x""y"` → `x y`; `a -and b` → `a -  b` (negation of `and` is `- `, between two single spaces).

## F4. Quoting rule

Q(s), applied to the text a term writes:

1. s needs quotes when any of these holds:
   - s is `and` or `or` in any ASCII letter case;
   - s starts with `-` and has a second character that is not U+000A;
   - s contains a space, U+3000, `(` or `)`.
2. Replace each `"` in s by `\"`.
3. If s needed quotes, write `"`, the result of step 2, `"`; otherwise the result of step 2.

So `-` alone is not quoted, `-a` is (`"-a"`), `a-b` is not, `(` is (`"("`), and `aNd` is (`"aNd"`). No other character is ever escaped or quoted by the writer; in particular `\` and `*` are written as they stand in the term's stored text.

## F5. Terms

Keys are always written in lower case, whatever the input's case (`DECK:x` → `deck:x`). "Q(...)" is F4. Stored texts are those of `grammar.md` G8 (after U, Q, D or R reading).

| Term | Written |
|---|---|
| whole collection | `deck:*` |
| plain text t | Q(t with every `:` replaced by `\:`) |
| field term: name f, mode, text t | Q(f′ + `:` + prefix + t′): f′ is f with every `:` replaced by `\:`; prefix is empty, `re:` or `nc:` by mode; t′ is t, except in normal mode when t starts with `re:` or `nc:`: then its first `:` becomes `\:` |
| tag term | as a field term whose name is `tag` |
| `deck:`, `note:`, `re:`, `nc:`, `sc:`, `w:`, `has-cd:`, `preset:` | Q(key + `:` + stored text) |
| template by ordinal | `card:` + (ordinal + 1), no quoting (`card:0` and `card:01` → `card:1`) |
| template by name | Q(`card:` + name) |
| `flag:` | `flag:` + number (`flag:007` → `flag:7`) |
| `is:` | `is:` + the state word |
| `added:`, `edited:`, `introduced:` | key + `:` + days after G8's raise of 0 to 1 (`added:0` → `added:1`) |
| `rated:` | `rated:` + days (0 raised to 1), then `:` + button when one was given |
| `resched:` | `resched:` + days as given (`resched:0` stays) |
| `did:`, `nid:`, `cid:` | key + `:` + the list as written, no quoting (`nid:007` stays) |
| `mid:` | `mid:` + id (F6) |
| `dupe:` | Q(`dupe:` + id (F6) + `,` + text with every `\` doubled) |
| `prop:` with `ivl`, `due`, `reps`, `lapses`, `pos`, `ease`, `s`, `d`, `r` | `prop:` + property + operator + operand (F6), no quoting |
| `prop:rated` | `prop:rated` + operator + days, then `:` + button when one was given |
| `prop:resched` | `prop:resched` + operator + days |
| `prop:cdn:` | `prop:cdn:` + key + operator + operand (F6), no quoting |
| `prop:cds:` | Q(`prop:cds:` + key + operator + value) |
| negated operator word | F3 |

## F6. Numbers

- Whole numbers are written in decimal with no `+`, no leading zeros, and `-` for negatives: `mid:+5` → `mid:5`, `prop:due=-0` → `prop:due=0`, `prop:ivl>=+5` → `prop:ivl>=5`, `dupe:+1,a` → `dupe:1,a`.
- Floats (32-bit, `grammar.md` G9) are written as Rust's `Display` for `f32` writes them: the shortest digit string that reads back as the same float, in plain positional notation, never with an exponent; no fraction part for whole values; `-0` for negative zero; `inf`, `-inf` and `NaN` for the special values.

| Operand as typed | Written |
|---|---|
| `1.0`, `1.`, `1E0` | `1` |
| `2.50` | `2.5` |
| `1e3`, `1E3` | `1000` |
| `.5`, `+.5` | `0.5` |
| `0.1` | `0.1` |
| `0.3333333333` | `0.33333334` |
| `1e-10` | `0.0000000001` |
| `16777217` | `16777216` |
| `123456789` | `123456790` |
| `1e39`, `1e400`, `infinity`, `inf` | `inf` |
| `-inf` | `-inf` |
| `nan`, `NaN` | `NaN` |
| `1e-50` | `0` |
| `-1e-50`, `-0` | `-0` |

## F7. Round trips

R1077: writing the canonical text of a canonical text gives it back unchanged, and a canonical text means the same as the input it came from. This holds for every input that parses, except in three cases where Anki's own canonical text does not round-trip. Meguri writes Anki's bytes there too (R1076 is byte parity):

| Input | Canonical text | Reading the canonical text again |
|---|---|---|
| `-and`, `-OR` (negated operator words) | `- `, `- OR ` | `- ` reads as the plain term `-` (canonical `-`); `- OR ` is `misplaced_or` |
| a `preset:` or `prop:cds:` value holding `\"` (their values keep escapes, then F4 escapes the `"` again): `preset:a\"b` | `preset:a\\"b` | `unclosed_quote` |
| `has\-cd:x` (a field named `has-cd`) | `has-cd:x` | the same text, but now a custom-data term |

The round-trip property tests (`tests/canonical_roundtrip.rs`) exclude exactly these three shapes and check everything else: canonical(canonical(t)) = canonical(t), and on the fixture collection the search canonical(t) returns the same ids as t.

## F8. Examples

All from the oracle; the scenarios hold many more.

| Input | Canonical text |
|---|---|
| `dog cat or  fish` | `dog cat OR fish` |
| empty, `   `, tab and line feed | `deck:*` |
| `a<TAB>b`, `a<U+00A0>b`, `a<U+3000>b` | `a b` |
| `"foo" "b a r"` | `foo "b a r"` |
| `field:"foo"` | `field:foo` |
| `key:"value with spaces"` | `"key:value with spaces"` |
| `key: "value"` | `key: value` |
| `\( \)` | `"(" ")"` |
| `\-foo`, `"-foo"` | `"-foo"` |
| `--a` | `-"-a"` |
| `\*\:\_` | `\*\:\_` |
| `deck:\:`, `deck:a\-b`, `deck:a\(b` | `deck::`, `deck:a-b`, `"deck:a(b"` |
| `"aNd" "oR"` | `"aNd" "oR"` |
| `"f o o"bar` | `"f o o" bar` |
| `tag:re\:x`, `f:re\:x` | `tag:re\:x`, `f:re\:x` |
| `re:\(` | `"re:\("` |
| `card:0`, `card:+1`, `card:65536` | `card:1`, `card:1`, `card:65536` |
| `rated:0`, `rated:2:+3`, `resched:0` | `rated:1`, `rated:2:3`, `resched:0` |
| `prop:ease>1.0`, `prop:s>1.0`, `prop:pos=+3` | `prop:ease>1`, `prop:s>1`, `prop:pos=3` |
| `"prop:cds:abc=foo bar"` | `"prop:cds:abc=foo bar"` |
| `dupe:1,a\\b` | `dupe:1,a\\b` |
| `-(a or b)`, `(a b)`, `((a b))` | `-(a OR b)`, `a b`, `(a b)` |
