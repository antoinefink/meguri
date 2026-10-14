# Protobuf record blobs

Spec: model/protobuf-records
Requirements: R1463, R1464, R1484, R1485, R1486, R903, R904, R322
Anki: 26.09.3
Scenarios: conformance/scenarios/model/proto/, conformance/scenarios/model/schema11/

Six columns of a schema-18 collection hold protobuf messages. This file gives each message's fields, how Anki encodes them, how Meguri decodes them, and how Meguri keeps what it does not understand. The JSON form of the same records (schema 11) is `model/schema11-json`. Number text is `model/json-numbers`.

## P1. Blob columns

| Column | Message (P6) | Notes |
|---|---|---|
| `notetypes.config` | notetype config | one per note type |
| `fields.config` | field config | one per field; the field's `ord` and `name` are columns, not message fields |
| `templates.config` | template config | one per template; `ord`, `name`, `mtime_secs` and `usn` are columns |
| `decks.common` | deck common | |
| `decks.kind` | deck kind | holds the normal or the filtered part |
| `deck_config.config` | deck config | |

- `tags.config` is a blob column too, but Anki never puts anything in it (new tag rows leave it NULL). Meguri reads it as optional opaque bytes and never interprets or changes it.
- Every value in these six columns has the SQLite storage class BLOB, including the zero-length BLOB of a message whose fields are all at their defaults. Meguri binds bytes as BLOB, never as TEXT or NULL, even when empty.
- A value of any other storage class (TEXT, INTEGER, REAL, NULL) in one of the six columns makes the row unreadable: Anki fails the read with an error. Meguri reports a typed error for the row and does not guess (`model/row-records` R10).

## P2. Wire format read by Meguri

Standard protocol buffers (proto3) binary encoding:

- A field is a key followed by a payload. The key is a varint whose value is `(field_number << 3) | wire_type`.
- Varints are little-endian groups of 7 bits, high bit set on every byte but the last, at most 10 bytes. A value has at most 64 bits, so the tenth byte may only be `00` or `01`: a tenth byte from `02` to `7f`, or one with the high bit set, makes the varint invalid. This holds for every varint: keys, lengths and values. A key whose value does not fit in 32 bits is invalid.
- Field numbers run from 1 to 536 870 911. Field number 0 is invalid.
- Wire types: 0 varint, 1 fixed 64-bit (8 bytes, little-endian), 2 length-delimited (a varint length, then that many bytes), 3 group start, 4 group end, 5 fixed 32-bit (4 bytes, little-endian). Wire types 6 and 7 are invalid.
- A group runs from a group-start key to the group-end key with the same field number, and may contain other fields and nested groups. A group-end key with no open group, or with another field number than the innermost open group's, is invalid.
- A buffer that ends inside a key, a varint, a fixed value, a length-delimited payload or a group is truncated and invalid. The payload of a nested message is the buffer of that message's fields: a group still open, or a length-delimited payload not finished, at the end of that payload is truncated, even when the bytes that follow in the outer message would complete it. A group still open at the end of the whole blob is truncated too.
- Nesting depth: the fields of the column's message are at depth 0; the fields inside a group, or inside the payload of a nested message, are one deeper than the group-start key or the message field around them. A field at depth 100 or more makes the blob invalid; a group-start key counts as a field, a group-end key does not. So at most 100 groups and nested messages are open at once, and the innermost of 100 holds nothing. Known fields reach depth 2 at most (a day limit's fields), so in practice the limit stops deeply nested unknown groups.

Examples (oracle, `conformance/scenarios/model/proto/decode-limits.yaml`), each a whole `deck_config.config` value unless named otherwise: `38ffffffffffffffffff01` decodes (unknown field 7, a 10-byte varint); `38ffffffffffffffffff02`, `38ffffffffffffffffff7f` and the 11-byte `38ffffffffffffffffff8001` do not. `3b` (a group of field 7 never closed) does not decode, `3b3c` does. One hundred nested groups `3b…3b 3c…3c` decode, 101 do not; 99 nested groups around `0800` (field 1, a varint, at depth 99) decode, 100 do not. As `decks.kind`: `0a025b5c` (a normal deck holding an empty unknown group 11) decodes; `0a015b` (the group still open at the end of the normal deck's payload), `0a015b5c` (its group end after that payload) and `0a032202414242` (a description of 2 bytes with only 1 left in the payload) do not.

How a payload becomes a value of a field the message declares (P6):

| Declared type | Accepted wire types | Value |
|---|---|---|
| `bool` | 0 | false when the varint is 0, true otherwise |
| `int32`, enum | 0 | low 32 bits of the varint as a signed integer; an enum keeps any number, known or not |
| `uint32` | 0 | low 32 bits as an unsigned integer |
| `int64` | 0 | the 64 bits as a signed integer |
| `float` | 5 | IEEE-754 binary32, little-endian, bit for bit (NaN payloads kept) |
| `string` | 2 | the bytes, which must be valid UTF-8 or the blob is invalid |
| `bytes` | 2 | the bytes, unchecked |
| message | 2 | the payload decoded as that message, recursively with these rules |
| repeated scalar (`float`, `uint32`) | 2 (packed: the payload is consecutive values) or the element's own wire type (one value) | each occurrence appends its values in order; both forms may be mixed |
| repeated message | 2 | each occurrence appends one element |

- A declared field arriving with a wire type the table does not accept makes the blob invalid (for example a varint for a `float` list).
- Singular scalars: the last occurrence wins.
- Singular messages: a later occurrence is merged into the earlier one (scalars of the later one win, lists are appended, nested messages merge).
- One-of (P6.5): an occurrence of a member replaces whatever member was set before; a second occurrence of the same message member merges into it.
- Fields the message does not declare, reserved numbers included, are unknown fields (P4).

Decoding is all or nothing: an invalid blob yields a typed error (P9) and no record. Meguri never panics on any input.

## P3. Encoding (what Anki writes)

Meguri encodes a message exactly as Anki does, so that a record Meguri creates or changes has the bytes Anki would give it.

- Fields are written in ascending field-number order, whatever order the message lists them in. A one-of member is written at its own field number.
- A field without explicit presence (marked "implicit" in P6) is left out when it holds its default: `false`, 0, enum 0, an empty string or byte string, or a float equal to 0.0. Negative zero equals 0.0, so a float of -0.0 is left out and reads back as +0.0.
- A field with explicit presence (marked "optional") is written whenever it is set, even when it holds the default.
- A singular message field is written whenever it is present, even when all its fields are defaults (a zero-length payload: key byte then `00`).
- Varints use the fewest bytes. `int32` and enum values below zero are written sign-extended to 64 bits, so they take 10 bytes. `int64` values below zero take 10 bytes.
- Floats are written as their 4 little-endian bytes, bit for bit.
- A non-empty repeated scalar field is written once, packed: key with wire type 2, the byte length, then the values. An empty one is left out.
- A repeated message field is written as one length-delimited entry per element, in order.
- Unknown fields are placed as P4 describes.

## P4. Unknown fields (Meguri)

Anki drops every field it does not declare when it rewrites a blob. Meguri keeps them (R1463, R1486). This is a deliberate deviation; a scenario can observe it only after Meguri rewrites a record that carried such a field.

- Capture happens at every message level: the six blob messages and every nested message (card requirement, normal and filtered deck, search term, day limit).
- An unknown field is kept as its exact bytes: from the first byte of its key to the last byte of its payload (for a group, through its group-end key). Unknown fields of one message are kept in the order they were read.
- On encoding, every field of a message, known or unknown, is written in ascending field-number order (P3). Unknown fields with different numbers are written in number order whatever order they were read in, also when no known field lies between them; unknown fields that share a number keep their read order. So a blob written by a later Anki (ascending order) comes back in the same layout after Meguri changes one known field, and a blob read out of order comes back sorted once Meguri re-encodes it (P5).
- A known field is never written from captured bytes. If the same unknown number appears in several places of the read blob, all occurrences are kept.
- Two records are equal when their known values are equal (floats bit for bit) and, for each field number, they hold the same unknown fields in the same order. The order between unknown fields of different numbers does not count, since encoding never keeps it.

Example (deck config, P6.6). Stored blob, with field 7 unknown to Meguri (a packed float list `[4.0]`):

```
1a 04 0000803f        fsrs_params_4 = [1.0]
2a 04 00000040        fsrs_params_5 = [2.0]
32 04 00004040        fsrs_params_6 = [3.0]
3a 04 00008040        field 7 (unknown) = packed [4.0]
48 14                 new_per_day = 20
```

After Meguri sets `fsrs_params_6` to `[0.5, 1.5]` and nothing else, the blob is:

```
1a 04 0000803f  2a 04 00000040  32 08 0000003f 0000c03f  3a 04 00008040  48 14
```

Every byte outside field 6 is unchanged (R1486).

Example of the order rule. Stored deck config blob, unknown fields 8 and 7 in that order before a known field:

```
42 00                 field 8 (unknown), empty
3a 00                 field 7 (unknown), empty
48 14                 new_per_day = 20
```

After Meguri sets `new_per_day` to 25 and nothing else, the blob is `3a 00  42 00  48 19`.

## P5. When Meguri re-encodes

- A record keeps the exact bytes it was read with.
- When a record is written, Meguri compares it with the record decoded from the bytes it was read with (equality as in P4: known values, floats bit for bit, and captured unknown fields). If they are equal, the original bytes are written unchanged, even when they are not in Anki's canonical form (P3). If they differ, the message is encoded per P3 and P4.
- A record Meguri creates has no original bytes and is encoded per P3.
- This decides bytes only. Whether a write happens at all, and how `usn` and modification times move, is the storage area's rule (an unchanged record is not written, R1472).
- Anki itself re-encodes every blob it writes and so drops unknown fields; Meguri's output equals Anki's whenever the read blob had no unknown fields and was in canonical form.

## P6. Messages

Columns: number, field name, type, presence ("implicit" or "optional"; lists and messages as P3 says), meaning. Field names are interface names (`interop-vocab.txt`). The schema-11 key of each field is in `model/schema11-json`.

### P6.1 Notetype config (`notetypes.config`)

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `kind` | enum | implicit | 0 normal, 1 cloze |
| 2 | `sort_field_idx` | uint32 | implicit | index of the sort field |
| 3 | `css` | string | implicit | card styling |
| 4 | `target_deck_id_unused` | int64 | implicit | legacy last-used deck; Anki no longer reads it, kept as is |
| 5 | `latex_pre` | string | implicit | LaTeX header |
| 6 | `latex_post` | string | implicit | LaTeX footer |
| 7 | `latex_svg` | bool | implicit | render LaTeX as SVG |
| 8 | `reqs` | repeated card requirement (P6.1.1) | list | legacy card requirements |
| 9 | `original_stock_kind` | enum | implicit | 0 unknown, 1 basic, 2 basic and reversed, 3 basic with optional reverse, 4 basic with typing, 5 cloze, 6 image occlusion |
| 10 | `original_id` | int64 | optional | the note type's id in the collection it was imported from |
| 255 | `other` | bytes | implicit | legacy JSON keys (P7) |

#### P6.1.1 Card requirement

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `card_ord` | uint32 | implicit | template ordinal |
| 2 | `kind` | enum | implicit | 0 none, 1 any, 2 all |
| 3 | `field_ords` | repeated uint32 | list (packed) | field ordinals |

### P6.2 Field config (`fields.config`)

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `sticky` | bool | implicit | keep the value after adding a note |
| 2 | `rtl` | bool | implicit | right-to-left text |
| 3 | `font_name` | string | implicit | editor font |
| 4 | `font_size` | uint32 | implicit | editor font size |
| 5 | `description` | string | implicit | placeholder text in the editor |
| 6 | `plain_text` | bool | implicit | open in the HTML editor by default |
| 7 | `collapsed` | bool | implicit | collapsed by default in the editor |
| 8 | `exclude_from_search` | bool | implicit | left out of unqualified searches |
| 9 | `id` | int64 | optional | stable field id, used to match fields when merging note types on import |
| 10 | `tag` | uint32 | optional | image-occlusion field tag |
| 11 | `prevent_deletion` | bool | implicit | the field cannot be removed |
| 255 | `other` | bytes | implicit | legacy JSON keys (P7) |

### P6.3 Template config (`templates.config`)

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `q_format` | string | implicit | front template |
| 2 | `a_format` | string | implicit | back template |
| 3 | `q_format_browser` | string | implicit | browser front template |
| 4 | `a_format_browser` | string | implicit | browser back template |
| 5 | `target_deck_id` | int64 | implicit | deck override; 0 means none |
| 6 | `browser_font_name` | string | implicit | browser font |
| 7 | `browser_font_size` | uint32 | implicit | browser font size |
| 8 | `id` | int64 | optional | stable template id, used to match templates when merging note types on import |
| 255 | `other` | bytes | implicit | legacy JSON keys (P7) |

### P6.4 Deck common (`decks.common`)

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `study_collapsed` | bool | implicit | collapsed in the deck list |
| 2 | `browser_collapsed` | bool | implicit | collapsed in the browser sidebar |
| 3 | `last_day_studied` | uint32 | implicit | day number the counters below belong to |
| 4 | `new_studied` | int32 | implicit | new cards studied that day (custom study may make it negative) |
| 5 | `review_studied` | int32 | implicit | reviews done that day |
| 6 | `learning_studied` | int32 | implicit | learning count of the old v1 scheduler; kept, not used |
| 7 | `milliseconds_studied` | int32 | implicit | time studied that day |
| 255 | `other` | bytes | implicit | legacy JSON keys (P7) |

Numbers 8 to 13 are reserved by Anki; Meguri treats them as unknown fields (P4).

### P6.5 Deck kind (`decks.kind`)

A one-of with two members. Exactly one must be set: a blob with neither member makes the deck unreadable (Anki reports the deck as invalid).

| # | Name | Type | Meaning |
|---|---|---|---|
| 1 | `normal` | normal deck (P6.5.1) | a regular deck |
| 2 | `filtered` | filtered deck (P6.5.3) | a filtered deck |

#### P6.5.1 Normal deck

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `config_id` | int64 | implicit | the deck's preset (`deck_config.id`) |
| 2 | `extend_new` | uint32 | implicit | legacy custom-study extension; kept |
| 3 | `extend_review` | uint32 | implicit | legacy custom-study extension; kept |
| 4 | `description` | string | implicit | deck description |
| 5 | `markdown_description` | bool | implicit | the description is Markdown |
| 6 | `review_limit` | uint32 | optional | deck review-limit override |
| 7 | `new_limit` | uint32 | optional | deck new-limit override |
| 8 | `review_limit_today` | day limit (P6.5.2) | message | today-only review limit |
| 9 | `new_limit_today` | day limit (P6.5.2) | message | today-only new limit |
| 10 | `desired_retention` | float | optional | deck desired-retention override |

The normal deck has no `other` field. Numbers 12 to 15 are reserved by Anki, and 11 is not declared; all are unknown fields for Meguri.

#### P6.5.2 Day limit

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `limit` | uint32 | implicit | the limit |
| 2 | `today` | uint32 | implicit | the day number it applies to |

#### P6.5.3 Filtered deck

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `reschedule` | bool | implicit | reschedule cards based on answers |
| 2 | `search_terms` | repeated search term (P6.5.4) | list | one or two searches |
| 3 | `delays` | repeated float | list (packed) | old-scheduler delays; kept |
| 4 | `preview_delay` | uint32 | implicit | older preview delay; kept |
| 5 | `preview_hard_secs` | uint32 | implicit | preview delay after Hard |
| 6 | `preview_good_secs` | uint32 | implicit | preview delay after Good |
| 7 | `preview_again_secs` | uint32 | implicit | preview delay after Again |

Meguri never creates filtered decks; it keeps them intact (R1483).

#### P6.5.4 Search term

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `search` | string | implicit | search text |
| 2 | `limit` | uint32 | implicit | card limit |
| 3 | `order` | enum | implicit | 0 oldest reviewed first, 1 random, 2 intervals ascending, 3 intervals descending, 4 lapses, 5 added, 6 due, 7 reverse added, 8 retrievability ascending, 9 retrievability descending, 10 relative overdueness |

### P6.6 Deck config (`deck_config.config`)

| # | Name | Type | Presence | Meaning |
|---|---|---|---|---|
| 1 | `learn_steps` | repeated float | list | learning steps in minutes |
| 2 | `relearn_steps` | repeated float | list | relearning steps in minutes |
| 3 | `fsrs_params_4` | repeated float | list | FSRS-4 parameters (schema-11 `fsrsWeights`) |
| 4 | `easy_days_percentages` | repeated float | list | Easy Days, one value per weekday |
| 5 | `fsrs_params_5` | repeated float | list | FSRS-5 parameters |
| 6 | `fsrs_params_6` | repeated float | list | FSRS-6 parameters |
| 9 | `new_per_day` | uint32 | implicit | new cards per day |
| 10 | `reviews_per_day` | uint32 | implicit | reviews per day |
| 11 | `initial_ease` | float | implicit | starting ease as a ratio (2.5) |
| 12 | `easy_multiplier` | float | implicit | easy bonus |
| 13 | `hard_multiplier` | float | implicit | hard interval |
| 14 | `lapse_multiplier` | float | implicit | new interval after a lapse |
| 15 | `interval_multiplier` | float | implicit | interval modifier |
| 16 | `maximum_review_interval` | uint32 | implicit | maximum interval in days |
| 17 | `minimum_lapse_interval` | uint32 | implicit | minimum interval after a lapse |
| 18 | `graduating_interval_good` | uint32 | implicit | graduating interval |
| 19 | `graduating_interval_easy` | uint32 | implicit | easy interval |
| 20 | `new_card_insert_order` | enum | implicit | 0 due order, 1 random |
| 21 | `leech_action` | enum | implicit | 0 suspend, 1 tag only |
| 22 | `leech_threshold` | uint32 | implicit | lapses before a card is a leech |
| 23 | `disable_autoplay` | bool | implicit | do not play audio automatically |
| 24 | `cap_answer_time_to_secs` | uint32 | implicit | maximum answer time recorded |
| 25 | `show_timer` | bool | implicit | show the on-screen timer |
| 26 | `skip_question_when_replaying_answer` | bool | implicit | replaying the answer skips the question audio |
| 27 | `bury_new` | bool | implicit | bury new siblings |
| 28 | `bury_reviews` | bool | implicit | bury review siblings |
| 29 | `bury_interday_learning` | bool | implicit | bury interday-learning siblings |
| 30 | `new_mix` | enum | implicit | review mix (below) for new cards |
| 31 | `interday_learning_mix` | enum | implicit | review mix (below) for interday learning |
| 32 | `new_card_sort_order` | enum | implicit | 0 template, 1 no sort, 2 template then random, 3 random note then template, 4 random card |
| 33 | `review_order` | enum | implicit | 0 due date, 1 due date then deck, 2 deck then due date, 3 intervals ascending, 4 intervals descending, 5 ease ascending, 6 ease descending, 7 retrievability ascending, 8 random, 9 order added, 10 reverse order added, 11 retrievability descending, 12 relative overdueness |
| 34 | `new_card_gather_priority` | enum | implicit | 0 deck, 1 lowest position, 2 highest position, 3 random notes, 4 random cards, 5 deck then random notes |
| 35 | `new_per_day_minimum` | uint32 | implicit | unused by Anki; kept |
| 36 | `question_action` | enum | implicit | 0 show answer, 1 show reminder |
| 37 | `desired_retention` | float | implicit | FSRS desired retention |
| 38 | `stop_timer_on_answer` | bool | implicit | stop the on-screen timer on reveal |
| 40 | `historical_retention` | float | implicit | retention assumed for missing history (schema-11 `sm2Retention`) |
| 41 | `seconds_to_show_question` | float | implicit | Auto Advance question time |
| 42 | `seconds_to_show_answer` | float | implicit | Auto Advance answer time |
| 43 | `answer_action` | enum | implicit | 0 bury card, 1 answer Again, 2 answer Good, 3 answer Hard, 4 show reminder |
| 44 | `wait_for_audio` | bool | implicit | Auto Advance waits for audio |
| 45 | `param_search` | string | implicit | optimizer search (schema-11 `weightSearch`) |
| 46 | `ignore_revlogs_before_date` | string | implicit | optimizer cut-off date, `YYYY-MM-DD` or empty |
| 255 | `other` | bytes | implicit | legacy JSON keys (P7) |

Review mix values: 0 mix with reviews, 1 show after reviews, 2 show before reviews.

Numbers 7, 8 and 39 are reserved by Anki (7 and 8 for future FSRS parameter lists); Meguri treats them as unknown fields (P4), so a later Anki's next parameter list survives in the blob.

Example: the default preset of a new Anki collection (oracle), in canonical form:

```
0a 08 0000803f 00002041     learn_steps [1.0, 10.0]
12 04 00002041              relearn_steps [10.0]
22 1c 0000803f ×7           easy_days_percentages [1.0 ×7]
48 14                       new_per_day 20
50 c801                     reviews_per_day 200
5d 00002040                 initial_ease 2.5
65 6666a63f                 easy_multiplier 1.3
6d 9a99993f                 hard_multiplier 1.2
7d 0000803f                 interval_multiplier 1.0     (lapse_multiplier 0.0 left out)
8001 949d02                 maximum_review_interval 36500
8801 01                     minimum_lapse_interval 1
9001 01                     graduating_interval_good 1
9801 04                     graduating_interval_easy 4
a801 01                     leech_action 1              (insert order 0 left out)
b001 08                     leech_threshold 8
c001 3c                     cap_answer_time_to_secs 60
ad02 6666663f               desired_retention 0.9
c502 6666663f               historical_retention 0.9
e002 01                     wait_for_audio true
```

## P7. `other`

- Field 255 of the notetype config, field config, template config, deck common and deck config holds the UTF-8 bytes of a JSON object: keys from the legacy JSON form that the protobuf form has no field for, such as add-on keys (R1464).
- Protobuf code treats it as opaque bytes: it never parses, validates, normalizes or rewrites them. A protobuf write of a record carries the `other` bytes it read, unchanged, even when they are not valid JSON.
- Only the schema-11 conversion reads and builds `other` (`model/schema11-json` S3).

## P8. Model-level rules for some fields

- FSRS parameter lists (R1486): `fsrs_params_4`, `fsrs_params_5` and `fsrs_params_6` are separate fields and are never merged, copied or cleared into each other. Meguri's model API offers a setter for `fsrs_params_6` only, the list of the FSRS version Meguri runs (R696); its only caller is the optimizer (U47). The two other lists, and any later list (an unknown field from a later Anki, or a `fsrsParams7`-style key in `other` that arrived through schema-11 JSON), are read-only and survive every write.
- Fields Meguri does not show or use survive every write of their record: SM-2 options and Easy Days in presets (R1484); `latex_pre`, `latex_post`, `latex_svg`, `reqs`, `q_format_browser`, `a_format_browser`, `original_stock_kind`, `original_id`, the field `tag` and `prevent_deletion` in note types (R1485, R903, R904); `browser_collapsed`, `extend_new`, `extend_review` and `learning_studied` in decks (R322).
- Meguri changes a record by editing the decoded fields it means to change; everything else, unknown fields included, comes from what was read (P5).

## P9. Decode errors

A blob that breaks P2 is refused with a typed error naming the column and the reason. The reasons Meguri distinguishes:

| Reason | When |
|---|---|
| truncated | the buffer ends inside a key, varint, fixed value, length-delimited payload or group; for the fields of a nested message, the buffer is that message's payload (P2) |
| bad varint | a varint longer than 10 bytes or with a tenth byte above `01`, or a key above 32 bits |
| bad field number | field number 0 |
| bad wire type | wire type 6 or 7, or a declared field with a wire type P2 does not accept |
| bad group | a group-end key with no open group, or whose field number differs from the innermost open group's |
| too deep | a field at depth 100 or more (P2) |
| bad UTF-8 | a `string` field that is not valid UTF-8 |
| no deck kind | a deck kind blob with neither member (P6.5) |

When several reasons apply, the first problem met reading from the start decides. At one key, the key's varint, field number and wire type 6 or 7 are checked first. Then a key of a field the message declares is checked against the wire types P2 accepts (a group-end key of a declared number is a bad wire type, as `0c` below); a group-end key of any other number is checked against the open groups (bad group); any other key is checked against the depth limit (too deep). Inside a group every key is part of an unknown field, so no field there counts as declared.

Examples, each a whole `deck_config.config` value unless named otherwise; Anki refuses every one that has a reason here and decodes the last two (`conformance/scenarios/model/proto/decode-limits.yaml`):

| Blob | Reason |
|---|---|
| `0a05` | truncated |
| `ff` | truncated (a key varint that never ends) |
| `0a020000` | truncated (packed floats need 4 bytes each) |
| `3b` | truncated (a group never closed) |
| `0a015b` as `decks.kind` | truncated (the group is still open at the end of the normal deck's payload) |
| `0a015b5c` as `decks.kind` | truncated (the same group; its end lies after the payload) |
| `38ffffffffffffffffff02` | bad varint (tenth byte `02`) |
| `808080801000` | bad varint (a key of 2^32) |
| `0000` | bad field number |
| `0800` | bad wire type (a varint for a float list) |
| `0f` | bad wire type (7) |
| `0c` | bad wire type (a group end for field 1, a float list) |
| `3b44` | bad group (field 8 ends while group 7 is open) |
| 101 × `3b`, then 101 × `3c` | too deep (the 101st group start is at depth 100) |
| `38ffffffffffffffffff01` | none: unknown field 7, a 10-byte varint |
| `fa0f02ffff` | none: `other` holding bytes that are not UTF-8 (`other` is `bytes`) |

The fuzz target `core/fuzz/fuzz_targets/proto_blobs.rs` feeds arbitrary bytes to every decoder: every input gives a record or one of these errors, every record that decodes re-encodes (P3, P4) to bytes that decode to an equal record (P4: known values bit for bit, unknown fields per field number in order; the one exception is a singular float field holding -0.0, which comes back as +0.0, P3), and encoding that record again gives the same bytes.
