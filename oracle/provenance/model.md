# Model area specs and model ops (U21)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/model/*.md` and
`specs/conformance/model-ops.md`. No code was copied; every rule was restated
in the spec writer's words and checked with black-box experiments against the
oracle image's `anki` wheel (blob bytes, card data rewrites, schema-11
leniency, refusals).

| Spec section | Anki file | Read for |
|---|---|---|
| protobuf-records P1, P6.1-P6.3 | `proto/anki/notetypes.proto`, `proto/anki/generic.proto`, `rslib/src/storage/notetype/mod.rs` | message fields and numbers, which message each column holds, stored `ord` = list position |
| protobuf-records P6.4-P6.5 | `proto/anki/decks.proto`, `rslib/src/storage/deck/mod.rs` | deck common and kind fields, reserved numbers, a kind blob with no member is an error |
| protobuf-records P6.6 | `proto/anki/deck_config.proto`, `rslib/src/storage/deckconfig/mod.rs` | deck config fields, enums, reserved numbers 7, 8, 39 |
| protobuf-records P1, P2, P9 | `rslib/src/storage/*/mod.rs`, `Cargo.lock` (prost 0.13.5) | blobs read with `as_blob` (TEXT refused), decode errors, encode order |
| protobuf-records P1 (`tags.config`) | `rslib/src/storage/tag/mod.rs`, `rslib/src/storage/tag/*.sql`, `rslib/src/storage/upgrades/schema17_upgrade.sql` | the column is never written (NULL) |
| card-data-json C1-C5 | `rslib/src/storage/card/data.rs`, `rslib/src/storage/card/mod.rs`, `rslib/src/serde.rs` | keys and types, per-key rounding, skip rules for `cd`, lenient per-key reading, memory state needs both values, rebuild on every card write |
| card-data-json C4 (custom data limits) | `rslib/src/storage/card/data.rs` | where the custom data limits apply |
| json-numbers N2, N4, N6 | `rslib/src/serde.rs`, `Cargo.lock` (serde_json 1.0.143 without `preserve_order`/`float_roundtrip`, serde-aux 4.7.0) | f32 reading through f64, `ryu` f32 text, integer-or-string and bool-from-anything readers |
| schema11-json S3 | `rslib/src/notetype/schema11.rs` (`other` parsing and serialization), `rslib/src/decks/schema11.rs`, `rslib/src/deckconfig/schema11.rs` | flattening, reserved key lists, HashMap order of top-level members |
| schema11-json S4-S6 | `rslib/src/notetype/schema11.rs`, `rslib/src/notetype/templates.rs`, `rslib/src/notetype/fields.rs` | keys, defaults, lenient readers, `did` and `req` conversions, random field and template ids, template mtime and usn stay 0 |
| schema11-json S7-S9 | `rslib/src/decks/schema11.rs` | `dyn` handling, `return` removal, day counters, percentage desired retention, filtered deck keys |
| schema11-json S10 | `rslib/src/deckconfig/schema11.rs`, `rslib/src/deckconfig/mod.rs` | keys, sub-object defaults, `ints`, swapped `order`, `other` split into `new`/`rev`/`lapse`, read-time range checks |
| schema11-json S10.5 (add-on keys paragraph), S14 | `rslib/src/deckconfig/schema11.rs` (`other` split and rebuilt per part) | add-on members survive output, input and a later save; S14 describes Meguri's goldens and has no Anki source |
| schema11-json S11 | `rslib/src/decks/name.rs`, `rslib/src/text.rs` (`normalize_to_nfc`) | native and human forms, component normalization |
| schema11-json S12 | `rslib/src/storage/upgrades/mod.rs`, `rslib/src/storage/{notetype,deck,deckconfig,config,tag}/mod.rs`, `rslib/src/storage/schema11.sql` | the five `col` JSON columns, map keys, the `dconf` retry through a plain JSON value, `''` at schema 18 |
| row-records R2-R9 | `rslib/src/storage/schema11.sql`, `rslib/src/storage/upgrades/schema1[4-8]_upgrade.sql`, `rslib/src/card/mod.rs`, `rslib/src/storage/card/mod.rs`, `rslib/src/revlog/mod.rs`, `rslib/src/storage/revlog/*`, `rslib/src/storage/note/*`, `rslib/src/storage/graves/mod.rs` | column types, enums, read tolerances (`unwrap_or_default` columns, low-8-bit casts) |
| model-ops L2 | `rslib/src/notetype/service.rs`, `rslib/src/notetype/mod.rs`, `rslib/src/decks/service.rs`, `rslib/src/decks/addupdate.rs`, `rslib/src/deckconfig/service.rs`, `rslib/src/deckconfig/mod.rs`, `rslib/src/sync/collection/changes.rs` | which save path matches sync's merge: notetypes `add_or_update_notetype(json, preserve_usn_and_mtime=true, skip_checks=true)`, decks `add_or_update_deck_legacy(deck, preserve_usn_and_mtime=true)`, presets `add_or_update_deck_config_legacy(json)` (stamps usn and mtime) |
| model-ops L1 | `rslib/src/collection/mod.rs`, `rslib/src/backend/dbproxy.rs` | note type and deck caches are not cleared by raw SQL writes, so the driver must reopen the collection before reading legacy JSON |
| model-ops L3 | `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/card/mod.rs` | suspending rewrites the whole card row through the card update path |
| protobuf-records P2, P9 (tenth varint byte, nested payload as the buffer of its fields, depth limit, reason order) | `Cargo.lock` (prost 0.13.5 with default features, so the decode recursion limit of 100 applies), `rslib/src/storage/{deck,deckconfig,notetype}/mod.rs`, `rslib/src/error/mod.rs` | every decode failure is one protobuf error to Anki; the boundaries (tenth byte `01`, 100 nested groups, a field at depth 100, groups and strings that end after a nested payload) come from oracle experiments, recorded as `model/proto/decode-limits`; the P9 reason names and their order are Meguri's own |
| protobuf-records P4 (unknown field order) | none | Meguri-only: Anki drops unknown fields, so their order is a spec decision |
| json-numbers N10, card-data-json C3 rules 1 and 7, schema11-json S3 output rule 1 | `rslib/src/serde.rs` (`default_on_invalid` reads a known key into a JSON value first), `rslib/src/storage/card/data.rs` (unknown keys are skipped), `Cargo.lock` (serde_json 1.0.143 without `float_roundtrip`: the fast decimal conversion and its out-of-range error) | a number beyond the f64 range fails the whole read wherever a value is read; unknown card-data keys are skipped unread; boundary cases from oracle experiments, recorded as `model/card-data/out-of-range` and `model/schema11/other-out-of-range` |
| schema11-json S10.1-S10.3, S10.5 input rule 1 | `rslib/src/deckconfig/schema11.rs` (`new`, `rev`, `lapse` read with `default_on_invalid`) | a part that does not read is replaced whole by its default, its extra members included; recorded as `model/schema11/preset-part-defaults` |
| model-ops L4 | `rslib/src/error/mod.rs` (a prost decode error becomes the protobuf error kind), `rslib/src/storage/deck/mod.rs` (a kind with no member is a database error) | which read failures `blob_decode` maps to `invalid_blob` |

Driver hints for the oracle maintainer (model ops):

- `legacy_json`: close and reopen the collection, then call the backend's
  `get_notetype_legacy`, `get_deck_legacy` and `get_deck_config_legacy` and
  parse the returned bytes (keep integer versus float kinds).
- `legacy_update`: read as above, apply the merge patch, then call the three
  add-or-update methods named in the table; map a backend `JsonError` to the
  rejection `invalid_json`, nothing else.
- `db_exec` blob literals and `CAST(... || X'..' AS BLOB)` need no driver
  support; a statement that writes must be followed by a reopen before any
  read through Anki's caches.
- `blob_decode`: find each record's id from its name column without
  decoding (`notetypes.name`, `deck_config.name`, `decks.name` with `::`
  turned into `\x1f`), reopen the collection, then read the record with the
  same backend getters as `legacy_json`. A backend error of the protobuf
  error kind is `invalid_blob`; anything else stays an `anki_error`. The
  oracle experiments behind `model/proto/decode-limits` expect every
  refused blob there to fail with that kind.
