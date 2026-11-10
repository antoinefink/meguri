# specs/servers/transcripts.md (U136)

Spec-writer session s-20261110T085429Z-165d83d3. Written in Meguri's own
words; no AnkiConnect, Migaku or Anki code, test code or fixture text was
copied. The recorder itself is oracle-maintainer work (U136) and is not
specified here beyond what it must output.

| Source | Read for | Spec sections |
|---|---|---|
| plan U136 (Approach incl. aqt offscreen, libfaketime with real monotonic time, `ADDON_PIN`; Execution note; Test scenarios), R1799-R1801, R1888, R1889, R1945, R1948, R1949 | scenario shape (setup + ordered requests + observations), goldens only from the oracle, normalization goals (logical ids, mod times, rendered HTML), known deviations list, equivalence check, self-test cases | all |
| `specs/conformance/dsl.md` D1-D5, D7.1, D10-D13; `observations.md` O1-O3, O6-O8; `normalized-dump.md` N2, N5; `render-ops.md` Q5.4 | reused rules: ids, YAML, hashing, clock, logical ids, golden text format, statuses, report, deviation entries; comparison; masks | T2, T5, T7, T9-T12, T14-T16 |
| AnkiConnect upstream repository, commit 532a3c8 (2025-11-06): `plugin/web.py` | reply header set (status line, `Content-Type: application/json`, `Access-Control-Allow-Origin`, `Access-Control-Allow-Headers: *`, `Content-Length`); 403 reply without `Content-Length` or body but with the two CORS headers; OPTIONS handling; request headers parsed only when written `Name: value`; body length taken from `Content-Length`; one reply per connection; empty body ping reply; Python `json.dumps` separators | T6.2, T6.4, T8, T11, T12, T16 |
| same commit: `plugin/__init__.py` (request handler, `requestPermission`, `multi`, `apiReflect`), `plugin/util.py` (defaults: apiVersion 6, config keys) | version default 4, bare result for version <= 4, error object for every version, key check, `requireApikey` member spelling, the permission dialog's Yes/No/ignore checkbox outcomes, reflect filtering by given list | T7.2, T10.5, T12, T18 |
| same commit: `README.md` | action parameter and reply shapes used to place id positions | T10.5 |
| Anki 26.09.3 `pylib/anki/cards.py` and `pylib/anki/template.py` (a card's question and answer are returned with the note type's CSS in a leading style element); AnkiConnect `plugin/util.py` (`cardsInfo` texts come from those methods) | T19.4 settled | T19 |
| AnkiConnect commit 532a3c8: `plugin/web.py` (`Content-Length` framing only, so a chunked request is answered as the ping); `plugin/util.py` (download error text holds the URL) | chunked bodies declared-only; the `<fixture>` normalization | T10.3, T12 |
| `local-data/research/09-ankiconnect.md` §1.1, §1.2 | add-on build date 2025-11-09, API version 6, polling interval | T9, T16 |
| `specs/servers/host-security.md` H10-H12, H19-H22; `specs/servers/ankiconnect-gui.md` G8; `specs/servers/ankiconnect-media.md` X8, X11.3; `specs/conformance/observations.md` O8 | host steps and outputs, interaction kinds and their answers, the media-dir mapping and scan, the `db_dump` table form | T4, T7, T13, T14, T20 |

Meguri decisions made here: the separate schema and runner job; step kinds and
their names; raw-socket request writing; default `Host` header; body forms;
the compared-header set; the `closed` member; id positions per action; the
1,000,000,000 id threshold; `#dN`/`#mN`/`#rand`/`#unknown`/`<media-dir>`/
`<profile>` tokens; `@<offset>` mod times; the `absent` transform; declared
`expect` matching rules; the 60 s reply timeout; the equivalence procedure's
compared columns.

Added after review (same session), all Meguri decisions: `server: host`
scenarios with per-exchange `server` and `address`, `host_env`, `enabled`
and `port` in `server_config`; the bind modes and namespace isolation of
T20; the `raw`, `http_version`, `expect_continue`, `async`, header `fill`,
`pipeline`, `open`, `close_wait` forms and the write-error rule; connection
outcomes in `expect`; the steps `await`, `config`, `restart_host`,
`resolve`, `events`, `sockets`, `logs`, `media_dir_scan`, `wait`, `status
until`, `interaction`, `lifecycle` `open_fail` and `{replace: ...}`, the
targeted `fault` forms; the `TestHost` defaults per kind; the media-dir
default; token assignment before a reply is normalized; `mod` only in
replies; `${fixture_base}` and `<fixture>`; the `getDecks` position; the
deviation `tags` selector and the `no-origin` tag; the `--tag` and id-prefix
filters of the job; and the step order with background exchanges. The
`retry_tick` step was folded into `advance`.

## Probes against the running add-on (2026-11-10)

Session s-20261110T085429Z-165d83d3, item `ac-envelope`. AnkiConnect 25.11.9.0
(commit `532a3c8`, `oracle/ankiconnect/ADDON_PIN`) in Anki 26.9.3 offscreen,
image `meguri-oracle-ankiconnect:spike`, `ORACLE_CLOCK_START=1894701600`.

| Spec text | Result |
|---|---|
| T7.2, T19.1 | the driver add-on's `allow`, `deny`, `deny_ignore` answers gave granted / denied / denied and the list changes stated; a prompt with no answer held the ping and `version` for at least 4 s |
| T9 `oracle.addon.sha256` | defined as ADDON_PIN's `archive_sha256` (the Dockerfile's `ADD --checksum` value); the installed files are checked against ADDON_PIN's per-file hashes at build |
| T10.1, T19.3 | `modelNamesAndIds`: stock types at `1894701600000` to `...005`; `createDeck`, `addNote`, its card: `1894701600000` each; after `deleteNotes` of the first note, the next `addNote` and its card got `1894701600000` again; after `deleteDecks`, the next `createDeck` got the freed deck id; `createModel` got `...006`; three containers at the same clock start gave the same ids |
| T10.1 order note | after the delete and re-add, `findNotes` and `findCards` listed the re-used id last |
| T10.1 error strings | `Note was not found: 1894701600099`, `model was not found: 1894701600099` |
| T10.3 | `notesInfo` and `notesModTime` `mod` equal the clock start (1894701600); `createModel` template and field ids are large random integers |

## Review fixes (2026-11-10, second pass)

Same session. Meguri decisions, no external source beyond the probes above:
retired tokens keep their entity's last real id for request substitution and
for error strings, live entities winning; `reused_ref` when a retired id was
reused by a live entity (the reference reuses freed ids, T19.3 probe); card
identity compared by note entity (T10.1); the `HEAD` exemption from the
Content-Length check (T8), following `servers/migaku-protocol` P4.
`conformance/oracle.json` was checked: it has no `ankiconnect_addon_sha256`
yet, so T9 names ADDON_PIN's `archive_sha256` as the current pin.

## Recorder issue pass (2026-11-10)

Same session. Issues reported by the oracle maintainer's recorder
(`oracle/ankiconnect/`, commit 5cae7aef, session s-20261110T114344Z-1cd5e318),
checked against the recorder's code (`recorder/__main__.py`,
`recorder/transcript.py`), its README and the 51 committed recordings under
`conformance/recordings/servers/ankiconnect/`. No AnkiConnect or Anki source
was read for this pass.

| Spec text | Source | Change |
|---|---|---|
| T1, T2, header | `conformance/AGENTS.md` (recordings rule), recorder README "Files" | transcripts live at `conformance/recordings/servers/<server>/<path>.yaml` with the golden `.json` beside it; ids unchanged; the recorder is `record.sh` until the `tools/mg` job exists |
| T5, T10.1 "Stock note types count" | recorded `migaku/destination-by-id` (`modelNamesAndIds`: stock kinds `#m1`-`#m6`, setup type `#m7`), `mpvacious/find-models-by-name` (`#m7`); `specs/storage/new-collection.md` B7, B11; `specs/notetypes/stock-kinds.md` K1, K2 | kept the numbering: Meguri's new collection already holds the six stock kinds at `now_ms`..+5 in K1 order, so no product decision is needed |
| T10.1 card numbering | recorder `Tokens._add_new` (ascending id per kind); `specs/conformance/normalized-dump.md` N5 | ascending card id, not template order; cloze caution from `conformance/AGENTS.md` |
| T10.1 retired tokens | T10.2 and T17 already state it; the recorder's `Tokens.real` raises `unknown_ref` for a retired token and its error-string lookup sees only live entities | spec kept; cross-reference added; recorder gap reported |
| T10.1 isolation marks | recorded `agent/find-cards` step 1 (`Invalid search: ` with U+2068/U+2069) | error strings keep them; a whole-string `replace` |
| T7.1, T14 | `specs/conformance/observations.md` O4 | `media` renamed `media_dir`; `media_trash`, `media_pending` named |
| T7.4 run errors | recorder `RunError` kinds (`permission_unused`, `clock_escaped`, `anki_exited`, `fixture_mismatch`, `fixture_unavailable`, `unsupported_step`, `invalid_scenario`) and the `ClockCheck` window | table of kinds; `invalid` status from `specs/conformance/dsl.md` D10 |
| T9 lock and driver hash | recorder `oracle_identity`, `recorder_sha256`, `RECORDER_FILES`; `oracle/driver/meta.py` `driver_files`; `oracle/ankiconnect/requirements.lock`; `conformance/oracle.json` | `ankiconnect_lock_sha256` added (the recorder does not write it yet); `driver_sha256` defined as the recorder computes it |
| T10.6 (new) | recorder `Normalizer.observation` (no id replacement); recorded `harness/tokens` dump (real keys, `mid`, `nid`, `did`); `specs/conformance/observations.md` O1, O4, O8; `specs/storage/graves.md` (grave types 0, 1, 2) | id positions in `db_dump` and `decks`; keys without a token kept as digits; tables limited |
| T16 "Record twice", T17 rows | recorder README "Clock" (libfaketime cache race, one recording in a dozen) | acceptance needs two byte-identical recordings |
| T18 `harness/tokens` | recorded `harness/tokens.yaml` | `{db_dump: {tables: [notes, cards]}}` (R1959) |

Meguri decisions made here: the run-error table; T10.6's positions, the
retired-first lookup for `graves`, digit keys for untokened ids, and the
table limit; the record-twice acceptance rule; the staleness of a golden
without `ankiconnect_lock_sha256`.
