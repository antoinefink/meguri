# AnkiConnect recorder image (U136 spike)

AGPL/GPL quarantine (LICENSE). Oracle maintainers and spec writers only. It runs Anki 26.9.3's desktop app (aqt) offscreen, under the oracle's libfaketime clock, with the AnkiConnect add-on and a Meguri driver add-on. This is the spike from U136's execution note. It has no `tools/mg` job, no compose service and no recorder protocol yet.

## Pins

- Base: `FROM ${ORACLE_BASE}`, a `meguri-oracle:<tag>` from `tools/mg oracle build`. That gives the base lock's `anki` 26.9.3, libfaketime and the `oracle` user. The driver under `/opt/oracle` is there but unused. For later recorder ops that run inside Anki, add modules and list them in `ORACLE_EXTRA_MODULES` (oracle/AGENTS.md).
- `requirements.lock`: `aqt[qt]==26.9.3` (PyQt6 6.11.0, Qt 6.11.2), resolved with `--constraint ../requirements.lock`, so every package the base already has keeps the same version. Regenerate it from `oracle/` with:
  `docker run --rm --init -v "$PWD:/lock" -w /lock/ankiconnect <base python image of ../Dockerfile> sh -c "pip install -q uv==0.12.24 && uv pip compile --quiet --no-annotate --generate-hashes --python-version 3.12 --python-platform x86_64-manylinux_2_40 --constraint ../requirements.lock --custom-compile-command 'see oracle/ankiconnect/README.md' requirements.in -o requirements.lock"`
- `ADDON_PIN`: upstream `git.sr.ht/~foosoft/anki-connect`, tag `25.11.9.0`, commit `532a3c816e08e91c4367e2799ce3779403d70274`. The source archive is pinned by sha256 in the Dockerfile's `ADD --checksum`, and the six installed `plugin/` files are pinned by sha256 in ADDON_PIN. The archive URL and hash appear in both files: change them together. It is installed as `addons21/2055492159`, its AnkiWeb id.

## Build and probe

```sh
cd oracle/ankiconnect
docker build --build-arg ORACLE_BASE=meguri-oracle:<tag> -t meguri-oracle-ankiconnect:spike .
docker network create --internal meguri-ankiconnect-spike
docker run -d --init --name ac-spike --network meguri-ankiconnect-spike \
  --tmpfs /scratch:mode=1777,exec -e ORACLE_CLOCK_START=1894701600 -e TZ=Europe/Paris \
  meguri-oracle-ankiconnect:spike
# ready when /scratch/recorder/ready exists (2-10 s)
docker exec ac-spike python3 -c 'import urllib.request as u; print(u.urlopen(u.Request("http://127.0.0.1:8765", b"{\"action\":\"version\",\"version\":6}")).read())'
docker rm -f ac-spike; docker network rm meguri-ankiconnect-spike
```

AnkiConnect binds `127.0.0.1:8765` inside the container. Probe it with `docker exec`. Never publish a port. `docker cp` cannot write into the tmpfs `/scratch`: pipe scripts in with `docker exec -i ... python3 -I - < script.py`.

## How it starts

- `entrypoint.sh` uses the base's clock setup: a frozen clock file (`ORACLE_CLOCK_START`, epoch seconds, default 2000-01-01), `FAKETIME_NO_CACHE`, real monotonic time. It also sets `QT_QPA_PLATFORM=offscreen` and `QTWEBENGINE_DISABLE_SANDBOX=1`, and passes `--disable-gpu` to Chromium. `ORACLE_NO_FAKETIME=1` runs on real time.
- `launch.py` seeds a new base folder through aqt's `ProfileManager`: language `en_US`, profile `oracle`, the software video driver, and the add-ons. It then calls `aqt.run()`. Without the seed, Anki's first run blocks on its modal language dialog.
- `addons21/meguri_recorder` writes `/scratch/recorder/ready` on `profile_did_open`. A 50 ms main-thread QTimer answers the add-on's permission QMessageBox from `/scratch/recorder/permission`, one answer per line (`allow`, `deny` or `deny_ignore`), and logs each answer to `/scratch/recorder/answered`.

## What works (2026-11-10, arm64 Docker Desktop)

- Anki starts offscreen with the frozen clock. Its log shows `2030-01-15 11:00:00` for TZ Europe/Paris. AnkiConnect loads.
- `{"action":"version","version":6}` returns 200, `Content-Type: application/json`, `Access-Control-Allow-Origin: http://localhost`, `Access-Control-Allow-Headers: *`, and `{"result": 6, "error": null}`.
- An empty GET returns `{"apiVersion": "AnkiConnect v.6"}`. Both match T18.
- Permission dialog (T19.1): `allow`, `deny` and `deny_ignore` give the expected replies. The add-on adds the origin to `webCorsOriginList` or `ignoreOriginList` in its meta.json. A second request from the ignored origin is denied with no dialog.
- Ids (T19.3): two fresh containers at the same clock start gave byte-identical results for createDeck, addNote ×2, addNotes, findNotes, notesInfo, findCards, deckNamesAndIds, modelNamesAndIds, deleteNotes and addNote.

## What does not, or is not done

- A prompt with no queued answer blocks the whole server: the add-on runs the dialog with `exec()`, so every later request waits. The recorder must queue each answer before its exchange, and must treat a visible dialog with no answer as `prompt_not_answered`.
- Under a frozen clock, every id is the clock's milliseconds plus the lowest free offset: the stock note types, the first deck, note and card all get `1894701600000`. A note added after a delete gets the deleted note's id back.
- At startup Anki tries its update check and the AnkiWeb add-on check. On the internal network both fail. They log a network error but show no dialog. mpv is absent, so audio falls back to mplayer, which is not installed either.
- Anki's stderr holds localized message text and Anki source paths. A future `tools/mg` job must filter it as oracle/AGENTS.md says before it reaches implementer-readable logs.
- Not done yet: the recorder protocol, setup through the driver add-on, header and body capture as T6.2/T8 define them, the compose service, `tools/mg oracle record servers/ankiconnect`, an amd64 build, and `ankiconnect_addon_sha256`/`ankiconnect_image` in conformance/oracle.json.
