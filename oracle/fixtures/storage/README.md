# Storage fixtures (oracle maintainers only; AGPL-3.0-or-later)

`make.sh` writes the collection files of `conformance/fixtures/storage/` from
the recipes of `specs/storage/schema-upgrade.md` U16. Run it on the host from a
full checkout; it needs Docker with linux/amd64 emulation.

| Fixture | Made by | Notes |
|---|---|---|
| `schema-14` | 2.1.26 (`anki` + `ankirspy` wheels) | notetype legacy key `tags` dropped by SQL, `vers` kept (one unknown key) |
| `schema-15` | 2.1.33 (`anki` + `ankirspy`) | ease cases; Default re-saved with `initialFactor` 2500, stored 250.0 |
| `schema-16` | 2.1.40 | |
| `schema-17` | 2.1.44 | this release names the fields table `FIELDS` |
| `schema-18` | 26.09.3 (oracle image) | |
| `schema-11` | 26.09.3, closed with the downgrade | `meguriExtra` on `Pair` and `Lang` |
| `schema-11-names` | `schema-11` + JSON edits | name cases |
| `ver-*-delete`, `ver-*-wal` | `schema-11` (10, 12, 13), `schema-18` (19) | journal mode switched by SQLite |
| `v1-scheduler-18`, `-11` | `schema-18` (`schedVer` 1), `schema-11` (member removed from the text) | |

- `locks/anki-<release>.lock` pins every wheel by hash (`uv pip compile
  --generate-hashes`, Python 3.8, x86_64 manylinux, `--exclude-newer` a week
  after the release). The legacy image is `Dockerfile.legacy`.
- `make.py` uses each release's own Python API, with fallbacks for the names
  that changed between 2.1.26 and 26.09.3, then edits the closed file with
  `sqlite3`. It checks `col.ver` after the release closes the file.
- The clock runs (libfaketime start-at, `@<epoch>` in the timestamp file):
  2.1.26 waits for the clock to move when it picks a note type id, so a frozen
  clock hangs it. Content is made on 2026-09-01 (day 0) and day 5 (UTC).
- Releases at 15 store a preset's legacy `initialFactor` F as F / 10, but
  read it back as stored × 10, so only presets saved through the legacy API
  hold the ×100 value; the Default the release creates holds 2.5.
- `make.py upgrade-check` upgrades copies with 26.09.3 and fails the run when
  a converted object's `other` JSON has more than one key (Anki writes it in
  hash-map order). Releases at 14 to 16 spell config's key column `key`.
- Remaking changes every fixture byte (ids and stamps come from the clock):
  re-record every storage golden with the new files, three times, and check
  that the recordings agree.
