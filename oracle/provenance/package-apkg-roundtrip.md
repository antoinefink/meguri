# Deck package round trips through the importer (U122, U123, U120)

Spec-writer notes for the round-trip rows of `specs/package/apkg-export.md` A17
and the note in `specs/package/legacy-export.md` L14. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code, test text or fixture value was copied, and every
expected value comes from the recorded oracle goldens. Written in session
s-20261117T144450Z-f49841c9 (spec writer).

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| package/apkg-export/roundtrip-deck-limit, roundtrip-collection, roundtrip-media, roundtrip-with-scheduling, roundtrip-without-scheduling (harvest hb8a40db6f911 h6bd4a40af394 hffc0559fcb06 h543bc62a3163 hf844f0b31fa4) | `pylib/tests/test_importing_exporting.py`, `pylib/anki/exporting.py`, `pylib/anki/importing.py` | Which behaviors the harvested Python export/import tests require (deck limit, whole collection with nondefault deck, media file presence, scheduling kept vs reset) and which export/import options the deprecated wrappers pass (legacy format on, media on, presets follow scheduling; import with merge, scheduling and presets on), so the scenarios exercise both the legacy and the latest format with matching options. No expectations taken from the assertions. |
| package/apkg-export/roundtrip-fsrs-params, package/apkg-export/fsrs-params-gathered (harvest hee3738c37da3 h514ddcb4bec7); apkg-export A7 | `rslib/src/import_export/package/apkg/tests.rs`, `rslib/src/import_export/gather.rs` (test module only) | That the obligation concerns all three stored FSRS parameter lists (versions 4, 5 and 6) of a non-default preset, both in the gathered package data and after a reimport with scheduling and presets on, with presets included in the export. Values and assertions were not used. |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| Deck-limited apkg round trip with a sibling card, a media clash and import undo, latest and legacy; scenario package/apkg-export/roundtrip-siblings-media-undo | `rslib/src/import_export/package/apkg/tests.rs` | Which behaviors the roundtrip test requires (sibling deck kept, empty child deck dropped, conflicting media under a hash name, static file written, undo empties the rows). |
