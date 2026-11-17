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

## Session s-20261117T144450Z-f49841c9 (spec writer): deferred harvest rows, batch 1

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows and write their scenarios; no code, test text or fixture value was copied, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| maintenance/check-media/templates/attribute-forms; maintenance/check-media#C9 (as relied on) | `rslib/src/notetype/checks.rs` | Which behavior the template-report test requires (a side whose media tag holds a field reference is listed, a side naming a file literally is not, with unquoted attributes). That the check covers media tags, sound tags and LaTeX runs, and that a field reference must sit on one line. Golden values come only from the oracle. |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| maintenance/check-media/report/store-added; maintenance/check-media#C11 (as relied on) | `pylib/tests/test_media.py` | Which behavior the harvested deck-integration test requires: after a file is added through the media store and another is written straight into the folder, Check Media lists the reference that has no file as missing and the folder file that no note uses as unused. Only the behavior was taken. Names, contents and structure are the scenario's own, and every expected value comes from the recorded oracle golden. |
| C6 and C11 scenarios refs/chevron-attributes, refs/source-tags, refs/entity-forms (reference extraction and decoding at Check Media level) | `rslib/src/media/check.rs` (tests module) | Only to learn which behaviors the harvested tests target: a > inside quoted attributes before src, a src after a closed tag, source tags inside media elements, and entity decoding of sound and img names. The fixtures were rewritten after review so they share no markup layout with the tests. All expectations come from the oracle goldens. |
| C5 and C20 scenario case/three-spellings (different-content case variants plus a third spelling with no exact file) | `rslib/src/media/check.rs` (tests module) | To learn that the harvested test pins how case-variant files with different content are deduplicated, and that a further spelling is reported missing on a case-sensitive folder. The outcome comes from the Linux oracle golden. |
| Classification of the rows media_check, long-filename rename, inline images and case-insensitive unused check as covered by existing C22 scenarios | `rslib/src/media/check.rs` (tests module) | To match each test's obligation against the existing scenarios report/basic, rename/invalid-chars, rename/long-name, inline/extract and refs/case-insensitive. |
| Check Media scenarios refs/several-tags, report/added-files, actions/trash-restore-check, actions/trash-empty-check (C6, C11, C16, C17) | `rslib/src/media/check.rs` (tests module: unicode_normalization, multiple_images), `rslib/src/media/service.rs` (tests module: media_check, restore_trash_check, empty_trash_check) | Which behaviors the harvested tests require: several image tags with alt before src in one field; the unused, missing and trash state of a check around trash, restore and empty; unused and missing for store-added files. Golden values come only from the oracle. |
