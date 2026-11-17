# JSON numbers and text

Spec: model/json-numbers
Requirements: R1460, R1461, R1464, R1486
Anki: 26.09.3
Scenarios: conformance/scenarios/model/card-data/, conformance/scenarios/model/schema11/

How Meguri reads and writes the JSON it shares with Anki: card `data` (`model/card-data-json`), the schema-11 objects and the `col` JSON columns (`model/schema11-json`) and the `other` bytes of protobuf records (`model/protobuf-records` P7). Each of those specs declares a type for every key it knows; this file says what the types mean on the way in and on the way out.

## N1. Declared types

| Type | Range |
|---|---|
| `i64` | -9223372036854775808 to 9223372036854775807 |
| `i32` | -2147483648 to 2147483647 |
| `u32` | 0 to 4294967295 |
| `u16` | 0 to 65535 |
| `u8` | 0 to 255 |
| `f32` | IEEE-754 binary32 |
| `bool` | `true`, `false` |
| `string` | any JSON string |

## N2. Reading a number into a declared type

- Integer types take a JSON number written without a fraction or an exponent, within the type's range: `5` and `-3` fit an `i32`; `5.0`, `5e0`, `"5"`, `true` and `null` do not, and neither does `-1` for a `u32` or `256` for a `u8`. N6 lists the keys that also accept strings.
- `f32` takes any JSON number, integer or not:
  - an integer literal that N10 reads as an integer (it fits a 64-bit integer, unsigned when positive, signed when negative) becomes the nearest `f32` (ties to even);
  - every other literal (one with a fraction or an exponent, an integer beyond those ranges, and `-0`) becomes a 64-bit float by N11 first, then that value becomes the nearest `f32` (ties to even). The two steps are Anki's and give the same result as one step for every input Anki itself writes;
  - a magnitude beyond the `f32` range but inside the 64-bit float range becomes an infinity (`1e40` and `1.7976931348623157e308` read as +∞).
- A number beyond the 64-bit float range never reaches a key: it makes the whole JSON text invalid (N10).
- A value that does not fit is handled by the rule of the key (refuse the object, or take the default); the record specs say which.

## N3. Writing integers

Decimal digits, a `-` before negative values, no `+`, no leading zeros, no fraction.

## N4. Writing an `f32`

Meguri writes the shortest decimal that reads back as the same `f32`: the fewest significant digits, and among those the one closest to the exact value. Then, with M the digits as an integer (n digits, no trailing zero) and the value equal to M × 10^k, let p = n + k:

| Case | Text | Examples |
|---|---|---|
| k ≥ 0 and p ≤ 13 | M, k zeros, `.0` | `3.0`, `10000000000.0`, `12345679.0` |
| 0 < p ≤ 13 (k < 0) | first p digits, `.`, the other digits | `1.2931`, `123.4568` |
| -6 < p ≤ 0 | `0.`, then -p zeros, then the digits | `0.212`, `0.001`, `0.000001` |
| otherwise, n = 1 | the digit, `e`, p - 1 | `1e-7`, `1e13` |
| otherwise | first digit, `.`, the other digits, `e`, p - 1 | `1.5e13`, `3.4028235e38` |

- The exponent is written in decimal with `-` when negative and never with `+` or leading zeros.
- Negative values start with `-`. Zero is `0.0` and negative zero `-0.0`.
- NaN and the infinities are written as `null`.
- This is exactly the text the `ryu` crate gives for an `f32`, which is how `serde_json` writes an `f32`; Meguri may use either.

`0.212` read as an `f32` and written back stays `0.212`; widening the `f32` to 64 bits first would print `0.21199999749660492`, which is wrong.

## N5. Writing a 64-bit float

Where a spec declares a 64-bit float, the same rule applies to the shortest 64-bit digits, with 16 in place of 13 and -5 in place of -6, and the exponent may have three digits. "Reads back" here means the correctly rounded reading (the float nearest to the digits), as in N4; it is not N11's reading. So a float that N11 gave can be written as digits that N11 reads as another float: the value read from `123456789012345678901` is written `1.2345678901234567e20`, which N11 reads as 1.2345678901234568e20 (N11 examples). No key in the model specs is a 64-bit float today; the rule exists for untyped values that Meguri must rebuild (N7).

## N6. Lenient readers

Some schema-11 keys accept more than N2. The schema-11 tables name the reader of each such key.

- **Integer or numeric string**: a JSON integer in range, or a JSON string consisting of an optional `+` or `-` and then one or more ASCII digits, in range. `"+5"` reads as 5 and `"-0"` as 0. `" 5"`, `"5.0"`, `"0x5"`, `""` and the number `5.0` do not fit.
- **Bool from anything**: reads as true or false, or does not fit:

  | Input | Value |
  |---|---|
  | `true`, `false` | itself |
  | integer 1, integer 0 | true, false |
  | non-integer number within 2^-52 of 1, or equal to 0 (`1.0`, `0.0`) | true, false |
  | string that is `true` or `false` ignoring letter case (`"TRUE"`, `"False"`) | true, false |
  | string holding an integer (optional sign, then digits) equal to 1 or 0 (`"1"`, `"+1"`, `"0"`) | true, false |
  | string that the Rust standard library reads as a 64-bit float within 2^-52 of 1, or equal to 0 (`"1.0"`, `"0.0"`, `"1e0"`) | true, false |
  | anything else (`2`, `0.5`, `"yes"`, `""`, `" true"`, `"1 "`, `null`, arrays, objects) | does not fit |

- **Default on invalid**: the key is read with its declared type (N2); a value that does not fit counts as if the key were missing, and the key takes its default.

## N7. Untyped values

Values Meguri has no type for (unknown keys of card `data`, unknown keys of schema-11 objects, the members of `other`) are kept as the exact text they were read with, from their first to their last character without surrounding whitespace, and written back as that text. Only card `data` can keep a value holding a number beyond the 64-bit float range; in schema-11 objects and `other` such a number makes the whole text invalid (N10).

- Anki parses such values and writes them again: numbers in their shortest form (`1.10` becomes `1.1`, `1e2` becomes `100.0`), whitespace removed, the members of nested objects in sorted key order, and the top-level members of `other` in no particular order. Meguri's output can therefore differ from Anki's in text. It can also differ in value, but only in a number that N11 does not read as its nearest 64-bit float: Anki writes the float N11 gave, while Meguri's kept text still holds the original digits (Meguri keeps `123456789012345678901` where Anki writes `1.2345678901234567e20`, and keeps `1.2345678901234567e20` where Anki writes `1.2345678901234568e20`). N11 gives the nearest float whenever S is at most 2^53 and E is from -22 to 22.
- Conformance compares parsed JSON values (`specs/conformance/observations.md` O7), so key order and number spelling inside untyped values never count. Scenarios that look at raw bytes of such values use values Anki writes in compact form with sorted keys. Untyped numbers in scenarios stay inside N11's exact range (S at most 2^53, E from -22 to 22), or the scenario names a deviation.

## N8. Writing strings and objects

- A string is written between `"` with these escapes and no others: `\"`, `\\`, `\b` (U+0008), `\t` (U+0009), `\n` (U+000A), `\f` (U+000C), `\r` (U+000D), and `\u00xx` with lower-case hex digits for every other character from U+0000 to U+001F. Everything else, `/`, U+007F and all non-ASCII characters included, is written as itself in UTF-8.
- Objects and arrays are written without any whitespace: `{"a":1,"b":[1,2]}`.
- The members of an object Meguri writes come in the order the record spec gives for its known keys, then the untyped members in the order they were read.

## N9. Reading strings and objects

- JSON text is read per RFC 8259, with whitespace allowed between tokens.
- `\uXXXX` escapes combine into surrogate pairs. An unpaired surrogate escape makes the text invalid.
- Duplicate keys: card `data` follows `model/card-data-json` C3 (as Anki does). Elsewhere, when an object repeats a key, the last occurrence counts; for known keys Anki sometimes refuses such an object instead, so Meguri can accept an object Anki would refuse, never the reverse.
- A number beyond the 64-bit float range makes the text invalid, except in a value that is skipped (N10).

## N10. Numbers beyond the 64-bit float range

Anki's JSON reader turns every number it reads into a value: an integer when the literal has no fraction and no exponent and fits a 64-bit integer (unsigned when positive, signed when negative), otherwise a 64-bit float (N11; `-0` is a float too). When that float would be infinite, the read fails, and the whole JSON text is invalid, not only the key that holds the number. Meguri reads the same texts as invalid:

- card `data`: the text reads as no keys at all (`model/card-data-json` C3 rule 1);
- `other`: nothing is added (`model/schema11-json` S3 output rule 1);
- a schema-11 object, and the JSON of a `col` column: the text is not valid JSON.

Where Anki skips a value instead of reading it, the range is not checked. That happens only to the values of unknown keys of card `data` (C3 rule 7): `{"pos":5,"zz":1e400}` reads as `pos` 5 with the unknown key `zz` kept as the text `1e400` (Anki drops `zz`). Every other value is read, nested values included: the values of known card-data keys (`{"pos":5,"cd":[1e400]}` reads as no keys), and every member of `other` and of a schema-11 object, known or not (`other` = `{"keep":1,"zz":{"a":[-1e400]}}` adds nothing).

Which numbers are out of range. Anki converts decimal text with a fast method that is not always exact near the largest 64-bit float (about 1.7976931348623157e308). Meguri decides the range with the same method, so both draw the boundary in the same place:

1. Take the literal's digits into an unsigned 64-bit accumulator S, starting at 0, each digit d making S × 10 + d, as long as that does not exceed 18 446 744 073 709 551 615. In the integer part, the first digit that does not fit and the rest of the integer part are dropped. Then, in the fraction part, digits are again taken while they fit, and the first one that does not fit and all after it are dropped.
2. Let E be the literal's exponent (0 without one), plus the number of integer-part digits dropped, minus the number of fraction digits taken.
3. The number is out of range when S is not 0, E ≥ 0, and either E > 308, or S rounded to the nearest 64-bit float, times the 64-bit float nearest to 10^E, rounded to the nearest 64-bit float, is infinite. A positive exponent too large for a 32-bit signed integer counts as E > 308.
4. Every other number is in range. A negative E never puts a number out of range: it may become 0.0 or a subnormal, with its sign (`1e-400` reads as 0.0, `1e-99999999999` too). A number whose digits are all 0 is 0 (`0e400`).

Examples (oracle, `conformance/scenarios/model/card-data/out-of-range.yaml` and `conformance/scenarios/model/schema11/other-out-of-range.yaml`):

| Literal | S, E | Range |
|---|---|---|
| `1e400` | 1, 400 | out |
| `0.1e310` | 1, 309 | out |
| `1e99999999999` | 1, exponent above 32 bits | out |
| `1` followed by 309 zeros | 10000000000000000000, 290 | out |
| `1` followed by 308 zeros | 10000000000000000000, 289 | in (1e308) |
| `1.7976931348623157e308` | 17976931348623157, 292 | in (the largest 64-bit float) |
| `1.7976931348623158e308` | 17976931348623158, 292 | out, although its nearest 64-bit float is the largest finite one |
| `17976931348623157081e289` | 17976931348623157081, 289 | out, although its value is just below the largest 64-bit float |
| `1797693134862315e293` | 1797693134862315, 293 | in |
| `1e-400` | 1, -400 | in (0.0) |
| `0e400` | 0, 400 | in (0) |

For a number in range, N11 gives the value of a number that is not read as an integer, and N2 then gives the nearest `f32` where the key is an `f32`.

## N11. The value of a number read as a 64-bit float

Anki's reader does not round decimal text to the nearest 64-bit float. A number that N10 does not read as an integer (it has a fraction or an exponent, or it is an integer literal beyond the 64-bit integer range of its sign, or it is `-0`) has the value below, computed from N10's S and E. Every operation is a 64-bit float operation rounded to the nearest float, ties to even.

1. Start from S rounded to the nearest 64-bit float.
2. When E ≥ 0, multiply it by the 64-bit float nearest to 10^E. (N10 rule 3 makes the number out of range when E > 308 or when this product is infinite; when S is 0 the result is 0 whatever E is, N10 rule 4.)
3. When E < 0, divide it by the 64-bit float nearest to 10^(-E). When E < -308, first divide it by the 64-bit float nearest to 10^308 and add 308 to E, as many times as it takes to bring E to -308 or above; then make the one division by the 64-bit float nearest to 10^(-E) with the E that is left. (E = -400: divide by 10^308, then by 10^92.)
4. A literal that starts with `-` gets the negated result, so zero and subnormal results keep their sign (`-0` and `-0.0e5` are -0.0).

This covers the negative integer literals below -9223372036854775808 too: one whose magnitude fits S has E = 0, so it reads as the nearest 64-bit float of its magnitude, negated.

The result is the nearest 64-bit float to the literal whenever S is at most 2^53 and E is from -22 to 22 (one rounding of exact values). Outside that, it can differ from the nearest float, and Meguri must give N11's result, never the result of a correctly rounded parser, wherever a spec reads JSON "as Anki reads it" (`storage/config-table` C3, `storage/schema-upgrade` U9.2). Examples (oracle, `storage/upgrade/from-11`, `storage/downgrade/contents` and `storage/downgrade/roundtrip`, through the config value `meguriFixture` of `storage/schema-upgrade` U16):

| Literal | S, E | Value | Nearest 64-bit float |
|---|---|---|---|
| `123456789012345678901` | 12345678901234567890, 1 | 1.2345678901234567e20 (123456789012345667584) | 1.2345678901234568e20 (123456789012345683968) |
| `1.2345678901234567e20` | 12345678901234567, 4 | 1.2345678901234568e20 | 1.2345678901234567e20 |
| `18446744073709551616` | 1844674407370955161, 1 | 1.8446744073709552e19 | the same |
| `1E2` | 1, 2 | 100.0 | the same |
| `2.50` | 250, -2 | 2.5 | the same |
| `1e-400` | 1, -400 | 0.0 | the same |

The second row is the shortest text of the first row's value (N5): Anki writes that value as `1.2345678901234567e20` and, reading that text again, gets another float.
