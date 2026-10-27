# Package fixtures (oracle maintainers only; AGPL-3.0-or-later)

`make.sh` writes `conformance/fixtures/package/` from the recipes of
`specs/conformance/package-ops.md` E4: Anki 26.9.3's forced backup of
`storage/schema-18.anki2` at 2030-01-14T12:00:00Z (`make.py backup`, clock
frozen by `run.sh`), rewritten with the placeholder, and the variants built
from it and the storage fixtures (`make.py build`), then the deck-package
refusal fixtures `apkg/*.apkg` from `storage/schema-11.anki2` (`apkg.py`, run
as `make.py apkg`). Run it on the host from a
full checkout after the storage fixtures exist; it needs only the oracle image.

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
