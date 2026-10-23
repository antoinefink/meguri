# Sync messages (protocol v11 bodies)

Spec: sync/wire-format
Requirements: R1513, R1521, R1522, R1523, R1529, R1530, R1575, R1577, R1579, R1580, R1581; R1464 (sync use), R1562 (decoders)
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/wire/

The JSON (and zip) inside each request and response of the sync protocol: what Meguri writes, what it accepts, and the exact reading rules of every member. How a body travels (the URL, the `anki-sync` header, zstd, `anki-original-size`, status codes) is `transport.md`; login is `login-endpoints.md`; what the status check decides from `meta` is `meta-status.md`; what a failed decode becomes is `errors.md`. What a normal sync does with the records (the merge rules, which rows are pending, the hold-back) is U107's; one-way sync is U109's; media sync is U111's. This file is only the shapes.

Facts marked "oracle" come from Anki 26.09.3 clients syncing with the official server of the same release, recorded in `conformance/recordings/scenarios/sync/wire/` (one exchange per line, bodies zstd-decoded and shown as JSON with keys sorted, so member order there is not the wire's; `specs/conformance/sync-ops.md` S9).

## W1. Conventions

### W1.1 Writing

- Every JSON body is one compact JSON text (no white space), written per `model/json-numbers` N3, N4 and N8. Members come in the order the tables below list them; a receiver never depends on member order.
- The schema-11 objects (note types, decks, presets) are written per `model/schema11-json` (output direction), including their `other` members (R1464).
- A method without arguments sends `{}` (`transport.md` T5).

### W1.2 Reading

Every response body Meguri decodes is read with these rules; a body that breaks any of them is a decode failure, which `errors.md` E6 turns into the kind `bad_response`. No input ever panics (R1562).

- The body is JSON per `model/json-numbers` N9 and N10. Arrays and objects may nest at most 127 levels deep, the outermost value counting as level 1; deeper nesting makes the whole body invalid, as in Anki's reader.
- An object member that this file does not name is ignored, at every level of the envelope (the server sends `hostNum` in `meta`, for example). Members of schema-11 objects follow `model/schema11-json` instead (unknown ones are kept in `other`).
- An envelope object that repeats a member this file reads is invalid.
- A member marked "required" must be present; `null` is not a value of any type here unless the table says so.
- Number readers, by declared type (`model/json-numbers` N1):
  - **strict**: a JSON number without fraction or exponent, within the type's range (N2). `1.0`, `"1"` and `true` do not fit.
  - **loose integer** (W11): any JSON number, converted as W11 says. Only the tuple elements marked so use it.
- `bool` takes only `true` and `false`; `string` only a JSON string.
- Tuples (W8, W9, W10, W12, W16) are JSON arrays read by position, with exactly the listed number of elements unless the table says the last ones may be missing. Anki would also read a tuple written as an object with named members; no server sends one, and Meguri refuses it.

### W1.3 Values that are counts or ids

Ids are `i64`, update sequence numbers `i32`, times in seconds or milliseconds `i64`, as in `model/row-records`. Counts are `u32`.

## W2. `hostKey`

`login-endpoints.md` L5.

## W3. `meta` (R1513)

Request:

| Member | Value |
|---|---|
| `v` | the number 11 |
| `cv` | the client identity (`transport.md` T3) |

Response, one object:

| Member | Reader | Required | Meaning |
|---|---|---|---|
| `mod` | strict `i64` | yes | the server collection's modification time, epoch milliseconds |
| `scm` | strict `i64` | yes | its schema time, epoch milliseconds |
| `usn` | strict `i32` | yes | its update sequence number |
| `ts` | strict `i64` | yes | the server's clock now, epoch seconds |
| `msg` | `string` | yes | a message for the user, often empty |
| `cont` | `bool` | yes | false when the server refuses to sync with this client |
| `empty` | `bool` | no; missing means false | true when the server collection has no cards |
| `media_usn` | strict `i32` | yes | the server's media update sequence number |
| `hostNum` | ignored | no | a number older protocol versions used; the official server sends 0 |

Example (oracle): `{"cont":true,"empty":false,"hostNum":0,"media_usn":0,"mod":1894701720000,"msg":"","scm":1894701660000,"ts":1894701780,"usn":2}`.

- Anki requires `hostNum` and refuses a body without it; Meguri does not need it and accepts its absence.
- What the client does with the record is `meta-status.md`.

## W4. `start` (R1521)

Request:

| Member | Value |
|---|---|
| `minUsn` | strict `i32`: the local `col.usn`, the server's update sequence number as of the last sync |
| `lnewer` | `bool`: true when the local `col.mod` is greater than the server's `mod` from `meta` |
| `graves` | always `null` (older clients sent their graves here; v11 sends them with `applyGraves`) |

Example (oracle): `{"minUsn":2,"lnewer":false,"graves":null}`.

Response: a graves object (W5.1), the deletions made on the server since `minUsn`.

## W5. Graves and `applyGraves` (R1522)

### W5.1 Graves object

| Member | Reader | Required |
|---|---|---|
| `cards` | array of strict `i64` card ids | yes |
| `decks` | array of strict `i64` deck ids | yes |
| `notes` | array of strict `i64` note ids | yes |

Meguri writes the members in this order, each array possibly empty. Order inside an array means nothing.

### W5.2 `applyGraves`

- Request: `{"chunk": <graves object>}` holding at most 250 ids in all three arrays together. A sync with no pending graves sends no `applyGraves` request; one with more sends as many requests as needed. Ids are taken from the card graves first, then the note graves, then the deck graves, so a request holds deck ids only after every card and note id went (the request that empties the card list fills its remaining room with notes, and so on).
- Response: the JSON text `null`. Meguri does not read the body beyond the transport's checks (`transport.md` T6.2), as Anki does not.

Example (oracle): `{"chunk":{"cards":[1894701600004,1894701600003],"decks":[],"notes":[1894701600002]}}`.

## W6. `applyChanges` (R1523, R1464)

### W6.1 The changes object

Request: `{"changes": <changes object>}`. Response: a changes object, the server's side.

| Member | Value | Reader | Required |
|---|---|---|---|
| `models` | array of note type objects (`model/schema11-json` S4) | each element per S4 input | yes |
| `decks` | a two-element array: first the deck objects (S7-S9), then the preset objects (S10) | W6.3 | yes |
| `tags` | array of tag names, as stored in the `tags` table | array of `string` | yes |
| `conf` | an object: every config key mapped to its value | W6.4 | no; `null` or missing means "not sent" |
| `crt` | the collection creation time, `col.crt`, epoch seconds | strict `i64` | no; `null` or missing means "not sent" |

- A side sends `conf` and `crt` only when its collection is the newer one: the client when it sent `lnewer` true in `start`, the server when the client sent false (R1523; who applies them is U107's, `storage/config-table` C9). The other members are always present, as empty arrays when nothing changed: `{"models":[],"decks":[[],[]],"tags":[]}`.
- What a side puts in `models`, the first and second `decks` arrays and `tags`: the note types, decks, presets and tags that are pending (U107 decides which; with hold-back, from the sync's snapshot).
- A refused object (`model/schema11-json` S1 "refuse") in a response makes the body a decode failure: no partial set of objects is applied.

### W6.2 `usn` in sent objects

Every note type, deck and preset object the client sends carries, as its `usn`, the server's update sequence number from this sync's `meta`, not the stored `-1`. The server stores the objects with that `usn` and sends them to other clients with it. Objects in a response carry whatever the server stores.

Example (oracle, abridged): a deck whose row holds `usn` -1 is sent as `{"id":1894701600000,"mod":1894701780,"name":"Words","usn":2,…,"x_deck":{"color":"#3a7","pinned":true}}` when `meta` gave `usn` 2.

### W6.3 `decks`

- Written: `[[<deck objects>],[<preset objects>]]`.
- Read: an array of exactly two arrays, decks first, presets second. Each deck object is read per `model/schema11-json` S7 input (with S8 or S9 by its `dyn`), each preset per S10 input. Anki would also read an object `{"decks":[…],"config":[…]}`; Meguri accepts that form too and never writes it.

### W6.4 `conf`

- Written: one member per row of the `config` table, the key as stored and the value as the JSON text the row's `val` holds (`storage/config-table` C2), without reformatting. A row whose `val` is not a BLOB holding valid JSON cannot be sent: the sync fails before `applyChanges` with the kind `db_check_required` (origin local, `errors.md` E7). Anki's sync fails on such a row too (`storage/config-table` C9).
- Read: a JSON object whose member values may be any JSON. A repeated key keeps its last value. Each value is kept as parsed; storing it is U107's, in the compact form of `storage/config-table` C5.

### W6.5 What cannot travel

Unknown protobuf fields of the records (`model/protobuf-records` P4) have no JSON form, so a schema-11 object cannot carry them: they stay on the device that has them, in Meguri as in Anki. Unknown JSON members do travel, as `other` members (R1464, `model/schema11-json` S3).

## W7. `chunk` and `applyChunk` (R1529)

### W7.1 The chunk object

| Member | Reader | Required | Meguri writes |
|---|---|---|---|
| `done` | `bool` | no; missing means false | always |
| `revlog` | array of review tuples (W10) | no; missing means empty | only when not empty |
| `cards` | array of card tuples (W8) | no; missing means empty | only when not empty |
| `notes` | array of note tuples (W9) | no; missing means empty | only when not empty |

### W7.2 Exchanges

- `chunk`: request `{}`; response a chunk object. The client asks again until a response has `done` true.
- `applyChunk`: request `{"chunk": <chunk object>}`; response `null`, not read beyond the transport's checks. The client sends until it has sent a chunk with `done` true.
- A chunk holds at most 250 objects across its three arrays. A sender builds each chunk from its remaining pending ids by taking, in turns, one review id, one note id and one card id (skipping a list once it is empty) until it has 250 objects or nothing is left. `done` is true exactly when the chunk holds fewer than 250 objects. So the last chunk may be empty (`{"done":true}`), and it is whenever the objects fill the earlier chunks exactly. Any split within the limit is accepted by the server; this one is Anki's.

Example (oracle): an empty server chunk `{"done":true}`; a client chunk `{"chunk":{"done":true,"revlog":[[1894701780000,1894701600000,2,3,-600,0,224,4200,0]],"cards":[…],"notes":[…]}}`.

## W8. Card tuple

Eighteen elements, the `cards` row (`model/row-records` R4) in column order:

| # | Column | Written | Reader |
|---|---|---|---|
| 1 | `id` | as stored | strict `i64` |
| 2 | `nid` | as stored | strict `i64` |
| 3 | `did` | as stored | strict `i64` |
| 4 | `ord` | as stored | strict `u16` |
| 5 | `mod` | as stored (seconds) | loose integer → `i64` |
| 6 | `usn` | the server's `usn` from `meta` (W6.2 applies to cards, notes and reviews too) | strict `i32` |
| 7 | `type` | as stored | strict, exactly 0, 1, 2 or 3 |
| 8 | `queue` | as stored | strict, exactly -3, -2, -1, 0, 1, 2, 3 or 4 |
| 9 | `due` | as stored | loose integer → `i32` |
| 10 | `ivl` | as stored | loose integer → `u32` |
| 11 | `factor` | as stored | strict `u16` |
| 12 | `reps` | as stored | strict `u32` |
| 13 | `lapses` | as stored | strict `u32` |
| 14 | `left` | as stored | strict `u32` |
| 15 | `odue` | as stored | loose integer → `i32` |
| 16 | `odid` | as stored | strict `i64` |
| 17 | `flags` | as stored | strict `u8` |
| 18 | `data` | W8.1 | `string` |

"As stored" means the value as Meguri reads the row (`model/row-records` R4, read tolerances included).

### W8.1 `data`

- Written: the text that `model/card-data-json` C4 builds from the card's values, the same text Meguri writes when it writes the row. A sender rebuilds it rather than copying the stored text: a stored `"decay":0.1542` goes out as `"decay":0.154` (oracle, `sync/wire/records-up`). Meguri's unknown keys (C3 rule 7) go out with it; Anki receivers drop them (deviation `card-data-unknown-keys`).
- Read: any string; it is card data text, parsed per C3 when the card is stored (U107).

Example (oracle): `[1894701600000,1894701600000,1894701600000,0,1894701780,2,1,1,1894702519,0,0,1,0,1,0,0,0,"{\"pos\":1,\"s\":2.3065,\"d\":2.118,\"dr\":0.87,\"decay\":0.154,\"lrt\":1894701780}"]`.

## W9. Note tuple

Eleven elements:

| # | Column | Written | Reader |
|---|---|---|---|
| 1 | `id` | as stored | strict `i64` |
| 2 | `guid` | as stored | `string` |
| 3 | `mid` | as stored | strict `i64` |
| 4 | `mod` | as stored (seconds) | strict `i64` |
| 5 | `usn` | the server's `usn` from `meta` | strict `i32` |
| 6 | `tags` | W9.1 | `string` |
| 7 | `flds` | the field values joined by U+001F, as stored | `string` |
| 8 | `sfld` | always `""` | `string`, ignored |
| 9 | `csum` | always `""` | `string`, ignored |
| 10 | `flags` | always 0 | strict `u32`, ignored |
| 11 | `data` | always `""` | `string`, ignored |

- Elements 8 to 11 carry nothing: a sender writes these constants whatever the row holds, and a receiver recomputes the sort field and checksum itself (R1528, U107) and stores `flags` 0 and `data` empty. A receiver still refuses a tuple whose element 8 to 11 has the wrong JSON type (a number for `sfld`, for example), as Anki does.
- The `notes.mod` element has no loose reading: `1894701780.0` is refused.

### W9.1 `tags`

- Written: the note's tags, split from the stored column at every space (U+0020) and every ideographic space (U+3000), empty pieces dropped, then joined with one space and with one space added at each end: `" jlpt::n5 vocab "`. No tags gives `""`.
- Read: the same split; storing is U107's (with `model/row-records` R3's form).

Example (oracle): `[1894701780000,"Gc//j`9Cht",1894701600006,1894701780,2," jlpt::n5 vocab ","moon\u001fluna","","",0,""]`.

## W10. Review tuple

Nine elements, the `revlog` row (`model/row-records` R5):

| # | Column | Written | Reader |
|---|---|---|---|
| 1 | `id` | as stored | strict `i64` |
| 2 | `cid` | as stored | strict `i64` |
| 3 | `usn` | the server's `usn` from `meta` | strict `i32` |
| 4 | `ease` | as stored | strict `u8` |
| 5 | `ivl` | as stored; negative values (seconds) stay negative | loose integer → `i32` |
| 6 | `lastIvl` | as stored | loose integer → `i32` |
| 7 | `factor` | as stored | loose integer → `u32` |
| 8 | `time` | as stored | loose integer → `u32` |
| 9 | `type` | as stored | W10.1 |

### W10.1 `type`

- The element may be missing: an eight-element tuple reads as `type` 0.
- A value that is not an integer from 0 to 5 (a string, `null`, `7`, `1.0`) reads as 0. It never refuses the tuple.

Example (oracle): `[1894701780000,1894701600000,2,3,-600,0,224,4200,0]`.

## W11. Loose integers

The tuple elements marked "loose integer" accept every JSON number, because older servers wrote some of them as floats:

1. A number written without fraction or exponent counts as its integer value; one outside the `i64` range counts as the nearest `i64` bound.
2. A number with a fraction or an exponent is read as the nearest 64-bit float (N2), then truncated toward zero, a value beyond the `i64` range giving the nearest bound.
3. The `i64` from steps 1 and 2 becomes the element's type:
   - `i64`: unchanged;
   - `i32`: its low 32 bits, as a two's-complement value (wrapping);
   - `u32`: 0 when it is negative, otherwise its low 32 bits.
4. Anything that is not a number (a string, `null`, `true`) refuses the tuple.

| Element | JSON | Value |
|---|---|---|
| card `due` (`i32`) | `1.0` | 1 |
| card `due` | `1.9` | 1 |
| card `due` | `-0.5` | 0 |
| card `due` | `1e3` | 1000 |
| card `due` | `2147483648` | -2147483648 |
| card `due` | `9223372036854775808` | -1 (the `i64` bound 9223372036854775807, low 32 bits) |
| card `ivl` (`u32`) | `-5` | 0 |
| card `ivl` | `4294967301` | 5 |
| review `ivl` (`i32`) | `-600` | -600 |
| review `factor` (`u32`) | `2500.0` | 2500 |
| card `mod` (`i64`) | `1894701780.0` | 1894701780 |
| card `due` | `"1"` | the tuple is refused |

## W12. `sanityCheck2` (R1530)

### W12.1 Counts tuple

Eight elements, every count a strict `u32`:

| # | Meaning |
|---|---|
| 1 | due counts: a three-element array `[new, learning, review]`; Meguri, like Anki, sends `[0,0,0]` |
| 2 | number of rows in `cards` |
| 3 | number of rows in `notes` |
| 4 | number of rows in `revlog` |
| 5 | number of rows in `graves` |
| 6 | number of note types (rows in `notetypes`) |
| 7 | number of rows in `decks` |
| 8 | number of rows in `deck_config` |

Which state the client counts (with hold-back, the sync's outgoing view) is U107's (R1530).

### W12.2 Exchange

- Request: `{"client": <counts tuple>}`.
- Response:

  | Member | Reader | Required |
  |---|---|---|
  | `status` | exactly the string `"ok"` or `"bad"` | yes |
  | `c` | a counts tuple: the client's counts as the server compared them | no: missing, `null` or anything that is not a valid counts tuple counts as absent |
  | `s` | a counts tuple: the server's own counts | no, same as `c` |

- `"bad"` is the failed sanity check (`errors.md`, kind `sanity_check_failed`, with `c` and `s` as its parameters when present). Any other `status` value refuses the body.
- The official server sets the due counts of `c` to zeros and the graves count of both `c` and `s` to 0 before comparing (oracle: the client sent graves 3, the answer shows 0).

Example (oracle): request `{"client":[[0,0,0],4,3,2,3,9,4,2]}`, answer `{"status":"ok","c":[[0,0,0],4,3,2,0,9,4,2],"s":[[0,0,0],4,3,2,0,9,4,2]}`.

## W13. `finish` and `abort`

- `finish`: request `{}`; response a JSON integer, read as strict `i64`: the server's new modification time in epoch milliseconds, which the client stores as its `mod` and `ls` (R1531, U107). Example (oracle): `1894701780000`.
- `abort`: request `{}`; response `null`. A client sends it after a failure and does not wait for the answer (R1532, U107).

## W14. `upload` and `download`

Raw collection bytes, not JSON (`transport.md` T5, T6.2). The `upload` answer is plain text: exactly `OK` on success, any other text being the server's refusal (`one-way#O6.4`). The `download` request body is `{}`; its answer is the server's collection file (`one-way#O7`). OK supplies no accepted-file hash, request receipt or transaction id; `anki-transfers#M4` governs lost/uncertain acknowledgement.

## W15. Media result envelope

Every media method's response except `downloadFiles` is an envelope object:

| Member | Meaning |
|---|---|
| `data` | the method's result (W16) |
| `err` | a string; empty on success |

Read:

1. If `data` is present and reads as the method's result, the call succeeded. `err` is then ignored, whatever it holds.
2. Otherwise, if `err` is a string, the server refused the call: the kind `server_message` with that text (`errors.md`).
3. Otherwise the body is a decode failure.

The official server always sends `{"data":…,"err":""}` on success.

## W16. Media methods

| Method | Request | `data` of the response |
|---|---|---|
| `begin` | `{"v": <client identity>}` (`transport.md` T3) | `{"usn": <strict i32>, "sk": <string>}`: the server's media update sequence number; `sk` is ignored (the official server puts the host key there) |
| `mediaChanges` | `{"lastUsn": <strict i32>}`: the last media update sequence number the client has applied | an array of change tuples, W16.1 |
| `downloadFiles` | `{"files": [<file name>, …]}`, at most 25 names (R1577) | no envelope: the body is a zip, W17.1 |
| `uploadChanges` | the body is a zip, W17.2 (not JSON) | a two-element array `[<processed: strict u32>, <current usn: strict i32>]` (`processed` is at most 25 from the official server): how many of the zip's changes the server processed, and its media update sequence number afterwards |
| `mediaSanity` | `{"local": <strict u32>}`: the number of media files the client has | the string `"OK"` when the server's count matches, `"mediaSanity"` when it does not; any other value is a decode failure |

### W16.1 Media change tuple

Three elements: `[<file name: string>, <usn: strict i32>, <sha1: string>]`. `sha1` is the file's SHA-1 in 40 lower-case hex digits, or `""` when the change is a deletion. What the client does with each change is U111's (R1576).

## W17. Media zips

Both zips use the standard zip format with each entry stored uncompressed. Meguri writes stored entries; when reading it accepts stored and deflated entries. Each zip has, besides the files, one entry named `_meta`.

### W17.1 `downloadFiles` answer

- File entries are named `0`, `1`, `2`, … in the order the server packed them, each holding one file's bytes.
- `_meta` holds a JSON object mapping each entry name to the file's real name: `{"0":"sun.mp3","1":"moon.jpg"}`.
- A file entry whose name `_meta` does not map is a decode failure for the whole zip. A `_meta` member with no matching entry is ignored.
- The zip may hold fewer files than were asked for: the server leaves out names its media database does not list (and answers 409 when a listed file is missing from its disk). What the client does with the names left over is U111's; it never repeats forever a request that returned no file.

### W17.2 `uploadChanges` request

- The zip carries one batch of at most 25 pending changes, in batch order. Additions (a file with content) are entries named by their position in the batch, counting deletions too: a batch of an addition, a deletion and an addition has entries `0` and `2`.
- `_meta` holds a JSON array with one two-element array per change, in batch order: `[<real name>, "<entry name>"]` for an addition, `[<real name>, null]` for a deletion. Example: `[["sun.mp3","0"],["old.png",null],["moon.jpg","2"]]`.
- The official server refuses a zip whose `_meta` is longer than 50 KiB or holds more than 25 changes, a real name longer than 255 bytes, not in Unicode NFC or containing `/`, and a file of 0 bytes or over 100 MiB. Meguri never sends such a zip: U111 keeps names at 120 bytes or less and excludes empty and oversized files (R1565, R1568).
- Which changes go into a batch, and the 2.5 MiB fill limit (R1579), are U111's. The `processed` answer and the update sequence number rule (R1580) are U111's.

## W18. Decoder coverage

Every response decoder of this file (W3 to W17, `hostKey` included) is a fuzz target input (`core/fuzz/fuzz_targets/sync_json.rs`): arbitrary bytes either decode or give a decode failure, never a panic, an abort, unbounded memory or a stack overflow (R1562).

## W19. Scenarios and goldens

| Scenario | Shows | Golden |
|---|---|---|
| `sync/wire/notetypes` | note type objects as sent: a plain type, one with add-on members at all three levels, a cloze type (W6.1) | recorded |
| `sync/wire/decks` | deck objects: default, normal with limits and add-on members, nested name, filtered (W6.1, W6.3) | recorded |
| `sync/wire/presets` | preset objects with `fsrsWeights`, `fsrsParams5`, `fsrsParams6` and add-on members at the top level and in `new` and `lapse` (W6.1, W6.3) | recorded |
| `sync/wire/records-down` | an Anki client sends every kind of record and the Meguri actor receives them (W4 to W13) | recorded, with a recording |
| `sync/wire/records-up` | the Meguri actor sends the same kinds and an Anki client receives them, including a rebuilt card `data` (W8.1) | recorded, with a recording |

- The three object scenarios are T0 scenarios: their `legacy_json` values are the objects a sender puts in `applyChanges`, except `usn` (W6.2). U106's tests use them as encode-decode pairs: decoding each golden object and encoding it again gives an equal JSON value, and encoding the record gives the golden object (compared as parsed JSON, so key order does not count).
- The two actor scenarios are live (pending until Meguri's executor syncs). Their recordings under `conformance/recordings/scenarios/sync/wire/` hold every request and response body of the oracle playing both sides: U106's tests take the tuples, graves, changes, counts and `meta` records there as decoder inputs and as expected encoder outputs (comparing parsed JSON).
- The loose-integer rules (W11), the eight-element review tuple and the invalid `type` (W10.1), the `decks` object form (W6.3), the envelope rules (W15) and the zips (W17) have no server that sends them: U106's tests cover them from the tables here, against the scripted stub (`transport.md` T13).
