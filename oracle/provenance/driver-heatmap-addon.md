# Oracle driver: heatmap add-on observations (U95, U96)

Read to build `heatmap_totals` and `heatmap_report`
(`driver/observe/heatmap_addon.py`, `heatmap/Dockerfile`; meanings in
`specs/conformance/stats-ops.md` H1 and H11). No code was copied into Meguri;
the module calls the add-on's own functions. Session
s-20261110T114344Z-1cd5e318 (oracle maintainer), resuming the U95 driver work
of `spec/U95` (`ops/heatmap_addon.py` there, written against a local AnkiWeb
copy; never merged).

## Source

The public archive of tag `v1.0.1` of github.com/glutanimate/review-heatmap
(commit `5ab7964ac5bbab33358ef9cd5dd4f31517f43e48`), pinned in
`heatmap/Dockerfile` (the archive) and `driver/observe/heatmap_addon.sha256`
(a digest over the installed files, without their paths: some upstream file
names match the private scan's tokens). Its 99 files under
`src/review_heatmap/` were compared on 2026-11-10 with the U95 pin of the
AnkiWeb build (manifest `v1.0.1`): every one is byte for byte the same. The
AnkiWeb build adds only generated or packaging files (Qt forms, icon
resources, `manifest.json`, the bundled `web/anki-review-heatmap.js`, a
credits module under `data/`), none of which the computation imports;
`consts.py` falls back to empty credits without that module.

| Add-on file (tag v1.0.1, `src/review_heatmap/`) | Read for |
|---|---|
| `activity.py` | `get_report`'s order (time limits, history query, forecast query from `_today`, `_get_activity`); the history query keyed by 00:00 UTC of the local date of `id / 1000 - rollover * 3600`; `limresched`, `limcdel`, `limdecks` and the deck screen's current-deck subtree; the forecast query (`did`, queues 2 and 3, `due` against `sched.today`, keys from `today` to before `stop`, counts negated); `_get_conf_history_limit` (`limhist` from today, `limdate` as a local date, the creation-date rule, the later bound); `_get_conf_forecast_limit` (0 is 73,000 days); `_get_activity` (streaks on the query keys, `round` to even, the history-over-forecast rule for today, `stop` as the last forecast key); no report at all when the history is empty |
| `times.py` | `daystart_epoch`: SQLite `localtime` and `start of day`, read back as UTC epoch seconds |
| `config.py`, `consts.py`, `libaddon/anki/configmanager.py`, `libaddon/utils.py` | the synced storage under the config key `heatmap`, its defaults and version; the key stored on first read when absent, merged and stamped when its version is missing or older; `deepMergeDicts` keeps a default whose JSON type differs from the stored member |
| `libaddon/platform.py`, `libaddon/consts.py`, `libaddon/debug.py` | what the computation modules need from `aqt.mw` at import (the add-ons folder) and from `set_addon_properties` |
| `renderer.py`, `views.py`, `controller.py`, `__init__.py` | which screens call `get_report` with `current_deck_only` (deck browser False, overview True); that the report's `stop`, `today` and activity dict go to the web view; the streak tints computed in Python (`_stats_formatting`, `_generate_stats_elm`) while cell levels are applied by the calendar in the web view |
| `web/anki-review-heatmap.js` of the AnkiWeb build (U95, `spec/U95` provenance) | the drawn date: a key moved back by the zone's offset at that instant, then its local date (`drawn_date`) |

| Anki file (26.09.3) | Read for |
|---|---|
| `pylib/anki/config.py` | `col.conf` reads the backend at each access; dicts it hands out are written back when changed; `get_immutable` raises `KeyError` for an absent key (the overlay mirrors this without writing) |
| `pylib/anki/decks.py` | `get_current_id`, `children`, `deck_and_child_ids`, `all_names_and_ids` |
| `pylib/anki/collection.py` | `v3_scheduler`, `setMod` (a no-op) |

## Probe results (2026-11-10, not goldens)

The 60 held scenarios ran once through `harness record` in the add-on image,
with goldens written to a scratch directory and discarded (`heatmap/README.md`).
These are for the spec writer; goldens wait for `tools/mg oracle record`.

- Agree with their spec tables: every `stats/day-buckets/` scenario (B7,
  including the oracle values named for `future-row` and `lord-howe-dst-start`),
  every `stats/heatmap-totals/` scenario (N9, including `filtered-deck-card`'s
  2/1/1 and `read-only`'s equal dumps), `heatmap-config/writer-output`, and
  `heatmap/` `forecast-queues`, `forecast-scope`, `forecast-overdue-only`,
  `today-cell`, `today-dst-shift`, `limdate-unit`, `limdate-creation`,
  `limhist-and-limdate`, `no-history-forecast`; `streaks/` all but `future-row`.
- `streaks/lord-howe-dst-start`: the history is drawn shifted
  (`{10-02: 1, 10-03: 2}`) but the streaks equal Meguri's (1, 75, 3, 3): the
  add-on computes streaks on its keys (A15 item 13 settled: only the history
  members need `heatmap-drawn-day-shift`).
- `streaks/future-row`: the add-on raises `ZeroDivisionError` (Days learned
  divides by `today - first day + 1`, which is 0 when the only active day is
  T + 1): no report, a run error `internal_error`. The scenario cannot get a
  golden as written.
- `heatmap/limhist-edge`: `limhist` 90 keeps T − 90 too (the window is the
  last `limhist + 1` dates; A15 item 2 says `limhist`).
- `heatmap/forecast-limfcst`: `limfcst` 30 keeps k < 30 (due +30 is out), and
  `last_forecast_day` is the last date with a count (2026-10-22), not T + 30.
- `heatmap/forecast-cap`: `last_forecast_day` is the last date with a count
  (2026-10-17), not T + 73,000.
- `heatmap/limits-odd`: negative `limhist` and `limfcst` (−5) apply: the
  history window starts at T + 5 and the forecast ends before T − 5, so both
  are empty (A15 item 4 says they are not set).
- `heatmap/limdate-fraction`: the history is unbounded both times. The key has
  no `version`, so the add-on's merge keeps the default `limdate` 0 for a
  float value (type mismatch); a fraction is never read as a date.
- `heatmap/forecast-filtered`: every forecast is empty (the add-on reads `due`
  and `did`; the scenario's filtered cards hold negative `due` values, which
  read as overdue).
  `forecast-filtered-day0`: Home counts 2 on T (the new card at position 0 in
  the filtered deck too), H's page 1.
