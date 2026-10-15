# macOS native oracle lane (KTD24)

Spec side, AGPL quarantine like the rest of `oracle/`. The behavior this lane must have is `specs/conformance/float-determinism.md` (F5–F7); this file says how the oracle maintainer builds and runs it. Implementers never read it or the venv: they use the lane only through `tools/mg oracle ... --platform apple` and the overlays it writes.

## What runs

The same driver as the container (`oracle/driver/`, so the same `driver_sha256`), speaking the same JSON-lines protocol (`conformance/schema/protocol.json`), but natively on the Mac with the `anki` 26.9.3 macOS arm64 wheel. Anki's FSRS code then uses Apple's math library, which is the point of the lane: it shows what Anki computes on a Mac, so that Meguri-on-Mac can be held to it and the Linux goldens' 1-ulp boundary cases can be recorded as overlays.

It is the one exception to Docker-first tooling, and it runs only on an arm64 Mac.

## Files

| File | Role |
|---|---|
| `requirements-macos.lock` | `anki==26.9.3` and its dependencies for macOS arm64, every wheel pinned by hash (made with uv for the `aarch64-apple-darwin` platform from `oracle/requirements.in`). Its SHA-256 is `conformance/oracle.json`'s `macos_lock_sha256` and every overlay's `oracle.lock_sha256`. |
| `run-native.sh` | `bootstrap`: make `build/oracle-macos-venv/` with a pinned Python and `pip install --require-hashes --no-deps -r requirements-macos.lock`, and build or fetch the pinned libfaketime; `worker`: start one worker on stdin/stdout (`TZ` and the `ORACLE_*` variables as for the container); `hello`: one worker's hello. |
| `README.md` | this file |

## Rules

- **Python.** A pinned standalone CPython (for example uv's managed 3.12, the container's minor version), never `/usr/bin/python3`: System Integrity Protection strips `DYLD_*` variables from protected binaries, and a hardened-runtime build may too. `bootstrap` records the interpreter path and version in the venv.
- **Pins.** The lock moves only with an oracle bump (`tools/mg oracle bump`), in the same change as `oracle/requirements.lock`, `conformance/oracle.json` (both hashes), every golden and every overlay. libfaketime is pinned by version and hash like any other input; nothing is installed system-wide and nothing needs a user step.
- **Time control** (F6.3). Start each worker with `DYLD_INSERT_LIBRARIES=<venv>/lib/libfaketime.1.dylib`, `DYLD_FORCE_FLAT_NAMESPACE=1`, `FAKETIME_TIMESTAMP_FILE=<worker scratch>/clock`, `FAKETIME_NO_CACHE=1`, `FAKETIME_FMT=%s` and `DONT_FAKE_MONOTONIC=1`. The worker's start-up self-test freezes the clock, checks Python's `time.time()` and Anki's scheduler day against it, moves it one day and checks again; it answers `time_control: "faketime"` only when all checks pass, otherwise `"none"` and runs on the real clock. Never set `ANKI_TEST_MODE` (it turns fuzz off and shifts Anki's clock).
- **hello** answers `platform: "macos-arm64"` and the SHA-256 of `requirements-macos.lock` as `lock_sha256` (protocol.json `hello_response`). Linux workers answer `linux-arm64` or `linux-x86_64` and `time_control: "faketime"`.
- **Scratch.** Each worker has its own scratch directory (and clock file) under `build/`, never `/tmp` shared between workers. SQLite stays on the local disk.
- **Clock-free only without faketime.** On a `none` worker, `tools/mg oracle record|compare --platform apple` run only scenarios tagged `clock-free` and report the others as not run (`clock_unsupported`).

## tools/mg

- `tools/mg oracle hello --platform apple`: bootstrap when needed, start one worker, report its hello (job result `details.hello`).
- `tools/mg oracle record --platform apple [--jobs N] <filter>`: writes `conformance/goldens-apple/<id>.json` per F7.2: the run requests set `fsrs_context: true`, observations are compared with the Linux golden value by value, every differing scalar becomes an entry with `unrounded` for day entries (F8.2), no overlay when nothing differs (an old one is deleted), nothing written on a structural difference. Entries outside the `fp-libm-boundary` class are listed in the result, which is then `fail`.
- `tools/mg oracle compare --platform apple [--jobs N] <filter>`: native run against golden plus overlay, no deviation applied; `tools/mg fp-probe` uses it to confirm its Apple side (F10.1).
- Any copy of `oracle/` files a job makes under `build/` is deleted when the job ends, as for the container jobs; the venv itself is covered by the implementer deny rule (`build/oracle-macos-venv/**`).

## Driver support this lane needs

- The `fsrs_next` observation (F5.4): a per-worker scratch collection like `observe/text.py`'s, presets and decks keyed by (parameter values, `dr`, `max_ivl`), one card rewritten per input, `lrt` from the scheduler's `next_day_at` with a re-evaluation when the day changes in between. It serves both lanes.
- `fsrs_context` in run requests (protocol.json): for every card of every `next_states` observation, the home deck's effective desired retention and the decay its preset's parameters give (`sched/fsrs-transitions` T2.1, T2.2).
- `platform` and `time_control` in `hello`, on both lanes.
