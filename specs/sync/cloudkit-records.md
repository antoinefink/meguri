# CloudKit records

Spec: sync/cloudkit-records
Requirements: R2043, R2044, R2045, R2052, R2053, R2057, R2058, R1572, R1592, R2066 (record contract); R1844, R1583 (CloudKit first download), R1584 (CloudKit placeholder), R1588 (CloudKit mass change), with `sync/policy#P13` (K9.3); supports R2047, R2048, R2049, R2050, R2051, R2054, R2055, R2056
Anki: none (Meguri design)
Scenarios: none yet
Status: Under review (R2066). Do not implement U113 or U114 until approved.

How Meguri represents an open collection in the user's private CloudKit database: zones, record types, record names, the payload of each entity kind, media assets, the local tracking store, revisions and acknowledgement, incoming apply, conflict rules, tombstones, epochs and structural generations. Eligibility, hold-back and the Sync Now boundary are `sync/policy`. Crash windows, account changes, conflict-copy operations and replacement flows are `sync/cloudkit-recovery`. Where this file leaves a decision to them, it names it.

The Anki sync specs (`sync/normal-sync`, `sync/hold-back`, `sync/media-sync`, `sync/media-tracking`, `sync/one-way`) describe manual Anki transfers only (KTD26). Nothing here reuses their USN, `col.ls`, `col.scm`, graves-upload or media-USN rules as routine-sync state.

Lines marked **Assumption** state Apple behavior this design depends on and has not verified. K23 lists them as open questions.

## K1. Terms

- **Entity**: one logical collection object that maps to one CloudKit record (K6). Its **kind** is one of K6's nine kinds; its **key** identifies it inside the collection.
- **Row image**: the stored column values of an entity's rows, read under `model/row-records`.
- **Payload**: the canonical encoding of a row image plus the kind's Meguri fields (K5.3, K6.1). A version with no payload (some tombstones, K17.2) has the empty payload: zero bytes.
- **Row digest**: SHA-256 of the payload with every Meguri field (K6.1, "Meguri" fields) cleared. **Payload digest**: SHA-256 of the full payload bytes. The empty payload's digest is SHA-256 of the empty byte string.
- **Epoch**: one published cloud collection (K18). **Structural generation**: one coherent group of changes inside an epoch (K19).
- **Binding**: the pair of one local collection and one epoch, under one CloudKit environment, container and Apple account (K10.2).
- **Acknowledged** (acked): the server holds a version that this device has seen and recorded, through a save result or a fetch.
- **Pending**: the local row digest differs from the last acked row digest of the entity, or a local deletion is not yet acked.
- **Flushed**: a collection commit is flushed when the writer's flush (`storage/open-and-connections#C9.2`) has covered it, so it survives a power loss.
- **Later**: for two versions X and Y of one entity, X is later when its stamp (K15.1) is greater, or the stamps are equal and its payload digest is greater as an unsigned big-endian number. Every "later wins" rule below uses this order, so it is the same on every device.

## K2. Database and zones

- Meguri uses only the private database of the app's CloudKit container. Container identifiers, environments and schema promotion are user setup (R2065); development and daily-driver builds use separate containers.
- Zones:

  | Zone | Name | Records | Lifetime |
  |---|---|---|---|
  | root | `meguri-root` | `MGActive` | one per Apple account; never deleted by Meguri |
  | collection | `c-` + epoch id (K4.1) | `MGEpoch`, `MGEntity`, `MGGeneration` | one per epoch; deleted after K18.5 |
  | media | `meguri-media` | `MGMediaRoot`, `MGBlob` | one per Apple account, shared by all epochs |

- Why separate zones: collection records can be fetched and sent before bulk media (R2057, R1592); media progress is independent of collection progress (R1572, R2050); a new epoch reuses the uploaded media (K9.1); an old epoch is removed with one zone deletion.
- Creating a zone that exists is not an error. Only the publisher of an epoch (K18.3, the `publish` op of `sync/cloudkit-recovery#V15`, a device's first publication at connection included, V13.3) creates a zone or a root record. It creates its collection zone. Decided 2026-10-27: between V15.2 steps 2 and 5 it also creates `meguri-root` (and saves `MGActive` as a new record, K18.3 step 5) when they are missing, and `meguri-media` with a new `MGMediaRoot` (K9.2) when that zone is missing. After an encrypted data reset it deletes `meguri-media` and creates it again with a new `MGMediaRoot` and media key, because the old key cannot be read (V10.1). The user's explicit V15 choice is what allows this; nothing else does. Decided 2026-10-27 (R2066 approval): these zone creates and saves are U114's own operations (`sync/cloudkit-recovery#V26.1`), never engine sends. An adopting device (K18.4) creates nothing: when `meguri-media` or `root` is missing, its binding sets `media_zone_missing` (K9.2, `sync/policy#P13.1`).
- A bound device that finds a zone missing creates nothing by itself, and what stops depends on the zone (K12.3, K12.5):
  - its bound collection zone, or `meguri-root` or `MGActive`: it stops sending, its binding becomes isolated, and `sync/cloudkit-recovery` asks for an explicit choice (R2054; V10.1, V10.2);
  - `meguri-media` while the collection zone exists: collection sync continues and the binding is not isolated. Media uploads and downloads stop for this binding for good: it never recreates the zone and never uses a media zone that another device recreated (K9.2 says how it tells). Status reports it until V15 publishes again (V10.1). The binding records this as `media_zone_missing` (K10.3), which never clears for that binding.
- One active collection per Apple account (R2043): the `MGActive` record names exactly one epoch. A device sends and applies collection records only for the epoch it is bound to, and sends only while `MGActive` names that epoch (K18.4). Exception: the publisher of an epoch sends to that epoch's zone during K18.3 steps 1 to 4, before `MGActive` names it.
- **Confirmed root** (`sync/cloudkit-recovery#V10.2`). An engine instance sends to its bound collection zone only after a fetch of `meguri-root` by that same instance has drained and confirmed the bound epoch (K12.1 rule 3). The binding of a `publish` op is exempt while the op's step is before `done` (the exception above). So a device that comes back online learns of an epoch switch before it sends.
  - Decided 2026-10-27: an incremental root fetch delivers no `MGActive` while it is unchanged, so confirmation reads tracking, not the fetch's records. A drained root fetch confirms the epoch when, after the core has handled its events, the binding's last applied `MGActive` (K10.3 `bindings`) names the bound epoch, and the fetch reported neither a deletion of `MGActive` or of `meguri-root` nor an `MGActive` version naming another epoch. A drained fetch that delivered nothing therefore confirms, and a device relaunched from a saved engine state sends its offline edits once its root fetch drains.
  - The last applied `MGActive` is set when the binding is created, from the `MGActive` version the publisher saved (K18.3 step 5) or the adopting device read (K18.4), and at each `MGActive` version or deletion a root fetch delivers. It commits in the tracking transaction that handles that fetched batch (K13.1 step 2), before an engine state that includes the batch's cursor is saved (K13.1 step 3), so a cursor never moves past an `MGActive` change that tracking has not recorded.
  - A binding that records no last applied `MGActive` (the publisher's binding before its switch, which is exempt, is the only expected case) is not confirmed by a drain that delivered nothing: U114 first reads `MGActive` by record ID, and the result is recorded and decides as `sync/policy#P10.1` ("Confirmed root") says.
- At most one CKSyncEngine object exists per process, the current engine instance (`sync/cloudkit-recovery#V3.4`).

## K3. Record types and fields

Six record types. Each has the field `e`, a CloudKit encrypted bytes value, and at most one asset field. No record type has any other custom field, and no custom field is plaintext.

| Record type | Zone | Fields | Mutable |
|---|---|---|---|
| `MGActive` | root | `e` | yes, conditional saves only |
| `MGEpoch` | collection | `e` | yes: state `publishing` to `complete` once |
| `MGEntity` | collection | `e`, `pa` (asset, payload when large) | yes |
| `MGGeneration` | collection | `e`, `ma` (asset, member list when large) | yes: state changes only |
| `MGMediaRoot` | media | `e` | no |
| `MGBlob` | media | `e`, `a` (asset, file bytes) | no |

This is the initial CloudKit schema (R2058). Adding a record type or field later needs a schema promotion, which is a user step (R2065); new data goes inside `e` instead, under a new envelope format (K5.4).

## K4. Record names

### K4.1 Identifiers

- An **epoch id** is 16 random bytes. Its text form is the bytes in RFC 4648 base32, lowercase, without padding (26 characters). The same text form is used for every 16-byte value named in this section.
- A **record key** is 32 random bytes made by the device that publishes the epoch and stored in `MGEpoch` (K5.2). A **media key** is 32 random bytes stored in `MGMediaRoot` (K9.2).

### K4.2 Names

| Record | Name |
|---|---|
| `MGActive` | `active` |
| `MGEpoch` | `epoch` |
| `MGEntity` | `e` + text form of the first 16 bytes of HMAC-SHA256(record key, kind code byte ‖ key bytes) |
| `MGGeneration` | `g` + text form of the generation id (K19.2) |
| `MGMediaRoot` | `root` |
| `MGBlob` | `b` + text form of the first 16 bytes of HMAC-SHA256(media key, content SHA-256) |

- Names carry no note text, file name, tag, deck name, config key or creation time (R2058, KTD25). One record type serves all entity kinds, so the service cannot count entities per kind.
- **Key bytes** per kind:

  | Kind | Key | Key bytes |
  |---|---|---|
  | note, card, revlog, deck, preset, notetype | the row id | 8 bytes, big-endian two's complement |
  | tag | the tag | UTF-8 of the tag after the case folding the `tags` table's `unicase` collation applies (`storage/schema-18`) |
  | config | the `KEY` | UTF-8 bytes as stored |
  | media name | the file name | UTF-8 of its case key (`media/media-files#M2`) |

- A receiver recomputes the name from the decoded kind and key. A mismatch refuses the record (K20).

## K5. The entity envelope

### K5.1 `MGEntity.e`

The field holds a protobuf message `meguri.cloud.v1.Entity`:

| No. | Field | Type | Meaning |
|---|---|---|---|
| 1 | `format` | uint32 | envelope format, 1 |
| 2 | `kind` | uint32 | kind code (K6) |
| 3 | `key` | bytes | key bytes (K4.2) |
| 4 | `origin` | bytes (16) | random identity given when the entity was first tracked (K11.1); changes only when K14.1 identifies the entity with a server entity |
| 5 | `erev` | uint64 | entity revision: the acked server `erev` plus 1; a new entity starts at 1 |
| 6 | `generation` | optional bytes (16) | the structural generation this version belongs to (K19) |
| 7 | `tombstone` | bool | true for a deletion (K17) |
| 8 | `deleted_at_ms` | int64 | tombstones only: deletion instant, epoch milliseconds |
| 9 | `payload` | optional bytes | the payload, when it is inline |
| 10 | `payload_sha256` | bytes (32) | payload digest, inline or asset; for the empty payload, SHA-256 of the empty byte string |
| 11 | `payload_size` | uint64 | payload length in bytes; 0 for the empty payload |

- Every save is conditional on the server's change tag (K12.2), so the server's `erev` for one record grows by exactly 1 per saved version, and two saved versions of one record never share an `erev`.
- When the encoded envelope with the payload inline would exceed 786,432 bytes, field 9 is absent and the payload is the asset `pa`. A claim batch may also move an anchor's payload to `pa` to stay within the request budget (K7, K19.3). Otherwise `pa` is absent. The empty payload is inline: field 9 present and empty. A receiver accepts either form and checks the digest and size (K20).

### K5.2 Other envelopes

Fields are listed as number, name, type. A field is always present unless marked optional.

| Message | Fields |
|---|---|
| `meguri.cloud.v1.Active` (`MGActive`) | 1 `format` uint32; 2 `epoch_id` bytes (16); 3 `activated_at_ms` int64; 4 `previous_epoch_id` optional bytes (16) |
| `meguri.cloud.v1.Epoch` (`MGEpoch`) | 1 `format` uint32; 2 `epoch_id` bytes (16); 3 `record_key` bytes (32); 4 `crt` int64 (`col.crt`); 5 `ver` uint32 (18); 6 `state` uint32 (1 `publishing`, 2 `complete`); 7 `reason` uint32 (1 initial, 2 replace cloud, 3 restore, 4 collection package import, 5 manual Anki import); 8 `published_at_ms` int64; 9 `counts` repeated `KindCount`, by kind code, present only when `state` is `complete` |
| `meguri.cloud.v1.KindCount` | 1 `kind` uint32; 2 `live` uint64 (live entities of that kind at completion) |
| `meguri.cloud.v1.Generation` (`MGGeneration`) | K19.2 |
| `meguri.cloud.v1.MediaRoot` (`MGMediaRoot`) | 1 `format` uint32; 2 `media_key` bytes (32); 3 `created_at_ms` int64 |
| `meguri.cloud.v1.Blob` (`MGBlob`) | 1 `format` uint32; 2 `sha256` bytes (32); 3 `sha1` bytes (20); 4 `size` uint64 |

### K5.3 Canonical encoding

- Messages are proto3. A field marked optional has explicit presence: it is written when present, also with a default value. A scalar field not marked optional is omitted when it holds its default value (0, false, empty bytes, empty string), as proto3 does.
- **`Stored` values** (decided 2026-10-27). A `Stored` field of a payload (K6.1) is never optional and is always written, also when its content is a default value. Inside it, the oneof member that holds the column's storage class is always written, also with a default value (integer 0, real 0.0, empty text, empty blob), as proto3 oneof presence does. So `Stored{integer: 0}` and `Stored{text: ""}` have different bytes, and a receiver always knows the storage class. A payload in which a `Stored` field is absent, or a `Stored` with no oneof member set, is non-canonical and refused (K20).
- Fields are written in field-number order, each present field once, no unknown fields, repeated scalars packed, repeated messages in the order this file gives.
- Two devices that encode the same row image produce the same bytes, so digests compare across devices.
- A receiver decodes the payload, encodes it again by these rules and compares the bytes. A difference refuses the record (K20). Payload format 1 has no compression.

### K5.4 Newer formats

A record whose `format` (envelope or payload) is above what this build supports is kept in the inbox (K13) with reason `format`, never applied and never overwritten by this device: the entity is not sent while such a record is the server's version. Status reports unsupported schema (R1561) until Meguri is updated.

## K6. Entity kinds and payloads

| Code | Kind | Key | Local rows | References (K12.1, K13.3) |
|---|---|---|---|---|
| 1 | note | `notes.id` | one `notes` row | notetype |
| 2 | card | `cards.id` | one `cards` row | note, deck (`did`, `odid`) |
| 3 | revlog | `revlog.id` | one `revlog` row | none |
| 4 | deck | `decks.id` | one `decks` row | parent deck, preset of a normal deck |
| 5 | preset | `deck_config.id` | one `deck_config` row | none |
| 6 | notetype | `notetypes.id` | its `notetypes` row with all its `fields` and `templates` rows | none |
| 7 | tag | tag (unicase) | one `tags` row | none |
| 8 | config | `config.KEY` | one `config` row | none |
| 9 | media name | file name (case key) | one live record of the media journal | blob (K9) |

- The `col` row is not an entity. `crt` and `ver` belong to the epoch (K5.2); `mod`, `scm`, `ls`, `usn` and `dty` stay device-local Anki bookkeeping (K11.3).
- `graves` rows are not entities; tombstones stand for them (K17.2).
- `usn` columns are not in payloads: they are device-local manual-transfer bookkeeping (K11.3). Every other column of every listed row is in the payload, so unknown Anki data in rows, blobs, card `data` and config values round-trips unchanged (R2045). Blob columns are copied as stored bytes and never parsed for the payload.
- Device-only state never enters a payload (R2045, R1468): it is not in the collection (`storage/config-table#C8`).

### K6.1 Payload messages

Column meanings are `model/row-records`. A payload carries each column's stored value, never the value a read tolerance gives (a `csum` outside the unsigned 32-bit range is carried as stored, not as 0). Types:

- integer columns: `int64`;
- text columns: `bytes`, the stored bytes (SQLite TEXT need not be valid UTF-8, which a proto `string` requires);
- blob columns: `bytes`;
- columns whose read tolerance in `model/row-records` accepts another storage class (card `due`, `odue` and `data`; revlog `ivl`, `lastIvl` and `time`) and note `sfld` (text or integer): `Stored`, a message with oneof { 1 `integer` int64, 2 `real` double, 3 `text` bytes, 4 `blob` bytes }, which keeps the storage class;
- a column that can hold NULL (`tags.config`): optional, absent for NULL.

"Meguri" fields are computed when the payload is built and are not stored in the collection.

| Kind | Message | Fields |
|---|---|---|
| note | `Note` | 1 `id`; 2 `guid`; 3 `mid`; 4 `mod`; 5 `tags`; 6 `flds`; 7 `sfld` (`Stored`); 8 `csum`; 9 `flags`; 10 `data`; Meguri: 11 `nt_struct_gen` (uint64, K19.2) |
| card | `Card` | 1 `id`; 2 `nid`; 3 `did`; 4 `ord`; 5 `mod`; 6 `type`; 7 `queue`; 8 `due` (`Stored`); 9 `ivl`; 10 `factor`; 11 `reps`; 12 `lapses`; 13 `left`; 14 `odue` (`Stored`); 15 `odid`; 16 `flags`; 17 `data` (`Stored`); Meguri: 18 `last_review_id` (int64: the largest `revlog.id` with this `cid` in the writer's collection when it built the payload, 0 when none); 19 `nt_struct_gen` (uint64: that of the card's note's notetype, K19.2) |
| revlog | `Revlog` | 1 `id`; 2 `cid`; 3 `ease`; 4 `ivl` (`Stored`); 5 `last_ivl` (`lastIvl`, `Stored`); 6 `factor`; 7 `time` (`Stored`); 8 `type` |
| deck | `Deck` | 1 `id`; 2 `name` (native form); 3 `mtime_secs`; 4 `common`; 5 `kind`; Meguri: 6 `parent_id` (int64: the parent deck's id, `decks/decks#D3`, 0 for a top-level deck) |
| preset | `Preset` | 1 `id`; 2 `name`; 3 `mtime_secs`; 4 `config` |
| notetype | `Notetype` | 1 `id`; 2 `name`; 3 `mtime_secs`; 4 `config`; 5 `fields` (repeated `Field`: 1 `ord`, 2 `name`, 3 `config`; by `ord`); 6 `templates` (repeated `Template`: 1 `ord`, 2 `name`, 3 `mtime_secs`, 4 `config`; by `ord`); Meguri: 7 `struct_gen` (uint64, K19.2) |
| tag | `Tag` | 1 `tag` (stored spelling); 2 `collapsed`; 3 `config` (optional: absent for NULL, present and empty for an empty blob) |
| config | `Config` | 1 `key`; 2 `mtime_secs`; 3 `val` |
| media name | `MediaName` | 1 `name` (bytes: exact stored file name); 2 `sha256` (bytes 32); 3 `sha1` (bytes 20); 4 `size` (uint64) |

A row that cannot be read under `model/row-records` R1–R10 has no payload. Its entity is **unreadable**: it is not sent, status lists it under needs attention with its kind and key, and Check Database owns the repair.

## K7. Sizes and limits

| Item | Limit | Above the limit |
|---|---|---|
| `MGEntity` envelope with inline payload | 786,432 bytes | payload moves to the asset `pa` (K5.1) |
| entity payload, inline or asset | 104,857,600 bytes | the entity is **oversize**: not sent, the server keeps its previous version, it stays pending, and status lists it under needs attention with kind and key. Entities that reference it are held (K12.1); others continue. A structural generation that contains it cannot be claimed (K19.3) |
| `MGGeneration` envelope with inline members | 786,432 bytes | the member list moves to the asset `ma` |
| media file | 104,857,600 bytes, the syncable size of `media/media-files#M7.1` | a file that never had an entity stays local only (no entity, no blob); a live entity whose file grows past the limit follows K9.4 |
| one batch (one request) in any zone | 250 records, saves and deletions together; and 1,572,864 bytes of non-asset data, counted as the sum over its records of the encoded `e` value plus 2,048 bytes per record for its name, type and system fields | the core builds every batch within both bounds (K12.1). A record that does not fit goes to the next batch; one record always fits alone. After `limitExceeded` both bounds are halved for the rest of that send (K12.3) |

**Assumption A1**: CloudKit accepts at most 1 MB of non-asset field data per record; 786,432 bytes leaves room for system fields and encryption overhead. The SDK headers state this limit (`CKRecord`).
**Assumption A1.1** (decided 2026-10-27): a request holds at most 250 records and about 2 MB of non-asset data. The sync engine's headers state the 250-record limit, and `CKError` states the 2 MB guideline and says the server can change its limits. The batch budget keeps 25% below 2 MB, and `limitExceeded` halves it (K12.3).
**Assumption A2**: one `CKAsset` in the private database can hold 104,857,600 bytes.

## K8. Encryption and metadata

- Card text, templates, CSS, deck and preset settings, collection settings, tags, file names and all other content are inside `e` (CloudKit encrypted values) or inside an asset (CloudKit asset encryption) (R2058).
- Zone names, record names (K4) and record type names carry no content. The service still sees record counts, sizes, record types, zone names and its own system fields such as creation and modification times. Product copy says that end-to-end protection of encrypted values and assets needs Advanced Data Protection and does not cover this metadata (R2058).
- **Assumption A3**: encrypted bytes values hold any byte string up to the record limit and need no index; Meguri never queries records by field.

## K9. Media

### K9.1 Identity

- A media file's content identity is the SHA-256 of its bytes. SHA-1 is carried only to match the media journal's records (`media/media-journal#J1`).
- File names are collection data: media-name entities (kind 9) live in the collection zone. File bytes are `MGBlob` records in the shared media zone, named by content (K4.2). Two names with the same content share one blob; a new epoch with the same files uploads no bytes again.
- The media journal keeps SHA-1 only. Tracking caches each file's SHA-256 by name, size, modification time and SHA-1 (`media_hashes`, K10.3), so a scan hashes only files whose journal record changed.

### K9.2 The media root

- `MGMediaRoot` named `root` holds the media key. A device that creates `meguri-media` (K2) creates the root with a new random key, saved as a new record. If the save finds an existing `root`, the device takes the server's key and discards its own.
- No `MGBlob` is saved before the device holds an acked `root`, so no blob is ever named with a discarded key. The save of a new `root` is an own operation (`sync/cloudkit-recovery#V26.1`), run only after the creation of `meguri-media` succeeded.
- Media-zone records are never fetched as changes (`sync/policy#P13.2`). A binding reads `root` by record ID, with the field `e`, and stores the media key it acked with the binding (K10.3 `bindings`). It never takes another key.
- Decided 2026-10-27: a binding that holds an acked key reads `root` again at each media start (`sync/policy#P13.1` `ResumeMedia`), before any blob work. When the read returns another media key, or unknown item, or zone not found, the media zone was deleted and maybe recreated by another device: the binding sets `media_zone_missing` (K2, K12.5) and does no more blob work. So no blob named with an old key is ever saved into a recreated zone, and no download looks for one there.

### K9.3 Blobs

- A blob is immutable. Saving a blob whose name already exists counts as success: the same name means the same content.
- **Blob existence** (decided 2026-10-27, `sync/policy#P13.2`). The engine never fetches `meguri-media`. A device learns whether a blob exists only from a record-ID read that names its fields (`e` alone, or `e` and `a` for a download) and from its own acked saves. The result is the blob's server state in `blobs` (K10.3): `present`, or `deleted` when the read reports unknown item. A blob never read is `unknown`.
- A device uploads a blob only when all hold: a live media-name entity of its binding names its content and is not flagged `file_missing`, `withheld`, `superseded` or `removal_held` (K9.4) or marked `media_changed` (below); the local file holds that content; and the blob's server state is `deleted`. A blob in state `unknown` is read first, so bytes already in the media zone are never uploaded again (`sync/policy#P13.3`).
- **Stale `present`** (decided 2026-10-27). A blob's `present` state can be stale: another device may delete the blob later (K9.5), and this device learns that only from a read. So a `present` row becomes `unknown` again, and the next read round (`sync/policy#P13.2`) reads it before any upload decision, in two cases:
  1. a local change of this binding makes a live media-name entity name that content where the entity's acked version did not: a new entity, a re-creation over a tombstone, or a content change;
  2. the first drained fetch of the bound collection zone after such a version is acked, so a deletion another device sent while that version was in flight is seen. The row's `recheck` mark (K10.3) is set at that ack and cleared by the drain, in the tracking commit that tracking makes before U114 reports `FetchDrained{collection}`, so that input's read round reads it.

  A read that finds the blob absent sets `deleted`, and the upload rule above sends it from the local file. Versions applied from the server change no `blobs` row: the device that wrote that version reads and uploads its blob. So a device that names content again after another device deleted its blob uploads it again, and no other device is left with `mediaMissing` (K9.5).
- **Clone mismatch** (decided 2026-10-27, `sync/policy#P13.3`). When the upload's clone of the local file, hashed again, still differs from the content the entity names, tracking durably sets the entity's mark `media_changed` with the SHA-256 found (K10.3). While it is set, the blob is not uploaded from that file and status counts the name under `mediaChanged`, also after a relaunch. The next feed item of that name, or a reconcile scan that reads other facts for it, clears the mark, and the name then compares as any local change (K11.4).
- A device downloads a blob when a live media-name entity names content that the local file under that name does not hold: the entity was applied from the server, its local file is missing (K9.4 `file_missing`), or it is `superseded` (K9.4). It reads the blob by record ID before it queues the download. When the read finds no blob (never uploaded, or deleted, K9.5), the name is reported as missing media: the attention item `mediaMissing` (`sync/policy#P3`, `#P14.4`).
- A media-name entity may reach a receiver before its blob. Until the blob is installed, rendering uses the pending-media placeholder and Check Media does not count the file as missing (R1584).
- Downloads go to a cloud staging directory on the media folder's volume, with bounded concurrency and bounded staging bytes (R2057; the bounds are `sync/policy#P13.2`). The receiver checks the SHA-256 and size against the blob envelope and the media-name entity, then installs the file through the media store's exact-name write (`media/media-files#M6`), which never replaces an existing entry without trashing it (M9). A failed check discards the download, keeps any existing file, and reports needs attention. An interrupted install leaves the existing valid file intact.
- **Media apply intent.** An install, a trash or a move that tracking makes for an incoming version is a media apply: an install here, a tombstone's trash (K9.4), a case collision below, and K15.9's move of a local file to its hash name. Before its first store call tracking durably records an apply intent (K13.2). For each name the apply touches (the hash name of K15.9 included) the intent lists its **expected states** in order: the state before; each state that one of the apply's own store calls leaves before the next call (absent, after a trash or a move away); and the state after. A state is the `MediaName` digest of the journal's facts, or the empty payload's digest for an absent name. Decided 2026-10-27, so that a local write is never replaced without a copy, and so that an apply never abandons itself at its own intermediate state:
  - **Before each store call** tracking reads the name's current journal facts (the store's change check, `media/media-files#M7.3`, has run). When they equal the state the previous call leaves (the before state, for the first call), the call runs. When they equal the state this call itself leaves, the call already ran (a replay) and is skipped. Otherwise a local write landed: the apply is abandoned, the intent is removed, the current facts are a local change of the entity (a dirty mark, K11.1), and the inbox entry stays ready. Its next apply meets a pending entity, so K9.4 and K15.9 decide.
  - **Feed items** of a name of the intent (`media/media-journal#J6`) whose facts equal one of its expected states are the apply's own and are not local changes, so the `removed` item of a trash or a move it makes is never a deletion signal (K9.4). Any other item of that name is a local change. When it comes before the apply's own items for that name in the feed, a store call of the apply has replaced it (the write landed between the check and the call): its bytes are in the media trash (`media/media-files#M9`, M10.1), and tracking keeps a conflict copy naming that trash entry, `media_content` for an install or `edit_vs_delete` for a tombstone's trash (K16). The incoming version stands. When it comes after them, it is a local change made after the apply, compared with the after state as K13.2 says.
  - When the store calls return, one tracking transaction deletes the inbox entry, updates the entity (and creates K15.9's new entity for the hash name) and removes the intent.
  - **At open**, an intent left by a crash is settled before any send (`sync/cloudkit-recovery#V2.4` step 8). Tracking first reads the feed items of the intent's names after the consumed counter (K10.3 `media_feed`) under the rules above. Then, per name: when it holds the after state, finish; when it holds an earlier expected state, continue the apply from the call after that state (the staged download, or a new read of the blob); otherwise abandon the apply as the first rule says, and the entry is applied again under K9.4 and K15.9.
- **Case collision.** The install refuses with `case_collision` (`media/media-files#M6`) when a present file has the entity's case key under another exact spelling, and changes nothing. That file is the same entity (K4.2). When its SHA-256 equals the acked content or the incoming content, no local change is at risk: under the media apply intent, tracking moves it to the trash (`media/media-files#M10.1`) and installs again. Otherwise the file holds a local change, and K15.9 decides first. A crash between the trash and the install leaves the name absent under the intent, which the settle above completes. Decided 2026-10-27: the server's exact spelling wins (K15.9), so every device ends with one spelling, and the replaced bytes stay in the trash.
- **Progress and resumption** (R1583, R1584, R1588, R1844, with `sync/policy#P13`). Each blob's upload and download state is durable in `blobs` (K10.3). After an interruption or a relaunch, an installed or acked blob is never transferred again, an interrupted download restarts only that blob (`sync/cloudkit-recovery#V6`), and the local work before transfers continue is bounded: the feed after the consumed counter and the `blobs` rows, and hashing only files whose journal facts changed (`media_hashes`, K9.1). The file count and bytes of a first full download are the live media-name entities whose content is not local, with the sizes of their `MediaName` payloads; status shows them with an estimate (`sync/policy#P14.4` `media`). Transfers run off the writer and never block study (R2057).
- **Assumption A4** (decided 2026-10-27, `sync/policy#P13.2`): false for the engine's fetch, which downloads every asset of every record it fetches; so the engine never fetches `meguri-media`. Meguri relies on A4 only for record-ID reads that name their fields: a read of a blob's `e` alone downloads no asset, so a device learns which blobs exist without downloading them, and the download queue serves visible cards first (R2057). The signed runs check it (K22.3).

### K9.4 Media-name entities and the media journal

- A media-name entity starts for a live journal record that is syncable (`media/media-files#M7.1`). A tracked file that has never been syncable (empty, larger than the limit) and a file that is not tracked (ignored, not normalized) have no entity.
- A live entity whose local file stops being syncable (it became empty or larger than the limit) stays live and is marked **unsyncable**: not sent, the server keeps its previous version, and status lists it under needs attention. It becomes sendable again when the file is syncable again. It never makes a tombstone.
- **Tombstones** (decided 2026-10-27). Only an explicit Meguri removal of the name makes a media-name tombstone: a feed item whose absence cause is `removed` (`media/media-journal#J6`: the trash of `media/media-files#M10.1`, whether Delete Unused, a Check Media fix, a staged operation's recovery or a deletion that a manual Anki media transfer applies calls it, and the old name of a Check Media rename). A trash that a media apply makes under its intent (K9.3) is not one.
- **Adoption window** (`sync/cloudkit-recovery#V14.4`). An adoption replaces the collection, not the media folder, so every media file on the device then belongs to the replaced collection. While the binding's adoption window is open (K10.3 `bindings`), the reconcile scan and the feed classify each name by what the adopted epoch holds for its case key, never as a local change (K11.4): clean, `superseded`, `file_missing`, `withheld`, or no entity. A `superseded` or `withheld` entity records the content digest it was classified with (K10.3 `entities`).
- **Withheld** (V14.4). A name the adopted epoch does not name, or holds as a tombstone, whose local file is present: a new entity, or the epoch's acked tombstone entity, flagged `withheld`. It is never sent and its blob is never uploaded (K12.1 rule 1, V22 invariant 11). It holds back no note (K12.1 rule 6). It is released, and becomes pending as a new entity (or as a pending re-creation over the epoch's tombstone, K11.4), when either holds:
  1. its local content changes to content other than the classified digest;
  2. a local op other than `CloudIncoming` and `CloudRekey` writes a live note whose fields reference the name (`text/media-references`) while the note's before-image did not (`engine/change-stream#S2.4`): the user chose the file. Inserting an existing file through the editor is such a write, although the media journal records nothing for it (`media/media-files#M3`). K12.1 rule 6 then holds that note until the name is acked.

  A removal (cause `removed`) drops the flag: a new entity leaves tracking, as an entity never sent does (K17.1), and a tombstone entity stays the epoch's acked tombstone. A name that goes absent with the cause `missing` stays withheld, so a file of the replaced collection that comes back unchanged is still not uploaded. A clean close and reopen, a scan that finds the same facts, and a new feed consumer (K10.3 feed floor) keep the flag.
- **Superseded** (V14.4). A name the adopted epoch holds live with content other than the local file's. It is not pending and not sendable, whatever its local digest (K12.1 rule 1): the reconcile scan, an apply and a merge never take its local file as the local version L (K11.4, K13.2, K15.9). Its acked content is a download candidate (K9.3); the install runs as a media apply, the exact-name write moves the old file to the media trash (`media/media-files#M9`), and the transaction that marks the apply done clears the flag. When the media zone holds no blob for the acked content, the name is reported as missing media, and the flag and the local file stay. When its local content changes to content other than the classified digest and the acked content, the user changed the file: the flag clears and the name compares as any local change. When it becomes absent, whatever the cause, the flag clears and it is a missing local file (next bullet): the bytes removed were the replaced collection's, so no tombstone is made.
- **Missing local file** (decided 2026-10-27). A name reported absent with the cause `missing` (its file facts were dropped while the journal record stays: the file was deleted, moved or evicted outside Meguri, or a restore left it out), and a name with no record and no retained removal item (J6), never makes a tombstone and is not a local change. The live entity is marked `file_missing` and is not sent (K12.1 rule 1). When acked, its content becomes a download candidate (K9.3); when the media zone holds no blob for it, it is reported as missing media. An entity never acked whose `sent` is false leaves tracking, as K17.1 does for an unsent deletion; one with `sent` true waits for its save result and then follows the acked case. A `withheld` entity is not marked (above). The mark clears when the name is present again, and the entity then compares as any local change.
- **Incoming versions over these flags** (decided 2026-10-27). An incoming version S (K13.2, a save conflict of K12.3 included) never merges with a file that is not a local change of the binding:
  - `file_missing` or `superseded`: the entity counts as clean, with L = its acked version. A live S: the entity takes S and keeps its flag, and S's content becomes the download candidate; when a `superseded` file already holds S's content, the entity is clean and the flag clears. A tombstone S: the entity takes the tombstone. A `file_missing` entity drops the mark; nothing is trashed (there is no file) and no copy is made. A `superseded` entity becomes `withheld`, keeping its classified digest: the epoch no longer names the name, and its file still belongs to the replaced collection, so it stays on this device and is not trashed.
  - `withheld`, live S: S wins without a merge. Its install runs as a media apply, the exact-name write moves the local file to the media trash, and a conflict copy (`media_content`) names that trash entry (K16), so the user can still recover the file; the flag clears when the apply is done. There is no move to a hash name, no note rewrite and no new entity (K15.9 never compares it). So a file of the replaced collection never comes back under another name, and the notes of the epoch keep the name another device fixed.
  - `withheld`, tombstone S: the entity takes the tombstone and stays withheld; its file stays.
- Tracking keys media names by case key (K4.2): items for two spellings of one case key update one entity, and the name that is present gives its exact spelling.
- CloudKit media state lives in the tracking store (K10), never in the media journal's dirty flag or media USN, which belong to manual Anki media transfers (R1572). A reset of Anki media state does not change CloudKit state.
- Local media changes reach tracking from the media journal's file-fact feed (`media/media-journal#J6`: name, content and live state changes after a given counter value, each absence with its cause); the reconcile scan (K11.4) compares every journal record and reads the same causes. While J6 reports the feed not complete, no media-name tombstone is made, and a name the journal has no record of is neither downloaded nor reported missing, so an unregistered local file is never replaced (J6, "Complete registration").
- **Removal before complete registration** (decided 2026-10-27). A feed item or a scan result with the cause `removed` for a live entity while J6 reports the feed not complete is an explicit Meguri removal, and it is never undone by a download. Tracking consumes the item as usual and records it on the entity as the mark `removal_held` (K10.3): the entity stays `live` in tracking, is not sent (K12.1 rule 1), is neither downloaded nor reported missing, and for K13.2, K15.9 and K9.4 ("Incoming tombstone") counts as a pending local tombstone. In the first tracking commit after J6 reports the feed complete (the commit that closes an adoption window included, K11.4), every entity with the mark becomes a pending tombstone (K17.1) and the mark clears; one whose name is present again by then drops the mark and compares as any local change. An incoming tombstone clears the mark (both sides deleted it); an incoming live version is handled as against a pending local tombstone (deletion wins, K17.3). A `withheld` entity follows the adoption-window rules instead (a removal drops its flag). So the mark is durable and the deletion reaches CloudKit once registration completes, also when the removal item itself is deleted by then.
- A media-name tombstone carries the `MediaName` payload of the last live version this device acked or applied (K17.2); after a K17.1 save conflict that is the server's version S.
- **Incoming tombstone** (decided 2026-10-27). An incoming media-name tombstone is compared with the receiver's own entity, never with the tombstone's payload, which can name content the receiver never held:
  - clean (the local file holds the acked content): the file moves to the media trash (`media/media-files#M10.1`) as a media apply (K9.3), with no copy;
  - pending with a local content change (the local file holds other content than the acked one, or the entity is a new or re-created one): deletion wins (K17.3). The file moves to the trash as a media apply, and a conflict copy (`edit_vs_delete`) names that trash entry (K16);
  - a pending local tombstone: both sides deleted it; the entity takes the incoming tombstone and nothing is trashed;
  - flagged `file_missing`, `superseded` or `withheld`: the bullet above.

  Media bytes are never deleted permanently by sync.

### K9.5 Removing blobs

- A device deletes an `MGBlob` record only when all hold: the device is bound to the active epoch and has completed a fetch of its collection zone; no other epoch zone is retained (K18.5, K10.3 `zones`); no live media-name entity names its content; no unresolved conflict copy on this device names it (K16); and no live entity has named it for 30 days (`blobs` "unreferenced since": when the last name referencing that content went away). When it runs, and how the deletion reaches the engine, is `sync/policy#P10.3` (`RunCleanup`) and K12.4.
- A device that holds a live media-name entity whose blob record is deleted (server state `deleted`, K9.3; a `present` state it may hold from before is read again as K9.3 "Stale `present`" says) uploads the blob again from its local file when the file has that content. Otherwise the name is reported as missing media (`mediaMissing`).

## K10. Local tracking store

### K10.1 File and ownership

- One SQLite file, `meguri-cloud.db`, in the collection directory (app-private Application Support, KTD12). It is never attached to the collection, never part of the derived store and never in a backup of the collection.
- The Rust core (meguri-sync) owns the file and its schema. The Swift CloudKit actor passes CloudKit facts to the core as opaque values: record system fields (from `encodeSystemFields`), engine state serializations, fetched records and save results. The core commits them together with the related tracking changes in one transaction. Engine state, inbox, acknowledgements and conflict copies therefore commit atomically with each other; only the collection file is separate (K13.2).
- Nothing for CloudKit is added to `collection.anki2`: no table, column, index, trigger or config key (KTD25). A local file with any tracking state still opens in Anki unchanged.
- Commits that record server facts (inbox rows, engine state, acknowledgements, sent marks, conflict copies, generation states, apply and re-key intents) are durable before any step that depends on them (K12, K13). Local dirty marks (K11.1) may commit without a full file sync; the reconcile scan covers their loss (K11.4).
- Tracking never records a fact that depends on a collection commit that is not flushed (K1): records are built only from flushed rows (K12.1), and an apply is marked done only after its commit is flushed (K13.2). A power loss can therefore lose collection commits, never tracking facts about them.
- A missing or unreadable tracking file is never rebuilt by inferring acknowledgements: the device treats it as a new connection to iCloud, which needs an explicit collection choice (R2055, `sync/cloudkit-recovery`).

### K10.2 Binding

- A binding row holds: a binding id (16 random bytes) and its creation instant; CloudKit environment, container identifier, Apple account identity (an opaque value from CloudKit), epoch id; the epoch's record key (K4.1, from the `MGEpoch` it published or adopted); the local collection's publication identity (`engine/collection-replace#R6.1`); state `active` or `isolated`, and, while isolated, the isolation reason (`sync/cloudkit-recovery#V2.2`, which gives the reasons and which one replaces another; `environment_changed` is derived and never stored); and the media and recovery facts K10.3 lists. Every table of K10.3 is keyed by binding except `media_hashes` (a cache of the media folder, shared by all bindings) and `recovery_ops` (its rows name the bindings before and after, `sync/cloudkit-recovery#V2.3`). What isolates a binding on account change is `sync/cloudkit-recovery` (R2055).
- At most one binding is active per container and environment (decided 2026-10-27, `sync/cloudkit-recovery#V9.1`). Builds that use separate containers can share one data directory, and so one tracking file; each has at most one active binding of its own. The newest binding of the running build's container and environment (by creation instant) is "the binding" of `sync/policy`.
- A binding is **usable** only while all hold: it is `active`; the open collection's publication identity equals the binding's; `cloudPublicationPending` is false (`engine/collection-replace#R6.1`); the running build's container and environment are the binding's; the account is the binding's. Otherwise nothing runs against it: no reconcile scan, no send, no apply, no purge. Its pending rows, inbox and conflict copies are kept for `sync/cloudkit-recovery`.
- A binding of another container or environment is invisible to the running build: the build never starts an engine for it, isolates it, or writes any of its rows (V9.1, V22 invariant 13). Only two reads see it: the feed floor (K10.3), which counts every active binding's consumed counter, and the derived `environment_changed` reason of the status (`sync/policy#P14.2`), when this build has no binding of its own.
- So a restore, a `.colpkg` import, a manual Anki import (FullDownload) or a switch back to an account whose local collection was replaced never sends the new collection's differences as edits or deletions to the old epoch (R1548, R2056).

### K10.3 Tables

Logical content; the physical schema is the implementer's.

| Table | Per row |
|---|---|
| `bindings` | the K10.2 values: binding id; created at; environment; container; account identity; epoch id; record key; publication identity; state `active` or `isolated`; isolation reason (`sync/cloudkit-recovery#V2.2`; absent while active). Media: the acked media key and whether `root` is acked (K9.2); `media_zone_missing`, set once and never cleared for this binding (K2, K9.2, K12.5). Recovery: the acknowledgement counter, which each acknowledgement takes (a save result or an applied fetched version), and the confirmation mark, the counter's value at the last drained root fetch that confirmed the epoch (`sync/cloudkit-recovery#V10.2`); the last applied `MGActive`: its epoch id and acked system fields, set when the binding is created from the `MGActive` it publishes or adopts and at each fetched `MGActive` version or deletion (K2, "Confirmed root"); the epoch's `crt`, set from the `MGEpoch` the binding publishes or adopts and then from each fetched `MGEpoch` of the bound epoch (K13.1 step 2; the crt check of `sync/policy#P6`); whether the media adoption window is open (K9.4, `#V14.4`) |
| `engine_state` | binding; the latest engine state serialization; when it was saved |
| `entities` | binding; kind; key bytes; record name; origin; acked system fields (absent until acked); server `erev`; acked row digest; acked payload digest; last observed local row digest; `local_rev`; `acked_rev`; `change_seq`; `sent` (K12.1); in-flight list (one entry per record handed to the engine and not yet acked: its `local_rev`, payload digest, image seq, and whether the image was held, `sync/policy#P7.2`, `#P12.4`); base payload (only while pending, K11.2); acked payload (media names only, K11.2); state `live`, `tombstone` or `none`; `deleted_at_ms`; flags `oversize`, `unreadable`, `unsyncable`, `file_missing` (K9.4), `withheld` (K9.4), `superseded` (K9.4), `removal_held` (K9.4), `media_changed` (K9.3, `sync/policy#P13.3`), `blocked_format`; for `withheld` and `superseded`, the content digest it was classified with (K9.4); for `media_changed`, the SHA-256 the clone check found; the acknowledgement counter value of its latest acknowledgement (`sync/cloudkit-recovery#V10.2`); `purge_due`: a tombstone marked for deletion from CloudKit (K17.4, `sync/policy#P10.3`); `seen_pass`: the id of the last absence pass that saw its record (K17.4) |
| `inbox` | binding; entry kind `record` or `deletion` (K13.1); record type; record name; `erev` (records only); system fields (records only); envelope bytes (records only); staged asset file; reason (`ready`, `generation`, `reference`, `structure`, `format`, `asset`, `apply_failed`; a deletion is `ready` or `apply_failed`); for `apply_failed`, the failure (`apply` or `rekey`) and its error (K13.2, K14.2); source `fetch` or `save_conflict` (K12.3); retry count; fetched at |
| `apply_intents` | binding; intent id; inbox ids; per key the op writes: its role (`entry` or `side_effect`), expected row digest before and after, and for a `side_effect` key of a clean entity its before-image payload; the op's seq once committed (K13.2); for a media apply, which has no op, the names instead, each with its expected states in order (before, the states its own store calls leave between them, after; K9.3) |
| `rekey_intents` | binding; kind; old key; new key; origin; the op's seq once committed (K14.2) |
| `generations` | binding; generation id; origin `local` or `remote`; state (K19.7); manifest; local op seq that made it; claim attempts; for a remote `waiting` generation, its last progress instant (`sync/cloudkit-recovery#V18`) |
| `generation_maps` | binding; notetype id; from and to `struct_gen`; field map; template map |
| `notetype_layouts` | binding; notetype id; local `struct_gen`; acked server `struct_gen` (K19.2) |
| `blobs` | binding; content SHA-256; record name; server state (`unknown`, `present`, `deleted`; K9.3); `recheck` (K9.3, "Stale `present`"); upload state; download state; staged path; unreferenced since; deletion due (K9.5, `sync/policy#P10.3`) |
| `zones` | binding; collection zone name; epoch id; first seen at (when this binding first learned of the zone); for the previous epoch's zone, the `MGActive.activated_at_ms` of the switch that retired it; deletion due (K18.5, `sync/policy#P10.3`). A row exists for every `c-` zone other than the bound one that the binding knows of: the zone of `MGActive.previous_epoch_id`, and zones the engine's database changes report |
| `media_hashes` | file name; size; modification time; SHA-1; SHA-256 (K9.1) |
| `media_feed` | binding; the journal's `feed_id`; the consumed counter: the feed counter up to which this binding's tracking has committed every item (`media/media-journal#J6`) |
| `conflicts` | binding; conflict id; kind; key; reason; losing envelope; winning payload digest; for a note or card copy, the note type's field names and template names at copy time; for a note copy, the deck of the note's first card at copy time (`sync/cloudkit-recovery#V17.3`, `#V17.4`); created at; resolved at |
| `refused` | binding; record name; reason; envelope bytes; seen at |
| `scan_state` | binding; `col.mod`, `col.scm` and file identity (K11.4) at the last drained close, and the file identity at the other commits of `sync/cloudkit-recovery#V8.2`; drained flag |
| `absence_passes` | binding; zone; pass id (16 random bytes); started at (K17.4) |
| `recovery_ops` | one row per recovery op, with the columns of `sync/cloudkit-recovery#V2.3`: op id; kind `prefetch`, `adopt`, `publish` or `abandon_generation`; step; binding before (with its state and isolation reason); binding after; epoch; source; candidate identity; active change tag; generation id and manifest change tag; fence before; staging; started at |
| policy rows | `sync/policy#P19` (last exchange, last attempt, last quiet point, tracking probe) |

- `local_rev` grows by 1 whenever the observed local row digest changes. `acked_rev` is the `local_rev` whose content the server acked (K12.3).
- `change_seq` is the value of a per-binding counter taken whenever the entity becomes pending or changes while pending: at each dirty mark (K11.1), when the reconcile scan makes it pending (K11.4), and when an apply leaves it pending with a merge result (K13.2). `sync/policy` uses it for the Sync Now boundary (R2048, `sync/policy#P12.2`).
- Applied inbox entries are deleted, not flagged, in the tracking transaction that settles their apply (K13.2); their staged files are deleted after that commit, and the sweep of `sync/cloudkit-recovery#V6` removes any left by a crash.
- `sent` is true once any version of the entity was handed to the engine; it never goes back to false.
- **Feed floor** (`media/media-journal#J6`). The journal has one `cloud_feed_floor`, and builds of several containers or environments can share the journal and the tracking file (K10.2). Decided 2026-10-27: the floor is at most the lowest consumed counter, for the journal's current `feed_id`, of every active binding in the tracking file, whatever its container or environment; isolated bindings do not count, so they never keep removal items alive. No build writes the floor above another binding's consumed counter, so one build consuming the feed never deletes a removal item another build has not consumed yet.
  - A binding with no `media_feed` row for the current `feed_id` is a new consumer. When no other active binding has such a row, tracking sets the floor to the journal's current counter; otherwise it leaves the floor as it is, which is at most the current counter. Then it takes the snapshot of its reconcile scan (K11.4) and records the snapshot's counter as its consumed counter.
  - After that, tracking writes the floor only after the tracking commit that stores a consumed counter, and only to the lowest consumed counter over the active bindings, never above it.
  - Tracking unsets the floor when the tracking file holds no binding any more.
  - A binding whose `feed_id` differs from the journal's, or whose consumed counter is below the floor, starts as a new consumer, so the reconcile scan runs before it consumes the feed (J6). A removal it missed then reads as `missing` (K9.4), so the file is downloaded again instead of deleted.

## K11. Local changes

### K11.1 Dirty marks

- A tracking observer (an in-core `ChangeObserver`, `engine/change-stream#S2`) runs for every committed op. For each entity the op changed it computes the new row digest (absent for a removed row) and, if it differs from the last observed one, increments `local_rev`, takes a new `change_seq` and records the digest. A first-seen entity gets a random `origin`.
- It records marks only for the usable binding (K10.2). An isolated binding gets none, so edits committed while a binding is isolated are not in its tracking; the path that makes it active again runs the reconcile scan first (`sync/cloudkit-recovery#V2.2`, "Restoring a binding"; K11.4).
- For a `CloudIncoming` op (K13) it records the op's seq in the apply intent and marks no key of the intent dirty, whether the key is an `entry` or a `side_effect` (K13.2): the settle makes `side_effect` keys pending as local changes after the WAL flush. The core also records the seq from the op's result, since the observer does not run for an empty change set (K13.2). For a `CloudRekey` op (K14) it records the seq in the re-key intent and marks the rewritten reference rows dirty (K14.2).
- Media-name entities get dirty marks from the media journal feed (K9.4). A feed item is consumed only after its result commits in tracking; the consumed counter (`media_feed`, K10.3) commits in the same transaction.
- A tracking observer run that fails or panics (S2 lets the commit stand), its dirty-mark commit included, sets an in-memory **scan required** flag. While it is set, no send and no apply runs, `scan_state` is not recorded as drained, and a reconcile scan (K11.4) starts, except during a tracking stop: then it starts when the stop clears (K11.4, "Failed commits"). The flag clears when that scan's results have committed.

### K11.2 Base capture

- When an entity becomes pending while it was clean (its row digest before the op equals the acked row digest), the observer stores the op's before-image, as a payload, as the entity's base. `engine/change-stream#S2.4` gives the tracking observer the before-images of notes, decks, presets, notetypes (with their `fields` and `templates` rows), tags, cards, review-log rows and config values.
- Media names take their base from the acked `MediaName` payload, which tracking keeps for every media-name entity (it is small).
- A pending entity keeps its base until it is clean again. After a merge the base becomes the server version merged with, in the tracking transaction that settles the apply after the WAL flush (K12.3, K13.2, K15), never earlier.
- A base lost in a crash is not recreated; K15.1 then merges without a base.

### K11.3 Anki columns

- Local changes keep Anki's stamping: `usn` -1 and the modification time (`storage/ids-usn-mtime#S2`). CloudKit tracking never reads `usn` as its pending marker and never writes `mod`, `mtime_secs`, `col.usn`, `col.ls` or `col.scm` for its own bookkeeping.
- `usn` is not in payloads (K6). A `CloudIncoming` op writes each changed or new row with the column values of the payload, modification time included, and `usn` -1, so the next manual Anki transfer from this device sends it (`storage/ids-usn-mtime#S2`). It sets `col.mod` to now when it changes something (`storage/ids-usn-mtime#S5`) and leaves `col.usn` and `col.ls` alone. It sets `col.scm` only when it applies a structural generation that changes local structure, as `notetypes/schema-changes` G1 says (`cloudkit_structure_applied`), so the next manual Anki transfer is one-way.
- A manual Anki transfer that rewrites only `usn` values changes no row digest, so it sends nothing to CloudKit. Manual transfers keep their own state; CloudKit never resolves or reads it (KTD26).

### K11.4 Reconcile scan

The scan makes tracking reflect the collection when the observer may have missed commits. It runs only against a usable binding (K10.2).

- At open, before the writer admits ops, tracking durably clears `scan_state`'s drained flag. At close and suspend, after the writer stops admitting ops and when the scan-required flag (K11.1) is clear, tracking records `scan_state` durably before the sidecar records the clean shutdown.
- The scan runs at collection open unless all hold: the sidecar reports a clean shutdown (`storage/durability-and-files#F3`), and `scan_state` was drained at the last close and records the opened file's `col.mod`, `col.scm` and file identity.
- The **file identity** is that of `sync/cloudkit-recovery#V8.2`: the file system's device id, the inode number and the creation time (`st_birthtime`) of `collection.anki2`. Tracking records it at each drained close and suspend, and at the other commits V8.2 lists (a binding created for the open file, a resume, a completed adoption). A recorded identity that differs from the opened file's is V8.2's `file_mismatch`, not a reason to scan: the binding is isolated and no scan runs against it.
- It also runs after an external change is detected (F3), when the scan-required flag is set (K11.1), when publishing reaches K18.3 step 2, and when an isolated binding becomes active again (`sync/cloudkit-recovery#V2.2`, "Restoring a binding", which sets the flag).
- It reads every entity on a reader snapshot, off the writer, and compares row digests with tracking. Dirty marks of ops committed after the snapshot take precedence. Sending waits for the scan; study does not.
- Each entity the scan makes pending, or finds pending with another digest, takes a new `change_seq` (K10.3).
- **Failed commits** (decided 2026-10-27). Any tracking commit that fails reports `TrackingWriteFailed` (`sync/policy#P3`; `noSpace` for SQLite full or `ENOSPC`), whichever writer made it: a dirty mark (K11.1), a scan result, an inbox insert or an acknowledgement. That sets the tracking stop (`sync/policy#P15.2`). A scan whose commit fails ends at once; the scan-required flag stays set, and the scan does not restart by itself. It starts again only when the tracking stop clears (a successful probe, or a local change whose commit succeeded, P15.2), and it runs before the engine starts again. So a full disk never makes the scan read the collection in a loop, and the closed gate always has an attention item (`localStorageFull` or `trackingError`).
- **Adoption window** (`sync/cloudkit-recovery#V14.4`, K9.4). While the binding's window is open, the scan classifies every media name by what the adopted epoch holds, as V14.4's table says, and never compares a name as a local change: clean, `superseded`, `file_missing`, `withheld`, or no entity. The window closes in the first tracking commit, at or after the binding's first scan, that consumes the feed while `media/media-journal#J6` reports it complete; the window is durable, so a crash or a relaunch keeps it open.
- After the window, a `superseded` or `withheld` entity keeps its flag while its local facts equal its classified digest. Other facts lift it as K9.4 says; the scan never takes a `superseded` file as L.

| Local row | Tracking | Result |
|---|---|---|
| present, digest equals the acked row digest | live | clean; a pending entity whose change was undone becomes clean |
| present, digest differs from the last observed one | live | pending; keep any base |
| present | none | new entity: random origin, pending, no base |
| present | tombstone, pending (not acked) | live and pending again; keep the base. An entity never acked is a new entity again |
| present | tombstone (acked) | id-keyed kind: re-key (K14.3). Name-keyed kind: pending re-creation with the tombstone as base |
| absent | live | pending tombstone; for a media name only when the absence cause is `removed`, otherwise a missing local file (K9.4). Cause `removed` while J6 reports the feed not complete gives the mark `removal_held` instead (K9.4). A `superseded` name becomes a missing local file whatever the cause; a `withheld` one follows K9.4 (cause `removed` drops the flag, `missing` keeps it) |
| absent | none, or tombstone | nothing |

For media names the local row is the journal record of the name's case key, and "absent" follows the absence cause the journal reports (`media/media-journal#J6`); a name with no record and no retained removal item reads as `missing`.

## K12. Sending and acknowledgement

### K12.1 Building a record

An entity is **sendable** when all hold:

1. it is pending, and not oversize, unreadable, unsyncable, `file_missing`, `withheld`, `superseded` or `removal_held` (K9.4), or format-blocked. An entity with one of the flags `file_missing`, `withheld` and `superseded` is not pending for the policy either (`sync/policy#P3`); one with `removal_held` is pending and blocked until registration completes;
2. `sync/policy` makes it eligible (`sync/policy#P7`). The policy supplies the image to send: the live row, the held-back earlier image (R2047), or nothing (deferred);
3. its binding is usable (K10.2), and no scan is running or required (K11.1, K11.4); and, for a collection-zone record, the current engine instance has drained a fetch of `meguri-root` that confirmed the bound epoch (K2 "Confirmed root", `sync/cloudkit-recovery#V10.2`), except for the binding of a `publish` op while the op's step is before `done`. Until that root fetch drains, the builder returns no collection record, whatever the engine schedules;
4. the commit that last wrote its rows is flushed (K1);
5. a live version only: every entity it references (K6) is acked live on the server or is sent in the same atomic batch (batch rules below). An entity whose reference is held (oversize, unreadable, not eligible, waiting) is held too, and status lists it;
6. a live note only: every media name its fields reference (`text/media-references`) whose local entity has never been acked is held back with it until that name is acked (K15.9). A name flagged `withheld` holds back no note (K9.4): it is never acked, and a note that already referenced it is the epoch's own. A local op that adds a reference to it releases it (K9.4 lift 2), so it is then pending and holds the note as any unacked name;
7. it does not depend on a local structural generation that is not claimed (K19.7 `local`, `claiming`): it is not a member or anchor of one, and it is not a note or card of a notetype that such a generation changes (K19.2);
8. no server version of it is in the inbox, whatever the entry's reason (`ready` included, so a save conflict's server version is applied first, K12.3; and `generation`, `format`, K5.4), and no apply intent names it (K13.2).

- At batch build time the core builds the envelope: `erev` = server `erev` + 1 (1 for a new record), the entity's origin, the generation id while the entity is a member of a generation that is not yet complete on the server (K19.3), and the acked system fields when the entity has been acked.
- Before it hands a batch to the engine, tracking durably sets `sent` and adds to the entity's in-flight list an entry with the record's `local_rev`, its payload digest, and the image seq and held flag the policy gave for its image (`sync/policy#P7.2`).
- Order within the collection zone: a generation's claim batch first (K19.3); then live records of notetypes, presets, decks (parents before children), media names, notes, cards, revlog, tags, config; then tombstones of cards, notes, decks (children before parents), presets, notetypes, revlog, tags, config, media names. Blob records go to the media zone after collection records: across requests, and within one request whose scope includes both zones (`sync/policy#P13.1`). A request scoped to one zone holds only that zone's records (below).
- **Send scope** (documented in the SDK headers, 2026-10-27: a batch that includes a change outside the scope of the send context's options makes the send fail with `invalidArguments`). The builder reads the scope of each batch request (its zone ids, excluded zone ids and record ids) and puts in the batch only records and deletions within it. A sendable name outside the scope stays pending and in the engine's pending list for a later request. So a `RequestSend{collection}` never carries a blob record or a media-zone deletion.
- **Batches** (decided 2026-10-27). Every batch stays within K7's request budget, and the core builds each one itself (the engine's batch initializer with records and an atomic flag; the engine applies no size limit to it). Ordinary batches are not atomic (`atomicByZone` false), so one failing record never fails the others. A batch is atomic only when it is a claim batch (K19.3) or holds a **reference group**: a live record sent together with entities it references that are not yet acked live (rule 5), with their own unacked references in turn. A reference group is never split across batches, and an atomic batch holds only whole groups. A group that does not fit one batch is not sent as a group: its referenced entities go first in ordinary batches, and the records that reference them wait under rule 5 until they are acked. So a card is never saved without its note, whichever batch fails.

### K12.2 Save policy

Every save is conditional on the server's change tag of the version this device last acked (CloudKit's default save policy for records with system fields; a new record without system fields). Meguri never uses a policy that overwrites the server version unconditionally.

**Assumption A5**: saving a new record (no system fields) whose name exists fails with a server-record-changed error that carries the server record; saving with system fields for a record that no longer exists fails with an unknown-item error and does not create it.

### K12.3 Outcomes

| Outcome | Tracking result |
|---|---|
| Saved | acked system fields, server `erev`, acked row digest and acked payload digest take the saved version's; that and every older entry leave the in-flight list. If the current local row digest equals the sent row digest, `acked_rev` = `local_rev` and the base is cleared; otherwise the entity stays pending and its base becomes the saved payload |
| Server record changed, and the server version's payload digest equals an in-flight digest | as saved, for that in-flight revision (an earlier save whose result was lost) |
| Server record changed, and the server version has a `generation` this device has not adopted, an `nt_struct_gen` above the local notetype's, or a newer format | the server version goes to the inbox (reason `generation` or `format`); the local change stays pending and is not sent until that version is adopted or applied (K19.5), then the next row applies |
| Server record changed, same origin, other content | the server version S goes through the inbox, so no tracking fact runs ahead of the collection write it depends on (K10.1). In the tracking transaction that records this result, S is checked as a fetched record (K20) and inserted into the inbox (source `save_conflict`, reason as K13.1 gives, normally `ready`); nothing else changes: the entity keeps its acked system fields, `erev`, digests and base B. Then K13.2 applies the entry like any fetched one: its op merges B, the current local row L and S (K15) and writes R; only after the WAL flush covers that op do the acked fields, `erev` and digests take S's and the base become S. R equal to S leaves the entity clean; otherwise it stays pending with a new `change_seq` and is sent again under K12.1. A local tombstone as L follows K17.1 |
| Server record changed, other origin | id-keyed kind: K14.1; name-keyed kind: the entity takes the server origin, then as the row above |
| Unknown item, entity acked | K17.4 |
| Batch request failed (another record of an atomic batch failed) | no change for this record; it stays pending and its name is added back at once (K12.4). A claim batch follows K19.4 |
| Limit exceeded (batch too large) | no change; the batch's records stay pending. For the rest of that send the core halves both bounds of K7's request budget (at least 1 record), and the names are added back at once (K12.4). A batch of one record that fails so is added back only at the next event trigger, so the loop is bounded. A claim batch counts a failed claim attempt (K19.4) |
| Asset upload or asset file error (`pa`, `ma`) | stays pending; the staged asset is built again at the next send |
| A record of a batch the engine reports sent, with no result for it | stays pending; its in-flight entry stays |
| Zone errors: `zoneNotFound`, `userDeletedZone` | by zone and cause, as K12.5 maps them; the same outcome as a fetched zone deletion with that cause |
| `quotaExceeded` | stays pending; error class `quotaFull` to status (R1561, R2059). The engine drops the name and Meguri adds it back only at the next event trigger (K12.4); no timer (`sync/policy#P15`) |
| `invalidArguments` (a batch with a change outside the send scope, K12.1, or another malformed request) | a programming error: nothing changes and the records stay pending. No name is added back at once; the next full update adds them at an event trigger (K12.4). Decided 2026-10-27: status reports `cloudError` at once, not after 2 failing cycles, and the error is logged with the request's scope and record names for Copy Debug Info (`sync/policy#P15`) |
| Throttling, network, authentication, other per-record errors | stays pending; error class to status (R1561) |

- **Save-conflict assets** (documented in the SDK headers, 2026-10-27: when a save fails with `serverRecordChanged`, CloudKit does not download the assets of the server record in the error, so every asset field of it has no file). The in-flight digest row and the `generation` and format checks of the rows above read only `e`: `payload_sha256`, `generation` and `format` are envelope fields (K5.1), so they need no asset. When S's payload is in `pa` (or a manifest's member list in `ma`), the entry still goes into the inbox in the transaction that records the result, with reason `asset`, and its `nt_struct_gen` check and its apply wait for the payload. Decided 2026-10-27: U114 then reads S by record ID with every field once, right after the event handling of that send returns (never inside it, `sync/cloudkit-recovery#V3.3`). That first read does not count toward K13.1's 5 attempts, which apply to retries. A missing file there is the known Apple behavior, never a sign of a corrupt asset. When the read fails, the entry follows K13.1's asset retries. A claim that waits on such an entry (K19.4 row 3) is sent again only after it applies.
- Acknowledgement compares content, not counters: an ack of revision N never clears revision N+1 (U113 test).
- **Assumption A6**: CloudKit's sync engine reports a batch's per-record results before it reports the engine state that reflects them, and the engine's event callback may await durable persistence before returning.
- **Assumption A10** (documented in the SDK headers, 2026-10-27; signed runs still check it, K22.3): zone loss reaches the device on two channels, each with its cause. A fetch of database changes reports each zone deletion with a reason: `deleted` (an app deleted it), `purged` (the user deleted the app's data in system settings) or `encryptedDataReset`. A send fails with `zoneNotFound`, whose user info says whether the user reset encrypted data, or with `userDeletedZone`. K12.5 maps both.

### K12.4 Engine pending changes

- Tracking is the only source of pending work. The sync engine's pending record-zone changes are a copy of it that may be stale after a crash (R2051).
- The **full update** (`sync/policy#P4` `UpdatePending{full}`): the adapter adds to the engine's pending changes the record name of every sendable entity, the record name of every blob ready to upload (K9.3; none while media is off or `media_zone_missing` is set), the control records that are due (K12.6), and the record deletions and zone deletions that `RunCleanup` marked due (`sync/policy#P10.3`: tombstones with `purge_due`, blobs with deletion due, `zones` rows with deletion due). It keeps each due control record and each due deletion until its result. It removes every other engine pending name. It hands back the names the engine dropped after a failure (parked, `sync/policy#P1`).
- It runs where `sync/policy` names it, and nowhere else (decided 2026-10-27, `sync/policy#P2`, `#P10.1`): at an engine start (`StartEngine`), when the gate opens (the end of a reconcile scan included), at a local change, a foreground return, Sync Now (P12.3 step 4), an engine account event, a drained fetch of the collection zone, the first drained root fetch of an engine instance, `SessionEnded`, `DeviceSettings{automatic: on}`, and in the background and quit flushes. Never at an `EngineError`, an `EngineActivity` or a `VolumeSpaceAvailable` report.
- Between full updates, `UpdatePending{changes}` (at a tracking commit that is not a local change) adds a name when its entity becomes sendable, its blob ready, its control record or its deletion due, and removes a name that stops being sendable. It never adds a parked or deferred name, so a failed save is not handed back by its own result.
- **Failed saves** (documented in the SDK headers, 2026-10-27). The engine keeps a failed save in its pending list only for the errors it retries itself: `notAuthenticated`, `accountTemporarilyUnavailable`, `networkFailure`, `networkUnavailable`, `requestRateLimited`, `serviceUnavailable`, `zoneBusy`. It drops the name on every other error. Decided 2026-10-27: after each sent-record-zone-changes event, once K12.3 has run for every record of the batch, the adapter adds back at once the names that failed with `batchRequestFailed` or `limitExceeded`, and a name whose `serverRecordChanged` result left it sendable. After a server-record-changed merge the entity is sendable again only once its inbox entry is applied (K12.1 rule 8); it is then added as any entity that becomes sendable. Names that failed with `quotaExceeded`, an asset error or any other error are not added back at once: the next full update adds them, at an event trigger, with no timer (`sync/policy#P2`). An unknown-item failure follows K17.4.
- When the engine asks for the next batch, the core builds the envelopes then (K12.1) and skips names that are no longer sendable (clean, re-keyed, held, tombstone already acked) and names outside the request's scope (K12.1, "Send scope").
- So a crash between a dirty mark and the engine's pending add loses nothing: the next engine start adds it again.

### K12.5 Zone loss (decided 2026-10-27)

A device that only fetches learns of zone loss from a fetched zone deletion; a device that sends learns it from a send error. Both map to one outcome, by zone and cause. Before it isolates, the actor reads the account status and identity again (`sync/cloudkit-recovery#V2.2`).

| Zone | Cause | Outcome |
|---|---|---|
| any Meguri zone | fetched reason `encryptedDataReset`, or `zoneNotFound` with the encrypted-data-reset flag | isolate `encrypted_data_reset`: an encrypted data reset is account-wide (V10.1) |
| the bound collection zone, or `meguri-root` | fetched reason `purged`, or `userDeletedZone` | isolate `zone_deleted_by_user` (V10.1) |
| the bound collection zone | fetched reason `deleted`, or plain `zoneNotFound` | `MGActive` is read again: when it names another epoch, isolate `epoch_switched` (K18.4); otherwise isolate `zone_missing` (V10.1) |
| `meguri-root` | fetched reason `deleted`, or plain `zoneNotFound`; or a fetch reports the `MGActive` record deleted | isolate `active_missing` (V10.2) |
| `meguri-media` | fetched reason `deleted` or `purged`, `userDeletedZone`, or plain `zoneNotFound` | no isolation; media work stops for this binding for good, and collection sync continues (K2, V10.1). A purge of all of Meguri's data also deletes the collection zone, whose row isolates |
| another `c-` zone | any | ignored; it is not this binding's (K18.5) |

- No row deletes the local collection or its pending rows. Apple asks an app to delete locally cached data and not resend it after `purged` and `encryptedDataReset`. Decided 2026-10-27: Meguri's local collection is the user's own data, not a cache, so it stays and nothing is sent again without the user's explicit V15 choice. What tracking holds only as a copy of the server (the binding's inbox entries that no apply intent names, their staged files, and `refused` rows) is deleted with the isolation. Pending rows, intents and conflict copies are kept (`sync/cloudkit-recovery#V1`).
- No row creates a zone again (K2).
- Decided 2026-10-27 (R2066 approval): a `publish` op creates its zones and saves `MGEpoch`, `MGMediaRoot` and `MGActive` with own operations (`sync/cloudkit-recovery#V26.1`), and the new binding's engine instance starts only once the op's step is `zones` (V15.2), so the engine never meets a zone of that op before it exists. A zone-not-found result of an own operation maps through this table as V26.1 says, with V26.1's one exception for `meguri-root` at the `MGActive` save.

### K12.6 Control records (decided 2026-10-27)

Records that are not entities and that the engine sends are **control records**. The engine sends only what is in its pending lists, so each must stay there until its result, or the step that needs it stalls with no error. Tracking derives which control records are due from rows it already keeps durably:

| Control record | Due while | Source row |
|---|---|---|
| `MGGeneration` with state `claimed`, with its claim batch (K19.3 step 1) | the local generation is `claiming` (K19.7) | `generations` |
| `MGGeneration` with state `complete` (K19.3 step 3) | the generation is `claimed` and every member is acked | `generations`, `entities` |

- Removed (2026-10-27): the rows for the zone saves of `meguri-root`, `meguri-media` and `c-<epoch id>`, for `MGEpoch`, `MGActive` and `MGMediaRoot`, and for a manifest with state `abandoned`. Decided 2026-10-27 (R2066 approval, with `sync/cloudkit-recovery#V26.1`): these are U114's own operations, outside the engine, with V26.1's conditions and outcomes; K12.3's rows never apply to them. The engine's send path carries only `MGEntity`, `MGGeneration` (states `claimed` and `complete`) and `MGBlob` records and the deletions `RunCleanup` marks due. So a `publish` op's zones and `MGEpoch` exist before its engine instance starts (V15.2, step `zones`), and no engine send waits on a zone that does not exist yet.
- The full update adds every due control record and keeps it until its result; neither form of `UpdatePending` removes it while it is due (K12.4). A crash loses nothing: the next engine start's full update derives them again from the source rows. A control record the engine dropped after a failure is parked like an entity (`sync/policy#P1`) and handed back by the next full update at an event trigger. No timer runs.
- When the engine asks for a batch, the builder builds a due control record from tracking at that moment, within the request's scope (K12.1), and skips one that is no longer due. In the collection zone a claim batch goes first (K12.1 order). A claim batch holds the manifest with its anchors, which rule 7 of K12.1 otherwise keeps unsendable.
- **New zones.** Removed (2026-10-27): no engine send creates a zone (above).
- **Failed zone saves.** Removed (2026-10-27): zone creation is an own operation (V26.1).
- Failed saves of control records follow K12.3 and stay due: a claim follows K19.4; a `complete` save that meets a server manifest with state `abandoned` makes the generation `lost`, as `sync/cloudkit-recovery#V18` says for a device whose own generation was abandoned (K19.6).

## K13. Incoming records

### K13.1 Order

For each fetched batch, in this order:

1. Decode and check every record (K20). A permanently refused record goes to `refused`. A record with a retryable problem goes to the inbox with reason `asset`.
2. In one durable tracking transaction: insert every accepted record into `inbox` (entry kind `record`), its asset moved into the cloud staging directory (an asset the actor already staged while the core was suspended, `sync/policy#P11.2`, is recorded at its staged path, not moved again); record a fetched `MGActive` version or deletion as the binding's last applied `MGActive` (K2), and the `crt` of a fetched `MGEpoch` of the bound epoch as the binding's epoch `crt` (K10.3); insert every record deletion the batch reports as an entry of kind `deletion` (record name, reason `ready`), unless neither an entity nor an inbox entry of the binding holds that record name; write the `refused` rows of step 1; and, while an absence pass of the zone runs, set `seen_pass` of every entity whose record name the batch delivered, as a record or a deletion (K17.4).
3. Only after step 2 commits may an engine state that includes the batch's cursor be saved. A cursor never outruns the inbox, for records and record deletions alike (R2051).
4. Apply ready inbox entries (K13.2). Entries waiting for a generation, a reference, a structure, a newer format or an asset stay in the inbox.

- Entries of one record name apply in the order they were fetched. An inbox entry with the same record name and `erev` as one already held is not inserted again. A later version, or a deletion, replaces an earlier entry of that name that no apply intent names; otherwise it waits behind that entry (`sync/cloudkit-recovery#V5` R2).
- An entry with reason `asset` is read again by record ID, with every field, only at `sync/policy#P4` `RetryAssetEntries`. That action runs at a drained fetch of the bound collection zone (`FetchDrained{collection}`, the drains of Sync Now steps 3 and 5 included), at most once per drain and at most 5 times in all, with no timer (decided 2026-10-27, `sync/policy#P2`, `#P10.1`). A foreground return, an account change and Sync Now retry it only through the collection fetch they start. A save conflict's first read (K12.3) is not one of the 5. After the fifth failed attempt it stays in the inbox and status reports needs attention (`assetUnavailable`). It never becomes ready without a matching asset.
- **Assumption A7**: the sync engine delivers fetched records and record deletions before the engine state that advances past them, and reports when a fetch has drained the zone. Decided 2026-10-27: U114 reports the end of a fetch per zone of its scope, as `sync/policy#P3` `FetchDrained{zone, fetched}` says: for a `fetchChanges` call also for a zone it had nothing to fetch for (`fetched` false). A first fetch from no cursor lists every zone that exists, so a zone it does not fetch is gone (K12.5's plain `zoneNotFound` row). The signed runs check both (K22.3).

### K13.2 Apply

- Before it submits a `CloudIncoming` op, the caller runs the day clock's `checkRollover()` and lets its op commit (`sched/day-clock#D8`), so a new day's unbury happens before another device's `lastUnburied` arrives (D7).
- A `CloudIncoming` op (`engine/op-model#O1`) takes ready entries in K12.1's order (live records, then tombstones and deletions). For each target it computes the current local row digest; when that differs from the acked row digest, the entity is pending with L = the current row, whatever tracking's marks say. The exception is a media-name entity flagged `file_missing`, `superseded` or `withheld`: its local file is not a local change, so K9.4 ("Incoming versions over these flags") decides, with L = its acked version for the first two. A media-name entity marked `removal_held` has L = a local tombstone (K9.4). It merges each entry with local pending changes (K15), using the base the entity holds (K11.2), and writes rows with the resulting values. A `deletion` entry applies as K17.4 says.
- Save-conflict entries (K12.3) apply the same way: the entity keeps its old acked fields and base B until this apply settles, so a replay merges B, L and S again, never S against itself.
- Before its COMMIT the op durably records an apply intent in tracking: the inbox ids and every key the op writes, each with its expected row digest before and after the op. Decided 2026-10-27: each key has a role. An `entry` key is the entity of one of the op's inbox entries. A `side_effect` key is any other entity the op writes: K13.3's reference results (a card moved to deck 1 or home from a filtered deck), K15.6's recomputed descendant deck names and its cycle and duplicate-name fixes, the cards a note tombstone removes (K17.3), and the notes and cards of a notetype tombstone. For a `side_effect` key whose entity is clean, the intent also holds its before-image as a payload, which becomes its base (K11.2).
- After COMMIT the core records the op's seq, taken from the op's result (`engine/op-model#O4`), in the intent, whether or not the change set is empty. The tracking observer records the same seq when it runs, which it does only for a change set that is not empty (`engine/change-stream#S2`); both writes are idempotent. Decided 2026-10-27: so an apply whose entries all equal the local rows (it writes nothing, `storage/ids-usn-mtime#S4`) still settles. The observer marks no key of the intent dirty, `entry` or `side_effect`; the settle below does what a dirty mark would. A later dirty mark on a key of the intent compares with the intent's after digest.
- Once the writer's flush covers that seq (`storage/open-and-connections#C9.2`; an op that wrote nothing is covered as soon as the commits before it are), one tracking transaction deletes the inbox entries (K10.3), updates their entities and removes the intent:
  - each `entry` entity takes the applied server version's acked system fields, server `erev` and acked digests. When the written row equals that version, the entity is clean and its base is cleared. Otherwise it stays pending: its base becomes that server version (K11.2), and it takes a new `change_seq` (K10.3);
  - each `side_effect` entity becomes pending as a local change: it takes a new `local_rev` and `change_seq` and records its new row digest (K11.1). An absent row makes a pending tombstone (K17.1). A clean entity takes the before-image the intent holds as its base; a pending one keeps its base. So these writes are sent onward (K13.3, K15.6, K17.3), Sync Now selects them (`sync/policy#P12.2`), and status never shows `upToDate` while they wait.
- At open, an intent left by a crash or a power loss is settled before any send, over all its keys, both roles: when every key matches its expected after digest (also when before and after are equal), finish the tracking update above; when every key matches its before digest, the op did not commit or was lost, and the entries are applied again; any other mix runs the reconcile scan (K11.4) and applies the entries again with K15.
- **Failed applies** (decided 2026-10-27). An apply op that rolls back for a reason other than a cancel (an error, a constraint failure such as `idx_decks_name`, a panic) settles its intent at once (`engine/op-model#O1.1`). In that settle transaction its entries take the inbox reason `apply_failed`, with failure `apply` and the error, and their retry count grows by 1 (K10.3). Such an entry is not ready: `sync/policy`'s `ApplyReady` does not take it at the `TrackingChanged` of that settle or of any other tracking commit. It is applied again only at an event trigger (`sync/policy#P2`), in an op of its own (one entry per op; a complete structural generation stays one op, K19.5), after the ready entries, so one entry that cannot apply never holds the others back. An op of one entry that applies settles as above. After the fifth failure the entry stays in the inbox and is tried again once per collection open, at its first event trigger; status reports the attention item `applyFailed` with its kind and key (`sync/policy#P14.4`). The entity is not sent while the entry waits (K12.1 rule 8), so no local version overwrites a server version this device could not apply. A cancelled op (O5) leaves its entries `ready`.
- Applying a version whose row digest equals the local row changes nothing (`storage/ids-usn-mtime#S4`), so replays are idempotent.
- **Size** (decided 2026-10-27). One plain `CloudIncoming` op takes at most 500 entries, which keeps its write phase near 200 ms (`engine/op-model#O6`). More ready entries are applied as a series of such ops, each with its own intent, in K12.1's order, so every reference an entry needs is applied by an earlier op or the same one (K13.3); interactive ops run between them, so a first fetch of 100,000 records never holds the writer for long (R2006). The one exception is a complete structural generation, which is applied in one op whatever its size (K19.5, R2053): with more than 500 entries it is submitted as the catalog op `cloud_incoming_apply` (`engine/op-model#O8`, `#O9`), a declared long write whose cancel between batches of 500 rolls the whole op back, and the generation is applied again later.

### K13.3 References

- Senders hold a record until what it references is acked (K12.1), so a missing reference is normally one still in flight. An entry whose references are not present locally and not in the same apply waits with reason `reference`. It is checked again whenever a record it references arrives. Waiting entries are counted in status; they never apply partly.
- The results below apply only when the reference is a known deletion: an acked or applied tombstone, or an entity absent from the server (K17.4). Only then does the result count as a local change, sent onward.

| Kind | Deleted reference | Result |
|---|---|---|
| card | note | not applied; the entry is dropped (the note's tombstone removes its cards, K17.3) |
| card | `odid` deck (card in a filtered deck whose home is gone) | `did` 1, `odid` 0, `odue` 0, `due` = the old `odue` |
| card | `did` filtered deck, home deck present | back home: `did` = `odid`, `due` = `odue`, `odid` 0, `odue` 0 |
| card | `did` normal deck | `did` 1 |
| note | notetype | not applied; kept as a conflict copy (`missing_notetype`, K16) |
| deck | parent | K15.6 |
| deck | preset of a normal deck | the deck's `kind` blob names preset 1 |

No apply exposes a card whose note, notetype or deck is missing (R2053).

## K14. Identity collisions

### K14.1 Detection

Two devices can create different objects with the same id: ids come from the clock (`storage/ids-usn-mtime#I2`). Two devices can also bring in the same object with the same id: the same `.apkg` imported on both. For an id-keyed kind, a server version of key K, met by fetch or as a save conflict, whose `origin` differs from the local entity's while the local entity has never been acked, is one of:

- **The same object** when: note, equal `guid`; card, equal `nid` and `ord` (notes are applied first, so `nid` already reflects any note re-key); revlog, equal `cid`; deck, equal name (`unicase`); preset, equal name; notetype, equal lists of field names and of template names. The local entity takes the server origin and is merged with the server version under K15 without a base. Nothing is re-keyed.
- **A collision** otherwise: the local object is re-keyed (K14.2).

The same origin mismatch for an acked local entity is refused (K20).

### K14.2 Re-key

- A `CloudRekey` op (`engine/op-model#O1`) gives the local object a new id: the smallest id greater than the old id that no row of its table uses and no entity of the binding's tracking holds, within I3's cap (`storage/ids-usn-mtime#I5`). If no such id exists, the re-key fails. Decided 2026-10-27: a re-key that fails so, or a `CloudRekey` op that rolls back for a reason other than a cancel, gives the inbox entry whose server version needed it the reason `apply_failed` with failure `rekey` (K10.3), and the entry is retried as K13.2's "Failed applies" says; status reports `rekeyFailed` with the kind and key (`sync/policy#P14.4`). The op rewrites the object's references in the same op and stamps the changed rows as local changes. The object keeps its origin.
- Keeping the new id just above the old one keeps a review's instant (`revlog.id`, `storage/ids-usn-mtime#I1`), a note's or card's added date, and the sort order, all to within a few milliseconds. A re-keyed card's interval fuzz, which is seeded from its id (I2), can change; this is accepted.
- Before its COMMIT the op durably records a re-key intent (kind, old key, new key, origin). Once the writer's flush covers the op, tracking moves the entity to the new key and record name and removes the intent, in one transaction. At open, an intent left by a crash is settled before any send: when the collection holds the object under the new key, finish the move; otherwise the op did not commit and the intent is removed.
- Then the server version is applied under K. Undo steps that touch a rewritten key are dropped (`engine/undo#M9.8`).

| Re-keyed | References rewritten |
|---|---|
| note | `cards.nid` of its cards |
| card | `revlog.cid` of its revlog rows |
| revlog | none |
| deck | `cards.did` and `cards.odid`; the value of `curDeck`; the entries of `activeDecks`; key names beginning `_deck_<id>_`; values of keys `_nt_<id>_lastDeck`; template deck overrides in notetype `templates.config` (a notetype change) |
| preset | the preset id in the `kind` blob of each normal deck that uses it |
| notetype | `notes.mid`; `fields.ntid` and `templates.ntid`; the value of `curModel`; key names beginning `_nt_<id>_`; values of keys `_deck_<id>_lastNotetype` |

### K14.3 Re-creation under a tombstone

A live local row under the key of an acked tombstone of an id-keyed kind (for example an import that keeps ids) is re-keyed the same way. Name-keyed kinds (tag, config, media name) have no collisions: the same key is the same entity, and K15 merges it.

## K15. Conflicts

### K15.1 The merge

- Merges run on the device that meets the conflict: at a save conflict (K12.3) or when an incoming version meets a pending local entity (K13.2). A clean entity takes the incoming version without a merge.
- Input: base B (K11.2), local L, server S. Output: result R, and at most one conflict copy (K16). Both sides are first mapped to the newest structural generation they reference (K19.5).
- Each kind is split into parts. Stamp columns (`mod`, `mtime_secs`) are not parts. Per part: changed on one side only, that side; changed on both to the same value, that value; changed on both to different values, the kind's rule. Without a base, every part that differs between L and S counts as changed on both sides.
- The merge is symmetric in content: swapping L and S gives the same parts of R. The exceptions where the server side wins by position are K14 (the unacked side is re-keyed) and K15.9 (S keeps the name).
- Stamp for "later" (K1): `mod` for notes and cards, `mtime_secs` for decks, presets, notetypes and config; none for revlog, tags and media names, which then order by payload digest alone.
- Result: R whose parts all equal S's is S, which is applied and the entity is clean. Otherwise R is written locally, stamped as a local change (`usn` -1, modification time now) when it differs from L, and stays pending. It is sent only through K12.1, eligibility and hold-back included: while `sync/policy` holds the entity back, nothing of it is sent until hold-back ends (`sync/policy#P7.2`, `#P8.3`; R2047).
- Wall-clock order decides only between two different values of the same part, and the losing content survives in a conflict copy, in the revlog, or in the trash (R2052).

### K15.2 Notes

| Part | Both changed differently |
|---|---|
| each field (by `ord` after mapping) | later version's value; conflict copy of the losing version |
| tags | set merge, no copy: R = tags in B kept by both, plus tags either side added; a tag either side removed is gone. Tags compare by the `unicase` fold; when both added one tag with different case, the later spelling |
| `guid`, `flags`, `data` | later version's value; no copy |
| `mid` | changes only through a structural generation (K19) |

`sfld` and `csum` of R are computed from R's fields (`text/checksum-and-sort-field`). The tags column is rewritten in its stored form.

### K15.3 Cards

- A card's **home due** is `odue` when `odid` ≠ 0, otherwise `due`. Its **filtered due** is `due` when `odid` ≠ 0, otherwise none. Both are compared as stored values.

| Part | Columns | Rule |
|---|---|---|
| scheduling | `type`, home due, `ivl`, `factor`, `reps`, `lapses`, `left`, `data`, and `queue` when it is not a hold | the side with the larger `last_review_id` wins the whole part; equal: part-wise three-way, both changed means the later version's |
| hold | `queue` -1 (suspended), -2 (buried by the scheduler), -3 (buried by the user), or none | three-way; both changed: later version's |
| placement | `did`, `odid`, filtered due | three-way on the whole part; both changed: later version's |
| `flags` | | three-way; both changed: later version's |
| `nid`, `ord` | | change only through a structural generation |

- R's `due` and `odue` are rebuilt from the merged parts: when R's `odid` ≠ 0, `due` = R's filtered due and `odue` = R's home due; otherwise `due` = R's home due and `odue` 0. So a card reviewed on one device while another empties its filtered deck comes home with the reviewed home due.
- `queue` of R is the hold when there is one. Otherwise it is the scheduling winner's `queue`, or, when that is a hold, the queue `sched/card-data#K3` gives for R's `type` and `due`.
- This is the deterministic history policy of R2052: the card state comes from the side whose newest review is the newest in the merged review history. FSRS memory state in `data` is not recomputed.
- Cards get no conflict copy: both reviews remain as revlog entities (R2052).

### K15.4 Revlog

- A revlog row is created once. Two different rows with one id are a collision (K14), never a merge.
- A later update (a repair) merges column by column; both changed means the version with the greater payload digest. No copy.

### K15.5 Decks

| Part | Rule when both changed |
|---|---|
| leaf name (last name component) | later version's |
| parent (`parent_id`) | later version's |
| `common` | later version's; conflict copy |
| `kind` | later version's; conflict copy |

### K15.6 The deck tree

- A deck's stored name is computed: the current name of the deck `parent_id` names, U+001F, the leaf; or the leaf alone when `parent_id` is 0. A receiver recomputes the names of a deck and its descendants after it applies any deck, so a child created on one device follows a rename made on another.
- A deck whose parent is a known deletion (K13.3) becomes top-level: `parent_id` 0, name = leaf. A deck whose parent has not arrived waits (K13.3).
- Cycles: concurrent moves can make decks each other's ancestors (X under Y on one device, Y under X on another). After applying decks and before computing names, every deck on a `parent_id` cycle with the largest id on that cycle becomes top-level, until no cycle is left.
- Names stay unique (`idx_decks_name`, `unicase`). When two decks end with the same name, the one with the larger id is renamed by the unique-name rule of `decks/decks#D2` (R61: `+` appended to its leaf until the name is unique). A filtered parent never takes a normal child: such a child becomes top-level (`decks/decks` D3). The nesting limit of D3 does not apply: every ancestor exists.
- Each change these rules make is a local change, sent onward. Every device computes the same result from the same decks.

### K15.7 Presets and notetypes

- Preset parts: `name`; `config`. Both changed: later version's; a `config` conflict keeps a conflict copy.
- Notetype parts, between versions with the same `struct_gen`: `name`; `config` (CSS and other options); the list of field names; each field's `config`; the list of template names; each template's `config` (its question and answer text). Both changed: later version's, with a conflict copy of the losing notetype (`notetype_conflict`).
- Notetype names stay unique (`idx_notetypes_name`): the notetype with the larger id is renamed by `notetypes/records` N4 step 7 (`+` appended). Field and template name lists are single parts, so a merge never produces duplicate names within a notetype.
- Different `struct_gen`: K19.5 and K19.6.

### K15.8 Tags and config

- Tag parts: stored spelling, `collapsed`, `config`. Both changed: the version with the greater payload digest. No copy.
- Config parts: the value. Both changed: later version's, no copy. Exceptions, when both values read as integers: for `nextPos` and `lastUnburied`, R holds the larger value.
- Tags and config keys are names: a version whose base is the tombstone of the same key is a re-creation and replaces the tombstone.

### K15.9 Media names

- These rules compare only a local file that is a local change of the binding. A name flagged `file_missing`, `superseded` or `withheld` is never compared here: K9.4 decides it. An incoming tombstone is K9.4's "Incoming tombstone".
- Every file move these rules make (a local file to its hash name, a trash) is a store call of the media apply, under its intent, with the absent state it leaves between calls listed as one of the apply's own states (K9.3).
- Same name, both created with different content: S keeps the name. The local file moves to the hash name of its lowercase candidate (`media/media-files#M4`), which becomes a new media-name entity, and every local note whose fields reference the old name (`text/media-references`) is rewritten to the new name. Notes that reference a name never acked are held until it is acked (K12.1 rule 6), so no note published with the old name expects the local content. A conflict copy (`media_content`) names the renamed file.
- Same name, both changed content: later version's (payload digest). The losing bytes stay in the media trash (`media/media-files#M10`) and a conflict copy (`media_content`) names that trash entry.
- Same case key, other exact spelling (the install's `case_collision`, K9.3; `A.png` locally, `a.png` on the server): the spelling is not merged, and S keeps it. When the local file holds the acked content or S's content, nothing local is lost: K9.3 trashes it and installs S. Otherwise the local file holds a local change, and it is handled as "both created with different content" above, whatever the base: it moves to the hash name of its lowercase candidate as a new media-name entity, local notes that reference the old name are rewritten, a conflict copy (`media_content`) names it, and S installs under its spelling. Decided 2026-10-27: a local content change under another spelling never wins in place, because keeping it would need a second spelling of one case key.

## K16. Conflict copies

- A conflict copy is made exactly where K9.3, K9.4, K13.3, K15, K17.1, K17.3, K19.4, K19.5 and K19.6 say. Its row in `conflicts` holds the losing version's full envelope (payload included), the winning version's payload digest, the kind, the key, the reason, and the copy-time names K10.3 lists (field and template names, and a note's deck). It commits in tracking before the collection op that replaces the losing content; when no local content is replaced (a server version that loses to a local tombstone, K17.1), it commits with the apply intent. A copy for bytes a media apply already replaced (K9.3) commits in the transaction that consumes the feed item that reveals it.
- Copies are device-local and never sent to CloudKit. They survive relaunch until the user resolves them (R2054). Listing, inspection, recovery through a normal validated edit and resolution are `sync/cloudkit-recovery`.
- **Media copies whose bytes are not on the device** (decided 2026-10-27). A media-name copy can name content this device never held: the deleting device's `edit_vs_delete` copy of K17.1, whose losing version is another device's edit that was never downloaded. The copy commits with its envelope as usual. The device then downloads its blob (read by record ID, K9.3, with the download queue's bounds) into the cloud staging directory and keeps the file while the copy's `conflicts` row exists, resolved or not, so `unresolve` still finds the bytes; it is deleted after the commit that deletes the row (`sync/cloudkit-recovery#V6`, `#V17.3`), and the staging sweep keeps files that a `conflicts` row names. While the copy is unresolved, this device never deletes that blob (K9.5). When the media zone no longer holds the blob, the copy lists its bytes as unavailable; the editing device keeps them in its media trash (K9.4, "Incoming tombstone").

| Reason | Kinds |
|---|---|
| `field_conflict` | note |
| `edit_vs_delete` | note, deck, preset, notetype, media name |
| `blob_conflict` | deck, preset |
| `notetype_conflict` | notetype (any K15.7 part) |
| `missing_notetype` | note, card |
| `structure_rollback` | notetype and its notes (K19.4, K19.6) |
| `unmapped_structure` | note, card (K19.5) |
| `media_content` | media name (renamed file or bytes in the trash) |

## K17. Tombstones

### K17.1 Representation

- A deletion saves the entity's `MGEntity` record as a new version with `tombstone` true and `deleted_at_ms`, never as a CloudKit record deletion. It goes through the same change tags and conflicts as any edit.
- An entity created and deleted while `sent` is false sends nothing and leaves tracking.
- An entity deleted while `sent` is true and never acked becomes a pending tombstone, saved as a new record. Its save can meet a server-record-changed result:
  - the server version's payload digest equals an in-flight digest: it is the version this device sent (K12.3, second row). It counts as saved, and the tombstone is saved again on its change tag;
  - same origin, other content: another device fetched this device's version and changed it. Decided 2026-10-27: it follows K12.3's same-origin row like any other save conflict, through the inbox. Deletion wins (K17.3): the server's version is kept as an `edit_vs_delete` conflict copy for the kinds K17.3 lists, the apply writes no row (the local row is already gone), the tombstone takes the server's change tag and `erev` when that apply settles, and it is sent again. A media-name tombstone sent again carries the server version's `MediaName` payload, since that is now the last live version (K17.2); its copy follows K16 for bytes not on the device;
  - another origin: the local object never reached the server: the tombstone is dropped and the server version is a new entity.

### K17.2 Payload and graves

- Tombstones of media names carry the `MediaName` payload of the last live version that this device acked or applied, the server's version after a K17.1 conflict (K9.4). The payload is informative: a receiver decides by its own entity, not by it (K9.4, "Incoming tombstone"). Other tombstones have the empty payload (K1).
- Applying a tombstone of a note, card or deck inserts the grave (`oid`, `type`, `usn` -1) with insert-or-ignore (`storage/graves#G2`), so a later manual Anki transfer from this device sends the deletion. Graves are Anki bookkeeping: they are not compared for convergence, and a manual transfer that rewrites them sends nothing to CloudKit.

### K17.3 Apply and conflicts

- A tombstone of an id-keyed kind applies only to a local entity with the same origin. Met against a local entity with another origin, it is a collision (K14), not a deletion: the local entity is re-keyed when unacked, and the tombstone never removes it.
- An incoming tombstone removes the local row. A note tombstone also removes local cards of that note that the deleting device did not know; they get pending tombstones. A deck tombstone removes no cards the deleting device did not send as tombstones: those follow K13.3. A notetype tombstone removes its remaining local notes and cards, each kept as a conflict copy (`missing_notetype`).
- Deletion wins over a concurrent edit. The local edit is kept as a conflict copy (`edit_vs_delete`) for notes, decks, presets, notetypes and media names. A card's edit keeps no copy: its revlog stays.
- For id-keyed kinds an acked or applied tombstone is final (R2052): no live version replaces it (K14.3). No undo or redo restores a row whose tombstone is acked or applied (`engine/undo#M9.9`).

### K17.4 Purge and absent records

- A tombstone record whose `deleted_at_ms` is more than 180 days (15,552,000,000 ms) before now is deleted from CloudKit by any device bound to the active epoch that holds it acked. Purging runs only while A5 and A9b hold, so it is off until the signed runs confirm A9b (`sync/cloudkit-recovery#V10.3`). When it runs is `sync/policy#P10.3` (`RunCleanup` sets `purge_due`, K10.3); K12.4 hands the deletion to the engine.
- An acked entity is absent from the server when a fetch reports its record deleted, when a completed absence pass did not see it (below), or when a save fails with unknown item. Each makes a `deletion` inbox entry (K13.1), applied by K13.2: a tombstoned entity is dropped from tracking, with no op; a live one gets a deletion under K17.3, through a `CloudIncoming` op and its apply intent. An entity never acked is new and is not affected.
- **Absence pass** (decided 2026-10-27). Absence is computed over a durable record of what one fetch from no cursor delivered, never over what one process or one fetch cycle saw:
  - A pass starts only when an engine instance that Meguri created with no saved state (`sync/cloudkit-recovery#V3.1` rule 5, V9.2, V14.2 step 9) starts its first fetch of the bound collection zone (A9a). Tracking writes an `absence_passes` row with a new pass id, replacing any earlier pass of that zone, before the first batch of that fetch is committed. Decided 2026-10-27: the engine's own restart after an expired change token is not detectable from its events (V10.3) and starts no pass. No heuristic may guess it: a pass over a fetch that was not complete would make false deletions. A pass that runs continues through such a restart, and records delivered again are marked again.
  - While the pass runs, K13.1 step 2 sets `seen_pass` to the pass id on every entity whose record the fetch delivered, as a record, a refused record or a deletion, whether or not it is applied or equal. An acknowledgement recorded during the pass (K12.3) sets it too.
  - The pass survives a kill, a failed fetch and a new engine instance: every saved cursor of the zone was saved after K13.1 step 2 marked what it covers (K13.1 step 3), so an instance that continues from such a cursor continues the same pass, and records delivered again are marked again.
  - When the engine reports that a fetch of the zone has drained (A7) with no error and fetched the zone (`sync/policy#P3` `FetchDrained{collection, fetched: true}`) while the pass runs, one tracking transaction makes a `deletion` entry for every acked entity of the zone whose `seen_pass` differs from the pass id, and removes the pass row. A fetch that ends with an error completes nothing, and neither does a drain that fetched nothing (`fetched` false): the pass waits for a later drained fetch that fetches the zone. A bound zone that the first fetch of an instance with no saved state does not list is gone, not empty: it is K12.5's plain `zoneNotFound` row (after `MGActive` is read by record ID), never an absence (A7).
  - A pass ends without a check when its binding is isolated or deleted, or its `engine_state` row is deleted (`sync/cloudkit-recovery#V9.2`); the next instance Meguri creates with no saved state starts a new pass. A gate that closes for a while (a scan, a lifecycle state, an account read) does not end it. So a fetch interrupted and resumed never treats the records delivered before the interruption as absent.
- A purge therefore never brings a deleted object back: a device that was offline longer than the retention still learns the deletion from the record deletion or from the absence.
- **Assumption A9a** (holds by construction): Meguri knows when it creates an engine instance with no saved state, so it knows that instance's first fetch of the zone starts from no cursor, and the engine reports that fetch's drain with no error (A7). A9a gates the absence pass.
- **Assumption A9b** (unverified; decided 2026-10-27, V10.3): the engine's own restart after an expired change token can be detected. No public engine event says so today, so A9b is taken as false: such a restart starts no pass, and A9b gates only tombstone purging, which stays off until a signed device run shows A9b holds.

## K18. Epochs

### K18.1 Definition

An epoch is one published cloud collection: one collection zone, one `MGEpoch`, one record key. The publishing device's binding links the epoch id to the publication identity of its local collection (`engine/collection-replace#R6.1`). A device that adopts the epoch gives its new local collection a publication identity and binds it to the same epoch id.

### K18.2 What starts one

- Initial publication: the R2046 connection action on a device with no binding in that account and environment, when the account has no `MGActive`.
- Replace iCloud collection (R125), including the cloud choice after a restore, a collection package import or a manual Anki import (R1548, R2056). That explicit choice is the only thing that clears `cloudPublicationPending` for a new publication.
- Nothing else. Structural changes, package (`.apkg`) imports and Check Database repairs are structural generations (K19).
- R1548's "remains local" choice unbinds the collection: it publishes nothing and adopts nothing, and the old binding stays isolated with its pending work.
- A bound device whose bound zone, `meguri-root` or `MGActive` disappeared never starts an initial publication: K2 applies, and only the user's V15 choice publishes again.

### K18.3 Publishing

Before step 1, the `publish` op creates the binding of the new epoch and isolates the binding it replaces, in one tracking commit (`sync/cloudkit-recovery#V15.2` step 2). Then:

1. Create the zone `c-<epoch id>` and save `MGEpoch` with state `publishing`. Create `meguri-root` and `meguri-media` (with a new `MGMediaRoot`) first when K2 allows it. These are U114's own operations (`sync/cloudkit-recovery#V26.1`), not engine sends: `MGEpoch` and `MGMediaRoot` are saved only after their zone's creation succeeded, and the binding's engine instance starts only once this step is done (V15.2, step `zones`).
2. Run the binding's reconcile scan: every live entity becomes pending. Tombstones are not published.
3. Send every entity; upload the blobs the media zone lacks, after the binding holds an acked `MGMediaRoot` (K9.2).
4. Save `MGEpoch` with state `complete` and the live counts per kind.
5. Save `MGActive` naming the epoch, conditional on the change tag of the `MGActive` version the user confirmed replacing, or as a new record when there was none. A conflict means another device switched meanwhile: nothing is overwritten and the user decides (`sync/cloudkit-recovery`).

- Local edits made from the binding's creation to step 5 are tracked under the new binding and sent like any change.
- Steps 1, 4 and 5 write through own operations (V26.1), with V26.1's outcomes; the entities and blobs of step 3 go through the engine. Step 3 is done when V15.2's condition ("K18.3 step 3 is done") holds, and the op's recorded steps are V15.2's (`confirmed`, `bound`, `zones`, `sent`, `complete`, `done`, and `cancelling`).
- The publisher records the step it reached in tracking. After a restart it resumes from that step, or the user abandons the publication through `sync/cloudkit-recovery`.
- The replaced epoch's zone is the cloud side's recoverable copy (R2056): for its 30 days (K18.5) `sync/cloudkit-recovery` can stage it as a local collection the way K18.4 does.

### K18.4 Detecting and adopting

- Every device fetches `meguri-root`. When `MGActive` names an epoch other than the bound one, the device stops sending to the old zone, keeps its pending work isolated, and reports needs attention (`sync/cloudkit-recovery`). The publisher of a new epoch during K18.3 steps 1–4 is the K2 exception.
- Adoption replaces the local collection and so needs the explicit confirmation of R2056: it shows the direction, the epoch's counts (`MGEpoch.counts`), the local collection's counts and this device's pending and isolated work. It starts only when `MGEpoch.state` is `complete`.
- Before the swap the device takes a safety backup of its local collection (`package/backup-triggers` T6.1).
- The device fetches the whole zone into a new staged collection: a new schema-18 file whose `col` row has `crt` and `ver` from `MGEpoch`, `mod` = now_ms, `scm` = now_ms, `ls` 0, `usn` 0, `dty` 0 and the empty legacy text columns (`model/row-records#R2`); every live entity's rows, with `usn` -1 (K11.3); an empty `graves` table; open structural generations handled as K19.5.
- The staged file passes the open gate, the integrity check and K13.3's reference checks, and replaces the local collection through `engine/collection-replace` (reason `CloudAdopt`, `engine/collection-replace#R1`). The new binding starts with every fetched entity acked, tombstones included as acked tombstone entities with no row, and no saved cursor: its first engine instance fetches the zone again from the start (`sync/cloudkit-recovery#V14.2` step 9); later changes arrive normally. Media follows in the background (R1592).
- A device with a local collection and no binding for the active epoch (a new device, a reinstall, a lost tracking file) never merges into the epoch: the user chooses which side replaces the other (R2055).

### K18.5 Old epochs

- The old collection zone stays for 30 days after the switch (`MGActive.activated_at_ms`), then a device bound to the new epoch deletes it.
- A collection zone that `MGActive` has never named (a publication abandoned, interrupted for good, or lost in K18.3 step 5) is deleted by its publisher once the user resolves it, or by any device bound to the active epoch when its `MGEpoch.published_at_ms` (or, without `MGEpoch`, the zone's creation time, else the `zones` row's first-seen instant) is more than 30 days old.
- The zone of a cancelled publication is deleted as `sync/cloudkit-recovery#V15.3` says: by the binding before, through a `zones` row with deletion due, or once by U114's own operation; otherwise this section's 30-day rule applies.
- Tracking keeps these zones in `zones` (K10.3). When the deletions run is `sync/policy#P10.3` (`RunCleanup`); K12.4 hands them to the engine. A failed deletion stays due.
- No device writes to a collection zone other than its bound epoch's, except these deletions.

## K19. Structural generations

This section is the CloudKit record layout of a schema change that `notetypes/schema-changes` G1 and G7 refer to: which ops publish a generation (K19.1), the `MGGeneration` record and its maps (K19.2), how it is claimed and completed (K19.3, K19.4), and how a receiver applies it (K19.5), setting `col.scm` as G1 says (`cloudkit_structure_applied`, K11.3).

### K19.1 What publishes as one

| Op | Reason code and name (`notetypes/schema-changes` G2 reasons; `apkg_apply` is the `engine/op-model#O9` catalog name) |
|---|---|
| note type save that adds, removes or reorders fields or templates, or changes the sort field | 1 `fields_changed`, 2 `templates_changed`, 3 `sort_field_changed` |
| note type deletion; Restore to Default that changes structure | 4 `notetype_removed`, 5 `notetype_restored` |
| Change Note Type | 6 `notes_notetype_changed` |
| preset deletion | 7 `preset_removed` |
| package import, whatever its size | 8 `apkg_apply`; also 9 `notetypes_merged` when it merges note types |
| Check Database repairs that are schema changes | 10 `check_database_repair` |

`collection_replaced`, `full_upload`, `sync_sanity_failed` and `force_one_way` are Anki bookkeeping with no CloudKit meaning. A whole replacement is an epoch (K18). `cloudkit_structure_applied` is the receiver's apply of a generation (K19.5) and publishes nothing.

### K19.2 Identity and manifest

- A generation id is 16 random bytes.
- Each notetype has a layout counter `struct_gen`. Tracking keeps, per notetype, the local value and the acked server value (`notetype_layouts`). A notetype payload's `struct_gen` is the local value. A note's and a card's `nt_struct_gen` is the local value of the note's notetype when the payload is built: the collection always holds a note in its notetype's current layout.
- A local op that changes a notetype's structure sets its local value to max(local value, acked server value) + 1 when it commits. So two local generations on one notetype get different values. Notes and cards of that notetype are not sendable until the generation is claimed (K12.1 rule 7), so no record under the new value reaches the server before the value is the server's.
- `MGGeneration.e` holds `meguri.cloud.v1.Generation`:

  | No. | Field | Type | Meaning |
  |---|---|---|---|
  | 1 | `format` | uint32 | 1 |
  | 2 | `generation_id` | bytes (16) | |
  | 3 | `reasons` | repeated uint32 | K19.1 reason codes, ascending |
  | 4 | `state` | uint32 | 1 `claimed`, 2 `complete`, 3 `abandoned` |
  | 5 | `anchors` | repeated string | record names of the notetypes whose `struct_gen` changes, ascending |
  | 6 | `members` | repeated `Member` | inline member list, by record name; empty when the list is in `ma` |
  | 7 | `maps` | repeated `LayoutMap` | by notetype id |
  | 8 | `type_changes` | repeated `TypeChange` | Change Note Type, by old notetype id |
  | 9 | `members_sha256` | optional bytes (32) | present when the member list is in `ma` |
  | 10 | `members_size` | optional uint64 | present when the member list is in `ma` |

  | Message | Fields |
  |---|---|
  | `Member` | 1 `record_name` string; 2 `erev` uint64 (the listed `erev`; anchors included) |
  | `MemberList` (the content of `ma`) | 1 `members` repeated `Member` |
  | `LayoutMap` | 1 `notetype_id` int64; 2 `from_struct_gen` uint64; 3 `to_struct_gen` uint64; 4 `field_map` repeated sint32; 5 `template_map` repeated sint32 |
  | `TypeChange` | 1 `old_notetype_id` int64; 2 `old_struct_gen` uint64; 3 `new_notetype_id` int64; 4 `new_struct_gen` uint64; 5 `field_map` repeated sint32; 6 `template_map` repeated sint32 |

- A map's entry i is the old `ord` that new `ord` i comes from, or -1 for none.
- The op that makes the change reports its maps (K24); tracking stores the generation with state `local` when the op commits, its members being the entities the op changed.

### K19.3 Publishing

1. **Claim**: one batch, atomic in the collection zone, holds the manifest (state `claimed`) and every anchor's new version. A generation without anchors claims with the manifest alone. The claim must fit K7's request budget in one batch: when it would not, every anchor in it carries its payload as the asset `pa` and the manifest its member list as `ma`, whatever their size (K5.1), which leaves a few KB of non-asset data per record. A generation with more than 249 anchors cannot be claimed (250 records per request) and reports `claimStuck`. **Assumption A8** (documented in the SDK headers, 2026-10-27: a batch built with its atomic-by-zone flag set saves all or none of its records in a zone, and the others fail with `batchRequestFailed`): the sync engine can send one batch atomically within one zone.
2. **Members**: after the claim is acked, sent in any batches, each with `generation` set. A member edited again locally before the generation is complete is sent in its latest state, still with `generation` set. Notes and cards of an anchor become sendable again under the claimed value.
3. **Complete**: the manifest is saved with state `complete` when every member is acked.

- The manifest saves of steps 1 and 3 are control records (K12.6): a full update never removes them from the engine's pending list while they are due.

- A generation cannot be claimed while one of its members is oversize or unreadable; status reports needs attention.
- A device claims its own generations in the order their ops committed.

### K19.4 Claim results

A claim batch is atomic: when any record in it fails, none is saved. The anchors' results decide:

| Anchor's server version | Result |
|---|---|
| a larger `struct_gen` than the local generation's `from_struct_gen` (another device's generation came first) | claim conflict: the generation becomes `lost`; the other generation is adopted (K19.5), then this one is rolled back (K19.6) |
| a tombstone (the notetype was deleted elsewhere) | claim conflict: deletion wins (K17.3). The local generation's changes to that notetype are kept as conflict copies (`structure_rollback`), and its notes and cards follow the tombstone (`missing_notetype`) |
| the same `struct_gen`, other content (a non-structural change elsewhere: CSS, a rename) | not a conflict. The server version goes through the inbox as K12.3's same-origin row says, and K13.2 applies it: its non-structural parts are merged into the local anchor (K15.7), keeping the local layout. The anchor takes the server's change tag only when that apply settles after the WAL flush, and only then is the claim sent again. After 5 failed attempts (any failed claim, `limitExceeded` included) the generation stays `local` and status reports `claimStuck` |
| the local generation tombstones the anchor, and the server version changed | the tombstone takes the new change tag and the claim is sent again (deletion wins, K17.3); the other device's changes are kept as `edit_vs_delete` copies |

- Records of the batch whose own result is "batch request failed" (K12.3) keep their state.
- A claim that fails only because a member was edited elsewhere is not possible: members are not in the claim batch. They merge under K15 when they are sent.

### K19.5 Adopting

- Records with a `generation` this device has not adopted wait in the inbox (reason `generation`), and so do notes and cards with `nt_struct_gen` above the local value of their notetype.
- A generation is complete for a receiver when it holds the manifest and, for every member, a server version that has this `generation` or an `erev` above the listed one (a tombstone counts the same way). A later version without `generation` is decodable: its `nt_struct_gen` names its layout, and a device writes such a version only after it has adopted this generation (K12.3).
- A complete generation is applied in one `CloudIncoming` op: anchors and the latest version of every member, merged with local pending changes after mapping (K15.1). Its maps are stored in `generation_maps`; the local notetypes take the anchors' `struct_gen` as both local and acked server value. When the apply changes local structure by `notetypes/schema-changes` G1's test (a note type's stored field or template positions or its sort field, a removed note type or preset, notes moved to another note type), the same op sets `col.scm` to now (`cloudkit_structure_applied`, K11.3); otherwise it leaves `col.scm`.
- A note or card written under an older `struct_gen` is mapped through the stored maps, oldest first. When a step of that chain is missing, the version is not applied and is kept as a conflict copy (`unmapped_structure`).
- Receivers never show part of a generation (R2053). A claimed generation that never completes stays waiting; abandoning it is the explicit action of `sync/cloudkit-recovery#V18`, which never saves `abandoned` over a `complete` manifest.
- Tracking records a waiting generation's last progress instant: when a drained fetch last brought its manifest or one of its members (`generations`, K10.3; V18).

### K19.6 Rolling back a lost generation

On the device whose claim failed:

- The other generation is adopted. For each conflicting anchor, the local notetype becomes the server's, and every local note and card of that notetype is rebuilt under the adopted layout, members or not: the collection holds them all in the lost layout.
- A clean note or card takes the server version. A pending note's content is mapped back through the inverse of the local map, then forward through the adopted map, and merged under K15.2. Content in a field that has no source in the base layout (a field the local change added) cannot be mapped and is kept, with the local notetype, as a conflict copy (`structure_rollback`). Cards of a template the local change added, which no adopted template maps to, are removed locally; they were never sent.
- Members of the local generation that do not depend on a conflicting anchor are published again as a new generation. The lost generation was never claimed, so the server holds nothing of it, and nothing that depends on it was sent (K12.1 rule 7).

### K19.7 States

| Origin | State | Meaning | Next |
|---|---|---|---|
| local | `local` | the op committed; not claimed | `claiming` |
| local | `claiming` | the claim batch is handed to the engine | `claimed`; `lost` (claim conflict, K19.4); `local` (retry, K19.4) |
| local | `claimed` | the claim is acked; members are being sent | `complete` |
| local | `complete` | the manifest is saved with state `complete` | none |
| local | `lost` | another generation won the claim | `rolled_back` (K19.6 done) |
| local | `rolled_back` | final | none |
| remote | `waiting` | manifest fetched; not complete for this device | `adopted`; `abandoned` |
| remote | `adopted` | applied (K19.5) | none |
| remote | `abandoned` | the manifest says `abandoned`, or `sync/cloudkit-recovery#V18` abandoned it; `abandoned` is never saved over `complete` | none |

## K20. Refusals

A refused record is never applied. It is recorded in `refused`, and status reports needs attention: unsupported schema for a format problem (R1561), conflict or recovery needed for the rest.

| Condition | Result |
|---|---|
| record in a collection zone other than the bound epoch's | ignored; the engine's fetch scope excludes every other zone (`sync/policy#P13.2`), so it is not fetched after a switch |
| `MGEpoch.epoch_id` differs from the zone's id | the whole zone refused |
| format newer than supported | inbox reason `format` (K5.4) |
| record name differs from K4.2's computation | refused |
| non-canonical encoding (K5.3) | refused |
| payload digest or size mismatch with an inline payload | refused |
| asset `pa` or `ma` missing, unreadable, or not matching its digest or size | retryable: inbox reason `asset` (K13.1) |
| payload that fails `model/row-records` reading | refused |
| id-keyed kind: origin differs from an acked local entity's | refused |
| name-keyed kind: origin differs from the local entity's | accepted: the local entity takes the server origin and merges under K15.8 or K15.9 |
| live version of an id-keyed key after its tombstone | refused (K17.3) |
| local `col.crt` differs from `MGEpoch.crt` | nothing sent or applied; needs attention. This is `sync/policy#P6`'s crt check: it runs at open, after every op that writes `col`, after a scan, when a fetched `MGEpoch` is recorded, at every barrier and before every `CloudIncoming` or `CloudRekey` op; the gate is closed with the engine running; nothing is isolated; it clears when the values are equal again, or V14 or V15 replaces one side |

## K21. Invariants

1. `collection.anki2` gains no table, column, index, trigger or config key for CloudKit and opens in Anki at schema 18 after any sequence of applies.
2. Every applied row reads under `model/row-records`; no card references a missing note or deck, and no note a missing notetype or a notetype with another field count.
3. Convergence: once every device has exchanged and nothing is pending, the canonical payload of every entity is byte-identical on every device, and `col.crt` is equal. `usn` columns, `col.mod`, `col.usn`, `col.ls`, `col.scm` and graves are excluded.
4. An acknowledgement never clears a newer local revision.
5. No engine state is saved with a cursor past records or record deletions that are not durably in the inbox, refused or applied.
6. No id-keyed entity becomes live again after its tombstone was acked or applied.
7. Concurrent reviews of one card all remain as revlog rows on every device.
8. No receiver shows part of a structural generation or an epoch that is not complete.
9. No record name, zone name or unencrypted field contains collection content.
10. The merge is deterministic and symmetric in L and S for content parts; the only position-dependent rules are K14 and K15.9.
11. Installing a media file never replaces a valid file with unverified bytes, and sync never deletes media bytes permanently.
12. Losing the tracking file never causes an inferred acknowledgement or an automatic upload into an existing epoch.
13. No record is built from a collection commit that is not flushed, and no apply is marked done before its commit is flushed.
14. A collection whose publication identity differs from its binding's, or whose publication is pending, sends nothing to that binding's epoch.
15. No record that depends on a layout or a reference the server does not hold is sent.
16. No tracking fact about a server version (acked system fields, `erev`, digests, base) is recorded before the collection write that applies it is flushed, save conflicts included (K12.3, K13.2).
17. An entity is found absent only by a record deletion, an unknown-item save result, or an absence pass that saw a whole fetch drain from no cursor, by an engine instance Meguri created with no saved state (K17.4); an interrupted fetch and the engine's own token restart never make an absence.
18. A local media write is never replaced by a media apply without a conflict copy that names its bytes in the trash (K9.3).
19. No media file of a collection that an adoption replaced is sent, as a name or as a blob, and none wins over the epoch's content, unless the user changed it or added a reference to it afterwards (K9.4; `sync/cloudkit-recovery#V22` invariant 11).
20. No collection record is sent by an engine instance before a drained root fetch of its own confirmed the bound epoch (the binding's last applied `MGActive` names it, and the fetch reported no other epoch and no deletion), except a publisher before its switch (K2, K12.1 rule 3).
21. A missing local file never makes a media-name tombstone; only an explicit Meguri removal does (K9.4).
22. No batch holds a record or deletion outside the scope of its send request (K12.1).
23. Every row a `CloudIncoming` op writes that is not an applied server version (a reference result, a deck tree fix, a card a note tombstone removes) becomes pending as a local change when the apply settles (K13.2).
24. An apply that fails is not tried again without an event trigger in between, and a server version this device could not apply is never overwritten by a local version (K13.2, K12.1 rule 8).
25. A full update never removes a due control record from the engine's pending lists (K12.6).
26. Nothing is sent or applied while the open collection's `col.crt` differs from the binding's epoch `crt` (K20, `sync/policy#P6`).
27. No zone, `MGEpoch`, `MGActive`, `MGMediaRoot` or `abandoned` manifest is written through the engine; each is an own operation (K12.6, `sync/cloudkit-recovery#V26.1`).

## K22. Test obligations

### K22.1 Merge tables

Focused Rust tests in meguri-sync cover every row below, and K15's rules in both argument orders (invariant 10).

| B | L | S | R | Copy |
|---|---|---|---|---|
| note fields `a`,`b` | `a2`,`b` | `a`,`b2` | `a2`,`b2` | none |
| note field `a`, mod 10 | `x`, mod 20 | `y`, mod 15 | `x` | S's version |
| note field `a`, met as a save conflict (K12.3) | `a2`, mod 20 | `a3`, mod 30 | `a3` (never L by default) | L's version |
| note tags `t1` | `t1 t2` | (removed `t1`) | `t2` | none |
| card: last review 100 | review 200, ivl 5 | review 150, ivl 3 | ivl 5; both revlog rows kept | none |
| card: queue 2 | suspended | reviewed, queue 2 | suspended, scheduling from S | none |
| card in filtered deck F (home H): `due` -100, `odue` 50 | reviewed in F, last review 200, home due 60 | F emptied: `did` H, `odid` 0, `due` 50 | `did` H, `odid` 0, `due` 60, `odue` 0 | none |
| deck `P` leaf `C`, parent P | (unchanged) | P renamed `Q` | child name `Q`U+001F`C` | none |
| decks X (id 5), Y (id 9), both top-level | X under Y | Y under X | Y top-level, X under Y | none |
| two decks named `Japanese`, ids 5 and 9 | | | id 9 renamed `Japanese+` | none |
| config `nextPos` 40 | 45 | 43 | 45 | none |
| config `lastUnburied` 10 | 11 | 12 | 12 | none |
| note live | edited | tombstone | deleted | `edit_vs_delete` |
| no base, note field `x` mod 20 | | `y` mod 15 | `x` | S's version |
| media name `a.png`, none | content C1 | content C2 | S's name and content; local file renamed to its hash name, local notes rewritten | `media_content` |
| media name `a.png` content C0 | C1 | C2 | the later by payload digest; losing version in the trash | `media_content` |

### K22.2 Durability

With a CloudKit fake that keeps change tags, reports deletions, expires cursors and can lose results, inject a crash at each point and prove convergence without loss or duplicates after restart:

- after a collection commit, before the dirty mark; and after the dirty mark, before the engine's pending add;
- after a collection commit returns, before the WAL flush, with an OS crash (the commit is lost);
- a tracking observer run that fails without a crash, then a clean close and reopen;
- after inbox insert, before the engine state save; and after the engine state save, before apply;
- after the apply intent, before COMMIT; after COMMIT, before the flush; after the flush, before the tracking update;
- after the sent mark, before the save; after a server save, before the acknowledgement; and a save that lands with its result lost, followed by a local delete;
- a save conflict (K12.3): after its inbox insert, before the apply intent; after the apply's COMMIT, before the WAL flush, with an OS crash; after the flush, before the tracking update. In each, the other device's change survives on every device;
- a fetched record deletion: after its inbox insert and the cursor save, before the apply;
- an absence pass: a fetch from no cursor killed halfway and resumed by a new instance from the saved cursor deletes nothing that the first half delivered, and finds every record absent from the whole zone; a `changeTokenExpired` that the fake surfaces, followed by a partial fetch that drains, starts no pass and makes no deletion entry;
- a reconcile scan whose commit fails with a full disk: `TrackingWriteFailed` is reported, the scan does not restart by itself, the gate stays closed with `localStorageFull`, and the scan runs once when the stop clears, before the engine starts;
- a media apply: a local write of the same name before its store call, between the call's check and the call, and during the settle at open, each ends with the local bytes kept (as a local change, or in the trash with a conflict copy); a K15.9 move to the hash name killed after the move and before the install completes the install at restart, and the apply never abandons itself at its own absent state;
- after a re-key commit, before its tracking move;
- after a generation claim, before its members; a claim lost after dependent local edits; and during adoption, before and after the collection swap;
- during publication, after each of K18.3's steps.

And without a crash:

- restore an older backup, import a `.colpkg`, and run a manual Anki import while bound: no record is sent to the bound epoch;
- a retryable asset failure on fetch: the record is read again at the next drained collection fetch, at most once per drain and 5 times in all, and never applied without its asset;
- a save conflict on an anchor or a large note whose server version has its payload in `pa` (K12.3): the server record arrives without its asset file, the entry is read by record ID once right after the send's event handling, that read is not counted among the 5, and the merge then runs;
- send scope (K12.1): with a blob ready to upload and a note pending, a request scoped to the collection zone receives only collection records, and a request scoped to `meguri-media` only blob records; a fake engine that fails a batch with a record outside its scope with `invalidArguments` never sees one;
- the confirmed root (K12.1 rule 3): a batch request before the instance's first root fetch drains returns no collection record; after a root fetch that finds `MGActive` naming another epoch, none is ever returned;
- an unacked tombstone whose save meets another device's edit of the same origin: deletion wins with an `edit_vs_delete` copy (K17.1);
- each K12.5 row, reached once through a fetched zone deletion and once through a send error;
- failed saves (K12.4): `batchRequestFailed`, `limitExceeded` and a merged `serverRecordChanged` are sent again in the same send; `quotaExceeded` and asset errors are sent again only after an event trigger, and no timer runs;
- batches (K12.1, K7): no batch exceeds 250 records or the byte budget; a note and its new cards saved in one atomic batch, or the cards held until the note is acked, so no card is ever saved without its note when one record fails; a claim whose inline size would exceed the budget is sent with its payloads as assets;
- an apply of 2,000 ready entries runs as plain ops of at most 500 entries, and answers commit between them;
- an apply whose entries all equal the local rows (after an adoption or a Resume whose new instance fetches from the start): the op writes nothing, its intent still gets its seq, it settles and deletes its inbox entries, and the next ready entries apply (K13.2);
- a relaunch from a saved engine state with offline edits and an unchanged `MGActive`: the root fetch drains with no records, confirms the epoch from the last applied `MGActive`, and the edits are sent; a root fetch that delivers a deletion of `MGActive` or a version naming another epoch confirms nothing (K2);
- side effects of an apply (K13.2): another device renames parent deck P; the child's recomputed name makes the child pending as a local change with a new `change_seq`, it is sent, and Sync Now selects it; a note tombstone whose note has a card the deleting device did not know makes that card's pending tombstone, which is sent; a crash after COMMIT and before the settle settles both roles at open;
- failed applies (K13.2): an entry whose merge violates `idx_decks_name` rolls its op back; the entries take `apply_failed`, 50 `TrackingChanged` inputs apply nothing, the next event trigger applies each entry in an op of its own, the other entries apply, and after the fifth failure `applyFailed` is shown and the entity is not sent; a re-key with no free id gives `rekeyFailed` from the inbox entry after a relaunch;
- control records (K12.6): a full update during a claim and while a manifest is due `complete` keeps the manifest in the engine's pending lists; a claim dropped after `quotaExceeded` is parked and handed back at the next event trigger; during each K18.3 step no zone save, `MGEpoch`, `MGActive` or `MGMediaRoot` reaches the engine's pending lists (they are own operations, tested in `sync/cloudkit-recovery#V23`), and the new binding's engine instance starts only at step `zones`;
- unchanged zones after a relaunch from saved state (`sync/policy#P3` `FetchDrained`): the root and collection fetches drain with `fetched` false, the epoch is confirmed from the last applied `MGActive`, offline edits are sent, and no absence pass completes; an instance with no saved state whose bound collection zone is gone gets no `FetchDrained` for it, reaches K12.5's plain `zoneNotFound` row after reading `MGActive`, and makes no `deletion` entry and no local deletion.

### K22.3 Properties and checks

- Random interleavings of local ops on two or three simulated devices with random delivery order converge to invariant 3.
- Acknowledgement of revision N while revision N+1 is pending leaves N+1 pending.
- Identity collisions for each id-keyed kind re-key the unacked side and keep both objects; a re-keyed revlog row keeps its review instant to within milliseconds.
- The same `.apkg` imported on two offline devices converges with no duplicate notes, cards, notetypes or decks.
- Two devices racing to claim the same notetype: one claim wins, the other device rolls back (K19.6), and no receiver ever applies a note or card under the losing layout.
- A claim against an anchor changed elsewhere without a structural change is retried and succeeds; a claim against a tombstoned anchor ends as K19.4 says.
- Adopting a generation whose map chain has a missing step keeps the version as `unmapped_structure`.
- A card whose note's save fails (conflict, oversize, not eligible) is not sent, and no receiver moves it to deck 1.
- A tombstone purged while a device is offline is never brought back, via record deletion events and via a cursor-less fetch.
- An oversize, an unreadable, an unsyncable and a newer-format entity are never sent or overwritten, and others continue.
- Media names (K9.3, K9.4, K15.9): a file deleted outside Meguri makes no tombstone and is downloaded again, or reported missing when the media zone has no blob for it; a file that Delete Unused trashes makes one; a file that leaves the size range is marked unsyncable and makes none; an incoming spelling `a.png` over a clean local `A.png` ends with `a.png` on both devices and the old bytes in the trash, and over a changed local `A.png` keeps the local bytes under their hash name with a `media_content` copy; a crash between a media apply's trash and its install makes no tombstone and completes the install at restart; a download interrupted by a relaunch fetches only the blobs not yet installed.
- Media flags against incoming versions (K9.4): an evicted `file_missing` file that meets another device's new content takes it and downloads it, and sends no tombstone; a `file_missing` name that meets a tombstone trashes nothing and makes no copy; a `superseded` file is never sent, also when the media zone has no blob for the epoch's content; a `withheld` file that meets another device's new content for that name is trashed with a `media_content` copy, is never moved to a hash name, and no note is rewritten; a `withheld` file that meets a tombstone stays; a note of the epoch that references a withheld name syncs its edits.
- Incoming media tombstones (K9.4): over a clean file, the file is trashed with no copy; over a local content change, deletion wins with an `edit_vs_delete` copy naming the trash entry; after a K17.1 conflict the tombstone sent again carries the server version's `MediaName`, the editing device trashes its acked file, and the deleting device downloads the bytes its copy names.
- Two builds of different containers sharing one data directory (K10.2, K10.3): each has its own active binding; neither writes the other's rows; a removal that one build has not consumed survives the other build consuming the feed and becomes a tombstone when the first build consumes it.
- Canonical `Stored` values (K5.3): a card with `odue` integer 0 and `data` empty text, and a note whose `sfld` is integer 0, round-trip with their storage classes unchanged (R2045); `Stored{integer: 0}` and `Stored{text: ""}` encode to different bytes; a payload with a `Stored` field absent, or a `Stored` with no oneof member, is refused as non-canonical.
- Stale blob state (K9.3, K9.5): device A deletes blob C while device B caches it `present`; B names C again (an import, or a new name for the same content); B reads C, finds it absent, uploads it, and no device ends with `mediaMissing`; the same when A's deletion lands after B's version is sent and before B's next drained collection fetch.
- Removal before complete registration (K9.4): the journal is recreated (new `feed_id`), and during its first registration the user runs Delete Unused on an acked name; the name is not downloaded and not reported missing, nothing is sent while the feed is not complete, and the tombstone is sent once it is complete, also after a relaunch in between; a clone mismatch's `mediaChanged` survives a relaunch and clears at the next feed item of that name.
- Bindings (K10.3): after a Resume with no cursor, a new note's record name is computed from the stored record key before any fetch; a `root` read that returns another media key sets `media_zone_missing`, and no blob is saved afterwards.
- Epoch publication and adoption: the adopted collection's counts equal `MGEpoch.counts`, and queued records of the replaced epoch are never applied to the new one.
- After every test above the collection opens in the Anki oracle without schema change, and Check Database reports no reference problem.
- Record names, zone names and unencrypted fields of every saved record contain none of the test collection's note text, file names, tags or deck names.

Signed multi-device runs in the development container (R2065) prove Apple's delivery, A1–A10 (A1.1 included; A9b until it is shown, purging stays off), and asset behavior; fake runs alone do not (R2064). The headers document A1, A1.1, A8, A10 and K12.4's retried errors; the signed runs still check them.

## K23. Assumptions and open questions

| Id | Assumption | If false |
|---|---|---|
| A1 | ≤ 1 MB non-asset data per record (documented: `CKRecord`) | lower the inline limit |
| A1.1 | ≤ 250 records and about 2 MB non-asset data per request (documented: the sync engine and `CKError`) | lower K7's batch budget; `limitExceeded` already halves it at run time |
| A2 | one asset holds 100 MiB | split blobs into parts, or lower the media limit |
| A3 | encrypted bytes values need no index | none expected |
| A4 | false for the engine's fetch (decided 2026-10-27, `sync/policy#P13.2`); relied on only for record-ID reads with named fields, where a read of `e` alone downloads no asset | blob existence needs another source than reads of `e`; the engine still never fetches `meguri-media` |
| A5 | create-over-existing and save-over-deleted fail as stated | re-key detection and tombstone purge need redesign; purge stays off |
| A6 | save results arrive before the state that reflects them | acknowledgement order needs another handshake |
| A7 | fetched records arrive before the state that advances past them; the end of a fetch is reported, per scope zone for a `fetchChanges` call, also for a zone with nothing to fetch; a first fetch from no cursor lists every zone that exists (K13.1) | the inbox order needs another handshake; a zone that a first fetch does not list proves nothing, and only a zone-not-found result or a fetched zone deletion leads to K12.5 |
| A8 | one batch can be atomic within one zone (documented: the batch's atomic-by-zone flag) | the claim needs a separate lock record |
| A9 | split into A9a and A9b (decided 2026-10-27, K17.4) | |
| A9a | the first fetch of an engine instance Meguri created with no saved state is a fetch from no cursor, and its drain is reported (holds by construction, with A7) | absence detection needs a full record-name listing |
| A9b | the engine's own restart after an expired change token is detectable (unverified; taken as false, V10.3) | as taken: such a restart starts no absence pass, and tombstone purging stays off |
| A10 | zone deletions are reported with their cause, on fetch and on send (documented: the fetched zone deletion reasons, `zoneNotFound` with the encrypted-data-reset flag, `userDeletedZone`) | every zone loss is treated as a user deletion: isolate, never recreate |

`sync/cloudkit-recovery` A13 (the engine retries quota exceeded itself) is false by the SDK headers: the engine keeps only the errors K12.4 lists. K12.4 states the rule that replaces it (K24).

Decided 2026-10-27 (R2066 review): one record type for all entities; keyed record names; row-exact payloads that carry modification times but not `usn` (receivers write `usn` -1); base capture only for pending entities; device-local conflict copies; no copies for cards, revlog, tags and config; card scheduling taken whole from the side with the newest review, FSRS memory state not recomputed (R2052); the `nextPos` and `lastUnburied` larger-value rule; `localOffset`, `curDeck` and `activeDecks` synced as other config keys, so devices in different time zones rewrite `localOffset` after each other's changes and the current deck follows the last device used, as with Anki; the `+` suffix for deck and notetype names a merge makes equal; the import identity rule of K14.1; re-keyed ids just above the old id; retention values 180 days (tombstones), 30 days (old and never-active epochs, unreferenced blobs).

## K24. Changes other specs need

This file does not change them; the owners do. Items marked **Done** have landed where they say (2026-10-27). No item is open (R2066 approval pass, 2026-10-27).

- **Done.** `engine/op-model` O1, O9: sources `CloudIncoming` (stamping caller-supplied modification times, `usn` -1, `col.mod` set) and `CloudRekey`; catalog entries for a large incoming apply. Landed in O1 (both rows and the stamping rules), O1.1 and O9 (`cloud_incoming_apply`).
- **Done.** `storage/ids-usn-mtime` S3: rows for both sources; the re-key allocation of K14.2. Landed in S3 (the `CloudIncoming` and `CloudRekey` rows), I4 and I5 (re-key allocation). `engine/op-model` O1.1 now cites S3 for the stamp rows.
- **Done.** `engine/change-stream` S2, S2.1: before-images for notes, decks, presets (`deck_config`), notetypes with their `fields` and `templates` rows, and tags, available to the tracking observer; an observer learns that its own run failed or panicked. Landed in S2.4 (with S1.2 for the change set of CloudKit ops).
- **Done.** `storage/open-and-connections` C9.2: the seq covered by the last completed flush, and a way to wait until a given seq is covered. Landed in C9.2 ("Covered seq", "Waiting for a seq", "Close and the covered seq").
- **Done.** `sched/day-clock` D7, D8 and `sched/app-coordination`: the CloudKit incoming apply as a caller of `checkRollover()` before it applies (K13.2), in place of U107. Landed in D7, D8 (callers and tests) and `sched/app-coordination` A1.
- **Done.** `engine/undo` M2, M9: CloudKit upload states from K12.3 acknowledgements; incoming and re-key targets drop steps (R2047); undo of a row whose tombstone is acked is refused. Landed in M2 and M9.6 to M9.10 (M9.7 acknowledgement, M9.8 targets, M9.9 acked tombstones).
- **Done.** `engine/collection-replace` R1, R6: a reason for adopting an epoch (K18.4), with a fresh publication identity bound to that epoch and no pending publication fence (the cloud side is the source); R1548's "remains local" choice unbinds (K18.2). Landed in R1 (`CloudAdopt`), R2, R5, R6.5 and R6.6; the unbound local-only decision is R6.1's, used by `sync/cloudkit-recovery#V12.2`.
- **Done.** `package/backup-triggers` T6.1: safety-backup rows for adopting an epoch (Replace local collection from iCloud) and for Replace iCloud collection (R2056). Landed in T6.1: a backup row for `CloudAdopt`, and a row that writes no backup for Replace iCloud collection (`sync/cloudkit-recovery` V25).
- **Done.** `media/media-journal`, `media/media-files` M11: a change feed for CloudKit, separate from the Anki dirty flag and media USN. Landed in `media/media-journal#J6` (feed, removal items, absence causes, floor, `feed_id`, complete registration), `media/media-files` M11 and M16, and the `case_collision` result of `media/media-files#M6`.
- **Done.** `notetypes/schema-changes` G1, G7: cite K19 for the generation layout; schema-classified ops report field and template maps (U34, U35, U121, U128). Landed: the header, G1 and G7 cite K19 (K19.1, K19.2, K19.5) and `cloudkit_structure_applied`.
- **Done.** `sync/interop-vocab.txt`: `nextPos`, `activeDecks`, `curModel`, `localOffset`, `lastDeck`, `lastNotetype`, `odue`, `odid`, `ntid`, `idx_decks_name`, `idx_notetypes_name`, `_deck_` and `_nt_` are listed.
- **Done.** `sync/policy` P14.4: the estimate in the `media` field for a first full download (K9.3, R1583), and the attention item `mediaMissing` (K9.3, K9.4, K9.5).
- **Done.** `sync/policy`: the refetch of `asset` inbox entries (P4 `RetryAssetEntries`, at a drained collection fetch, K13.1); P12.5's retry-after waits removed (a throttled exchange ends `waiting` at once); no retry timer and no quota pause (P2, P15.1).
- **Done.** `engine/op-model` O9: the `cloud_incoming_apply` row cites K13.2's rule (plain ops of at most 500 entries; a complete structural generation of more than 500 entries is `cloud_incoming_apply`).
- **Done.** `media/media-journal` J6: a removal item for dropped facts (cause `missing`) is never a deletion signal for CloudKit (K9.4).
- **Done.** `sync/policy` (2026-10-27): `UpdatePending{full}` before `RequestSend` at `Foreground` (P11.1) and at Sync Now step 4 (P12.3); `superseded` not pending for the policy (P3, with P12.2, P14.2 row 9, P14.3, P13.3 and P21); the confirmed root (P4, P10.1's first `FetchDrained{root}` row, P21); P15's `changeTokenExpired` row (only when the engine surfaces it, no absence pass); the update forms and parked marks (P1, P4), which K12.4 now follows.
- **Done.** `sync/cloudkit-recovery` V24 A13: false for quota exceeded by the SDK headers; K12.4's add-back at event triggers replaces it (V10.3, V24).
- **Done** (checked 2026-10-27, R2066 approval pass). `sync/policy`: P1 "The binding" and P3 `Binding` (the newest binding of the running build's container and environment; with none but one of another, the derived reason `environment_changed`); P15's `invalidArguments` row and P10.3 item 1 (A5 and A9b); P15.2 (a held scan runs before `StartEngine`, with its P21 test); P13.1 (send scope, the `root` re-read, `removal_held`); P4 `RetryAssetEntries` and P12.3 step 4 (the save-conflict asset read); P13.3 and P3 `blobsUnknown` (stale `present`, the durable `media_changed` mark, with P21 tests); P4 `ApplyReady{retryFailed}`, P10.1, P12.3 step 3, P12.4 and P12.6 (`apply_failed` entries retried only at event triggers, one per op, and keeping a result from `succeeded`), P14.4 `applyFailed` and `rekeyFailed`, with P21 tests; P1 and P4 `UpdatePending` (due control records kept by both forms, parked when dropped).
- **Done** (checked 2026-10-27, R2066 approval pass). `sync/cloudkit-recovery`: V3.1 rule 2, V4, V5 and V23.1 (deletion entries and the save-conflict apply); V1, V10.1 and V10.2 (K12.5); V2.4 step 8 (media apply intents); V6 (the sweep keeps files a `conflicts` row names); V14.4 and V23.2 (incoming versions over `withheld` and `superseded`); V9.1 (the `signIn` row, and the `configuration` row); V11 (`mediaMissing`, `applyFailed` and `rekeyFailed`, the transient authentication reason, `crtMismatch`); V10.2 (confirmation by the last applied `MGActive`); V25 (no item open).
- Decided 2026-10-27 (R2066 approval): the earlier request that V15.2 step 3 and V18 step 2 treat zone saves and the `MGEpoch`, `MGActive`, `MGMediaRoot` and `abandoned` manifest saves as control records is dropped. They are U114's own operations (`sync/cloudkit-recovery#V26.1`), and K2, K9.2, K12.5, K12.6 and K18.3 now say so (K21 invariant 27).
- **Done** (2026-10-27). `engine/change-stream` S2.4 and `engine/op-model` O1.1: for a `CloudIncoming` op the core records the seq from the op's result in the intent, also for an empty change set, and the observer marks no key of the intent dirty, `side_effect` keys included; the settle marks them (K11.1, K13.2). O1.1: an apply op that rolls back other than by a cancel gives its entries the reason `apply_failed` (K13.2 "Failed applies").
- **Done** (2026-10-27). `media/media-journal` J6: its U114 test list has Delete Unused during a first registration after a recreated journal: no download, no missing report, and a tombstone once the feed is complete (K9.4 "Removal before complete registration").
