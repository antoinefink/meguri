# Package fixtures (oracle maintainers only; AGPL-3.0-or-later)

`make.sh` writes `conformance/fixtures/package/` from the recipes of
`specs/conformance/package-ops.md` E4: Anki 26.9.3's forced backup of
`storage/schema-18.anki2` at 2030-01-14T12:00:00Z (`make.py backup`, clock
frozen by `run.sh`), rewritten with the placeholder, and the variants built
from it and the storage fixtures (`make.py build`), then the deck-package
fixtures `apkg/*.apkg`: the refusal fixtures and the crafted `card-ids-ahead`
and `deck-tree` from `storage/schema-11.anki2` (`apkg.py`, run as `make.py
apkg`), and `legacy-1` and `v1-scheduler`, Anki 2.1.26's own exports
(`oldclient.py` through `oldclient-run.sh` in the storage maker's legacy
image, `meguri-fixtures-legacy:2.1.26`, which `make.sh` builds). Run it on the host from a
full checkout after the storage fixtures exist; it needs the oracle image and
the storage maker's 2.1.26 legacy image (linux/amd64), both of which `make.sh`
builds, with `--apkg-only` too.

- The maker writes every archive itself: stored entries dated 1980-01-01 in
  E4's order. Anki's own frames (the backup's `collection.anki21b` and
  `media`) are copied byte for byte; every other zstd frame is the maker's
  raw-block frame (no zstd library in the image).
- `corrupt-page` zeroes the cell count of `ix_cards_nid`'s root page and
  fails unless that page is a leaf index page, the copy's integrity check is
  not `ok` and `col.ver` still reads 18.
- Stock check (R1959): `make.py sqlite-files` writes every SQLite file the
  packages hold (raw entries and the contents of the maker's frames) and the
  collection Anki imports from `backup/standard.colpkg`; the storage maker's
  `stock-check` must pass on all of them before anything is copied.
- Same image, same storage fixtures: the same bytes (checked twice). Remaking
  the storage fixtures or bumping the pin changes them: remake, then
  re-record `package/collection-package` and `package/apkg-import` with
  `tools/mg oracle record`.
- `--apkg-only` writes only `apkg/`; the collection packages stay as committed
  (other goldens depend on their bytes) and the stock check imports the
  committed backup.
- Old client (`oldclient.py`): 2.1.26 cannot run under a frozen clock (it waits
  for the clock to move when it picks a note type id), so Python's `time.time`
  is replaced by a clock that moves 1 ms per read from the instant the recipe
  sets; libfaketime freezes the clock the backend reads at that instant and
  `random` is seeded. The backend writes `col`'s JSON in hash-map order: the
  maker sorts its keys (same lengths, `secure_delete` on). secure_delete is
  turned on for the source and both export collections right after 2.1.26
  creates them; each exported collection is checked against 2.1.26's stock
  text, then by the 26.09.3 stock check. Any change to the recipe changes every
  id: re-record `package/apkg-import/legacy-1`, `v1-scheduler` and
  `package/notetype-merge/by-name`.
