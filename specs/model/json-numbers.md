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
  - an integer literal becomes the nearest `f32` (ties to even);
  - a literal with a fraction or an exponent becomes the nearest 64-bit float first, then that value becomes the nearest `f32` (ties to even). The two steps are Anki's and give the same result as one step for every input Anki itself writes;
  - a magnitude beyond the `f32` range becomes an infinity (`1e40` reads as +∞).
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

Where a spec declares a 64-bit float, the same rule applies to the shortest 64-bit digits, with 16 in place of 13 and -5 in place of -6, and the exponent may have three digits. No key in the model specs is a 64-bit float today; the rule exists for untyped values that Meguri must rebuild (N7).

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

Values Meguri has no type for (unknown keys of card `data`, unknown keys of schema-11 objects, the members of `other`) are kept as the exact text they were read with, from their first to their last character without surrounding whitespace, and written back as that text.

- Anki parses such values and writes them again: numbers in their shortest form (`1.10` becomes `1.1`, `1e2` becomes `100.0`), whitespace removed, the members of nested objects in sorted key order, and the top-level members of `other` in no particular order. Meguri's output can therefore differ from Anki's in text but not in JSON value, for every value Anki itself can write.
- Conformance compares parsed JSON values (`specs/conformance/observations.md` O7), so key order and number spelling inside untyped values never count. Scenarios that look at raw bytes of such values use values Anki writes in compact form with sorted keys.

## N8. Writing strings and objects

- A string is written between `"` with these escapes and no others: `\"`, `\\`, `\b` (U+0008), `\t` (U+0009), `\n` (U+000A), `\f` (U+000C), `\r` (U+000D), and `\u00xx` with lower-case hex digits for every other character from U+0000 to U+001F. Everything else, `/`, U+007F and all non-ASCII characters included, is written as itself in UTF-8.
- Objects and arrays are written without any whitespace: `{"a":1,"b":[1,2]}`.
- The members of an object Meguri writes come in the order the record spec gives for its known keys, then the untyped members in the order they were read.

## N9. Reading strings and objects

- JSON text is read per RFC 8259, with whitespace allowed between tokens.
- `\uXXXX` escapes combine into surrogate pairs. An unpaired surrogate escape makes the text invalid.
- Duplicate keys: card `data` follows `model/card-data-json` C3 (as Anki does). Elsewhere, when an object repeats a key, the last occurrence counts; for known keys Anki sometimes refuses such an object instead, so Meguri can accept an object Anki would refuse, never the reverse.
