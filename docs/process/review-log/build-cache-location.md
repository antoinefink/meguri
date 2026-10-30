# Rust build cache location

Start: `7751eaf7`. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The optional cache path applies to dev, fuzz and private services. Defaults
stay unchanged; private access stays read-only. Independent review is clear.
`core-format/20261030T145333Z-15401` passes with an actual external bind mount.

`gate/20261030T143745Z-64354` was stopped at the disk boundary during the full
Rust build. Static checks, canaries, lint and dependency checks had passed.
Retain its cancelled result. No full gate or unit acceptance is claimed.
Only idle Meguri build caches were removed; source and receipts are retained.
