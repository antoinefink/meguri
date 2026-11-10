# AnkiConnect media actions and the external media folder (U143)

Spec-writer notes for `specs/servers/ankiconnect-media.md` X1-X15
(session s-20261110T085429Z-165d83d3, wave `servers`, package
`ankiconnect-actions`). No code was copied and no oracle probe was run; the
U136 goldens are the authority for the recorded actions. The external folder
(X8, X11), the readable folders and the source limits (X3.3, X3.4) are
Meguri's design from R1770-R1773 and the U143 approach; they have no
AnkiConnect counterpart.

Sources:

- AnkiConnect add-on (GPL-3.0-or-later, read only): `https://git.sr.ht/~foosoft/anki-connect`
  at commit `532a3c8` (2025-11-06), the last public commit before the
  installed build's date of 2025-11-09; see the actions note for the pin.
- Anki at tag 26.09.3 (`refs/anki-26.09.3`).
- `local-data/research/09-ankiconnect.md` §1.7, §4 (mpvacious writing into
  `getMediaDirPath`), §5.
- The plan: R1761, R1762, R1769-R1773, AE79, units U143, U144, U135;
  `docs/plans/research/coverage/servers.md` (the `getMediaDirPath` decision).

| Spec section | Sources read | Read for |
|---|---|---|
| X1 | Meguri design (R1770, R1772; `specs/servers/host-security.md` H10.2 for the config members), no Anki or AnkiConnect source | roots, canonical paths, flat names |
| X2 | AnkiConnect media actions (signatures and defaults) | parameters, results; `affected` is Meguri's (R1780) |
| X3.1 | AnkiConnect `storeMediaFile` | truthiness of the three sources, precedence, the missing-source error text |
| X3.2 | AnkiConnect `storeMediaFile` (non-validating base64 decode); Python 3 standard library decoder behavior, tried in this session on synthetic inputs (`YQo`, `YQ==YQ==`, `YQo==`, `-_8=`) | characters outside the alphabet discarded; the runtime stopping at the first padding and accepting extra `=` (`ankiconnect-media-base64`); Meguri's strict rule after the discard is Meguri design |
| X3.3 | Meguri design (R1772; plan U143 `paths.rs` approach), no AnkiConnect source (the add-on opens any path) | order of checks, no existence probe outside the roots, open-then-verify, the size cap |
| X3.4 | AnkiConnect `plugin/util.py` (`download`, `webTimeout`); `specs/servers/host-security.md` H8, H13 | the non-200 error text, the add-on's 10 s timeout and 200-only rule (Meguri uses R1773's 30 s and H13's 2xx and cap) |
| X3.5 | Meguri design (`engine/op-model` O2, O6; R1791, R1792), no source | bytes obtained before the op |
| X4 | AnkiConnect `storeMediaFile`, `deleteMediaFile`; Anki `pylib/anki/media.py` (`write_data`, `trash_files`), `rslib/src/media/files.rs` (`remove_files`: missing file is a no-op, the name joined to the folder unchecked), `rslib/src/media/mod.rs` | `skipHash` as lower-case MD5 hex compared exactly, `deleteExisting` default true and trash-then-write, the stored name returned; the add-on trashing by the raw `filename` (so a path-like name reaches outside the folder), which X4 step 3 forbids. The X4 examples are Meguri's own (A, B), not the add-on README's |
| X5 | AnkiConnect `retrieveMediaFile` | last path component, NFC, illegal characters stripped, base64 or `false` |
| X6 | AnkiConnect `getMediaFilesNames`; the documented glob rules of Python's standard library, which the add-on uses (from general knowledge, not re-read) | glob semantics, hidden names, default `*`, directory order |
| X7 | AnkiConnect `deleteMediaFile`; Anki `rslib/src/media/files.rs` | trash move, path joined without checks, `null` result |
| X8 | AnkiConnect `getMediaDirPath`; `docs/plans/research/coverage/servers.md` | Anki returns its media folder; Meguri's error decision |
| X9 | Meguri design from X4 and `specs/text/media-filenames.md` F1 | |
| X10 | AnkiConnect media attachment handling and `README.md` (attachment members) | members, list or single object, audio then video then picture, `[sound:]` and `<img src="">` appended, `deleteExisting` defaulting to false for attachments, the escaped error text appended, the `'fields'` key error from the failure path (in `addNotes` caught per note), a non-list `fields` ignored on success, case-sensitive field names; pre-reading the sources before the op is Meguri design (X3.5) |
| X11 | plan U143 approach, U144 approach, R1770, R1771, AE79; Meguri design for the index states, suspension, the grant/apply order, commit ordering, collision guard, interface and notices (review fixes of this session), no Anki or AnkiConnect source | index, baseline, stability rule, triggers and their timings, the pre-commit hook, exact-name ingest |
| X12 | Meguri design (proposed wording), no source | |
| X13 | the rows above | where Meguri differs |
| X14 | Meguri design from the sections above and the plan's U143 test scenarios; byte contents and file names are this session's own | expected replies derived from reading, not recorded |
| X15 | the plan's Product Contract (R1762, R1770, R1771) and U143/U144 against the sections above | the open decisions |

Similarity: run `tools/mg similarity` from the main checkout before merging.
