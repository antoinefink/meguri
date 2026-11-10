# Graphs-page oracle image

AGPL quarantine (`LICENSE`). Oracle maintainers and spec writers only. The Meguri oracle plus Anki's desktop package aqt 26.9.3, for the observation `stats_presentation` (`specs/conformance/stats-ops.md` H14, `specs/stats/graph-presentation.md` P22). The driver module is `driver/observe/stats_presentation.py`. It is in the base image's driver but loaded only here (`ORACLE_EXTRA_MODULES`). Every other kind runs here as in the base image.

## How it works

- The page is the built graphs page in the aqt wheel (its sveltekit bundle). The image installs `../ankiconnect/requirements.lock` (`aqt[qt]==26.9.3` resolved on the base lock), which pins the wheel by hash, so the page is the pin's. It has no lock of its own: two web-server lines of that lock match the private scan and are approved only at that path. The page and its assets stay in this image (`stats/graphs-common` G12 decision 2, R1960). Goldens hold only the values the page shows.
- Per worker, the module starts a server on 127.0.0.1 and one runner (`driver/observe/stats_presentation_page/runner.py`). The server serves the bundle and answers the page's backend calls from the scenario's collection. The runner is one offscreen QtWebEngine view (PyQt6 from the same lock, no other browser). For each request the page loads fresh. The runner then evaluates `probe.js` (next to it) every 50 ms until it reports. Both sit in the driver, so `driver_sha256` and the base image tag cover them.
- The server answers `graphs` with the response the module computed for the request's search and days. It ignores the page's own search and days. `getGraphPreferences` answers with the collection's preferences, with `backlog`/`separate` replaced. `setGraphPreferences` is dropped. `i18nResources` goes through the backend. Any other call fails the request.
- The probe waits for the page to settle. It clicks `all history` for history `all` and waits for the second graph call. Then it clicks the graph's range radio by its English label. It reads the bins from the d3 data bound to the hover columns (edges, entries) and the no-data overlay's pointer state. For each drawn bin it sends a mouse move and reads the tooltip, then a click and reads the captured browser hand-off. It also reads the table rows. For Card Counts it reads the table and the pie's values. It computes nothing the page did not bind, except a bin's value: the sum of its entries' counts (P4.3 item 4).
- The page is English (the backend's default language). Its text is Anki's own: like `studied_today_text`, this kind is an exception to the no-message-text rule, because P22 compares text exactly.
- The runner's stdout carries only its JSON lines, and its stderr goes to /dev/null. Page console messages are dropped: Chromium and page logs can name Anki's sources, and worker stderr reaches implementer-readable logs. Failures come back as short codes in `internal_error` details.
- Clock: the runner keeps the worker's libfaketime with a 1 s cache (`FAKETIME_CACHE_DURATION=1`) instead of a read per call. Timers use the real monotonic clock. Only the Calendar reads the date in the page.

Speed: about 1 s per request after the first one, plus about 3 s to start the runner per worker.

## Graphs

| Graph | State |
|---|---|
| Future Due, Intervals, Stability, Difficulty, Retrievability, Added | read |
| Card Counts | read (`sectors`, no bins) |
| Reviews | not yet. Needs: the Time checkbox; its hover columns bind other data than the histograms (stacked day entries); its tooltip is a table, so lines become `[label, value]` pairs. P8 must give the bin value the probe can take from that data. |
| Hourly Breakdown, Answer Buttons | not yet. Needs: their own hover columns (24 hours; 12 button bars). The emoji removal of P21 for Hourly lines. A decision (spec writer) on what `x0`/`x1` mean for these bars: P13 and P14 do not define edges. |
| Retention | not yet. Needs: the Young/Mature/All picker and the table's header row. The value is `{table}` only. |
| Calendar | not yet. Needs: the year buttons, and the page's date at the scenario's clock (check the 1 s cache after `set_clock`). The value is `{no_data}` only. |

The module refuses the "not yet" graphs before any collection, as `unsupported_observation`, after checking their controls.

## By hand (no tools/mg job yet)

```
tools/mg oracle build                                    # prints meguri-oracle:<tag>
cd oracle
docker build -f graphs-page/Dockerfile --build-arg ORACLE_BASE=meguri-oracle:<tag> -t meguri-oracle-graphs-page:probe .
docker network create --internal mg-graphs-probe
docker run --rm --init --network mg-graphs-probe --tmpfs /scratch:exec,mode=1777 \
  -e TZ=UTC -e TMPDIR=/scratch -e ORACLE_SCRATCH_ROOT=/scratch \
  meguri-oracle-graphs-page:probe selftest test_stats_presentation
```

`harness record --scenarios /scenarios --goldens <scratch dir> --filter stats/presentation` can run there for a probe, with the scenarios mounted read-only. Never write `conformance/goldens/` that way: goldens come only from `tools/mg oracle record`. On 2026-11-10 two such runs of the eight readable P23 scenarios gave the same bytes. They matched the worked examples of P6, P10, P11 and P15 and the counts in the scenarios' headers.

A pin bump regenerates `../ankiconnect/requirements.lock` (`ankiconnect/README.md`), and this image follows it.

## What a tools/mg job needs

- `oracle record` and `compare` (and `conformance --live`): route a selection that holds a scenario observing `stats_presentation` to this image, as the heatmap add-on image is routed. Build this Dockerfile (context `oracle/`, `-f graphs-page/Dockerfile`, `--build-arg ORACLE_BASE=meguri-oracle:$MG_ORACLE_TAG`, default network for apt and pip) as `meguri-oracle:$MG_ORACLE_TAG-graphs-page-<16 hex of a hash over the Dockerfile and ../ankiconnect/requirements.lock>`. The runner and probe are in the base tag already. A selection holding both heatmap and presentation scenarios must be split by image, or one image must stack both.
- The golden's `oracle` member: `lock_sha256` is the base lock's and `driver_sha256` covers the runner and probe. The aqt lock also decides the values (the page is in the aqt wheel). Like the AnkiConnect recorder's `ankiconnect_lock_sha256`, the golden format needs a member for it (a spec change, `conformance/dsl` D11 and `golden.schema.json`), or a rule that this lock moves only with the base pin, before goldens are committed.
- `oracle test`: build the image and run `selftest test_stats_presentation` in it after the base run (the base run skips those tests).
- `--platform apple`: refuse these scenarios (there is no native lane for the page).
- No user step: everything comes from the pinned wheels and Debian packages.
- With the first golden, the pending `conformance/deviations.yaml` entry `stats-presentation-text-marks` (P21).
