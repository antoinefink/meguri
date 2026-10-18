# Preservation fixture (oracle maintainers only; AGPL-3.0-or-later)

`make.sh` writes `conformance/fixtures/preservation/kitchen-sink.anki2`
(`specs/conformance/normalized-dump.md` N9) from the recipe scenario
`preservation/recipe/kitchen-sink`. Run it on the host from a full checkout;
it needs Docker and builds the oracle image unless given `--tag`.

- `make.py recipe` runs the recipe's steps without its last observe step in
  the oracle image, through the driver's `Session` as a worker would, with
  `secure_delete` turned on for every `Collection()` the session opens. The
  recipe deletes Anki's stock note types with `db_exec`; without the pragma
  their text stays in free space (the stock check finds 17 of their texts).
  The pragma marks the collection modified in Anki's DB proxy, which matters
  only on a commit request, and the driver sends none: the stored values are
  the golden's.
- The collection closes without the downgrade: schema 18, WAL, checkpointed,
  nothing beside it. `inspect` checks that, SQLite's integrity check and the
  N6.1 query (clean).
- `stock-check` is the storage maker's (`../storage/make.py`), skipping the
  pieces the recipe's own note types hold (`{{cloze:Text}}<br>`, `font-weight:`).
- `open-check` starts a fresh worker on the made file and runs
  `preservation/fixture/open`, whose observe step must be the recipe's last
  one; its value must equal the recipe golden's last step exactly. The golden
  must be current (its `scenario_sha256`).
- `tools/mg oracle check` must find no Check Database problem (K5 counts 0).
  Check Database rebuilds the tags table at the client's usn -1, so every
  tag's usn goes from the synced 9 to -1 and the K9 status is `fail` with
  `changed_tags`; `rebuild-check` runs Check Database itself and fails on any
  change outside `col`, `config` and the tags' usn.
- Remaking changes the notes' guids and the page layout, never the recipe
  golden: re-record the `preservation/` goldens that open the fixture
  (`tools/mg oracle record preservation/fixture preservation/churn
  preservation/edit preservation/review preservation/render`) and compare
  three times.
