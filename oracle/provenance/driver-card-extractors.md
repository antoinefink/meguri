# Oracle driver: card_extractors (U82a)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to call Anki's card-data SQL
functions behind the `card_extractors` observation
(`driver/observe/card_extractors.py`); meanings in
`specs/conformance/extractor-ops.md`. No code was copied. The specs' own
sources are in `search-card-extractors.md`.

| Anki file | Read for |
|---|---|
| `rslib/src/storage/sqlite.rs` | the four function names, their argument counts and order (data, due, ivl, days elapsed, next day boundary, now), that the memory-state extractor panics on a key other than `s`, `d` and `dr`, that the next day boundary is never read |
| `rslib/src/backend/dbproxy.rs`, `pylib/anki/dbproxy.py`, `pylib/anki/_backend.py`, `pylib/anki/utils.py` | `col.db` sends its request as JSON (orjson): JSON values bind by type, an array of byte values binds as a BLOB, rows come back as JSON (serde_json), where a non-finite REAL is written as null; only statements starting with `select` leave the collection's state alone |

Probed in the oracle image (pinned by `driver/tests/test_card_extractors.py`):
a byte list binds as a BLOB (an empty one too), `CAST(? AS TEXT)` on it keeps
the bytes, invalid UTF-8 and a byte order mark included, `35.0` binds as REAL,
`-0.0` survives the bridge, ±∞ comes back as null, and multi-argument
`min`/`max` keep NULL, `-0.0` and finite values while turning ±∞ into
±f64::MAX.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| search/card-extractors X6 SM-2 fallback for relative overdueness (harvest row h4c7fe3f35b39) | `rslib/src/storage/sqlite.rs`, `rslib/src/storage/card/mod.rs` | Which behavior the test proves (relative overdueness under FSRS falls back to an SM-2 key when the memory state is missing), to match it to existing scenarios |
