# specs/servers/ankiconnect-clients.md (U136, R1945)

Spec-writer session s-20261110T085429Z-165d83d3. Only action names,
parameter shapes, envelope traits and call orders are recorded; no client
code, no personal content, no session transcripts. Every example value is
synthetic. Client source code (Yomitan, asbplayer, mpvacious) was not read
in this session: the per-client facts come from the research dossier, which
records the set of actions per client and the envelope traits, not query
forms, media forms or call orders.

| Source | Read for | Spec sections |
|---|---|---|
| `local-data/research/09-ankiconnect.md` §1.2-§1.7, §4 | per-client action sets, origin kind, `version` sent (Yomitan 2, asbplayer 6, mpvacious 6), key use, Yomitan's `apiReflect` probe and `multi` batching of `findNotes`, Yomitan's `storeMediaFile` with base64 data and `guiBrowse` with `nid:` queries, mpvacious's `findNotes` with `added:1` and its writes into `getMediaDirPath`; field-name case rules; `addNotes` all-or-nothing; extension-origin rule of the default list | C2, C3 ("Used by", "known" forms) |
| `local-data/research/01-user-collection.md` (AnkiConnect row) | the actions the user's earlier AI-agent sessions called (names only) | C2, C3 |
| `local-data/research/12-template-compat.md` (JPMN) | a card template posting to `127.0.0.1:8765` and failing soft | C2, C9.7 |
| AnkiConnect upstream repository, commit 532a3c8 (2025-11-06): `README.md` sections of the listed actions; `plugin/__init__.py` (`requestPermission` member `requireApikey`; `storeMediaFile` content-source order data, path, url; `guiBrowse` refusing a `columnId` that is not an active browser column with `invalid columnId: <id>`; `addNotes` adding then deleting on failure; `guiAddCards` returning the unsaved note's id; `updateNoteFields` ignoring members other than `id`, `fields` and media); `plugin/util.py` (`download` raising `<url> download failed with return code <status>` for a status other than 200) | parameter names and types, reply shapes, the `requireApikey` spelling; C1's M note; the expected replies of `yomitan/gui-browse-nid`, `yomitan/add-notes-all-or-nothing`, `asbplayer/gui-add-cards`, `none/add-note-url-media`, `asbplayer/update-note-fields-whole-note` | C1, C3, C7 |
| Anki 26.09.3 (`refs/anki-26.09.3`): `pylib/anki/browser.py` (default card columns noteFld, template, cardDue, deck), `qt/aqt/browser/table/model.py` (column index only for active columns), `pylib/anki/notes.py` (a new note comes from the backend with id 0) | `noteCrt` is not an active column in a fresh profile; `guiAddCards` returns 0 | C6, C7 |
| plan R1679, R1734-R1788, R1744, R1945; `specs/render/media-serving.md`; `specs/servers/ankiconnect-gui.md` G3, G8, G9; `specs/servers/ankiconnect-media.md` X3, X3.4, X11.3; `specs/servers/ankiconnect-actions.md` (deviation `ankiconnect-addnotes-rollback`); `specs/servers/ankiconnect-envelope.md` E7.2 | Meguri's reply rules referenced; the card webview origin; the R1679/R1744 conflict for `null` | C3, C7, C9 |
| independent review of this package (2026-11-10), which reported shapes it found in the Yomitan and asbplayer sources | the candidate forms of C3 rows 40 to 47 and C9.4, C9.10; recorded as unverified, not read or checked against a client release in this session | C3 rows 40-47, C9.4, C9.10 |

Meguri decisions made here: the shared synthetic setup, extension ids,
header choices for scenarios (`application/json` for the extension envs),
the `no-origin` and `client:none` tags, the scenario list and the literal
parameters of each scenario, and which forms are marked known, assumed or
unverified.

Probe of the pinned add-on (2026-11-10, wave 5 item `ac-actions`):
AnkiConnect 25.11.9.0 (`ADDON_PIN`, commit `532a3c8`) in Anki 26.9.3, image
`meguri-oracle-ankiconnect:spike`, internal network, tmpfs `/scratch`,
`ORACLE_CLOCK_START=1894701600`, `TZ=Europe/Paris`, no published port;
containers `ac-actions-*`, removed afterwards. The C6 setup was made through
the add-on's actions; every single-call C7 oracle row except
`asbplayer/request-permission-key` (needs a key in the add-on's config), and
each call of the sequence rows on its own, was sent with its env's `Origin`, `Content-Type` and `version`. URL attachments
used a loopback HTTP server inside the container; the picture was a synthetic
1x1 PNG, not the not-yet-written `pixel.png` fixture. Observed and written
into C3 and C7: `multi` with a query-less `findNotes` answering `[]`; the
add-on's `model was not found: Nope`; scan-order card lists; the
`storeMediaFile` names (given name, or `<stem>-<SHA-1 hex>.<ext>` with
`deleteExisting: false` and different bytes); `Card 1` and stock CSS for
`createModel` without names or CSS; the model's `did` after adds;
`guiEditNote` failing on an existing note and Anki's inconsistent-database
text for an unknown one; `guiSelectedNotes` returning the earlier selection.
Every other reply was as the first pass expected. C9.1 to C9.10 are client
facts and were not probed. Scripts and raw outputs stayed in the session
scratchpad.

Second review fix (same session): C7 `mpvacious/find-models-by-name`'s `did`.
The add-on's note builder (installed `addons21/2055492159/__init__.py`,
`createNote`) sets `did` on the note type dict that Anki's Python model
manager caches, so the add-on's own `addNote` and `canAdd*` change what
`findModels*` report in that session; Anki 26.09.3's `Collection.add_note`
(`pylib/anki/collection.py`) and schema-11 conversion
(`rslib/src/notetype/schema11.rs`, `did` from the stored target deck) do not.
A setup applied through the library therefore leaves `did` `null`. C9.11 now
cites `specs/servers/ankiconnect-gui.md` G11, which already declares both
`guiEditNote` differences.
