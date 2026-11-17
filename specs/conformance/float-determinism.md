# Floating-point determinism probe and the macOS oracle lane

Spec: conformance/float-determinism
Requirements: R1886, R1889, R1890, R1892, R1902
Anki: 26.09.3
Scenarios: conformance/scenarios/fp-probe/

FSRS works in 32-bit floats and calls the platform's math library (`powf`, `expf`, `logf`). Apple's library and glibc differ by one unit in the last place on a small share of FSRS-range inputs, so the Linux oracle's goldens can differ from what Anki computes on a Mac when an unrounded interval sits on a rounding boundary. Meguri uses the platform library, as Anki on macOS and AnkiMobile do (KTD24). This file defines how that is measured (the probe), how a Mac-side oracle runs (the native lane), how its differences are stored (overlay goldens), which differences are allowed (the `fp-libm-boundary` class) and how the runner uses them.

The scheduling rules the probe exercises are `sched/fsrs-transitions` (review cards) and `sched/fuzz`.

## F1. Sides and the decision rule

- Four sides compute the same next states and fuzz table:

  | Side | Where | Math library |
  |---|---|---|
  | Linux oracle | oracle container (`tools/mg oracle`), the committed goldens | glibc |
  | Apple oracle | native lane on this Mac (F6) | Apple |
  | Meguri-macOS | `meguri-cli fp-probe` natively on this Mac | Apple |
  | Meguri-Linux | `meguri-cli fp-probe` in the dev container | glibc |

- Expected: Meguri-macOS equals the Apple oracle bit for bit, and Meguri-Linux equals the Linux oracle bit for bit (same crate, same library). The Linux and Apple oracles differ only inside the class of F8.
- How often the oracles differ: the two libraries disagree in the last bit on a small share of calls (KTD24 estimates 0.6 % of FSRS-range inputs). That changes a memory value on a few percent of inputs, but an interval only when the change carries a rounded quantity across a half. The probe found a differing memory value on 3.3 % of its inputs and no differing interval among 400,000 (F11). The plan expected 0.1–1 % of intervals to differ, the share of differing library results; intervals differ far less often. At this corpus size a days rate of 0, or a handful of days, is normal and not a sign that the Apple side did not run (F10.1 checks that separately).
- Decision (recorded in F11 once the probe has run): when Meguri-macOS equals the Apple oracle on the whole corpus and the fuzz table agrees on every side, Meguri keeps the platform library and adopts the overlay goldens (F7). When Meguri-macOS differs from the Apple oracle, overlays are not adopted and a new unit vendors a glibc-equivalent math port into a patched fsrs (the KTD24 fallback).
- The milestone check on the user's own collection (aggregate agreement counts only) belongs to U19 and U246 and uses this lane; it is not specified here.

## F2. The probe corpus

### F2.1 Files

`conformance/corpora/fp-probe/` holds:

- `inputs.jsonl`: 100,000 lines, UTF-8, each ending in LF. Line `k` (from 0) is one JSON object with `id` = `k`, keys sorted, no white space. Every member is required and no other is allowed:

  | Member | JSON type | Range | Meaning |
  |---|---|---|---|
  | `id` | integer | 0–99,999 | the line index |
  | `card_id` | integer | 1 to 2^53 − 1 | the card's id; with `reps`, the fuzz seed (`sched/fuzz#Z1`) |
  | `reps` | integer | 0–4,294,967,294 | the card's review count before the answer |
  | `s` | number (`f32`) | 0.001–36,500 | FSRS stability in the card data |
  | `d` | number (`f32`) | 1–10 | FSRS difficulty in the card data |
  | `elapsed_days` | integer | 0–20,000 | whole days from the last review to the next day boundary (`sched/fsrs-transitions#T2.3`) |
  | `scheduled_days` | integer | 1–36,500 | the card's current interval (`cards.ivl`) |
  | `dr` | number (`f32`) | above 0, below 1 | desired retention |
  | `params` | string | a name in `param-sets.json` | the parameter set (F2.3) |
  | `max_ivl` | integer | 1–36,500 | the maximum interval of the card's preset |

  An `f32` member is written as the shortest decimal that both readings of `model/json-numbers#N2` (one step to the nearest `f32`, or through the 64-bit float of `model/json-numbers#N11`) map to the same `f32`, always with a fraction or an exponent. So every reader gets the same `f32`.
- `param-sets.json`: one JSON object, the parameter sets of F2.3 by name, each a list of numbers written as above.
- `generate.py`: the generator (F2.4).

### F2.2 What the corpus covers

The files are the definition; this is what the generator draws (proportions are of all inputs):

- `params`: `default` 40 %, `fsrs6-a`, `fsrs6-b` and `fsrs6-c` 15 % each, `fsrs5-a` 10 %, `fsrs4-a` 5 %.
- `dr`: the 30 values 0.70, 0.71, …, 0.99. 60 % are drawn from 0.90 (weight 3), 0.85, 0.80 and 0.95 (weight 1 each); the other 40 % uniformly from all 30.
- `max_ivl`: 36,500 (75 %), 3,650 (8 %), 365 (8 %), 100 (5 %), 30 (2 %), a value from 1 to 10 (2 %).
- `s`: log-uniform on [0.001, 1] (20 %), [1, 3,650] (70 %), [3,650, 36,500] (10 %). `d`: 1 (3 %), 10 (3 %), otherwise uniform on [1, 10]. 80 % of `s` values are rounded to 4 decimal places and 80 % of `d` values to 3, as card data stores them (`model/card-data-json#C4`); the rest are any `f32`.
- `scheduled_days`: for 70 %, the interval the set would give `s` at `dr`, times a factor uniform on [0.85, 1.15], rounded and clamped to [1, 36,500]; otherwise log-uniform on [1, 36,500]. About 9 % exceed `max_ivl`.
- `elapsed_days`: 0 (5 %), `scheduled_days` times a factor uniform on [0.9, 1.3] (60 %), [0, 0.9] (15 %, early), [1.3, 4] (15 %, late) or [4, 20] (5 %, very late), rounded and capped at 20,000.
- `card_id`: a millisecond timestamp from 2015 to 2029 (98 %) or a value below 10^6 (2 %). `reps`: log-uniform on [1, 500] (97 %), 0 (1 %), at least 10^6 (2 %).

`conformance/scenarios/fp-probe/next-states-edges.yaml` adds hand-picked inputs at the ends of these ranges.

### F2.3 Parameter sets

| Name | Values | Stored under | Decay the model uses |
|---|---|---|---|
| `default` | none (the crate's FSRS-6 defaults) | all three keys empty | 0.1542 |
| `fsrs6-a` | 21 | `fsrsParams6` | 0.1305 |
| `fsrs6-b` | 21 | `fsrsParams6` | 0.3412 |
| `fsrs6-c` | 21 | `fsrsParams6` | 0.6003 |
| `fsrs5-a` | 19 | `fsrsParams5` | 0.5 |
| `fsrs4-a` | 17 | `fsrsWeights` | 0.5 |

The values are in `param-sets.json` and in every probe scenario. They are made up: plausible optimizer results that the crate does not need to clip. With the 17-value set the crate converts the parameters itself, which takes one more math-library call per set.

### F2.4 Generator

`python3 -B -I conformance/corpora/fp-probe/generate.py` (seed 20261015) writes `inputs.jsonl`, `param-sets.json`, the 100 scenarios `conformance/scenarios/fp-probe/corpus/next-states-000.json` to `-099.json` (scenario `n` holds inputs 1,000 × n to 1,000 × n + 999 in order, member for member as in `inputs.jsonl`, in one `fsrs_next` request with all six parameter sets) and `conformance/scenarios/fp-probe/corpus/fuzz-table.json` (F4).

- The output depends only on the seed and the Python version (3.13.1 wrote the committed files). Never edit it by hand: regenerate, then re-record the goldens (`tools/mg oracle record fp-probe`) and the overlays (`tools/mg oracle record --platform apple fp-probe`) in the same change.
- It checks every `f32` text it writes (F2.1) and that no 21-value set needs clipping.
- It never computes an expected output; goldens come only from the oracle (R1878).

## F3. Next states of one probe input

### F3.1 What the oracle observes

Each input is a stored review card, and the oracle reads the four answer states Anki 26.9.3 computes for it. The card, in a collection with FSRS on (`fsrs` true), the load balancer off (`loadBalancerEnabled` false) and `fsrsShortTermWithStepsEnabled` off:

- is in a normal deck that sets no desired retention of its own, whose preset has the input's parameter set under the key of F2.3 (the other two parameter keys empty), desired retention `dr`, maximum interval `max_ivl`, relearning steps `10m`, learning steps `1m 10m` and leech threshold 8;
- has id `card_id`, type 2 and queue 2 (review), `ivl` = `scheduled_days`, `due` = today, `factor` 2500, `reps` = `reps`, `lapses` 0, `left` 0, `odid` 0, and data with `s`, `d` and an `lrt` for which `sched/fsrs-transitions#T2.3` gives `elapsed_days`.

### F3.2 The rule

The four states are those of `sched/fsrs-transitions#T3`–`#T5` for that card:

1. One crate call (`sched/fsrs-transitions#T1`): `FSRS::new` with the set's values, then `next_states(Some(MemoryState { stability: s, difficulty: d }), dr, elapsed_days)`, with `s`, `d`, `dr` and the parameters read as `f32` (`model/json-numbers#N2`). It gives `S_r`, `D_r` and the unrounded interval `u_r` for each rating.
2. Again (`#T4`, one relearning step): a `relearning` state whose review part has `scheduled_days = max(1, round(u_again))` and memory state (`S_again`, `D_again`). No fuzz and no clamp.
3. Hard, Good, Easy (`#T5`), with current interval `scheduled_days`, `M_raw = M = max_ivl`, and the fuzz factor of the seed `card_id + reps` (`sched/fuzz#Z1`, `#Z2`): no-regression minimum (`sched/fuzz#Z5`), the button order (Good at least Hard + 1, Easy at least Good + 1), the clamp to [1, `max_ivl`], then the fuzz pick (`sched/fuzz#Z4`).

### F3.3 Row

The probe keeps, per input, twelve values:

| Column | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Value | again days | again S | again D | hard days | hard S | hard D | good days | good S | good D | easy days | easy S | easy D |
| Name | `again_days` | `again_s` | `again_d` | `hard_days` | `hard_s` | `hard_d` | `good_days` | `good_s` | `good_d` | `easy_days` | `easy_s` | `easy_d` |

Days are integers (for again, the review part's `scheduled_days`); S and D are the memory state's stability and difficulty, written per `observations.md` O6. These are the `scheduled_days` and `memory_state` members of the O5 states, in a fixed order.

### F3.4 Fuzz is on

Anki 26.9.3 has no switch for fuzz (R664). Only its unit-test mode (`ANKI_TEST_MODE`) turns it off, and that mode also shifts Anki's clock; the oracle refuses to start with it. So the probe observes intervals as Anki gives them, fuzz included, and every input carries a fuzz seed (`card_id`, `reps`).

Fuzz calls no math-library function (`sched/fuzz#Z2`–`#Z4`), so it adds no platform difference of its own. It only changes where a one-ulp change of an unrounded interval crosses a rounding boundary, which F8.2 accounts for. The class has to cover fuzzed intervals anyway, since every scheduler golden has fuzz on.

### F3.5 Example

Input 4 of `fp-probe/next-states-edges` is the review card of `conformance/goldens/smoke/fsrs-new-card.json` step 6 (`sched/fsrs-transitions#T6`), so its row is that golden's next states:

`[1, 0.6369821, 7.3921676, 10, 9.240417, 4.7481427, 15, 13.836124, 2.1041174, 26, 23.900333, 1.0]`

## F4. The fuzz table

- `fp-probe/corpus/fuzz-table` observes the existing `fuzz_table` kind (`observations.md` O4), unchanged: 10,000 distinct (card id, reps) pairs at the 20 intervals 1, 2, 3, 4, 5, 6, 7, 8, 13, 20, 21, 30, 50, 90, 91, 100, 365, 1,000, 10,000 and 36,500, for a card whose preset has the maximum interval 36,500. Each delta is `sched/fuzz#Z6`.
- Pairs: card ids are millisecond timestamps from 2015 to 2029 (80 %), below 10^6 (10 %), within 10^6 of 2^53 − 1 (5 %) or anywhere from 1 to 2^53 − 1 (5 %); review counts are 0–200 (70 %), 201–100,000 (20 %) or 100,000–4,294,967,294 (10 %). The observation stores `reps + 1` in the card, so 4,294,967,294 is the largest count it accepts.
- The four sides of F1 must give identical rows: no tolerance and no overlay (F8.1 excludes `fuzz_table`). Any difference fails the probe (F10).
- R1892: this table's golden is re-recorded with every oracle bump, like every golden.

## F5. The `fsrs_next` observation

### F5.1 Request

`{fsrs_next: {param_sets, inputs}}` in an `observe` list, at most once per observe step:

- `param_sets`: an object with at least one member; each name matches `^[a-z0-9][a-z0-9-]*$`, each value is a list of 0, 17, 19 or 21 numbers.
- `inputs`: a list of 1 to 1,000 objects with exactly the members of F2.1, each in F2.1's range except `id`; `params` names a member of `param_sets`.
- `id` is any integer from 0 to 2^53 (`dsl.md`'s integer limit; `uint` in `scenario.schema.json`), not only the corpus's 0–99,999. It only labels the input: ids need not be distinct or in order, and rows follow input order whatever the ids.
- Numbers are read as `f32` per `model/json-numbers#N2` where F2.1 says `f32`, and parameter values likewise.
- Anything else is `invalid_scenario`. No valid input is refused: the observation has no rejection kinds.

### F5.2 Value

`{"rows": [<row>, ...]}`: one F3.3 row per input, in input order.

### F5.3 A pure observation

`fsrs_next` never reads or changes the scenario's collection, so it may appear in any observe step, with or without setup, like `text` (O9). Its value does not depend on the scenario's clock or zone: each input is placed relative to the day it is evaluated on.

### F5.4 How the oracle evaluates it

Normative for the driver:

- One scratch collection per worker process, made on first use in the worker's scratch directory with the collection settings of F3.1, never the scenario's collection.
- One preset and one normal deck per distinct (parameter values, `dr`, `max_ivl`), made on first use and kept for later requests.
- One card, rewritten for each input with the fields of F3.1, then Anki's scheduling states read for it.
- `lrt` is placed from the scheduler's `next_day_at` read for that input. If the scheduler's day changes between placing an input and reading its states (a rollover on a real clock, F6.3), the input is evaluated again.
- One stored-card update and one scheduling-states call per input. The oracle-harness measurements put a scheduling-states call under 0.1 ms, so a 1,000-input request should take about a second and the 100 corpus scenarios should record in minutes with `tools/mg oracle record --jobs 4 fp-probe`. The limit of 1,000 inputs per request keeps one response line small.

### F5.5 Meguri

Meguri's adapter evaluates F3.2 on each input directly, without a collection, and `meguri-cli fp-probe` (F10) uses the same code.

### F5.6 Comparison

Days columns compare exactly; S and D columns within relative 1e-5, the R1886 tolerance for FSRS memory states (`observations.md` O7).

## F6. The macOS native oracle lane

### F6.1 Environment

- The same driver (`oracle/driver/`, the same `driver_sha256`) and the same protocol (`conformance/schema/protocol.json`), run natively on the Mac by `oracle/macos/run-native.sh`, from a virtualenv at `build/oracle-macos-venv/` installed with hash checking from `oracle/macos/requirements-macos.lock` (the `anki` 26.9.3 macOS arm64 wheel and its dependencies). This is the one exception to Docker-first tooling (KTD24). The wheel holds Anki's Python sources, so implementers never read the venv (U2 deny rule) and use the lane only through `tools/mg`.
- `conformance/oracle.json` pins the macOS lock as `macos_lock_sha256` (SHA-256 of `oracle/macos/requirements-macos.lock`) next to `lock_sha256`. The member appears with the lock file and then changes only when adopting an oracle bump, with both lock files, every golden and every overlay. Without it no overlay is fresh (F7.3).

### F6.2 hello

A native worker's `hello` answers `platform: "macos-arm64"`, `lock_sha256` = the SHA-256 of `oracle/macos/requirements-macos.lock`, `anki_version`, `driver_sha256`, `tz` and `time_control` (F6.3). Linux workers answer `platform` `linux-arm64` or `linux-x86_64`, the hash of `oracle/requirements.lock` and `time_control: "faketime"`. `tools/mg oracle hello --platform apple` starts a native worker and reports its hello.

Tools accept a native worker only when it answers `macos-arm64`, the `anki_version` of `conformance/oracle.json` and its `macos_lock_sha256`.

### F6.3 Time control

1. **faketime.** libfaketime is injected with `DYLD_INSERT_LIBRARIES` and `DYLD_FORCE_FLAT_NAMESPACE=1`, under a Python that keeps `DYLD_*` variables: never `/usr/bin/python3` (protected by System Integrity Protection) and no build whose hardened runtime drops them. The clock file works as in the Linux image: `FAKETIME_TIMESTAMP_FILE` with `FAKETIME_NO_CACHE=1`, `FAKETIME_FMT=%s` and `DONT_FAKE_MONOTONIC=1`. At start the worker freezes the clock at a fixed instant and checks that both Python's clock and Anki's scheduler day follow it, then moves it one day and checks again. When both checks hold it answers `time_control: "faketime"` and may run any scenario.
2. **none** (the fallback). The worker runs on the real clock and answers `time_control: "none"`. On such a worker `clock.start`, `advance` and `set_clock` cannot hold, so tools run only scenarios tagged `clock-free` there and report every other selected scenario as not run (`clock_unsupported`), without an overlay.

A `clock-free` scenario gives the same observations at any instant: it places any time-relative state by data, relative to the scheduler's own day when it runs (`fsrs_next` places `lrt` itself, F5.4; `fuzz_table` uses no time; a `db_exec` can use SQLite's `unixepoch()`), and observes nothing that holds the instant (`ids`, `mod`, review-log ids, `timing`). The fp-probe scenarios are `clock-free`; the Linux oracle records them under its frozen clock like any other.

### F6.4 Commands

- `tools/mg oracle hello --platform apple`.
- `tools/mg oracle record --platform apple [--jobs N] <filter>` (F7.2).
- `tools/mg oracle compare --platform apple [--jobs N] <filter>`: runs the scenarios on native workers and compares them with their goldens with overlays applied (F9 step 2), no deviation applied. The probe uses it to confirm its Apple side (F10.1).

They run only on an arm64 Mac; anywhere else they fail with a usage error. Each native worker has its own scratch directory and clock file under `build/`.

## F7. Overlay goldens

### F7.1 Format

`conformance/goldens-apple/<scenario id>.json` exists for each scenario whose Apple-oracle observations differ from its Linux golden. Schema: `conformance/schema/overlay.schema.json`. Text layout as goldens (`dsl.md` D11).

```json
{
  "entries": [
    {
      "apple": 16,
      "linux": 15,
      "pointer": "/steps/0/fsrs_next/rows/417/6",
      "unrounded": {"apple": 15.500000953674316, "linux": 15.499999046325684}
    },
    {
      "apple": 15.500001,
      "linux": 15.499999,
      "pointer": "/steps/0/fsrs_next/rows/417/7"
    }
  ],
  "linux_observations_sha256": "<hex>",
  "oracle": {
    "anki_version": "26.9.3",
    "driver_sha256": "<hex>",
    "lock_sha256": "<hex of oracle/macos/requirements-macos.lock>",
    "platform": "macos-arm64"
  },
  "scenario_id": "fp-probe/corpus/next-states-000",
  "scenario_sha256": "<hex>"
}
```

- `scenario_id`, `scenario_sha256`: as in a golden (`dsl.md` D3, D11), for the scenario the Apple run ran.
- `linux_observations_sha256`: the lower-case hex SHA-256 of the RFC 8785 (JCS, `dsl.md` D3) text of the Linux golden's `observations` value. So re-recording a Linux golden with the same observations leaves its overlay fresh, and any changed observation makes it stale.
- `oracle`: the Apple run's `anki_version`, `driver_sha256`, `lock_sha256` (the macOS lock) and `platform` (`macos-arm64`).
- `entries`: at least one, in pointer order (`observations.md` O7: members by key, items by index). Each has:
  - `pointer`: an RFC 6901 pointer into the observations, to a scalar;
  - `linux`: the Linux golden's value there;
  - `apple`: the Apple oracle's value there, never equal to `linux`;
  - `unrounded`, on day entries only (F8.2): `{linux, apple}`, the rating's unrounded interval on each side as a 64-bit float.

### F7.2 Writing

Only `tools/mg oracle record --platform apple <filter>` writes overlays: never by hand and never from Meguri's output.

1. Each selected scenario needs a fresh Linux golden (`dsl.md` D10); otherwise that scenario is an error and its overlay is left as it is.
2. The scenario runs on native workers (F6.4; only `clock-free` ones on a `none` worker, F6.3).
3. Its observations are compared with the Linux golden's value by value. Two scalars differ when their JSON values differ: number kind and value (1 and 1.0 differ), string, boolean, null.
4. A structural difference (a member or an item on one side only, or a scalar against an object or array) writes nothing: the existing overlay stays and the scenario is reported as failed.
5. Otherwise every differing scalar becomes an entry, in the class or not. With no difference, no overlay is written and an existing one is deleted.
6. Day entries get `unrounded` (F8.2). The recorder takes the stability of the same rating on each side, and the desired retention and decay from the scenario's input and parameter set (`fsrs_next`) or from the run's `fsrs_context` (`next_states`; `protocol.json`, run requests with `fsrs_context: true`).
7. Its result lists, per scenario, the entries and those outside the class (F8), which make the record `fail`. `tools/mg goldens-check` then rejects that overlay until a decision (F11) settles the case: the class is widened by a change of this spec, or the KTD24 fallback is started.

### F7.3 Freshness

An overlay is stale when its `scenario_sha256` differs from the scenario's hash, its `linux_observations_sha256` from the current Linux golden's, its `oracle.anki_version` from `conformance/oracle.json`'s, or its `oracle.lock_sha256` from `conformance/oracle.json`'s `macos_lock_sha256` (or that member is missing). It is orphaned when its scenario or the scenario's Linux golden is missing. The fix is `tools/mg oracle record --platform apple <id>`.

### F7.4 Checks

`tools/mg goldens-check` fails when an overlay does not parse or match its schema; is stale or orphaned; has an entry whose pointer does not resolve to a scalar of the Linux golden equal to its `linux`, whose `apple` equals its `linux`, whose pointer repeats, or that is out of pointer order; or has an entry outside the class of F8. Where `oracle/` is present it also checks that `conformance/oracle.json` has `macos_lock_sha256` exactly when `oracle/macos/requirements-macos.lock` exists, and that it is that file's hash. Its report lists each overlay with its count of day and memory entries.

## F8. The `fp-libm-boundary` class

### F8.1 Pointers

A next-state set is either the object at `/steps/<i>/next_states/<#cN>` (its ratings `again`, `hard`, `good`, `easy`; never `current`) or the row at `/steps/<i>/fsrs_next/rows/<k>`.

| Kind | In `next_states` (`<r>` a rating) | In an `fsrs_next` row |
|---|---|---|
| day | `/<r>/scheduled_days`, `/<r>/review/scheduled_days` | columns 0, 3, 6, 9 |
| memory | `/<r>/memory_state/stability` and `/difficulty`, also under `/<r>/learning/` and `/<r>/review/` | columns 1, 2, 4, 5, 7, 8, 10, 11 |

An entry's rating is its `<r>` segment, or its column's (0–2 again, 3–5 hard, 6–8 good, 9–11 easy). Every other pointer is outside the class: `current` states, card and review-log rows, `fuzz_table`, `ids`, `timing`, learning delays (`scheduled_secs`) and every other kind.

### F8.2 Day entries

A day entry is in the class when all of these hold:

1. `linux` and `apple` are integers that differ by exactly 1.
2. `unrounded` holds finite positive numbers `u_L` and `u_A` with `|u_A − u_L| ≤ 1e-5 × u_L`.
3. Each is the rating's unrounded FSRS interval, recomputed in 64-bit float as `u = S × (r^(−1/w) − 1) / (n^(−1/w) − 1)`, where `n` is 0.9 as an `f32` (0.8999999761581421, as the crate uses it), from that side's stability `S` of the same rating (the sibling `memory_state/stability`, or the next column; the Apple side's is the `apple` of that pointer's entry when it has one, else the Linux value), the desired retention `r` and the decay `w` the model used (`sched/fsrs-transitions#T2.1`). For `fsrs_next` entries goldens-check recomputes both from the scenario's input and parameter set and the observations, and each must match the recorded value within relative 1e-9. For `next_states` entries it takes them as recorded.
4. A rounding boundary explains the flip, either:
   - some point `p` of `B(u_L)` or `B(u_A)` lies within `1e-5 × max(1, p)` of a half-integer (`k + 0.5` for an integer `k`), where `B(u) = {u, u − Δ(u), u + Δ(u)}` and `Δ` is `sched/fuzz#Z3` evaluated in 64-bit float; or
   - the rating is good and the same set has a day entry for hard in the class, or the rating is easy and the same set has one for good in the class (Good's minimum is Hard + 1 and Easy's is Good + 1, `sched/fsrs-transitions#T5`).

Why these points: a one-ulp change of `u` can change a result only where a rounded quantity crosses a half: `u` itself (Again's days, and the comparison in the no-regression minimum), `u − Δ` and `u + Δ` (the fuzz range), and `u = 2.5`, where fuzz starts. The pick inside the range is exact on every platform.

### F8.3 Memory entries

A memory entry is in the class when `linux` and `apple` are both numbers written with a fraction or an exponent (O6), they differ, and, with `m = max(|linux|, |apple|)`:

- a stability (the member `stability` of F8.1, or `fsrs_next` columns 1, 4, 7, 10): `|apple − linux| ≤ max(1e-4 × m, 1e-5)`, a relative bound of 1e-4 or an absolute floor of 1e-5 days, whichever is larger;
- a difficulty (the member `difficulty`, or columns 2, 5, 8, 11): `|apple − linux| ≤ 1e-5 × m`, the R1886 tolerance.

The rule uses only the two values and the pointer.

Why stability needs more room (evidence in F11.1). FSRS computes a new stability from the difference between a math-library result close to 1 and 1 itself. After a lapse, the new stability is, unless a cap applies, a multiple of `(1 + S)^w − 1`, with `S` the old stability and `w` a parameter of about 0.2. After a recall, it grows with `e^x − 1`, where `x` is a parameter times `1 − R` and the retrievability `R` is just below 1 for a card reviewed early. When that difference is small, one unit in the last place of the power or exponential (2^-23, about 1.19e-7, between 1 and 2) is a large part of it. The corpus figures below are worked out in 64-bit floats for the 85 % of inputs with the default or a 21-value set:

- After a lapse the relative difference is `2^-23 / ((1 + S)^w − 1)`: 9.4e-5 at `S` = 0.0048 and about 5e-4 near the 0.001 minimum. In absolute terms it stays small, because the cap keeps the factor in front of the difference below about `1/w`. Over the corpus, wherever the relative difference exceeds 1e-5, the absolute difference is at most 5.1e-7 days. The floor of 1e-5 days leaves a margin of about 20 and is a tenth of the precision at which card data stores stability (`model/card-data-json#C4`).
- After a recall the relative difference is roughly the factor in front of `e^x − 1` times 2^-23, and smaller when `e^x − 1` is not small. It was 1.04e-5 on the probe and at most 1.9e-5 for any corpus input. To exceed 1e-4, a parameter set would need a factor more than five times the corpus's largest.
- Difficulty involves no such difference: it differed by one or two units in the last place (at most relative 1.9e-7) on the probe.

The bound covers only the differences between the two oracles. Meguri's comparison with the expected values keeps O7's relative 1e-5 (F11.3).

### F8.4 Examples

| Entry | In the class? |
|---|---|
| `/steps/0/fsrs_next/rows/417/6`, `linux` 15, `apple` 16, `unrounded` 15.499999046325684 and 15.500000953674316 | yes: `u` lies on 15.5 |
| the same pointer with `linux` 15 and `apple` 18 | no: three days apart |
| the same pointer, `linux` 15, `apple` 16, `unrounded` 15.2 on both sides | no: `B(15.2)` is about {15.2, 12.705, 17.695}, none within 1.5e-4 of a half |
| a good day entry with `unrounded` 14.972222 on both sides, `linux` 13, `apple` 14 | yes: `Δ` is 2.472222, so `u − Δ` is 12.5 |
| an easy day entry with no boundary of its own, `linux` 26, `apple` 27, in a row whose good day entry is in the class | yes: Easy follows Good |
| `/steps/0/fsrs_next/rows/417/7`, `linux` 13.836124, `apple` 13.836125 | yes: relative 7e-8 |
| `fp-probe/corpus/next-states-029` `/steps/0/fsrs_next/rows/820/1` (an again stability), `linux` 0.0041170507, `apple` 0.0041174362 | yes: 3.9e-7 apart, under the 1e-5 floor (relative 9.4e-5) |
| a stability entry with `linux` 0.0011, `apple` 0.0011005 | yes: relative 4.5e-4, but only 5e-7 apart, under the floor |
| `fp-probe/corpus/next-states-089` `/steps/0/fsrs_next/rows/977/10` (an easy stability), `linux` 59.598206, `apple` 59.598827 | yes: relative 1.04e-5, under 1e-4 |
| a stability entry with `linux` 2.3065, `apple` 2.3066 | yes: relative 4.3e-5 |
| a stability entry with `linux` 2.3065, `apple` 2.3075 | no: relative 4.3e-4, and 1e-3 apart |
| a stability entry with `linux` 0.0041, `apple` 0.0042 | no: 1e-4 apart, above the floor (relative 2.4e-2) |
| a stability entry with `linux` 2 (written without a fraction), `apple` 2.0000001 | no: not both written with a fraction |
| `/steps/2/next_states/#c1/again/learning/memory_state/difficulty`, `linux` 7.3921676, `apple` 7.392168 | yes: relative 5e-8 |
| a difficulty entry with `linux` 7.3921676, `apple` 7.3925 | no: relative 4.5e-5; difficulty keeps 1e-5 |
| `/steps/2/next_states/#c1/current/memory_state/stability` | no: `current` is stored data |
| `/steps/3/card/#c1/data/s`, `/steps/0/fuzz_table/rows/12/5` | no: not a next state |

### F8.5 What the class leaves out

A boundary flip can reach later steps: answering at a flipped interval stores another due date, and a memory state that rounds differently in the card data (`model/card-data-json#C4`) feeds every later state. Those differences are outside the class, so such a scenario fails on macOS until a decision (F11). Scenarios meant for both platforms observe next states before answering where they can. The scheduler units revisit this class with the probe's evidence.

The day rules keep relative 1e-5 for the two unrounded intervals (F8.2 rules 2 and 4), while F8.3 allows a stability up to relative 1e-4. An unrounded interval is the stability times a factor both sides compute alike, so a day flip caused by a stability difference above relative 1e-5 is outside the class. It then goes to a decision with its evidence. The probe had no such flip (F11.2).

### F8.6 The deviation entry

`conformance/deviations.yaml` lists the class as the R1889 deviation `fp-libm-boundary`, with the transform `apple_overlay` (`dsl.md` D13) and the test `core/crates/meguri-testkit/tests/overlay.rs overlay_applies_on_macos_only`, which asserts F9: on macOS the overlay's values are expected, on Linux the golden's.

## F9. Runner rules

- Overlays apply only where Meguri's executor runs natively on macOS (T0's native lane). The Linux lane never reads `conformance/goldens-apple/`, even when it is there.
- On macOS, for a scenario with an overlay:
  1. A stale or orphaned overlay (F7.3) makes the scenario `stale`, with the fix `tools/mg oracle record --platform apple <id>`. An overlay that fails its schema or F8 makes it `invalid`. The runner checks with the same code as goldens-check.
  2. The expected observations are the Linux golden's, with each entry's `apple` value put at its pointer.
  3. Then the other deviation transforms of `dsl.md` D13 apply, in their order; `fp-libm-boundary` is step 2 and comes first.
  4. Then the O7 comparison, tolerances and `compare.set` included.
- The oracle's own replay in T1 never uses overlays. Live chains (`dsl.md` D9.2, D9.3) mix platforms, Meguri's segments natively and the oracle's in Linux workers, so they do not use overlays either: a live scenario whose Meguri segment observes FSRS values at a boundary fails on macOS and goes to the F11 decision.
- Report (`dsl.md` D12): each scenario whose overlay applied has `overlay_applied`, the number of entries applied, and lists `fp-libm-boundary` under `deviations_applied`; `totals` gains `overlay_applied` (entries) and `overlay_scenarios` (scenarios). The text report has the line `overlays: <entries> values in <scenarios> scenarios`.
- A scenario that passes on macOS only with its overlay passes: KTD24 reads R1890's "exactly" against Anki on the same platform.

## F10. The probe report

`tools/mg fp-probe` (host job; the Meguri-Linux side runs in the dev container) produces the report and fails or passes on it.

### F10.1 Sides

- `linux_oracle`: the goldens of `fp-probe/corpus/next-states-*` and `fp-probe/corpus/fuzz-table`.
- `apple_oracle`: the same goldens with their overlays applied (F9 step 2); a scenario without an overlay has the Linux values. Overlays hold every differing value, and the probe confirms that they still do with `tools/mg oracle compare --platform apple fp-probe/corpus`: any difference there fails the probe ("Apple side not reproduced"), so this side is exactly what the Apple oracle observes.
- `meguri_macos`: `meguri-cli fp-probe`, release build, natively on this Mac.
- `meguri_linux`: `meguri-cli fp-probe` in the dev container.

Goldens and overlays must be fresh; otherwise the probe fails and lists the stale ones. `meguri-cli fp-probe` evaluates F3.2 on every line of `inputs.jsonl` with `param-sets.json`, and `sched/fuzz#Z6` on the request of `fp-probe/corpus/fuzz-table`, and writes the rows; it never reads goldens. Every value compares exactly: integers equal, `f32` values equal as `f32`, no tolerance.

### F10.2 Pairs and status

| Pair | Gate |
|---|---|
| `linux_oracle` vs `apple_oracle` | no: reported only |
| `apple_oracle` vs `meguri_macos` | yes |
| `linux_oracle` vs `meguri_linux` | yes |
| fuzz table: all four sides | yes |

The status is `fail` when a gate pair has any difference, when the fuzz table differs between any two sides, or when a side could not be produced (with the reason); otherwise `pass`. Differences between the oracles never change the status; goldens-check is what rejects those outside the class.

### F10.3 Report file

`build/mg/fp-probe/<run id>/report.json`, an artifact of the job result:

```json
{
  "report_version": 1,
  "status": "pass",
  "corpus": {"path": "conformance/corpora/fp-probe/inputs.jsonl", "sha256": "<hex>", "inputs": 100000},
  "sides": {
    "apple_oracle": {"lock_sha256": "<hex>", "overlays": 0},
    "linux_oracle": {"lock_sha256": "<hex>"},
    "meguri_linux": {"commit": "<sha>", "platform": "linux-arm64"},
    "meguri_macos": {"commit": "<sha>", "platform": "macos-arm64"}
  },
  "pairs": [
    {
      "left": "linux_oracle",
      "right": "apple_oracle",
      "gate": false,
      "inputs": 100000,
      "differing_inputs": 0,
      "differing_days": 0,
      "differing_memory": 0,
      "days_rate": 0.0,
      "differences": []
    }
  ],
  "fuzz": {
    "scenario": "fp-probe/corpus/fuzz-table",
    "pairs": 10000,
    "intervals": 20,
    "mismatches": []
  }
}
```

- `sides`: the oracle sides give the lock hash their goldens or overlays recorded (`overlays`: the number of overlay files applied); the Meguri sides give the commit and platform they ran on.
- `pairs` has the three pairs of F10.2 in that order (the example shows the first). `differing_days` and `differing_memory` count differing values, `differing_inputs` the inputs with at least one, and `days_rate` is `differing_days / (4 × inputs)`.
- `differences` lists every difference, sorted by input id then column: `{id, column, name, left, right}`, `name` from the F3.3 table (`good_days`, `easy_s`, …). On the oracle pair each also has `in_class` (F8 applied to it as an overlay entry would be).
- `fuzz.mismatches`: `{card_id, reps, interval, values: {<side>: <delta>}}` for every table cell where the sides differ.
- The text summary gives the counts and rate per pair, the first 20 differences of each pair, the fuzz result and the status.

## F11. Decision record

Each side of the probe first ran on its own on the committed corpus on 2026-10-15, because the single `tools/mg fp-probe` job needs one checkout with both `oracle/` and Meguri's probe, which only U14's integration gives. The first green `tools/mg fp-probe` on `feat/meguri-v1` after U14 merged ran all four sides in one job on 2026-10-16 and gave every number below, so the decision stands as first recorded.

| Item | Value |
|---|---|
| Probe run | First, sides run one at a time. Oracle sides: the committed goldens and overlays of `fp-probe/corpus`. The Apple side is reproduced by `tools/mg oracle compare --platform apple fp-probe/corpus`: 101 of 101 scenarios pass, with 5,549 overlay entries applied. Meguri sides: `meguri-cli fp-probe` on macOS and in the dev container, in the U14 implementer's probe run `20261015T221715Z-93715`. The integrated run: `tools/mg fp-probe` run `20261016T003103Z-71186`, status `pass`. Its Apple side, `tools/mg oracle compare --platform apple fp-probe/corpus` run `20261016T003103Z-71354`, passed 101 of 101 scenarios with 5,549 overlay entries applied, under time control `faketime`. |
| Date, commit | 2026-10-15. The Apple compare ran at `spec/U14` 329234e; the Meguri sides at `unit/U14` 3d5af28 with uncommitted changes. The integrated run: 2026-10-16, at `feat/meguri-v1` e41ecaf (U14's merge) with a clean tree; Meguri-macOS natively on an arm64 Mac (`macos-arm64`), Meguri-Linux in the dev container (`linux-arm64`). |
| Corpus `sha256` | `c1daa0fcd5dc5a6a8230acd609c8f40de70edb00955e74d9ec79c27f80c14306` (100,000 inputs) |
| Linux vs Apple oracle | 3,288 of 100,000 inputs differ. 0 of 400,000 days differ (rate 0.0). 5,549 memory values differ: 4,915 stabilities and 634 difficulties. 12 entries were outside the class as first written (every memory value within relative 1e-5). None is outside it under F8.3 as revised (F11.1). |
| Apple oracle vs Meguri-macOS | 0 differences |
| Linux oracle vs Meguri-Linux | 0 differences |
| Fuzz table | 0 mismatches: 10,000 pairs × 20 intervals are equal on all four sides. `fp-probe/corpus/fuzz-table` has no overlay. |
| Decision | Overlays adopted. Meguri keeps the platform math library (KTD24), and F8.3 is widened for stability. The KTD24 fallback (a patched fsrs with a vendored glibc-equivalent math port) is not started. O7 is unchanged (F11.3). |
| Recorded by | spec writer, session `s-20261015T223556Z-c74bba35` (U14); the integrated run by spec writer session `s-20261016T050935Z-079ecba2` (U14) |

### F11.1 The entries outside the first class

All 12 are stabilities in `fp-probe/corpus` scenarios. `Row` is the input's row in its scenario and `id` its line in `inputs.jsonl`:

| Scenario | Row (`id`) | Column | Input `s`, set | `linux` | `apple` | Relative | Apart |
|---|---|---|---|---|---|---|---|
| `next-states-007` | 429 (7429) | 1 `again_s` | 0.0155, `fsrs6-c` | 0.010866672 | 0.010866291 | 3.5e-5 | 3.8e-7 |
| `next-states-009` | 529 (9529) | 1 `again_s` | 0.0112895155, `default` | 0.008719591 | 0.0087199425 | 4.0e-5 | 3.5e-7 |
| `next-states-017` | 638 (17638) | 1 `again_s` | 0.0382, `default` | 0.026549045 | 0.026548726 | 1.2e-5 | 3.2e-7 |
| `next-states-019` | 18 (19018) | 1 `again_s` | 0.0331, `fsrs6-c` | 0.025712885 | 0.025713312 | 1.7e-5 | 4.3e-7 |
| `next-states-020` | 503 (20503) | 1 `again_s` | 0.0368, `fsrs6-c` | 0.033080965 | 0.033080474 | 1.5e-5 | 4.9e-7 |
| `next-states-026` | 627 (26627) | 1 `again_s` | 0.0096, `fsrs6-a` | 0.008743496 | 0.008743873 | 4.3e-5 | 3.8e-7 |
| `next-states-029` | 820 (29820) | 1 `again_s` | 0.0048447875, `default` | 0.0041170507 | 0.0041174362 | 9.4e-5 | 3.9e-7 |
| `next-states-051` | 896 (51896) | 1 `again_s` | 0.0155, `fsrs6-c` | 0.013848968 | 0.013848482 | 3.5e-5 | 4.9e-7 |
| `next-states-087` | 748 (87748) | 1 `again_s` | 0.05237645, `fsrs6-c` | 0.04590779 | 0.045908276 | 1.1e-5 | 4.9e-7 |
| `next-states-087` | 768 (87768) | 1 `again_s` | 0.0382, `default` | 0.027307305 | 0.027306978 | 1.2e-5 | 3.3e-7 |
| `next-states-089` | 977 (89977) | 10 `easy_s` | 36.2029, `fsrs6-c` | 59.598206 | 59.598827 | 1.04e-5 | 6.2e-4 |
| `next-states-096` | 643 (96643) | 1 `again_s` | 0.013814153, `default` | 0.011778707 | 0.011778318 | 3.3e-5 | 3.9e-7 |

Each is one unit in the last place of the library result that F8.3 names. For the 11 again stabilities, `2^-23 / ((1 + s)^w − 1)`, worked from the input's `s` and its set's `w`, gives the relative difference to within 1 %. For the easy stability, one unit in the last place of the exponential gives 1.042e-5, which is the observed value. Every other memory entry was already within relative 1e-5: at most 7.7e-6 for stabilities and 1.9e-7 for difficulties.

Widening F8.3 changes only which entries goldens-check accepts; the overlays stay as recorded.

### F11.2 Intervals

No interval differed. The unrounded intervals can be recomputed in 64-bit floats from the differing hard, good and easy stabilities. That recomputation finds 6 inputs where a rounded quantity of F8.2 (`round(u)`, `u − Δ` or `u + Δ`) moved by one. All 6 have intervals above 2,800 days, where one unit in the last place is 0.0002 to 0.002 day. In each, the result was the same day on both sides: a moved bound changes the fuzz pick for only some fuzz factors, and a moved `round(u)` matters only next to the current interval (`sched/fuzz#Z5`). Day entries in overlays are therefore expected to be rare, and the day rules of F8.2 are unchanged and still untested by real entries.

### F11.3 O7 keeps relative 1e-5

O7's tolerance compares Meguri with the expected values, not the two oracles with each other. On macOS the expected values include the overlay's exact Apple values (F9), and Meguri-macOS equals the Apple oracle bit for bit. On Linux, Meguri-Linux equals the Linux oracle bit for bit. So no comparison needs more than 1e-5, and a wider O7 would only loosen R1886 for Meguri.

Some comparisons cross platforms without an overlay: live chains (F9), and the U19 and U246 milestone counts on the user's collection. There, a stability that differs by more than relative 1e-5 is reported, and F8.3 is the explanation that R1902 asks for.
