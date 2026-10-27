# Check Media and the media trash actions (U130)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/maintenance/check-media.md`.
No code was copied; every rule was restated in the spec writer's words. No scenario was
recorded: the `check_media` vocabulary of C21 does not exist in the driver yet, so the
statements await the C22 scenarios.

| Spec section | Anki file | Read for |
|---|---|---|
| C3 | `rslib/src/media/check.rs`, `rslib/src/sync/media/mod.rs` | folder pass: non-UTF-8 and `.DS_Store` skipped, directories listed, files above 100 MiB listed and dropped, empty files dropped silently |
| C4, C5 | `rslib/src/media/check.rs`, `rslib/src/media/files.rs`, `rslib/src/media/mod.rs` | descending name order, the normalized-name test (Apple NFC case), rename by adding the lowercased name and deleting the old file, case-insensitive duplicate handling |
| C6 | `rslib/src/media/check.rs`, `rslib/src/text.rs` | per-reference order: remote skip, inline extraction, NFC, renamed lookup, the "renamed during an earlier sync" transform rule, field rewrite and its minimal escaping |
| C7 | `rslib/src/media/check.rs`, `data-encoding` 2.11.1 (`BASE64`) | the data URL pattern, payload trimming and strict decoding, the `paste-<sha1>.<ext>` name, overwrite and count |
| C8 | `rslib/src/latex.rs`, `rslib/src/cloze.rs` | the three LaTeX forms, the cloze reveal used before extraction (without the MathJax pass), newline replacement (verbose-mode pattern: `<br>`, `<br/>`, `<div>`), stripping, `latex-<sha1>` naming and the `latexsvg` choice |
| C9 | `rslib/src/media/check.rs`, `rslib/src/notetype/checks.rs` | the `_` prefix rule; templates not scanned; the report of templates with field replacements inside media, sound and LaTeX tags |
| C10 | `rslib/src/media/service.rs`, `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs` | the check runs in one transaction without an undo op: notes updated with sort field and checksum recomputed, mtime and usn stamped, undo emptied and `col.mod` moved even without changes |
| C11 | `rslib/src/media/check.rs`, `rslib/io/src/lib.rs` | unused and missing computation, case sensitivity probed on the folder, missing notes |
| C12, C13 | `qt/aqt/mediacheck.py`, `ftl/core/media-check.ftl`, `rslib/src/tags/bulkadd.rs` | which buttons the report offers, the `missing-media` tag and its "Update Tag" undo op |
| C14 | `qt/aqt/mediacheck.py`, `rslib/src/media/mod.rs`, `rslib/src/media/files.rs` | confirmation, trashing in chunks of 25, deletions recorded pending in the media database |
| C16, C17 | `rslib/src/media/check.rs`, `qt/aqt/mediacheck.py` | restore by rename or by add when the name exists; empty trash without confirmation; trash count and size |
