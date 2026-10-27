# The config table

Spec: storage/config-table
Requirements: R323, R324, R1466, R1468, R1472, R1474
Anki: 26.09.3
Scenarios: conformance/scenarios/storage/config/

The `config` table holds the collection's synced settings: Anki's own keys, keys that other clients and add-ons write, and Meguri's own keys. Meguri reads and writes it without disturbing any key it does not change. Stamping terms (`usn`, `now_s`, `now_ms`, `col.mod`) are those of `ids-usn-mtime.md`.

## C1. Table and keys

| Column | Content |
|---|---|
| `KEY` | the key name; the primary key |
| `usn` | `ids-usn-mtime.md` S3; C9 for sync |
| `mtime_secs` | epoch seconds of the last write |
| `val` | the value: JSON text as bytes (C2) |

- The DDL is `storage/schema-18` S2. Files that releases at schemas 14 to 16 created spell the key column `key`, and the upgrade keeps that text; SQLite matches column names in any letter case, so statements naming `KEY` work on both, but code that reads column names from the schema must not expect `KEY`.
- Keys compare byte for byte: `curDeck` and `curdeck` are two keys. A key appears once.
- A key's name says who owns it. Anki's keys include `curDeck`, `curModel`, `activeDecks`, `lastUnburied`, `nextPos` and `creationOffset`. Per-object families are `_nt_<note type id>_<name>` (for example `_nt_<id>_lastDeck`, the deck a note of that type was last added to) and `_deck_<deck id>_<name>` (for example `_deck_<id>_lastNotetype`, the note type last used in that deck), with the id in decimal. Meguri's keys are in C8. Every other key belongs to someone else (C7).
- The types, defaults and meanings of `pasteImagesAsPng`, `pasteStripsFormatting`, `addToCur` and `firstDayOfWeek` are `editor/config-keys`; those keys hold scalars and are written whole (C4), never through C6.

## C2. Values

- `val` holds the JSON text of one value (RFC 8259) in UTF-8. Anki stores it with SQLite's BLOB storage class, and so does Meguri.
- Anki writes every value in the compact form of C5. Values that other writers stored can have any spacing, member order, escapes and number spellings; Meguri keeps them as they are until it writes that key.
- Meguri's raw read returns `val`'s exact bytes. Meguri never re-encodes a value it does not write.

## C3. Typed reads

A typed read asks for a value of an expected shape (a deck id, a boolean, an object with known members). It returns the reader's documented default when the key is missing, when `val`'s storage class is not BLOB (Anki treats a TEXT value as missing), when the bytes are not valid JSON, or when the JSON does not have the expected shape. A read never fails and never writes. A key whose value cannot be read this way keeps its bytes until something writes that key.

Numbers read as Anki reads them:

| Text | Reads as |
|---|---|
| digits with no fraction and no exponent, from -9223372036854775808 to 18446744073709551615 | that integer |
| such digits outside that range | the nearest 64-bit float (`18446744073709551616` is the float 1.8446744073709552e19) |
| `-0` | the float -0.0 |
| any number with a fraction or an exponent | the nearest 64-bit float (`1E2` is the float 100.0) |

When an object repeats a member name, the last occurrence counts.

## C4. Writing a value

A write sets one key to one JSON value.

1. When the key exists and its stored text is the same value as the new one, nothing is written: the row keeps its bytes, `usn` and `mtime_secs`, and the write does not move `col.mod` (R1472).
2. Otherwise the row gets the new value in C5's compact form, stored as a BLOB, with `usn` -1 and `mtime_secs` `now_s`; a missing key is inserted that way. The operation sets `col.mod` to `now_ms` when it commits (R1474, `ids-usn-mtime.md` S5). Writing `curDeck` is such a write.

The stored text is the same value as the new one when all of these hold:

- its storage class is BLOB and it is valid JSON;
- no object in it repeats a member name;
- its compact form (C5) equals the new value's compact form, byte for byte.

So member order, spacing and escapes do not count (`{"b": 1, "a":2}` is the same value as `{"a":2,"b":1}`, and `"é"` as `"é"`), and numbers compare after C3's reading (`1e2` and `100.0` are the same value). An integer is never the same value as a float: writing `1.0` over a stored `1` is a change, since their compact forms are `1.0` and `1`.

Anki compares bytes instead: it writes whenever the new value's compact bytes differ from the stored bytes, even when they hold the same value with other spacing or member order, and so reformats the stored text. Meguri keeps the stored bytes then (R323, R1472). These are deviations `config-same-value-other-bytes` (scenario `storage/config/rewrite-same-value`) and `config-same-value-other-bytes-add-note` (scenario `storage/config/other-bytes-kept`) in `conformance/deviations.yaml`.

A value stored with the TEXT storage class is never the same value: Meguri replaces it with the new value as a BLOB. (Anki fails such a write with a database error.)

## C5. The compact form

Meguri encodes a value exactly as Anki does, so the same value always gives the same bytes. It is the same encoding as the schema upgrade's for `config.val` (`storage/schema-upgrade` U9.2):

- no whitespace between tokens;
- object members sorted by name, comparing names by Unicode code point (which is the byte order of their UTF-8): `{"B":2,"_x":3,"a":5,"a1":4,"b":1}`; each name once;
- strings: `"` as `\"`, `\` as `\\`, U+0008 as `\b`, U+0009 as `\t`, U+000A as `\n`, U+000C as `\f`, U+000D as `\r`, every other code point below U+0020 as `\u00` and two lower-case hex digits (`\u001b`); every other character, `/`, U+007F and all non-ASCII included, as its UTF-8 bytes;
- integers in decimal, `-` for negatives, no `+` and no leading zeros;
- floats (C3) from the shortest digit string that reads back as the same 64-bit float:
  - absolute value at least 1e-5 and below 1e16: written out in full, with `.0` added when it has no fractional digits (`0.00001`, `0.85`, `100.0`, `1000000000000000.0`);
  - any other non-zero value: the first digit, then `.` and the remaining digits when there are any, then `e` and the decimal exponent, with `-` when negative and no `+` (`1e-6`, `1.5e-7`, `1e16`, `2.5e20`, `1.2345678901234568e17`);
  - zero is `0.0`, negative zero `-0.0`; a negative value gets a leading `-`;
- `true`, `false`, `null`.

| Value | Bytes |
|---|---|
| object with `weeks` 52, `mode` "year", `streak` true | `{"mode":"year","streak":true,"weeks":52}` |
| array 1, 2.5, "Été", object with `b` an empty array and `a` an empty object | `[1,2.5,"Été",{"a":{},"b":[]}]` |
| a deck id | `1894701600000` |
| the float 1.0 | `1.0` |
| the float 0.000001 | `1e-6` |
| the string `a/b`, U+0001, `"`, line feed | `"a/b\u0001\"\n"` |

## C6. Changing part of a value

Some settings are members inside one key's object (U96's heatmap settings, U84 and U210 lists). Changing one member keeps every other member with its value and its kind (an integer stays an integer, a float a float, an unknown member stays). The whole value is then written per C4: when the result is the same value as the stored one, nothing is written; otherwise it is written in compact form, so the other members' spacing and order are not kept.

## C7. Keys Meguri does not use

Meguri never writes, re-encodes, reorders or removes a key it does not use (R323, R1466). Examples are `heatmap`, `onigiri_sidebar_collapsed`, `_nt_<id>_lastDeck` for a note type Meguri never adds to, `cardStateCustomizer`, easy-days and filtered-deck keys, and keys of clients and add-ons Meguri does not know. Such a key keeps its bytes, `usn` and `mtime_secs` through every local operation. The only exceptions are Anki's own: removing a note type or deck removes its key family (C10), and a sync can replace the whole table (C9).

## C8. Meguri's keys and device-only state

- Meguri's settings that follow the collection to other devices use keys that begin with `meguri`, followed by a lower-camel-case name (for example `meguriExample`). Anki and other clients keep such keys as unknown keys (R1468).
- Device-only state never enters the `config` table (R324, R1468): credentials and the host key, the sync server endpoint, sync progress and the auto-sync pause flag, display toggles, answer keys, card zoom, integrations, UI state, and the optimizer's run diagnostics. They live in Swift settings, the core's sidecars or the derived store (KTD12). A sync from an Anki client can replace the whole table (C9), so a device-only value stored there would be lost or would spread to other devices.
- `lastFsrsOptimize` is Anki's synced key and has no `meguri` prefix (KTD12).

## C9. Sync and the config table (Anki)

For U107; the table behaves this way when Anki clients sync:

- The side whose `col.mod` is newer sends its whole config table with the other changes. Sending sets `usn` to 0 on every key whose `usn` is not 0.
- The receiving side replaces its whole table with the received keys: keys missing from the set are removed, and every received key is written with `usn` the server's USN, `mtime_secs` the time of the merge, and the value in C5's compact form.
- So an Anki client reformats every value it receives, and nothing that is only in one device's table survives a sync from the other side.
- Anki cannot send its table when a value is not valid JSON or is not stored as a BLOB: its sync fails.

## C10. Removing keys

- Removing a key deletes its row and leaves no grave. It counts as a change: `col.mod` moves. Removing a missing key changes nothing.
- Removing a note type removes every key that begins with `_nt_<id>_`; removing a deck removes every key that begins with `_deck_<id>_` (U29, U28).

## C11. Scenarios

| Scenario | Covers | Golden |
|---|---|---|
| `storage/config/unchanged-value-no-write` | C4 for the keys adding a note writes | recorded |
| `storage/config/other-bytes-kept` | C4 for a stored value with other spacing that adding a note writes again; deviation `config-same-value-other-bytes-add-note` | recorded |
| `storage/config/unknown-keys-survive` | C2, C7 through reviews, edits, additions, deletions and a day rollover | recorded |
| `storage/config/rewrite-same-value` | C4, C5 through the config API; deviation `config-same-value-other-bytes` | recorded |
| `storage/config/current-deck` | C4 for `curDeck` (R1474) | recorded |

The ops are specified in `specs/conformance/record-ops.md`. U23's Rust tests cover what no scenario shows: C3's reading rules and defaults, C5's encoding table, C6, the TEXT storage class, and the 1,000-operation survival property.
