# AnkiConnect transcript recorder (U136)

AGPL/GPL quarantine (LICENSE). Oracle maintainers and spec writers only. It runs Anki 26.9.3's desktop app (aqt) offscreen, under the oracle's libfaketime clock, with the AnkiConnect add-on and the Meguri driver add-on `meguri_recorder`. The recorder (`recorder/`) takes one transcript scenario (`specs/servers/transcripts`) and writes its golden (T9) with T10's normalization. It has no `tools/mg` job and no compose service yet (see "What a tools/mg job needs").

## Pins

- Base: `FROM ${ORACLE_BASE}`, a `meguri-oracle:<tag>` from `tools/mg oracle build`. That gives the base lock's `anki` 26.9.3, libfaketime, the `oracle` user and the driver under `/opt/oracle`, which the add-on uses for setup and observations (read-only: never edit the driver for the recorder).
- `requirements.lock`: `aqt[qt]==26.9.3` (PyQt6 6.11.0, Qt 6.11.2), resolved with `--constraint ../requirements.lock`, so every package the base already has keeps the same version. Regenerate it from `oracle/` with:
  `docker run --rm --init -v "$PWD:/lock" -w /lock/ankiconnect <base python image of ../Dockerfile> sh -c "pip install -q uv==0.12.24 && uv pip compile --quiet --no-annotate --generate-hashes --python-version 3.12 --python-platform x86_64-manylinux_2_40 --constraint ../requirements.lock --custom-compile-command 'see oracle/ankiconnect/README.md' requirements.in -o requirements.lock"`
- `ADDON_PIN`: upstream `git.sr.ht/~foosoft/anki-connect`, tag `25.11.9.0`, commit `532a3c816e08e91c4367e2799ce3779403d70274`, build date `2025-11-09` (`oracle.addon.build`). The source archive is pinned by sha256 in the Dockerfile's `ADD --checksum` (goldens record it as `oracle.addon.sha256`), and the six installed `plugin/` files are pinned by sha256 in ADDON_PIN. The archive URL and hash appear in both files: change them together. It is installed as `addons21/2055492159`, its AnkiWeb id.

## Build, test, record

```sh
tools/mg oracle build                       # prints meguri-oracle:<tag>
cd oracle/ankiconnect
docker build --build-arg ORACLE_BASE=meguri-oracle:<tag> -t meguri-oracle-ankiconnect:recorder .
docker run --rm --network none meguri-oracle-ankiconnect:recorder test     # recorder unit tests, no Anki
cd ../..
oracle/ankiconnect/record.sh meguri-oracle-ankiconnect:recorder conformance/recordings/servers/ankiconnect/harness/*.yaml
```

- `record.sh <image> <scenario.yaml>...` runs each scenario in its own fresh container (T16) on a new `--internal` network with no published port, `conformance/fixtures` mounted read-only at `/fixtures` (`ORACLE_FIXTURES`), and writes `<name>.json` next to `<name>.yaml`. The id is the path under `conformance/recordings/` without the suffix. It removes its container and network on exit.
- By hand: `docker run --rm -i --init --network <internal net> --tmpfs /scratch:mode=1777,exec -e ORACLE_ANKICONNECT_IMAGE=<tag> <image> record --id <id> < scenario.yaml > golden.json`. Exit 0 with the golden on stdout; 1 for a run error, 2 for an invalid scenario, with `{"error": {kind, ...}}` on stderr. stderr also carries `{"warning": "id_left_in_place", step, pointer}` lines (T10.1). Only recorder-authored JSON reaches either stream: Anki's output goes to `/scratch/anki.log` and dies with the container. `ORACLE_FULL_TRACEBACKS=1` appends that log's tail to stderr on a failure: by hand only, never in a job (localized text, Anki paths).
- `docker cp` cannot write into the tmpfs `/scratch`; probe with `docker exec` (AnkiConnect binds `127.0.0.1:8765` inside the container). Never publish a port.

## Files

- Inputs and goldens live at `conformance/recordings/servers/ankiconnect/<path>.yaml` and `.json` (scenario id `servers/ankiconnect/<path>`). T2's `conformance/scenarios/servers/` and `conformance/goldens/servers/` are not usable yet: the DSL suite loads every YAML under `scenarios/` and goldens-check every golden under `goldens/`. Move them to T2's paths once those skip `servers/`.
- The scenario format is T3 exactly (YAML 1.2 core schema, read by `driver.canon`, hashed as D3). The recorder runs `source: oracle`, `server: ankiconnect` only, and refuses declared-only members and steps as `invalid_scenario`.

## How a recording runs

1. `recorder/__main__.py` checks the scenario, writes the clock file at `clock.start` and starts `entrypoint.sh anki` in `clock.tz` with the add-on config of T4 (`ORACLE_ANKICONNECT_CONFIG`; `launch.py` writes it into the add-on's meta.json, which AnkiConnect reads on every request). The recorder itself runs on real time.
2. It waits for the add-on's `ready`, for a LISTEN socket on 8765 (`/proc/net/tcp`, no probe connection) and for Anki's clock to read `clock.start`.
3. Control channel (`addons21/meguri_recorder`): the recorder writes `/scratch/recorder/request.json`; the add-on's 50 ms main-thread timer runs it and writes `response-<id>.json`. Ops: `setup` (the driver's setup sections on a `Session` built around `mw.col`; session resources such as the `http_fixtures` server start too, and its origin is `${fixture_base}`), `snapshot` (ids, identities and mods of notes, cards, decks, note types; media folder, profile name, the TTS message of render-ops Q5.4), `observe` (the driver's observations; one that would reopen the collection is refused), `now` and `gui` (whether the Browse window, when open, has run the search its search box shows, with no editor save pending; `browser_will_search` records the box's text at each search). The timer skips control requests while a modal dialog is up.
4. Steps: before an exchange the recorder polls `gui` until it reports settled (T16; at most 10 s, then `timeout`): AnkiConnect's `guiBrowse` replies before Anki runs its search, which waits for the editor's save. An exchange is raw bytes on a fresh socket (T6.2, T6.3), read while writing (T6.4), then a snapshot, the tokens (T10.1: notes first, then cards by note token and ord, decks, note types; a gone or changed entity is retired and keeps its last id), the normalization (T10.3, then T10.1 at T10.5's positions and in error strings, live entities first, then retired ones). A retired token in a request sends its last id, or fails as `reused_ref` once a live entity of its kind has that id. `permission` appends to `/scratch/recorder/permission`, which the add-on's timer consumes when the dialog is up; a dialog with nothing queued writes `prompt_waiting` and the exchange fails as `prompt_not_answered`; a queued answer left at the end is `permission_unused`. `interaction` outputs `{}` (Meguri's side only, T7.5). `media_dir_write` writes into Anki's media folder and stamps the file and the folder with the scenario clock. `advance` writes the clock file and waits until Anki's clock reads it.
5. The golden: T9's members, `ankiconnect_lock_sha256` the hash of this image's `requirements.lock`, `driver_sha256` T9's line list (`transcript.recorder_sha256`) over the driver's files and the recorder's own (`recorder/` without tests, the add-on, `launch.py`, `entrypoint.sh`), text as conformance/dsl D11 (`canon.golden_text`).

## Clock

- libfaketime 0.9.10 with `FAKETIME_NO_CACHE` races in Anki's many threads: one recording in about a dozen gave a setup card a real-time id. The recorder starts Anki with `ORACLE_FAKETIME_CACHE_SECS=1` (`FAKETIME_CACHE_DURATION=1`: libfaketime re-reads the file only once the whole-second difference exceeds 1 s), so `advance` waits 1 to 3 s for the `now` op to see the new instant.
- Every snapshot is checked: a new id (at least 1,000,000,000) outside `[now ms, now ms + 10^7)`, or a note or card `mod` outside `[clock.start, now]`, fails the run as `clock_escaped`. 24 parallel recordings of `clients/migaku/card-creation` after the change were byte-identical with no escape. Record twice and compare before committing a golden (T17).

## Proven (2026-11-10, arm64 Docker Desktop)

- `harness/version`, `harness/ping`, `harness/tokens` (T18) and `clients/migaku/card-creation` recorded; each re-recorded minutes later byte-identical. tokens shows setup notes as `#n1`, `#n2`, cards `#c1`…, the new note `#n3`, `mod` `@0`.
- Re-recorded after T10.6 and retired tokens (2026-11-10): every `harness/` and `clients/` golden except `clients/migaku/` (held), each twice in fresh containers and byte-identical. `clients/mpvacious/update-last-card` was not: its `guiSelectedNotes` read `[]` in 1 run of 8 (the browser's search not yet run, C9.12); fixed by the `gui` wait below. One run of `clients/asbplayer/create-model-minimal` failed as `clock_escaped` (a real-time note id) and was recorded again as a pair.
- The `gui` wait (2026-11-10): right after `guiBrowse` the op read unsettled (box `nid:<id>`, last search `deck:current`) and settled on the next poll. `clients/mpvacious/update-last-card` then recorded 22 times in fresh containers (a pair, 10 in parallel, 8 in sequence, then the pair of the full re-record), all byte-identical, `guiSelectedNotes` `["#n2"]`. Every other `harness/` and `clients/` golden except `clients/migaku/` was re-recorded twice for the new `driver_sha256`, byte-identical.
- Probed, not committed: `unknown_ref` for `#n9` with two notes (T17); `prompt_not_answered`; `deny` then `allow` through `requestPermission`; a note added after a delete gets the deleted id and the next token; `advance 1h` gives `mod` `@3600`; `#rand`, `<media-dir>`, `<profile>`, `#d1` in `findModelsByName`.

## Not done, and traps

- Not recorded yet: `async` exchanges (`unsupported_step`). Of Anki's windows, only the Browse window's pending search is waited for; another window state a later request reads (an Add or Edit window's note) would need its own `gui` check. `${fixture_base}` comes from the driver's fixture server inside Anki's process; no recording uses it yet.
- `observe` outputs get T10.6's tokens after the driver's own normalization: `db_dump` row keys and id columns, `decks` `/<name>/id`; a row key with no token stays decimal, a `graves` `oid` looks among retired entities first. Other kinds and columns keep real ids, so transcripts dump only `notes`, `cards`, `revlog`, `graves`, `decks` and `tags`.
- At startup Anki tries its update and add-on checks; on the internal network both fail quietly. mpv and mplayer are absent (audio actions cannot play).
- The add-on runs its permission dialog with `exec()`: a dialog with no answer holds every request, the ping included.
- An amd64 build is untested.

## What a tools/mg job needs

`tools/mg oracle record servers/ankiconnect[/<filter>]` (T16) would: build the base (`oracle build`), then this image with a content-hash tag over `oracle/ankiconnect/` and the base tag; select the scenarios; run each in a fresh container as `record.sh` does (unique `--internal` network, tmpfs `/scratch` with `exec`, fixtures read-only, `ORACLE_ANKICONNECT_IMAGE` set to the tag); write the golden atomically from stdout; copy the recorder's stderr JSON lines (and nothing else) into the job log; record twice and fail on any byte difference; and report stale goldens against `conformance/oracle.json`, which needs `ankiconnect_addon_sha256`, `ankiconnect_lock_sha256` and `ankiconnect_image` (T9; until then the pins are `ADDON_PIN`'s `archive_sha256` and this folder's `requirements.lock`). `tools/mg oracle test` would add `entrypoint.sh test`.
