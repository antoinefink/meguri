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
