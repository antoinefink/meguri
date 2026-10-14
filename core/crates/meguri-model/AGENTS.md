# meguri-model

- Field numbers, wire types, JSON keys, readers and defaults come only from `specs/model/`. A message's `merge_field` and `encode_fields` list the same fields; `encode_fields` writes them in ascending field number (P3). `tests/proto_roundtrip.rs` has its own field tables from P6: change both with the spec.
- Equality of blob content is `BlobMessage::same_content` (floats bit for bit, unknown fields included), never `==` on floats. `ProtoBlob::to_bytes` relies on it to write the read bytes back unchanged (P5).
- `other` bytes and unknown fields are opaque outside `schema11/`; nothing else parses or edits them. Only `fsrs_params_6` has a setter (R1486).
- JSON values are read from serde_json raw text (`json.rs`) with std's parsers and written with ryu. Never route a typed value through `serde_json::Value` or `f64`: `0.212f32` widened prints `0.21199999749660492`. `json::parse_value` is the only entry for text: it also rejects unpaired surrogate escapes, which serde_json's raw capture lets through.
- Tests use the model goldens as conversion pairs (`specs/model/schema11-json.md` S14) and never write them.
