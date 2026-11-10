# AnkiConnect media actions and the external media folder (U143)

Spec-writer notes for `specs/servers/ankiconnect-media.md` X1-X16
(session s-20261110T085429Z-165d83d3, wave `servers`, package
`ankiconnect-actions`). No code was copied. The first pass ran no oracle
probe; the probe pass of 2026-11-10 below added X16 and settled the
add-on-behavior claims of X3 to X10 and X13. The U136 goldens remain the
authority for the recorded actions. The external folder
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
| X4 | AnkiConnect `storeMediaFile`, `deleteMediaFile`; Anki `pylib/anki/media.py` (`write_data`, `trash_files`), `rslib/src/media/files.rs` (`remove_files`: missing file is a no-op, the name joined to the folder unchecked), `rslib/src/media/mod.rs` | `skipHash` as lower-case MD5 hex compared exactly, `deleteExisting` default true and trash-then-write, the stored name returned; the add-on trashing by the raw `filename`, which X4 step 3 replaces with the transformed last component. The probe (X16.5, X16.8) showed what the raw name does: `../x` leaves the outside file in place (the trash folder is a sibling of the media folder, so `../` names the same place on both sides), `sub/x` fails with `No such file or directory`, and only a subdirectory or the media folder itself (`""`) can be moved. The X4 examples are Meguri's own (A, B), not the add-on README's |
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
| X16 | the probe below | observed replies, field values and media-folder effects |

Similarity: run `tools/mg similarity` from the main checkout before merging.

## Probe pass, 2026-11-10 (X16)

Session s-20261110T085429Z-165d83d3 (spec-writer, workflow item `ac-media`).

- Target: AnkiConnect add-on **25.11.9.0** (upstream tag `25.11.9.0`, commit
  `532a3c816e08e91c4367e2799ce3779403d70274`, `oracle/ankiconnect/ADDON_PIN`)
  in Anki 26.9.3 (`aqt[qt]==26.9.3`), image `meguri-oracle-ankiconnect:spike`
  (id `e63ede4de692`), built from `oracle/ankiconnect/`.
- Run: arm64 Docker Desktop, `--internal` network and container both named
  with `ac-media`, `--tmpfs /scratch`, `ORACLE_CLOCK_START=1894701600`,
  `TZ=Europe/Paris`, no published port. Requests went to `127.0.0.1:8765`
  from `docker exec ... python3 -I -` scripts (urllib), version 6, no key,
  no `Origin`. An HTTP stub for the `url` rows ran on the container's
  loopback (port 8099) inside the probe script. Container and network were
  removed afterwards.
- Probe date: 2026-11-10. Profile: the image's new `oracle` profile on the
  container's case-sensitive file system. Files placed directly into the
  media folder for the X16.6 to X16.8 rows (subfolder, hidden name, outside
  files) were written by the probe script, not through Anki.
- Read for the probe design, not copied: the add-on's media actions, its
  attachment handling and `download` helper (to pick inputs), Anki
  26.09.3's `pylib/anki/media.py` (`_legacy_write_data`, which keeps the
  last path component) and `rslib/src/media/files.rs`
  (`normalize_filename`, `add_data_to_folder_uniquely`), to explain the
  observed `a/b.mp3` → `b.mp3` reply. Every X16 row states only what the
  requests returned and what the folders held afterwards.
- Probe scripts and raw outputs stayed in the session scratchpad; they hold
  no private data and were not committed.

| Spec section | Changed by the probe |
|---|---|
| X1 | the Falsy term (the add-on's tests of optional members) |
| X3.1 | falsy members absent (`data: 0`); precedence and no fallback confirmed |
| X3.2 | non-ASCII refused like the add-on; `YQo=garbage` row; empty-bytes row confirmed |
| X3.3, X3.4 | the add-on's path and URL behavior noted; status text confirmed for 201, 204, 404, 500 |
| X4 | last `/` component kept (as the add-on's media layer does); `skipHash` and `deleteExisting` value rules from the add-on; replies confirmed |
| X5 to X8 | the add-on's observed behavior noted against Meguri's rules |
| X9, X14 `media/store-traversal` | `../collection.anki2` stored as `collection.anki2`, `a/b.mp3` as `b.mp3` |
| X10 | non-string `fields` entries skipped; repeated names; failed adds keep files in the add-on; `canAdd*` store files; `updateNote` without `fields` ignores attachments |
| X13 | url-limits, names-order, delete-existing, base64, flat-names, path-roots rows updated; `ankiconnect-media-attach-fields-type` added; `ankiconnect-canadd-media` proposed for actions A18 |
| X15 Q5, Q7 | probe facts recorded; Q7's policy stays open |
| X2, X3.1 | re-probe below | registry type any for the content members, `skipHash`, `deleteExisting`; only the chosen member's type checked |
| X3.2, X5, X7, X10.1, X10.2, X12, X13, X14 | re-probe below and review findings | `=`/`==` row; `""`, directory names and case variants; attachment error paths; list or object `fields` entries; object, `null`, `true` `fields` on failure; placeholders; `ankiconnect-media-case-variants`, `ankiconnect-media-non-files`; `canAdd*` named by actions A18 `ankiconnect-canadd-no-media` |

## Re-probe, 2026-11-10 (review fixes)

Same session, image and settings as the probe pass above, in a new
container and `--internal` network both named `acm-fix-q4z9` (removed
afterwards). One `python3 -I -` script created deck `D` and note type
`Pair` (`Term`, `Gloss`) with `createDeck` and `createModel`, then sent
these requests, reading the media folder directly after each:

- `storeMediaFile` with `data: "YQo="` plus `url: 5`, `path: 5`,
  `url: true`, `path: ["x"]`; with `data: 0` plus `path: "/etc/hostname"`;
  with `data: "="` and `data: "=="`.
- `storeMediaFile` `clip.mp3` A, then `Clip.mp3` B; `retrieveMediaFile`
  `CLIP.MP3`, `Clip.mp3`, `""`; `storeMediaFile` `b.mp3` B, then
  `deleteMediaFile` `B.MP3`.
- `addNote` (`Pair`, `D`) with one `audio` attachment each, `data` `YQo=`
  (success) or `YQ` (failure), and `fields` set to `["Gloss", ["x"]]`,
  `["Gloss", {"a": 1}]` (success); `["Gloss", ["x"]]` (failure);
  `{"Gloss": 1}` (both); `null`, `true`, `[null, "Gloss"]`, `{}`, `[]`
  (failure); `["Gloss", null, true, 1.5]` (success).

The script and its output stayed in the session scratchpad (no private
data). Every request is listed above, so a reviewer can re-run it against
the same image. The X16 rows marked "(re-probe)" state the replies.
