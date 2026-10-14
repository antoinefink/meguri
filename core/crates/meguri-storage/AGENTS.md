# meguri-storage

- Only this crate issues SQL against `collection.anki2`. Never keep a statement or a read transaction open across FFI calls: an open read transaction stops checkpoints and the `-wal` grows.
- Every connection registers `unicase` first (`conn_init`), with `meguri_text::fold::compare_folded` (the `=2.6.0` pin; another `unicase` reorders the name indexes). The probe registers nothing else; writer and readers then run the initializers.
- The open gate writes nothing before phase 4: the probe and the pre-upgrade copy open with `immutable=1` unless a `-wal` or a non-empty `-journal` lies next to the file (`schema/version.rs`). Never open a file that may be refused read-write, and never set a pragma before the refusals.
- Never add a schema object to the collection (R2016). Statement texts are `schema/ddl18.sql` (the normalized tokens of `specs/storage/schema-18.md`, interop facts); step 14's `tags` is the only DDL outside it. Compare `sqlite_master` names case-insensitively: schema-17 files store `FIELDS`, schemas 14 to 16 config's `key`.
- Upgrade steps that rebuild a table drop and recreate it, then rerun the stored text of its foreign indexes and triggers (`upgrade::foreign_dependents`). Never `ALTER TABLE ... RENAME` a collection table: SQLite rewrites every view and trigger that names it to the new name.
- The upgrade runs in one `BEGIN IMMEDIATE` on the writer; the WAL switch happens before it. Fault points: `storage.upgrade.after_copy`, `storage.upgrade.in_transaction`, `storage.conn_init.unicase`.
- Step 14's compact JSON (`legacy_json::compact`) reads numbers with serde_json's own parser on purpose: the spec and goldens want `123456789012345678901` as `1.2345678901234567e20`. serde_json's `float_roundtrip` anywhere in the build would change it.
- `raw` (feature `test-hooks`) is the conformance runner's and the tests' only raw SQL; product code gets typed calls.
- Tests copy fixtures into temporary directories and never open `conformance/fixtures/` with SQLite. They get `test-hooks` through the dev-dependency on meguri-testkit; fault points are process-global, which nextest's process per test keeps apart.
