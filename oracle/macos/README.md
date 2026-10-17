# macOS native oracle lane (KTD24)

Spec side, AGPL quarantine like the rest of `oracle/`. The behavior this lane must have is `specs/conformance/float-determinism.md` (F5–F7); this file says how the oracle maintainer builds and runs it. Implementers never read it or the venv: they use the lane only through `tools/mg oracle ... --platform apple` and the overlays it writes.

## What runs

The same driver as the container (`oracle/driver/`, so the same `driver_sha256`), speaking the same JSON-lines protocol (`conformance/schema/protocol.json`), but natively on the Mac with the `anki` 26.9.3 macOS arm64 wheel. Anki's FSRS code then uses Apple's math library, which is the point of the lane: it shows what Anki computes on a Mac, so that Meguri-on-Mac can be held to it and the Linux goldens' 1-ulp boundary cases can be recorded as overlays.

It is the one exception to Docker-first tooling, and it runs only on an arm64 Mac.

## Files

| File | Role |
|---|---|
| `requirements-macos.lock` | `anki==26.9.3` and its dependencies for macOS arm64, every wheel pinned by hash. Made by `tools/mg oracle lock --platform apple`: uv in the oracle's base image, `--python-platform aarch64-apple-darwin`, constrained to `oracle/requirements.lock`, so both lanes run the same versions (only `distro`, Linux-only, is absent). Its SHA-256 is `conformance/oracle.json`'s `macos_lock_sha256` and every overlay's `oracle.lock_sha256`. |
| `run-native.sh` | `bootstrap`, `worker`, `hello [--tz Z]`, and `harness <args>` (the harness natively: `record-apple`, `compare-apple`, `time`; its workers are `run-native.sh worker`). |
| `README.md` | this file |

`bootstrap` makes `build/oracle-macos-venv/` in the checkout that holds this `oracle/` (an implementer worktree's `tools/mg` uses the main checkout's):

| Path | What | Pinned by |
|---|---|---|
| `python/` | python-build-standalone CPython 3.12.15 (the container's version), `aarch64-apple-darwin` install_only_stripped, release 20261001 | URL and sha256 in `run-native.sh` |
| `faketime/libfaketime.1.dylib` | libfaketime 0.9.13 built from its source tarball with the Mac's clang (`Makefile.OSX`: arm64 and arm64e, dyld interposing) | URL and sha256 in `run-native.sh` |
| `venv/` | `pip install --require-hashes --no-deps --only-binary :all: -r requirements-macos.lock` | the lock |
| `stamp` | the three hashes the lane was built from | |
| `downloads/` | the checked archives | |

`worker` and `harness` refuse a lane whose `stamp` differs from the current pins (exit 78, "run bootstrap"); `bootstrap` rebuilds only what changed and holds `build/oracle-macos-venv.lock` while it works. `tools/mg oracle ... --platform apple` runs `bootstrap` first.

## Rules

- **Python.** The pinned standalone CPython, never `/usr/bin/python3`: System Integrity Protection strips `DYLD_*` variables from protected binaries (`/bin/bash` and `/usr/bin/env` too, so `run-native.sh` sets them itself just before it `exec`s the venv's interpreter). Python runs with `PYTHONNOUSERSITE`, `PYTHONSAFEPATH` and `PYTHONDONTWRITEBYTECODE` (no `__pycache__` in `oracle/driver/`).
- **Pins.** The lock moves only with an oracle bump (`tools/mg oracle bump` stages it next to `requirements.lock` and puts its hash in the staged `oracle.json`), in the same change as `oracle/requirements.lock`, `conformance/oracle.json` (both hashes), every golden and every overlay. Python and libfaketime move by editing their pins in `run-native.sh`; nothing is installed system-wide and nothing needs a user step.
- **Time control** (F6.3). Each worker starts with `DYLD_INSERT_LIBRARIES=<venv root>/faketime/libfaketime.1.dylib`, `DYLD_FORCE_FLAT_NAMESPACE=1`, `FAKETIME_TIMESTAMP_FILE=<worker scratch>/clock`, `FAKETIME_NO_CACHE=1`, `FAKETIME_FMT=%s` and `DONT_FAKE_MONOTONIC=1`. The worker's start-up check (`driver/timecontrol.py`, both lanes) freezes the clock at 2030-01-15T10:00:00Z, checks Python's `time.time()`, a new collection's `crt` and Anki's scheduler day, moves it one day and checks again, then restores the clock file. It answers `time_control: "faketime"` only when all checks pass, otherwise `"none"` and runs on the real clock (a container worker refuses to start instead). Never set `ANKI_TEST_MODE` (it turns fuzz off and shifts Anki's clock). `ORACLE_NATIVE_FAKETIME=0` starts a worker without libfaketime, to exercise the `none` path. A timing worker (`ORACLE_CLOCK_CACHE=1`, `tools/mg oracle time`) gets `FAKETIME_CACHE_DURATION=2147483647` instead of `FAKETIME_NO_CACHE`: libfaketime reads the clock file once, which the harness wrote before starting it, and its start-up check only confirms the clock stands at that instant (`timed-operations.md` T2.3). Here a per-read file access costs 14 to 30 µs per clock read against under 1 µs cached, enough to double some of Anki's timed operations (FSRS recompute 220 to 730 ms against 95 ms on the 10k fixture).
- **Timed waits hang under faketime here.** On macOS libfaketime does not translate `pthread_cond_timedwait` deadlines, and Python computes them from the faked wall clock: under a clock frozen in 2030, `Lock.acquire(timeout=)`, `Event.wait(t)` or `queue.get(timeout=)` waits until the real clock reaches 2030. `time.sleep`, `select`/`poll` and subprocess waits are fine (relative). Never add a timed lock or event wait to a worker.
- **hello** answers `platform: "macos-arm64"` and the SHA-256 of `requirements-macos.lock` as `lock_sha256` (protocol.json `hello_response`). Linux workers answer `linux-arm64` or `linux-x86_64` and `time_control: "faketime"`.
- **Scratch.** Each worker has its own scratch directory (and clock file): under the job's run directory for `tools/mg`, `build/oracle-macos-scratch/` by hand; never `/tmp` shared between workers. SQLite stays on the local disk.
- **Clock-free only without faketime.** On a `none` worker the harness runs only scenarios tagged `clock-free` and reports the others as `clock_unsupported` (not run, overlay untouched).
- **No sync scenarios.** Actor scenarios need the sync-faults proxy on the oracle network, which the native lane does not have; they error there. `time`'s full syncs run against its in-process loopback stub and work on both lanes.

## tools/mg

- `tools/mg oracle hello --platform apple [--tz Z]`: bootstrap when needed, start one worker, report its hello (`details.hello`); `fail` when it is not the pinned oracle (platform, `anki_version`, `macos_lock_sha256`), `warn` when it has no faketime.
- `tools/mg oracle record --platform apple [--jobs N] <filter>`: `run-native.sh harness record-apple`, writing `conformance/goldens-apple/<id>.json` per F7.2: needs fresh Linux goldens; the run requests set `fsrs_context: true`; observations are compared with the Linux golden value by value; every differing scalar becomes an entry in pointer order, with `unrounded` for day entries (F8.2); no overlay when nothing differs (an old one is deleted); nothing written on a structural difference. Entries outside the `fp-libm-boundary` class are listed in the report (`scenarios[].outside_class`), and make the result `fail`. `details.overlays` and the report's `overlay_totals` give the counts the probe needs (inputs, entries by kind, differing rows, days rate).
- `tools/mg oracle compare --platform apple [--jobs N] <filter>`: `run-native.sh harness compare-apple`: a native run against golden plus overlay, exact, no deviation applied; a stale overlay (F7.3) is `stale_overlay`. `tools/mg fp-probe` uses it to confirm its Apple side (F10.1).
- `tools/mg oracle time <dir> --platform apple [--op N]... [--now T] [--tz Z] [--repeat N]`: `run-native.sh harness time` (`specs/conformance/timed-operations.md`), the primary series of `tools/mg perf-ratio`: one timing worker, its scratch under the run directory, or under `build/private/` when `<dir>` is there; `timing.json` with this lane's lock as `oracle.lock_sha256`. A worker without faketime is `clock_unsupported`. Its sync operations use the driver's loopback stub, not the sync-faults proxy.
- `tools/mg oracle lock --platform apple`: regenerate `requirements-macos.lock` (adopting it is a bump).
- The lane copies nothing out of `oracle/`: it runs the driver in place. The venv holds Anki's Python sources and is covered by the implementer deny rule (`build/oracle-macos-venv/**`).

## Driver support this lane needs

- The `fsrs_next` observation (`driver/observe/fsrs_next.py`, F5.4): a per-worker scratch collection like `observe/text.py`'s, presets and decks keyed by (parameter values, `dr`, `max_ivl`), one card rewritten per input, `lrt` from the scheduler's `next_day_at` with a re-evaluation when the day changes in between. It serves both lanes.
- `fsrs_context` in run requests (`driver/observe/sched.py`): for every card of every `next_states` observation, the home deck's effective desired retention and the decay its preset's parameters give (`sched/fsrs-transitions` T2.1, T2.2).
- `platform` and `time_control` in `hello` (`driver/meta.py`, `driver/timecontrol.py`), on both lanes.
- Overlays (`driver/overlay.py`, harness `record-apple` and `compare-apple`): the value-by-value diff, the F8 class, unrounded intervals, freshness and application.
