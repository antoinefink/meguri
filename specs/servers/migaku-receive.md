# Migaku `/receive`

Spec: servers/migaku-receive
Requirements: R1691, R1692, R1693, R1694, R1695, R1696, R1697, R1698, R1699, R1700, R1701, R1702, R1703, R1704, R1705, R1706, R1707, R1729, R1730, R1797 (Migaku branch); supports R1709, R1721, R1800, R1946, R1948; AE72, AE73
Anki: 26.09.3 (the note add and the media store, through the specs cited)
Scenarios: `conformance/recordings/servers/migaku/receive/` (declared, `servers/transcripts` T13; id `servers/migaku/receive/<P24 row id>`) holds 31 of P24's 35 rows, some of them in part. P24 names the rows and the parts that run in Meguri's own tests, and why.

How Meguri's Migaku server turns one `POST /receive` into exactly one note with its media: atomically, without duplicates when the client retries, and in arrival order. It also covers the wait when the collection is busy.

Interfaces: the Migaku browser extension 1.30.16 and 1.31.0.17 (Chrome bundle versions), which post to `http://127.0.0.1:44432/receive`. The Migaku Anki add-on 0.6.6 serves this route in Anki, and it is the compatibility reference for the payload, the mapping sources and the reply shapes. P23 lists where Meguri differs from it.

Other specs cover the parts around this route:

- binding, the `Host` check, body limits (H8), the collection waiter (H9.2), the integrations store (H10), host events (H11.1), the audio transcoder (H11.3), the shared fetcher (H13) and the server lifecycle: `servers/host-security`;
- origins, CORS headers, `OPTIONS`, 404 and 405, the body-limit reply and the WebSocket: `servers/migaku-protocol`;
- destination profiles, field maps, defaults, language detection and the decision kind: `servers/migaku-routing`;
- bracket syntax: `servers/migaku-syntax`;
- the received log after a row is written, moves, Retry and legacy `/create`: `servers/migaku-received-log`.

## P1. Terms

- **Arrival**: the instant the server has read the whole request body. Every timer in this file starts at arrival, and arrival order is the order of these instants. Arrival times are read from the host's wall clock and the timers run on its timer clock (`servers/host-security` H21), both injectable for tests.
- **Item**: one element of `sentenceAudio`, `wordAudio` or `images`. Its **key** is the array's name, and its **index** is its 1-based position in that array.
- **Raw id**: an item's `id` exactly as sent, when it is a JSON string.
- **Plan**: the resolved destination that routing returns (P5).
- **Retry key**: the raw ids of a request's items, whether it has any item, and its `targetWord` and `sentence` pair. P12 compares retry keys.
- **The switch**: the pending-reply switch of P14. It is off.

## P2. Stages

A request goes through these stages in this order. A failure at any stage ends the request with the reply of P18 and changes nothing in the collection or the media folder.

1. Parse and check the body (P3, P4).
2. Early retry check (P12): a match replies at once.
3. Resolve the destination (P5) and check the field plan (P7).
4. Read or download the items that the plan uses (P8 to P11). This runs at the same time as stage 5's wait.
5. Wait for this request's turn (P13) and for the collection (P14).
6. Commit (P16): retry check, media staging and the note add, in one op.
7. Complete or write the received-log row (P17 step 3, P20), with its warnings (P21), and reply.

## P3. The body

- The route is `POST /receive`. The method, origin and `OPTIONS` rules are in `servers/migaku-protocol`.
- The body is read as UTF-8 JSON whatever the `Content-Type`, and also when that header is missing (R1691). A leading U+FEFF is ignored.
- The body must be one JSON object. Keys other than those of P3's table are ignored. When a key occurs twice, its last occurrence counts.
- Text keys: `targetWord`, `sentence`, `translation`, `definitions`, `exampleSentences`, `notes`. Each is a JSON string or `null`. A missing key and `null` both read as the empty string. Any other JSON type is an error.
- Item keys: `sentenceAudio`, `wordAudio`, `images`. Each is a JSON array or `null`. A missing key and `null` both read as an empty array. Any other type is an error, and so is an element that is not a JSON object.
- The payload carries no deck, note type or language (R1692).

Example payload (synthetic):

```json
{"targetWord": "猫[ねこ;h]",
 "sentence": "<strong>猫[ねこ;h]</strong> が[;] 好[す;n2]き{。}",
 "translation": "I have a cat.", "definitions": "<p>cat</p>",
 "exampleSentences": "", "notes": "",
 "sentenceAudio": [{"id": "a1b2c3d4e5f", "title": "s", "input": "data:audio/mp4;base64,AAAAGGZ0eXBNNEEgAAAAAE00QSBpc29t", "r2Url": null, "uploadId": null}],
 "wordAudio": [],
 "images": [{"id": "0f1e2d3c4b5", "title": "shot", "src": "data:image/webp;base64,UklGRg4AAABXRUJQVlA4TAIAAAAvAA==", "alt": ""}]}
```

## P4. Items

| Key | Data field | Other fields (all ignored) |
|---|---|---|
| `sentenceAudio`, `wordAudio` | `input` | `title`, `r2Url`, `uploadId`, any other |
| `images` | `src` | `title`, `alt`, any other |

- `id`: when it is a JSON string, it is the raw id (P1). Any other value, or a missing `id`, makes the item's id **unusable**. An unusable id never stops the request (P10, P12).
- The data field must be a non-empty JSON string, either a `data:` URL or an `http`/`https` URL (P8). A missing data field, an empty one or one of another type is an error of that item, reported only when the item is used (P6).
- `r2Url` and `uploadId` are not used, even when the data field is missing (Q3).
- Stage 1 checks only that each item is a JSON object. Everything else about an item, its data field included, is checked when it is read in stage 4, and only items the plan uses are read (P6). So an unused item with no data, or with only `r2Url`, never fails the request.

## P5. Destination

Stage 3 asks routing (`servers/migaku-routing`) for a plan, from `targetWord` and `sentence` as received. Routing applies the manual override, detection and the uncertain fallback. The plan holds:

| Part | Use here |
|---|---|
| profile id and name | log row; error messages |
| language and decision kind (`Auto`, `Fallback` or `Override`; a card routed by `servers/migaku-routing` D11 step 4 is `Fallback`) | log row; routing state after the commit (P16) |
| route and the detection: the detected language or none, and `confident` or `uncertain` (D13.1) | log row (`servers/migaku-received-log` L2, L4) |
| note type id, deck id | the add (P16) |
| field plan: for each field of the note type in ordinal order, one source key of P6, `none` or `default_only`, and a default text (empty when the profile has none) | P7 |
| tags to add | the add |
| normalize audio loudness (on or off) | P11 |

The note type's current fields, in field order, are read when the plan is resolved (`servers/migaku-routing` D13.1). A field the plan does not mention gets source `none` and an empty default.

Routing's refusals (`servers/migaku-routing` D7.1) end the request with these errors (P19): no confirmed profile; the profile's note type is missing; the profile's deck is missing or not usable (R1715); the profile maps no field. The note type and the deck are checked again at commit, on the generation the commit runs on (P16). Meguri never picks another destination by itself.

## P6. Mapping sources

The source keys, and their values for one payload:

| Source key | Value |
|---|---|
| `targetWord`, `sentence`, `translation`, `definitions`, `exampleSentences`, `notes` | the text as received (P3) |
| `sentenceAudio` | one `[sound:<name>]` per `sentenceAudio` item, in array order, joined with LF `<br>` LF (the six characters `\n<br>\n`) |
| `wordAudio` | the same for `wordAudio` |
| `images` | one `<img src="<name>">` per `images` item, in array order, with the same joiner |
| `firstImage` | the `<img src="<name>">` of the first `images` item; empty when there is none |
| `restImages` | the `images` value built from the second item on; empty with fewer than two items |
| `targetWordNoSyntax`, `sentenceNoSyntax` | `servers/migaku-syntax` S5 applied to `targetWord` and to `sentence` |
| `reading` | `servers/migaku-syntax` S6 applied to `targetWord` |

`<name>` is the stored name the item gets in the media folder (P10). Stored names never hold `"`, `<`, `>` or `&`, so they are written as they are, with no escaping. (A reused file keeps its own spelling, which may hold capitals or another case variant of the name, `media/media-files` M3.)

An item is **used** when the field plan maps at least one field to a source that includes it: `sentenceAudio` or `wordAudio` for audio items; `images` for every image; `firstImage` for the first image; `restImages` for the second image on. Only used items are read, downloaded and stored. An unused item can never fail the request, and it leaves no file.

## P7. Field values

For each field of the note type, in field order:

1. Its source key gives its value (P6). With source `none` or `default_only`, the value is the empty string.
2. When that value is the empty string and the field's default text is not empty, the value is the default (R1713, `servers/migaku-routing` D5). This applies to a field with source `none` too: the 11-field Custom map has `none` on Is Vocabulary Card and Is Audio Card, and their defaults fill them.

The values are passed to the add exactly as built: no sanitizing, no reformatting, and `<strong>` and bracket syntax kept (R1694). The add then applies the save normalization of `notes/note-records` N2 (control characters other than TAB and LF removed, NFC when `normalize_note_text` is on, R775), as for every note add.

Two checks, in stage 3, before any item is read:

- **No mapped field.** When no field of the plan has a source key (each has `none` or `default_only`, whatever its default), the request fails with `No fields to map to.` (R1703). Routing reports this as its last check (`servers/migaku-routing` D7.1). The extension knows this string.
- **Nothing to add.** When every field that has a source key gets an empty source value, the request fails with the "nothing to add" error (P19), and defaults do not count. A text source is empty when its text is the empty string. An item source is empty when it includes no item. So a payload with every key missing, sent to the Custom profile, fails this way and adds no note.

## P8. Reading item data

**Case.** The schemes `data`, `http` and `https` and the `;base64` token compare without regard to ASCII case (`DATA:`, `HTTPS:` and `;BASE64` are accepted).

**`data:` URLs** (R1695). The form is `data:` + an optional media type with optional parameters + an optional `;base64` + `,` + data.

- The MIME type is the media type's `type/subtype` in lowercase. Parameters such as `;codecs=opus` are ignored. A missing media type means no MIME type.
- With `;base64`, the data is decoded with the standard alphabet (`A`–`Z`, `a`–`z`, `0`–`9`, `+`, `/`). Padding `=` is optional and ASCII white space is skipped. Any other character makes the URL invalid. Without `;base64`, the data is percent-decoded.
- No `,`, or a decoding failure, makes the URL invalid.

**`http` and `https` URLs** (R1701):

- The item is fetched with a GET through the shared fetcher (`servers/host-security` H13). Only `http` and `https` are accepted, also on every redirect hop.
- At most 5 redirects are followed: a sixth redirect response fails the item.
- The item has 8 s from the start of its fetch, name resolution included, to the last byte of its body (`servers/host-security` H13). Beyond that it fails with a timeout.
- The final response must have a 2xx status. Any other status fails the item and the error names the status.
- The body may be at most the Migaku body limit (128 MiB, 134,217,728 bytes, R1678, `servers/host-security` H8). A larger body fails the item.
- The MIME type is the response `Content-Type`'s `type/subtype` in lowercase, or none when the header is missing.
- No destination host is refused, loopback and LAN included, as with AnkiConnect (plan open question on remote media destinations).

Each way a fetch can fail gives one P19 message:

| Failure | P19 case |
|---|---|
| the URL does not parse as an absolute `http` or `https` URL with a host | unsupported URL |
| a redirect whose `Location` names another scheme, or does not resolve to an absolute URL | unsupported URL |
| a sixth redirect response | more than 5 redirects |
| a 3xx response without `Location`, or any other final status outside 2xx | non-2xx status |
| name resolution, connection, TLS or a transport error before the last body byte | connection or TLS failure |
| the 8 s pass first | 8 s exceeded |
| the body passes the limit | body over the Migaku body limit |

When two of these could apply, the one the fetch meets first in time counts.

**Any other form** (`ftp:`, `file:`, `blob:`, a relative URL) fails with "unsupported URL" (P19).

**Empty data.** Data that is 0 bytes after reading fails the item.

**Concurrency.** A request reads and downloads all its used items at the same time, at least 8 at once. With 8 or fewer remote items, a request takes about as long as its slowest item. When an item fails, the reply reports the first failing item in the order `sentenceAudio`, `wordAudio`, `images`, each by index. The other items are cancelled once every item before that one has finished or failed.

## P9. File extension

The extension comes from the first row that applies:

1. The MIME type is in this table:

   | MIME types | Extension |
   |---|---|
   | `audio/mp4`, `audio/m4a`, `audio/x-m4a` | `m4a` |
   | `audio/mpeg`, `audio/mp3`, `audio/mpeg3`, `audio/x-mpeg`, `audio/x-mp3` | `mp3` |
   | `audio/ogg`, `audio/opus`, `audio/vorbis` | `ogg` |
   | `audio/webm` | `webm` |
   | `audio/wav`, `audio/x-wav`, `audio/wave`, `audio/vnd.wave` | `wav` |
   | `audio/flac`, `audio/x-flac` | `flac` |
   | `audio/aac`, `audio/x-aac`, `audio/aacp` | `aac` |
   | `image/webp` | `webp` |
   | `image/jpeg`, `image/jpg`, `image/pjpeg` | `jpg` |
   | `image/png` | `png` |
   | `image/gif` | `gif` |
   | `image/avif` | `avif` |
   | `image/heic`, `image/heif` | `heic` |
   | `image/svg+xml` | `svg` |
   | `image/bmp` | `bmp` |

2. The MIME type is `audio/<s>` or `image/<s>`, and `<s>` reduces to an extension. Remove a leading `x-`, then everything from the first `+`, `.` or `;`. The rest is used when it is 1 to 10 characters of `a`–`z` and `0`–`9`.
3. The content's first bytes match this table, tried in order:

   | First bytes | Extension |
   |---|---|
   | `ID3` | `mp3` |
   | any 4 bytes, then `ftyp`, then `avif` or `avis` | `avif` |
   | any 4 bytes, then `ftyp`, then `heic`, `heix` or `mif1` | `heic` |
   | any 4 bytes, then `ftyp` (any other brand) | `m4a` |
   | `OggS` | `ogg` |
   | `fLaC` | `flac` |
   | `RIFF`, any 4 bytes, `WAVE` | `wav` |
   | `RIFF`, any 4 bytes, `WEBP` | `webp` |
   | 1A 45 DF A3 | `webm` |
   | 89 50 4E 47 0D 0A 1A 0A | `png` |
   | FF D8 FF | `jpg` |
   | `GIF87a` or `GIF89a` | `gif` |
   | FF, then a byte b with (b AND F6) = F0 (ADTS) | `aac` |
   | FF, then a byte b with (b AND E0) = E0 and bits 2–1 of b not both 0 (MPEG audio frame) | `mp3` |

4. Otherwise the item fails with "unknown media format" (P19).

So a specific audio or image MIME type always decides, as R1695 says, and the first bytes are read only when the MIME type gives no extension. The MIME types that R1695 calls generic (none, the empty string, `application/octet-stream`, `binary/octet-stream`) are neither in the table nor `audio/*` or `image/*`, so they go straight to row 3. A non-media MIME type such as `text/html` also goes to row 3, so an HTML error page served with status 200 fails as an unknown format. The item's key does not matter: a `wordAudio` item that is a PNG is stored as `.png`.

| MIME | First bytes | Extension |
|---|---|---|
| `audio/mp4` | anything | `m4a` |
| `application/octet-stream` | `ID3`… | `mp3` |
| none | FF FB 90 64 | `mp3` |
| `audio/ogg` | anything | `ogg` |
| `image/gif` | anything | `gif` |
| `audio/x-speex` | unknown | `speex` |
| `audio/3gpp` | any 4 bytes, `ftyp`, `3gp4` | `3gpp` (row 2 comes before the first bytes) |
| `text/html` | `<!doctype` | failure |

## P10. File names and storage

1. **Desired name**: the raw id with every character outside `A`–`Z`, `a`–`z`, `0`–`9`, `_` and `-` removed, then `.`, then the extension (R1699). When the id is unusable or nothing is left after the removal, 11 random lowercase hexadecimal digits take its place (the random source is injectable for tests).
2. **Normalization** (P11) may replace the bytes and the extension before naming.
3. **Storage**: the desired name and the bytes go through `media/media-files` M3 inside the commit op, staged as M13 says. M3 lowercases a new name, reuses a present file with the same content (R1700: no new file), and gives the hash name of M4 when a case variant holds other content. The field gets the **stored name** M3 returns, never the desired name.
4. Items are stored in the order `sentenceAudio`, `wordAudio`, `images`, each by index. Staged files count as present for later items of the same request (M13). So two items with the same id and the same bytes share one file, and with other bytes the second gets a hash name.
5. Converting audio to MP3 is not done, and nothing else is transcoded (R1696, R1698). WebP stays WebP. Existing notes keep their `.mp3` references.

| Raw id | Extension | Desired name | Folder before | Stored name |
|---|---|---|---|---|
| `a1b2c3d4e5f` | `m4a` | `a1b2c3d4e5f.m4a` | none | `a1b2c3d4e5f.m4a` |
| `a/b..c` | `m4a` | `abc.m4a` | none | `abc.m4a` |
| `ABC_x-1` | `webp` | `ABC_x-1.webp` | none | `abc_x-1.webp` |
| `` (empty) | `mp3` | `<11 hex>.mp3` | none | `<11 hex>.mp3` |
| `../../` | `png` | `<11 hex>.png` | none | `<11 hex>.png` |
| 42 (a number) | `png` | `<11 hex>.png` | none | `<11 hex>.png` |
| `k9` | `m4a` | `k9.m4a` | `k9.m4a`, same bytes | `k9.m4a` (nothing written) |
| `k9` (the item's bytes are B) | `m4a` | `k9.m4a` | `k9.m4a` with other bytes A | `k9-<hash of B>.m4a`: the hash of the item's bytes B (`media/media-files` M4), never of the file already there |

## P11. Loudness normalization (R1697)

When the plan's normalize setting is on:

- Each used audio item (`sentenceAudio`, `wordAudio`) is passed after reading to the host's audio transcoder (`AudioTranscoder`, `servers/host-security` H11.3). It runs on a blocking thread the engine registered (`engine/jobs`), never on a server worker.
- The call's contract:

  | Input | Output |
  |---|---|
  | the item's bytes as read (P8); the target loudness in LUFS, the profile's, −20 unless the profile sets another; the minimum bit rate, 128,000 bit/s | either **encoded**: new bytes holding AAC audio of at least that bit rate in an MPEG-4 container, at the target loudness; or **failed**, with a message for the log only |

- On **encoded**, the result replaces the item's bytes, and its extension is `m4a`. P9 is skipped for that item.
- On **failed**, the request fails with the normalization error (P19). The original audio is not stored in its place.
- The host always has a transcoder: the macOS app's implementation (U144), or in Linux test builds a pass-through that answers **encoded** with the input bytes unchanged. Tests that turn normalization on with the pass-through send M4A audio only, so the stored `.m4a` name stays true to the bytes. A host built without any transcoder answers **failed** for every call.

Images are never passed to it. The setting is off by default, and then nothing is transcoded.

## P12. Retries (R1705)

A client that times out (the extension waits about 10 s) sends the same payload again. Meguri answers such a **retry** with the first note's id and creates nothing.

**Key match.** The retry key of a request R **matches** the retry key of an earlier receive E when R's `targetWord` and `sentence` are byte-equal to E's and one of these holds:

- **by media**: R has at least one item, every item of R has a raw id that is not empty, and every one of those raw ids is among E's raw item ids;
- **by text**: neither R nor E has an item, and at least one of `targetWord` and `sentence` is not empty.

This is the one definition of a matching key. `servers/migaku-received-log` L7 uses it too. A request that has an item with an empty or unusable id never matches. Requiring equal text for a media match narrows R1705, which speaks of media ids only: a real retry resends the same payload, so it still matches, while a different word sent with the same media ids does not (Q5).

**Retry.** A request R is a retry of an earlier receive E when all of these hold:

- E created a note (its log row reached Created, P20), and the row's live note (`servers/migaku-received-log` L3) exists;
- E arrived at most 600 s before R;
- R's key matches E's.

When several earlier receives qualify, the latest one is E. Otherwise R is new.

The check runs twice:

- **early** (stage 2), against receives that have already committed. It needs a collection read for the live note's existence and for open intents (P17). While the collection is not available, the early check finds no match and the commit check decides. A match replies at once and skips routing and media.
- **at commit** (stage 6, inside the op), against every receive committed before it. Commits follow arrival order (P13), so an identical request still in flight commits first, and the later one then matches it. This check decides. The early check never makes the result differ from what the commit check alone would give.

A matching request replies `{"success": true, "created": true, "id": <id>}` with HTTP 200, where `<id>` is the id of E's live note: the note E created, or the note a move made from it (`servers/migaku-received-log` L12). It writes no log row and changes no row. A match against a **Pending** entry (the switch on, P15) replies as P14's pending reply and queues nothing.

The receive index that holds the raw ids, the text pair, whether there were items, the arrival time and the log row of each created receive lives in the integrations store. Entries older than 600 s may be dropped.

## P13. Order and concurrency (R1707)

- Each request runs off the main thread. Nothing on the request path waits on the main thread or calls into Swift, except to post host events (P20) and, with normalization on, to call the transcoder from its blocking thread.
- Stages 1 to 4 of different requests run concurrently.
- Commits (stage 6) are strictly in arrival order. The server keeps one FIFO for pending entries (P15) and live requests. A request commits only after every entry ahead of it has committed or failed. A request that fails before stage 5 leaves the FIFO and holds no one up.
- So note ids increase in arrival order (`storage/ids-usn-mtime` I2), and ten concurrent requests give ten notes whose ids follow their arrival order.
- The add goes through the interactive queue as a `LocalApi` op (`engine/op-model` O1, O2). UI writes and server writes therefore commit in the order they reach that queue.

## P14. A busy collection (R1706, R1797)

The collection is unavailable exactly when `engine/lifecycle` L6 says it is not available. For example, an exclusive phase of a full sync, restore or `.colpkg` import makes it unavailable, and so does Check Database holding the long-operation slot, without any lifecycle change. A `CloudAdopt` that holds the slot but has not entered its exclusive phase does not.

- Stage 5 waits on the waiter (`servers/host-security` H9.2, which wraps L6) until the collection is available or 5 s after arrival, whichever comes first. The 5 s limit is a limit on the collection only, never on the request's turn in the FIFO (P13):
  - At the 5 s mark, the busy outcome of the table below applies only when the collection is unavailable at that instant.
  - A request that is blocked only by earlier entries of the FIFO, with the collection available, has no limit: it waits for its turn, however long the earlier requests take (P8 allows 8 s per remote item).
  - When its turn comes after its 5 s mark and the collection is unavailable then, it takes the busy outcome at once. The same holds when the collection became unavailable after the 5 s mark while the request was still waiting for its turn.
- When the commit op is refused with `CollectionUnavailable` or `StaleGeneration` (`engine/lifecycle` L3, L4), the request goes back to stage 5 with the rest of its 5 s. It then resolves the destination again on the new generation (P5).
- When the collection is still unavailable 5 s after arrival and the request's items are ready, the outcome depends on the switch:

| Switch | Reply | Queue | Log row |
|---|---|---|---|
| off (default) | HTTP 400 `{"success": false, "error": "Meguri is busy"}` | nothing | Failed, with the payload kept (P20) |
| on | HTTP 200 `{"success": true, "created": false}` | a pending entry (P15) | Pending |

- If the items are not ready at the 5 s mark (a slow download), the request waits for them, up to P8's 8 s per item. Then it follows the same table, unless the collection became available meanwhile, in which case it commits.
- Items that fail end the request with the item's error, busy or not.

**The switch.** It is not a user setting and not a member of the integrations config document (`servers/host-security` H10.2). It is a boolean parameter of the Migaku server's construction, which the host passes when it creates the server. The app always passes **off**. Tests construct the server with either value, so one test build runs both rows of the table. The open question is whether extension 1.31 treats `{"success": true, "created": false}` as success (Q1). The add-on sends that reply only in its "intercept" mode, and the sources do not say what the extension then shows. The switch may be turned on only after a recording of extension 1.31 against that reply shows it as a success, committed with this spec's update (U147's manual check).

## P15. The pending queue (switch on only)

- A pending entry holds the raw body, the bytes and extension of every used item after P8 to P11, the arrival time, the whole plan as resolved at arrival (profile id and name, decision, route, detection, note type id, deck id, the field plan with its sources, defaults and the note type's field list it was built from, tags to add) and a receive intent (P17). It is written to the integrations store, with the bytes in files under `meguri-integrations/` (`servers/host-security` H10.1), before the reply is sent.
- It waits in the FIFO of P13 at its arrival position.
- The queue drains on every `Available` event (`engine/lifecycle` L6) and at launch once the collection is open. It goes in arrival order, before any live request that arrived later.
- Each entry commits as in P16, against the current generation, with the plan stored at arrival: routing is not asked again (`servers/migaku-routing` D7.2), and a change to the profile's field map made while the entry waited does not apply to it. P16 step 2's checks run on that plan:
  - the profile is gone: Failed with `The Migaku destination for this card no longer exists.`;
  - its note type or deck is gone (for example after a collection replacement): Failed with the R1715 message of P19;
  - the note type's field list differs from the stored one: Failed with the "fields changed" message of P19. A pending entry is never sent back to stage 3, since stage 3 would route it again.
- A failed entry adds no note. Its row turns Failed with the message, and the raw body stays as the row's Failed payload, pruned 7 days after the instant the row turned Failed (`servers/migaku-received-log` L5). The item files are deleted. Retry then runs the payload from stage 1, with a new routing decision (`servers/migaku-received-log` L14).
- Committing turns the row Created and removes the entry, its raw body and its files.
- Pending entries never expire and are never pruned (`servers/migaku-received-log` L5).

## P16. The commit

One op, `migaku_receive`, with source `LocalApi` (R1789; `engine/op-model` O1), planned on the generation of the plan's check. Its op-catalog entry (`engine/op-model` O9, which U139 adds with the op) is: undo class not destructive (`engine/undo` M11), no long write.

1. The commit retry check (P12). On a match the op writes nothing, and the request's intent and its hidden row are deleted in one integrations-store transaction (P17). The reply is P12's.
2. Check that the note type and the deck exist and that the note type's fields are still the ones the plan was built from. A missing note type or deck fails with P19's R1715 errors. When a field was added, removed, renamed or moved since stage 3, the op writes nothing and a live request goes back to stage 3 once, reusing the items it has already read. If the new plan uses an item that was not read, that item is read then, and its failure fails the request as usual. A second change fails with the internal error (P19). A pending entry fails instead (P15).
3. Stage every used item (P10), in P10's order.
4. Add the note with `notes/note-records` A1:
   - the note type and deck of the plan;
   - the field values of P7, built from the stored names;
   - the plan's tags to add, stored as `tags/tags` T4 says;
   - the GUID of the receive intent (P17);
   - `update_add_defaults` false, so `curModel`, `_nt_<id>_lastDeck` and `_deck_<id>_lastNotetype` do not change (`notes/note-records` A4, R801).

   Cards are generated and placed as for any add (`notes/card-generation`, `notes/deck-placement-and-position`). `mod`, `usn`, the position counter and `col.mod` follow A2 and A3. The next sync uploads the note like one added in the editor (R1704).
5. Any failure aborts the op. No note is added and every staged file of the op is deleted (`media/media-files` M13), so no media file is left behind (R1702).
6. A full disk while staging or committing fails with `No space left on device` (R1703).

After the op commits, the server reports the commit to the routing state with the profile, the decision and the arrival time as `at` (`servers/migaku-routing` D12): the profile's received count goes up by one, and an `Auto` decision sets the last route. A retry answered with an existing note reports nothing.

The op is never recorded for undo and truncates the undo history only where it intersects (`engine/undo` M7, R1795). Live deck counts in an open review update through the change stream, without re-rendering the current card (R1708, U175).

## P17. Receive intents and crash recovery

The note is in the collection and the receive index and log row are in the integrations store: two databases, written at different instants. Intents keep them consistent across a crash.

1. Before submitting the commit op, the server writes an **intent** to the integrations store. It holds a new GUID (`text/guid` G3), the arrival time, the retry key (P1), and the row it belongs to:
   - for a pending entry, the entry's Pending row (P15);
   - for a Retry, the Failed row being retried (`servers/migaku-received-log` L14);
   - for a live HTTP request, a new **hidden row**, written in the same transaction. It holds every row part that is known before the commit (`servers/migaku-received-log` L2: endpoint `receive`, receive time, target word, snippet, profile, language, decision, routing route and detection, deck id, note type id, retry key), the status Failed with the internal error (P19) as its fallback, and the raw body as its payload. A hidden row is left out of every read, count and event of the log (`servers/migaku-received-log` L6, L9) while its intent is open.
2. The op adds the note with that GUID.
3. After the op commits, one integrations-store transaction marks the intent Created with the note id, updates the receive index and turns the row Created: note id, stored media names, warnings (P21), payload deleted. A hidden row stops being hidden. The "received log changed" event follows (P20).
4. When the op fails, one transaction discards the intent and turns the row Failed with the request's P19 message, the hidden row of a live request becoming visible. When the op is refused before it ran (`CollectionUnavailable` or `StaleGeneration`, P14) or the request goes back to stage 3 (P16 step 2), the intent and its hidden row are deleted, and the next commit attempt writes new ones.

An **open intent** is one not yet marked Created. Launch recovery, the pending drain and the early and commit retry checks resolve open intents first. An intent whose request or Retry is still in flight is left to it.

- The intent's GUID exists in the collection: the intent completes as in step 3, with that note's id. The note is never added again. The stored media names are read from the note's fields that the plan maps to `sentenceAudio`, `wordAudio`, `images`, `firstImage` and `restImages` (`text/media-references` M4), and the warnings are computed then (P21).
- The GUID is absent and the intent belongs to a pending entry: the entry runs again.
- The GUID is absent and the intent belongs to a Retry or a live HTTP request no longer in flight: the intent is discarded. The row is Failed with the internal error and keeps its payload. A hidden row becomes visible, with this recovery's instant as its failed instant; a retried row keeps its failed instant (`servers/migaku-received-log` L5). The client never got a success, so its own retry is new.

So a process killed between the collection commit and step 3 leaves exactly one note and one row. After restart, the drain and an identical client retry within 600 s both find it, and the row is Created with that note's id.

## P18. Replies

Every reply is a JSON reply as `servers/migaku-protocol` P1 defines it (`Content-Type: application/json; charset=UTF-8`, compact JSON, members in the order shown; the spaces in this file's examples are for reading) and carries the CORS headers of its P6.

| Case | Status | Body |
|---|---|---|
| note created | 200 | `{"success": true, "created": true, "id": <note id>}` |
| retry of a created receive (P12) | 200 | `{"success": true, "created": true, "id": <earlier note id>}` |
| busy, switch on (P14), or retry of a pending entry | 200 | `{"success": true, "created": false}` |
| any failure of this file | 400 | `{"success": false, "error": "<message of P19>"}` |

`<note id>` is a JSON integer. Every failure of this file uses status 400 (R1702), an unexpected engine error included (`Meguri could not add the card.`). A panic caught by the host is `servers/migaku-protocol` P8.2's 500. Body limits, routes and origins answer as `servers/migaku-protocol` says, before any stage of this file.

## P19. Error messages

These are the exact strings. `<key>` is the item's key, `<n>` its index, `<profile>` the profile's name and `<status>` the decimal HTTP status.

| Case | Message |
|---|---|
| body not JSON | `Invalid card data: the request body is not JSON.` |
| body not an object | `Invalid card data: the request body is not a JSON object.` |
| text key of another type | `Invalid card data: <key> must be a string.` |
| item key not an array | `Invalid card data: <key> must be a list.` |
| item not an object | `Invalid card data: <key> item <n> must be an object.` |
| item without data | `Invalid card data: <key> item <n> has no media data.` |
| no confirmed profile | `No Migaku destination is set up in Meguri.` |
| profile gone (pending entry, P15) | `The Migaku destination for this card no longer exists.` |
| note type missing (R1715) | `Destination note type for <profile> no longer exists.` |
| deck missing (R1715) | `Destination deck for <profile> no longer exists.` |
| fields changed (pending entry, P15) | `The fields of the destination note type for <profile> changed while the card waited.` |
| no mapped field (R1703) | `No fields to map to.` |
| nothing to add (P7) | `Nothing to add: every mapped field is empty.` |
| invalid `data:` URL | `Could not read <key> item <n>: invalid data URL.` |
| unsupported URL (P8's table included) | `Could not read <key> item <n>: unsupported URL.` |
| empty data | `Could not read <key> item <n>: no data.` |
| unknown format | `Could not read <key> item <n>: unknown media format.` |
| non-2xx status | `Could not download <key> item <n>: HTTP <status>.` |
| more than 5 redirects | `Could not download <key> item <n>: too many redirects.` |
| 8 s exceeded | `Could not download <key> item <n>: timed out.` |
| body over the Migaku body limit | `Could not download <key> item <n>: the file is too large.` |
| connection or TLS failure (name resolution and transport errors included) | `Could not download <key> item <n>: connection failed.` |
| normalization failed | `Could not normalize the loudness of <key> item <n>.` |
| disk full (R1703) | `No space left on device` |
| busy, switch off (P14) | `Meguri is busy` |
| anything else | `Meguri could not add the card.` |

When the note type and the deck are both missing, the note type message is sent. Meguri never sends `No such file or directory`, which the extension also recognizes: it writes no temporary files. The extension also treats timeout errors in a special way. Whether the words `timed out` in the download message trigger that handling is not known (Q6).

## P20. Received-log rows and events

This unit owns the received-log table and the writes of its rows. The row layout, statuses, retention and everything that happens to a row later are `servers/migaku-received-log`. A `/receive` writes at most one row:

| Outcome | Row |
|---|---|
| body not JSON, not an object, or over the body limit | none |
| retry (P12) | none; the earlier row is unchanged |
| created | Created, with note id, deck, note type, stored media names and warnings (P21) |
| pending (switch on) | Pending, with the payload reference |
| any other failure | Failed, with the message of P19, the decision kind, route and detection when routing ran, and the payload kept for 7 days from the instant the row turned Failed (R1702, `servers/migaku-received-log` L5) |
| a caught panic (`servers/migaku-protocol` P8.2's 500) | none from the request itself. When the panic came after the intent of P17 step 1, that intent is resolved as an open intent of a request no longer in flight, so the row is Failed with the internal error and its payload, or Created when the note was committed |

A Created row replaces matching Failed rows, as `servers/migaku-received-log` L7 says.

After every row write that a read can see (a hidden row, P17, is not one), the server posts the host event "received log changed" (`servers/host-security` H11.1, no payload). After a row turns Created it also posts the "received" event with the row id, the note id and the deck's full name. Swift shows the toast "Received → <deck>" with Move… and Open only when Meguri is frontmost, and never a system notification (R1709, U145).

## P21. Warnings

Computed after the commit from the stored note, and stored on the Created row. They never stop the add.

- **Possible duplicate** (R1729). The **target field** is the first field, in field order, whose plan source is `targetWord` or `targetWordNoSyntax`. With no target field, there is no check. The **key** of a text is: `servers/migaku-syntax` S5, then `text/strip-html` H4, then white space trimmed at both ends, then NFC. The warning is set when another note of the same note type has a value in a field with the target field's name whose key equals the key of the new note's target field. An empty key never matches. Comparison is exact and case-sensitive. The add-on makes no duplicate check either.
- **Front would be empty** (R1730). Set when at least one card the add created renders a blank question in review mode, the blank result of `render/card-render-pipeline` K4 (`render/template-syntax` T10), and that question's AV list is empty. A blank question that still plays audio, for example from a literal `[sound:…]` in the template, is not flagged. The usual case is an audio-vocabulary card with no word audio.
- **Media missing**. Set when a local name referenced by the stored fields (`text/media-references` M4, remote names left out, M5) does not resolve in the media folder (`media/media-files` M5). This catches media that the payload's HTML names but did not send.

## P22. Equivalence with Anki (R1948)

The note a receive creates must equal the note Anki 26.09.3 creates for the same add: same note type, deck, field values after N2, tags, generated cards, `sfld` and `csum`, and stored media bytes under the same names. U136's harness checks this. It replays the add that each receive fixture produced through the `anki` oracle and compares `db_dump` and media. P23's differences come before the add, in what Meguri decides to add, so they do not affect this check.

## P23. Differences from the add-on

| Add-on 0.6.6 | Meguri |
|---|---|
| converts audio to `.mp3` with ffmpeg by default | stores the original container, normally `.m4a` (R1696) |
| optional loudness normalization of its own | per-profile AAC normalization through the host (R1697) |
| uses an open Add window's note type, deck and tags, or one configured destination | routing profiles (R1710); the Add window is never used |
| "intercept" mode fills the Add window and replies `created: false` | not offered |
| fails when the collection is busy | waits up to 5 s (P14) |
| (not known whether it detects retries) | P12 |
| (not known whether a failure can leave media behind) | atomic (P16) |
| writes the Add screen defaults through Anki's add | does not (R801) |
| accepts every origin and binds every interface | `servers/migaku-protocol`, `servers/host-security` |

Two differences from R1706 itself, not from the add-on:

| R1706 | Meguri, while the switch is off (P14) |
|---|---|
| after 5 s busy, store the payload in the pending queue and reply `{"success": true, "created": false}` | after 5 s busy, reply HTTP 400 `Meguri is busy`, queue nothing, write a Failed row with the payload kept for Retry. Temporary, until Q1 is settled (plan Assumptions) |
| a retry is matched by media ids alone | a media match also needs equal `targetWord` and `sentence` (P12, Q5) |

None of these can be compared with an oracle: there is no Migaku add-on oracle (U136). The expected replies and notes of P24 come from this spec.

## P24. Scenarios

The transcripts are in `conformance/recordings/servers/migaku/receive/`, one file per row, named by the row's id (`servers/transcripts` T2), except the four rows that the table after the rows keeps out. They are declared scenarios (T13), and every one that creates a note carries a T14 `equivalence` block (P22). The runner (U136) does not exist yet. The steps they use: `migaku_routing` (T5.1) for the profiles and the override; `host_env.migaku_pending_switch` (T4.1) for the switch; `lifecycle` `exclusive`, `ready`, `long_op`, `long_op_end` and `{replace: ...}` for a busy or replaced collection; `advance` (T7.1) for the host's test clock; `parallel`; `restart_host`; `fault {migaku_crash: ...}`; `routing`; `observe` with `db_dump` `notes` and `media_dir`; `received_log` (T7.3). Remote items come from the scenario's `http_fixtures` (`conformance/dsl` D6.9, `conformance/editor-ops` E2) at `${fixture_base}` (T10.2). A request whose pending entry must have drained before a check is followed by a later request (the sentinel), whose commit waits for the entries ahead of it (P13, P15).

Each scenario defines its own 11-field note type `T-Mining` with the field names of R1712 and a synthetic template whose question is `{{#Is Audio Card}}{{Word Audio}}{{/Is Audio Card}}{{^Is Audio Card}}{{Sentence}}{{/Is Audio Card}}`. Profiles: `JA` (Japanese, defaults Is Vocabulary Card empty and Is Audio Card `x`), `DE` (German, both `x`), both with R1712's map, a deck each (`JA::Mining`, `DE::Mining`) and no tags, unless a row says otherwise. Media: `M4A` = base64 `AAAAGGZ0eXBNNEEgAAAAAE00QSBpc29t`, `M4A2` = `AAAAGGZ0eXBNNEEgAAAAAE00QSBpc29tAAAACA==`, `WEBP` = `UklGRg4AAABXRUJQVlA4TAIAAAAvAA==`, `MP3` = `SUQzBAAAAAAAAA==`, `GIF` = `R0lGODlhAQABAAAAAA==`, `OGG` = `T2dnUwACAAAAAAAAAAA=`. "Remote" items are served at `${fixture_base}`; a path with no fixture gets 404. The routing override forces the profile wherever detection is not under test.

| Id | Setup | Request | Reply | After |
|---|---|---|---|---|
| `ae72-japanese` | override `JA` | `targetWord` `猫[ねこ;h]`, a kana `sentence`, `sentenceAudio` ids `s1`, `s2` with `data:audio/mp4;base64,<M4A>` and `<M4A2>`, `images` id `i1` `data:image/webp;base64,<WEBP>` | 200 `{"success": true, "created": true, "id": <nid>}` | one note in `JA::Mining`; Sentence Audio `[sound:s1.m4a]\n<br>\n[sound:s2.m4a]`; Screenshot `<img src="i1.webp">`; Images empty; Is Audio Card `x`; Is Vocabulary Card empty; files `s1.m4a`, `s2.m4a`, `i1.webp`; row Created |
| `ae73-remote-404` | override `DE`; fixture `/a` serves M4A, no fixture `/b` | `sentenceAudio` `a1` `${fixture_base}/a`, `a2` `${fixture_base}/b` | 400 `{"success": false, "error": "Could not download sentenceAudio item 2: HTTP 404."}` | no note; no file in the folder, `media.staging/` or the journal; row Failed with payload kept |
| `ae73-slow-retry` | override `DE`; commit delayed 11 s by the harness's fault hook | request R (`sentenceAudio` `r1` M4A); after 10 s the same R again | first: 200 `created: true, id: N`; second: 200 `created: true, id: N` | exactly one note; one row |
| `retry-text-window` | override `DE`; clock injectable | no items, `targetWord` `Hund[Hund,nn,m]`, `sentence` `Der Hund.`; same body at +599 s; same body at +1200 s | 200 id N; 200 id N; 200 id M ≠ N | two notes |
| `retry-empty-ids` | override `DE`, Images ← `images` | the same body twice, 60 s apart, with one `images` item with `"id": ""` and GIF | two 200 with different ids | two notes; two `<11 hex>.gif` files |
| `retry-media-other-word` | override `DE` | body A: `targetWord` `Hahn[Hahn,nn,m]`, `sentence` `Der Hahn kräht.`, `sentenceAudio` id `s9` (M4A); 30 s later body B: the same `sentence` and item, `targetWord` `kräht[krähen,v,]` | 200 id N; 200 id M ≠ N | two notes; B's Sentence Audio `[sound:s9.m4a]`, the file reused (P10) |
| `retry-text-after-media` | override `DE` | body A with `targetWord` `Katze[Katze,nn,f]`, `sentence` `Die Katze.` and one `images` item `i7` (GIF); 30 s later body B with the same two texts and no item | 200 id N; 200 id M ≠ N | two notes (a text match needs E without items too) |
| `content-type` | override `DE` | three valid bodies with different texts and no items: one sent as `text/plain`, one with no `Content-Type`, one with an extra key `"zzz": 1` | three 200 | three notes |
| `no-field-mapped` | profile `X` whose map is all none | any valid body | 400 `No fields to map to.` | no note; row Failed |
| `all-keys-missing` | override `DE` | `{}` | 400 `Nothing to add: every mapped field is empty.` | no note |
| `verbatim-html` | override `DE`; `normalize_note_text` on | `sentence` `<strong>Hahn[Hahn,nn,m]</strong> kräht[krähen,v,]. cafe` U+0301 U+0007 `!`; `targetWord` `cafe` U+0301 U+0007 | 200 | Sentence stored as `<strong>Hahn[Hahn,nn,m]</strong> kräht[krähen,v,]. café!`: the HTML and the brackets byte-equal, `café` with U+00E9, the U+0007 removed (N2); Target Word `café` (U+00E9); both equal to the oracle's add of the same fields (P22) |
| `tags` | profile `DE` with tags to add `mined` | valid body | 200 | note tags ` mined ` |
| `naming` | override `DE`; folder holds `k9.m4a` = M4A and `k8.m4a` = M4A | `sentenceAudio` ids `a/b..c` (M4A), `` (M4A2), `k9` (M4A), `k8` (M4A2) | 200 | Sentence Audio names `abc.m4a`, `<11 hex>.m4a`, `k9.m4a` (reused, not rewritten), `k8-<hash of M4A2>.m4a` |
| `extensions` | override `DE`, map Word Audio ← `wordAudio`, Images ← `images` | `wordAudio`: `w1` `data:application/octet-stream;base64,<MP3>`, `w2` `data:audio/ogg;base64,<OGG>`; `images`: `g1` `data:image/gif;base64,<GIF>` | 200 | names `w1.mp3`, `w2.ogg`, `g1.gif` |
| `remote-limits` | fixtures: `/r6` redirects 6 times, `/slow` answers after 9 s, `/ok1`–`/ok3` each after 2 s | separate requests: `${fixture_base}/r6`; `ftp://127.0.0.1/x`; three items `/ok1`–`/ok3`; then `${fixture_base}/slow`, last so that its delay does not overlap the concurrent fetches | 400 `too many redirects.`; 400 `unsupported URL.`; 200 within 3 s; 400 `timed out.` | one note, from the `/ok1`–`/ok3` request |
| `disk-full` | fault hook: staging write fails with ENOSPC | valid body with one item | 400 `No space left on device` | no note; no staged file left |
| `busy-short` | `ExclusiveBusy` for 2 s from arrival | valid body | 200 `created: true` after about 2 s | one note |
| `busy-long-off` | switch off; `ExclusiveBusy` for 10 s | valid body | 400 `Meguri is busy` after about 5 s | no note from this body, also after `Ready`: a later request after `Ready` gets the first note id; row Failed, payload kept; no pending entry |
| `busy-long-on` | switch on; `ExclusiveBusy` for 10 s | valid body | 200 `{"success": true, "created": false}` after about 5 s | row Pending; on `Ready` one note and row Created; the entry survives a restart before `Ready` |
| `busy-check-db` | switch on; Check Database holds the slot for 10 s | valid body | 200 `created: false` after about 5 s | note created right after the slot is released, with no lifecycle change |
| `busy-three` | switch on; `ExclusiveBusy` for 10 s | three bodies at 0, 1 and 2 s | each 200 `created: false`, at 5, 6 and 7 s | three notes after `Ready`, ids in arrival order |
| `busy-three-off` | switch off; `ExclusiveBusy` for 10 s | three bodies at 0, 1 and 2 s | each 400 `Meguri is busy`, at 5, 6 and 7 s | no note from these bodies (a later request after `Ready` gets the first note id); three Failed rows |
| `pending-replaced` | switch on; one pending entry for `DE`; then `CollectionReplaced` by a new collection that has only the stock note types and no `DE::Mining`, so no note type has the id stored for `T-Mining` | — | — | no note; row Failed `Destination note type for DE no longer exists.` (the note type message wins, P19) |
| `crash-between` | fault hook: kill after the collection commit, before P17 step 3; once for a live request, once for a drain (`crash-between` holds the live request) | restart, then the same body again within 600 s | 200 with the first note's id | exactly one note; row Created with that id |
| `order-ten` | override `DE` | ten different bodies sent at once, then one more | ten 200, then 200 with the eleventh id | note ids increase in arrival order |
| `add-defaults-kept` | override `DE`; `curModel` and the two key families set to other ids | valid body | 200 | `curModel`, `_nt_*_lastDeck`, `_deck_*_lastNotetype` unchanged |
| `warn-duplicate` | override `DE`; a `T-Mining` note with Target Word `<b>Hund</b>` exists | `targetWord` `Hund[Hund,nn,m]` | 200 | row warning Possible duplicate |
| `warn-empty-front` | override `DE` | items only in `sentenceAudio`; no `wordAudio` | 200 | row warning Front would be empty |
| `warn-media-missing` | override `DE` | `definitions` `<img src="absent.png">` | 200 | row warning Media missing |
| `unused-items` | profile `DE` mapping no field to `images`, `firstImage` or `restImages` | `images` items `${fixture_base}/b` (404), one with no `src`, one with only `"r2Url": "https://example.invalid/x"` | 200 | the fixture server saw no request; no image file |
| `used-item-no-data` | override `DE` | one `sentenceAudio` item `{"id": "q1"}` with no `input` | 400 `Invalid card data: sentenceAudio item 1 has no media data.` | no note; row Failed |
| `case-insensitive-data` | override `DE` | `sentenceAudio` `c1` `DATA:audio/mp4;BASE64,<M4A>` | 200 | file `c1.m4a` |
| `remote-redirect-scheme` | fixtures: `/to-ftp` answers 302 with `Location: ftp://127.0.0.1/x`; `/no-loc` answers 302 without `Location` | two requests, one item each | 400 `unsupported URL.`; 400 `HTTP 302.` | no note |
| `crash-hidden-row` | fault hook: kill after P17 step 1, before the op runs | restart | — | no note; one Failed row with the internal error and its payload; the row was never readable before the restart |
| `fifo-slow-first` | override `DE`; collection available; fixture `/slow7` answers M4A after 7 s | request A with one item `${fixture_base}/slow7`; 1 s later request B with no item | A: 200 `created: true` after about 7 s; B: 200 `created: true` right after A, not busy at its 5 s mark (P14) | two notes, A's id lower |

AE71's routing case is `servers/migaku-routing`'s.

Rows and parts of rows that run in Meguri's own tests (`core/crates/meguri-servers/tests/`, U139), each for the capability that the transcript format lacks:

| Row | Part | Missing capability |
|---|---|---|
| `ae73-slow-retry` | all | a test hook that delays the commit (`servers/host-security` H22 lists none) |
| `disk-full` | all | a test hook that fails a staging write with ENOSPC; an observation of `media.staging/` |
| `add-defaults-kept` | all | a `db_dump` of `config`: `servers/transcripts` T10.6 leaves `config` out of transcript dumps |
| `fifo-slow-first` | all | the row mixes a real-time download with the 5 s mark on the test clock, and no step lines the two up |
| `ae73-remote-404` | no file in `media.staging/` or the media journal | an observation of `media.staging/` and of the journal (`observe` has `media_dir`, `media_trash` and `media_pending` only) |
| `naming`, `retry-empty-ids` | the `<11 hex>` names | random names have no T10 normalization and the random source no H22 parameter, so no expected name can be written |
| `crash-between` | the drain half | no step waits until the fault hook has fired during a drain, so a restart could come before it |
| `crash-hidden-row` | the row never readable before the restart | the host is down then |
| `warn-duplicate`, `warn-empty-front`, `warn-media-missing` | the row's warning | `received_log` rows have no warnings member, and the `received` call's row has no stated JSON form; the transcripts check the add |
| `unused-items` | no request reached the fixture server | no step reports the fixture server's requests |
| `remote-redirect-scheme` | the redirect to `ftp://` | `http_fixtures` `redirect_to` names only a path on the fixture server's own origin |
| `pending-replaced` | a pending entry whose deck alone is gone | no step deletes a deck alone, and no replacement is promised to keep a note type's id |
| `busy-long-off` | no pending entry | no step reads the pending queue; the transcript's later request after `Ready` gets the first note id, so nothing drained before it |

The fuzz target (`core/fuzz/fuzz_targets/migaku_receive_payload.rs`) feeds arbitrary bodies and checks that no request panics and that no staged file survives a request.

## P25. Open questions

- **Q1. The pending reply.** Does extension 1.31 treat HTTP 200 `{"success": true, "created": false}` as success? The add-on sends it only in intercept mode, and the sources say nothing about how the extension shows it. Until a recording settles it, the switch stays off (P14).
- **Q2. Retry match.** R1705 says the media ids "match a card received in the last 10 minutes". P12 reads this as "every raw id of this request is among one earlier receive's ids". A retry by the extension resends the same payload, so equal sets and the subset rule give the same result for it.
- **Q3. `r2Url`, `uploadId` and items without data.** R1691 names `r2Url` and `uploadId` but gives them no meaning. Whether the extension ever sends an item whose media is only in `r2Url`, or an item with an empty or missing `input` or `src` (for example when no audio was found), is open. Meguri ignores both keys, skips such an item while it is unused, and fails the request when a used item has no data (P4). If the extension does send used items without data, skipping them may be the better rule.
- **Q4. The R1715 wording.** R1715's text, "Destination note type (or deck) for <profile> no longer exists.", is read as two messages, one for the note type and one for the deck (P19).
- **Q5. Are media ids unique per card?** How the extension makes item ids is not known. If it gives the same ids to the sentence audio and screenshot of one subtitle line each time, two words mined from that line within 10 minutes carry the same ids. P12 therefore also needs equal `targetWord` and `sentence` for a media match, so the second word is added (`retry-media-other-word`). If ids turn out to be unique per mining action, the extra condition changes nothing for real retries.
- **Q6. Timeout errors.** The extension handles timeout errors in a special way, but what it matches (its own client timeout, a status, or words in `error`) is not known. Meguri's download message contains `timed out` (P19). If the extension keys on those words and then, for example, retries silently, a slow item would be retried rather than reported. A recording of the extension against a `timed out.` reply would settle it.
