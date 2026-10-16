# Conformance scenarios, goldens and the runner

Spec: conformance/dsl
Requirements: R1874, R1877, R1878, R1879, R1880, R1881, R1882, R1883, R1884, R1885, R1888, R1889, R1950
Anki: 26.09.3
Scenarios: conformance/scenarios/smoke/

This file defines the scenario language that both the oracle driver and Meguri's executor implement, the golden files the oracle writes, how the runner decides a scenario's status, the known-deviations list and the harvest catalog. Observation values and comparison rules are in `observations.md`. Scenarios with several syncing clients (actors) are in `sync-ops.md`. The machine-checkable forms are `conformance/schema/scenario.schema.json`, `golden.schema.json`, `observation.schema.json`, `area.schema.json` and `deviations.schema.json`; when this text and a schema disagree, fix both in one change.

## D1. Files and ids

- Scenarios live at `conformance/scenarios/<area>/<path>.yaml` (authored) or `.json` (generated). `.yml` is read like `.yaml`. A file named `area.yaml` or `area.yml` is the area's metadata (D9), never a scenario.
- A scenario's `id` equals its path relative to `conformance/scenarios/` without the suffix: `conformance/scenarios/smoke/timing-advance.yaml` has id `smoke/timing-advance`. Ids are lower case: segments of `a-z`, `0-9`, `.`, `_`, `-`, separated by `/`, at least two segments. Two files with the same path apart from the suffix are an error, since they would share one golden.
- The first segment is the scenario's area.
- Its golden is `conformance/goldens/<id>.json` (D7).
- JSONL is for generated corpora that only the live oracle consumes (nightly fuzzing). JSONL files are not scenarios and have no goldens.

## D2. YAML subset and parsing

- A scenario file holds one document. Authored YAML is read with the YAML 1.2 core schema. A plain (unquoted) scalar resolves by these rules, in order; every other scalar is a string:

  | Plain scalar | Value |
  |---|---|
  | `true`, `True`, `TRUE`, `false`, `False`, `FALSE` | boolean |
  | `[-+]?[0-9]+` | integer, base 10 (`017` is 17) |
  | `0o[0-7]+` | integer, base 8 |
  | `0x[0-9a-fA-F]+` | integer, base 16 |
  | `[-+]?(\.[0-9]+\|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?` | floating-point number |
  | `[-+]?\.(inf\|Inf\|INF)`, `\.(nan\|NaN\|NAN)` | rejected: no JSON form |
  | `~`, `null`, `Null`, `NULL`, empty | null |

  So `yes`, `on`, `1_000`, `0b101` and `2030-01-15` are strings. Quoted scalars are always strings.
- Rejected: aliases (`*a`), merge keys (`<<`), every explicit tag (`!!str`, `!x`, `!`), duplicate keys, and mapping keys that are not strings (a plain `1`, `true` or `null` key). An anchor alone is allowed and has no effect.
- Integers must lie within ±2^53, and numbers must be finite.
- `.json` scenarios are JSON (RFC 8259) with no duplicate keys.
- Text is UTF-8.

## D3. Scenario hash

`scenario_sha256` is the lower-case hex SHA-256 of the RFC 8785 (JCS) serialization of the parsed scenario value, encoded as UTF-8. So comments, key order, quoting style, flow or block layout and `.yaml` versus `.json` never change it; any change to a parsed value does. JCS in short:

- object members sorted by their keys' UTF-16 code units, no whitespace anywhere;
- strings escape `"` `\` and the controls U+0000–U+001F (`\b \f \n \r \t`, otherwise `\u00xx` lower-case hex); every other character is written as itself;
- an integer is written in decimal; any other number in the ECMAScript `Number.prototype.toString` form of its shortest round-trip double (`1.0` and `1` both give `1`; `1e21`, `1e-7` and `0.000001` stay as written; `-0` gives `0`).

The `compare` section (D5) is part of the value, so changing a comparison hint makes the golden stale.

## D4. Top-level keys

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | D1 |
| `description` | no | one line for people |
| `tags` | no | list of strings: matrix dimensions and other metadata read by checkers and tools, never by executors; `generated` marks generator output, `clock-free` a scenario whose observations hold at any instant (`float-determinism.md` F6.3) |
| `clock` | yes | D5 |
| `compare` | no | D5 |
| `setup` | no | D6 |
| `actors` | no | the clients of a multi-party scenario (`sync-ops.md` S2); they make the scenario live (D9.3) |
| `server` | no | the scenario's sync server (`sync-ops.md` S3.2); only with `actors` |
| `steps` | no | D7; missing means none; `handoff` steps make the scenario live (D9.2) |

No other key is allowed.

## D5. Clock and comparison hints

- `clock.tz`: an IANA zone name. A scenario runs in exactly one zone. The oracle runs it on a worker started in that zone (R1871); Meguri's executor answers the zone's offset at each instant from the IANA database.
- `clock.start`: ISO 8601 date-time in whole seconds, `YYYY-MM-DDTHH:MM:SS`, optionally followed by `Z` or `±HH:MM`. Without an offset it is wall time in `clock.tz`: a wall time inside a DST gap is invalid, and a repeated wall time means its first (earlier) occurrence.
- `clock.mode`: `frozen` (default) or `step`. In both modes the clock stands still; only `advance` and `set_clock` move it. The mode is recorded in the golden.
- Every instant an op records (note and card ids, review times, modification times) is the clock's current instant.
- Randomness that is not seeded from collection data comes from a generator seeded with the first eight bytes of `scenario_sha256` read as a big-endian unsigned integer (Meguri's executor; the oracle has no such source to control). Each Meguri actor of an actor scenario has its own seed (`sync-ops.md` S2.5).
- In an actor scenario, every actor and the sync server share the clock; the server may run at a fixed offset from it (`sync-ops.md` S3.2).
- `compare.set`: a list of pointer patterns (`observations.md` O3). An array at a matching pointer is compared as a multiset. Use it only where the order is SQLite's unspecified row order.

## D6. Setup

Setup runs on a fresh, empty collection created at `clock.start`, in this section order whatever the key order: `config`, `presets`, `decks`, `notetypes`, `notes`, `cards`, `revlog`. In an actor scenario it runs on the first actor's collection only (`sync-ops.md` S2.2).

### D6.1 config

A mapping of collection config keys to JSON values, written in sorted key order. Examples: `fsrs`, `loadBalancerEnabled`, `rollover`.

### D6.2 presets

A list of deck option presets: `{name, <option>: <value>, ...}`. A preset named `Default` edits the collection's default preset; any other name creates a preset. A name may appear once. Options:

| Option | Value | Meaning |
|---|---|---|
| `learn_steps`, `relearn_steps` | list of durations (D7.1) | learning and relearning steps |
| `graduating_interval_days`, `easy_interval_days` | integer | first review intervals |
| `new_per_day`, `reviews_per_day` | integer | daily limits |
| `max_interval_days`, `min_lapse_interval_days` | integer | interval bounds |
| `starting_ease` | number | initial ease as a ratio (2.5 is stored as 2500) |
| `easy_bonus`, `hard_interval`, `interval_modifier`, `lapse_new_interval` | number | SM-2 multipliers |
| `leech_threshold` | integer | lapses before a card is a leech |
| `leech_action` | `suspend` or `tag_only` | what happens to a leech |
| `bury_new`, `bury_reviews`, `bury_interday_learning` | boolean | sibling burying |
| `desired_retention` | number | FSRS desired retention |
| `fsrs_params` | list of numbers | FSRS parameters |

### D6.3 decks

A list of deck names (`::` nests) or `{name, preset}`. A deck without a preset uses `Default`. The preset must be `Default` or defined in `presets`.

### D6.4 notetypes

A list of `{name, kind, fields, templates, css}`: `kind` is `standard` (default) or `cloze`; `fields` a non-empty list of field names; `templates` a non-empty list of `{name, front, back}` (template texts, empty when missing); `css` a string (empty when missing). Scenarios define their own note types and CSS; stock note types appear only in scenarios about interop with them (R1959).

### D6.5 notes

A list of notes, each as the `add_note` argument (D7.2), added in order. Each note gets the next `#nN`; its cards get the next `#cN` in template order.

### D6.6 cards (draft)

A list of `{card: #cN, <column>: <value>, ...}` written into the stored card row after the notes are added: `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses`, `left`, `odue`, `flags` (integers) and `data` (an object). The scheduler area may extend this list when the driver supports it.

### D6.7 revlog (draft)

A list of review-history rows `{card: #cN, id, ease, ivl, lastIvl, factor, time, type}` (integers; `id` is the review instant in epoch milliseconds), inserted in order. The scheduler area finalizes it with the driver support.

## D7. Steps

A step is a mapping with exactly one key, the op name. Steps run in order. Each step produces one output (`observations.md` O1).

### D7.1 Values used by several ops

- **Logical ids.** `#nN` is the N-th note and `#cN` the N-th card created in the scenario, from 1, by setup or by ops, in creation order; a note's cards are numbered in template order (R1884). A reference to a note or card that does not exist yet is an `unknown_ref` error. In an actor scenario the numbering spans all actors (`sync-ops.md` S2.4).
- **Duration.** One or more `<digits><unit>` parts, unit `d` (86 400 s), `h`, `m` or `s`: `20m`, `1d`, `23h40m`, `90s`. A day is always 86 400 seconds, also across a DST change. In steps (`learn_steps`) a duration is converted to minutes.
- **Date-time.** As `clock.start`.
- **Rating.** `again`, `hard`, `good` or `easy`.

### D7.2 Ops

Ops marked *draft* have no driver support yet. Their arguments are the v0 shape; the owning area fixes the arguments and the output when it adds the driver op, and updates this table, the schema and the typed model together. A scenario is checked against the arguments `scenario.schema.json` requires when it loads, in T0 too; other keys of a draft op are allowed until its area fixes it. An op records no rejection (D7.3) unless its row lists rejection kinds.

| Op | Argument | Output | Status |
|---|---|---|---|
| `advance` | duration | none | stable |
| `set_clock` | date-time | none | stable |
| `add_note` | `{notetype, deck?, fields, tags?}`: `fields` is a list of strings in field order (missing trailing fields stay empty) or a mapping of field name to string; `tags` a list of strings; `deck` a deck from `setup.decks` (default deck when missing). Field values and tags must be strings: quote numbers, booleans and null. | `{note: #nN, cards: [#cN, ...]}` | stable |
| `answer` | `{card: #cN, rating, time_ms?}`: `time_ms` is a non-negative integer, 0 when missing | none | stable |
| `open` | `{fixture: <path under conformance/fixtures/>}`: replace the collection with a copy of a fixture file and open it, upgrade included (`storage-ops.md` P1); only in scenarios with no `setup` whose first step is `open` | none; rejections `file_too_old`, `file_too_new`, `v1_scheduler` | stable |
| `observe` | list of observation requests (`observations.md` O2); the pure kinds `text` and `fsrs_next` (`float-determinism.md` F5) never touch the collection | `{<kind>: <value>, ...}` | stable |
| `edit_note` | `{note: #nN, fields?, tags?}`; `fields` as in `add_note` (a list replaces from the first field, later fields keep their text), `tags` replaces all tags | none | stable (`record-ops.md` W2) |
| `delete_notes` | `{notes: [#nN, ...]}`: notes that no longer exist are skipped | none | stable (`record-ops.md` W3) |
| `set_config` | `{key, value}`: one collection config key and its JSON value | none | stable (`record-ops.md` W4) |
| `add_note_tags`, `remove_note_tags` | `{notes: [#nN, ...], tags: <text>}`: add the tags of the text to the notes, or remove the matching tags from them | `{count}` | stable (`tag-ops.md` G1, G2) |
| `delete_tags` | `{tags: <text>}`: remove the tags and their descendants from every note and from the registry | `{count}` | stable (`tag-ops.md` G3) |
| `rename_tag` | `{from: <name>, to: <text>}` | `{count}`; rejection `invalid_name` | stable (`tag-ops.md` G4) |
| `reparent_tags` | `{tags: [<name>, ...], parent: <name or "">}` | `{count}` | stable (`tag-ops.md` G5) |
| `clear_unused_tags` | `{}` | `{count}` | stable (`tag-ops.md` G6) |
| `set_tag_collapsed` | `{tags: [<name>, ...], collapsed: <boolean>}` | none | stable (`tag-ops.md` G7) |
| `set_flag` | `{cards: [#cN, ...], flag: <integer>}` | `{count}`; rejection `invalid_flag` | stable (`tag-ops.md` G8) |
| `bury`, `suspend` | `{cards: [#cN, ...]}` | none | draft (sched) |
| `forget` | `{cards, restore_position?: bool, reset_counts?: bool}` | none | draft (sched) |
| `set_due` | `{cards, days: <due-date text such as "0", "1-7" or "3!">}` | none | draft (sched) |
| `reposition` | `{cards, start, step?, randomize?, shift?}` | none | draft (sched) |
| `update_preset` | `{name, <option>: <value>, ...}` with the D6.2 options | none | draft (deckconfig) |
| `legacy_update` | `{notetype \| deck \| preset: <name>, patch: <object>}`: merge `patch` into the record's schema-11 object and save the result (`model-ops.md` L2) | none; rejection `invalid_json` | stable (`model-ops.md` L2) |
| `build_filtered` | `{name, search, limit?, order?, reschedule?}` | `{deck: <name>, cards: <count>}` | draft (sched; preservation only) |
| `import` | `{fixture: <path under conformance/fixtures/>, ...options}` | counts defined by the package area | draft (package) |
| `export` | `{format: apkg \| colpkg, name: <file name>, ...options}`; the legacy package form is exactly `{format: apkg, name, legacy: true}` (`model-ops.md` L6) | legacy form: `{notes: <count>}`; other forms: `{name, sha256}` of the file written | draft (package); the legacy form is stable (`model-ops.md` L6) |
| `check_db` | `{}` | the problem counts of Anki's Check Database (`collection-check.md` K5, K10); rejection `corrupt` | stable (`collection-check.md` K10) |
| `db_exec` | `{sql, params?: [...]}`: one raw SQL statement on the schema-18 tables, to prepare stored states and read rows | `{rows: [[...], ...]}`, `[]` when it returns none | stable (`record-ops.md` W1) |
| `reopen` | `{}`: close the collection and open it again from its file | none | stable (`sync-ops.md` S6) |
| `actor` | a name from `actors`: the actor that runs the next steps | none | stable (actor scenarios, `sync-ops.md` S2.3) |
| `sync` | `{kind: login \| normal \| full_upload \| full_download}` | `{result: <result>}`: `ok`, or for `normal` one of `no_changes`, `full_sync`, `full_upload`, `full_download`; rejections `clock_incorrect`, `network` | stable (actor scenarios, `sync-ops.md` S4) |
| `net_fault` | `{kind: reset, direction: to_client \| to_server}` or `{kind: clear}` | none | stable (actor scenarios, `sync-ops.md` S5) |
| `converge` | `{}` | `{differences, dump}` (`sync-ops.md` S7.5) | stable (actor scenarios, `sync-ops.md` S7) |
| `handoff` | `meguri` or `oracle`: the side that runs the next steps | none | stable (live scenarios, D9.2) |

An op with no output records `{}`; an op with output records `{<op>: <output>}`. An op's output never has a member named `error`.

### D7.3 Rejections

- When Anki refuses an op as part of its behavior (a duplicate name, an invalid input, a missing note), the step records `{<op>: {"error": "<kind>"}}`, the op changes nothing, no logical id is assigned, and the run goes on with the next step.
- Kinds are lower-case identifiers (`[a-z][a-z0-9_]*`) from the op's closed list in the D7.2 table, which the owning area writes with the driver op; the observation schema restricts that op's `error` to the list. A refusal whose kind is not in the list is a run error (`anki_error` on the oracle), until the area adds the kind. Kinds are compared exactly, never the message (R1887).
- Executor failures stay run errors: `invalid_scenario`, `unsupported_op`, `unknown_ref`, `worker_died` and the other protocol error kinds stop the run and give the scenario `error` (D10).

## D8. Executors and capabilities

- An executor runs a scenario and returns its observations or an error. The oracle executor is a pool of oracle workers (`conformance/schema/protocol.json`), keyed by time zone. Meguri's executor runs the core in-process, with a scenario clock (D5) and the seeded random source, through the time and randomness seams (R1875).
- Each executor declares capabilities: the setup sections, ops and observation kinds it implements. Before running, the runner compares the scenario's needs with them. A scenario that needs anything missing is `pending` (D10) and is not run.
- Meguri's executor implements a part of the language through per-area adapters (`core/crates/meguri-testkit/src/exec/<area>.rs`). `meguri-cli conformance capabilities` lists them. The adapters that write and open a collection in a handoff directory count as the `handoff` op. The ability to play an actor of an actor scenario, through the parties executor, counts as the `actor` op (`sync-ops.md` S10.2).

## D9. Area metadata and live scenarios

### D9.1 area.yaml

`conformance/scenarios/<area>/area.yaml` (schema `area.schema.json`), at most one per area (`area.yml` counts: both is an error):

```yaml
area: smoke
active: [runner-empty]   # Meguri must pass these
```

- Entries are patterns over scenario ids relative to the area directory (`runner-empty` is `smoke/runner-empty`), with `*` matching any run of characters (also `/`) and `?` one character. An entry that matches no scenario is an error.
- `active` scenarios are the area's promise: Meguri passes them (in T0, or for a live scenario in T1). Areas add scenarios here as their implementation lands.
- An area with no `area.yaml` has no active scenarios.

### D9.2 Live scenarios

A scenario with `handoff` steps is live: Meguri's output is the oracle's input (export then import, Check Database on a Meguri-written collection, KTD23 T1). Sync between Meguri and Anki clients uses actors instead (D9.3).

- **Segments.** Handoff steps split the steps into segments. Each `handoff: <side>` ends a segment and names the side that runs the next one; handoffs alternate, so the first segment runs on the side the first handoff leaves (`handoff: meguri` first means the oracle runs the setup and the first steps). Handing over to the side that already has the collection is invalid.
- **What crosses.** At a handoff the side closes its collection and copies its session directory (the collection file `collection.anki2`, its media and every file an op wrote, such as an `export`'s package) to `<handoff dir>/<scenario id>@<handoff step index>/` (`@` is outside the id alphabet, so one scenario's handoff directories never lie inside another's, such as `smoke/rt` and `smoke/rt/1`, which T1 runs at the same time), together with the clock (epoch seconds) and the real ids behind `#nN` and `#cN` (protocol.json `handoff_state`). The next side copies that directory, opens the collection at the same clock and keeps numbering logical ids after the handed ones. It runs no setup. The handoff directory is one directory both see: the runner's on the host, mounted into oracle workers (`ORACLE_HANDOFF`). Areas add files to the session directory with the driver. Clients that sync with each other are actors (D9.3), not handoffs.
- **Golden.** A live scenario has a golden like any other, recorded by `tools/mg oracle record`: the oracle runs every segment itself, and each handoff only closes and reopens its collection. So the golden is what the oracle observes of its own output.
- **Pass.** T1 runs the segments on their sides (the chain) and compares the chain's observations, the oracle's observations of Meguri's output included, with the golden by the usual rules and deviations (`observations.md` O7). A handoff step records `{}`.
- **Capabilities.** Meguri needs `handoff`, the setup sections when it runs the first segment, and the ops and observation kinds of its own segments. Without them the scenario is `pending`.
- Meguri's executor alone, and T0, never run a live scenario.

### D9.3 Actor scenarios

A scenario with `actors` is live: several clients, each with its own collection, sync with a sync server that the oracle worker runs for the scenario (`sync-ops.md`).

- It has no `handoff` step. The ops `actor`, `sync`, `net_fault` and `converge` and the key `server` need `actors`.
- **Golden.** The oracle plays every actor, Meguri actors included (`sync-ops.md` S10.1).
- **Runs.** The parties executor runs it in T1: first the oracle plays every actor (the replay), then Meguri plays the Meguri actors (the mixed run), each compared with the golden (`sync-ops.md` S10.3).
- **Capabilities.** What the Meguri actors use (`sync-ops.md` S10.2); nothing when it has no Meguri actor.

## D10. Status of a scenario

The runner gives each scenario exactly one status, checking in this order:

| Status | When | Fails T0 | Fails T1 |
|---|---|---|---|
| `invalid` | the file does not parse, violates the schema (the runner checks what `scenario.schema.json` requires, not only `goldens-check`), or its id does not match its path; on macOS also when the scenario's overlay fails its schema or the `fp-libm-boundary` class (`float-determinism.md` F7.4, F8) | always | always |
| `missing` | no golden | if active | always |
| `stale` | the golden's `scenario_sha256` differs from the scenario's hash, or its `oracle.lock_sha256` differs from the current pin (`conformance/oracle.json`); on macOS also when the scenario's overlay is stale or orphaned (`float-determinism.md` F7.3) | if active | always |
| `pending` | Meguri lacks a setup section, op or observation kind the scenario uses (for a live scenario: what its own segments use, D9.2, or what its Meguri actors use, D9.3) | if active | if active |
| `live` | T0 only: a live scenario Meguri could join; T0 does not run it | never | (not in T1) |
| `error` | an executor failed while running it (a panic in an adapter included) | if active | always |
| `fail` | observations differ from the golden after comparison rules and deviations | if active | always |
| `pass` | observations match | never | never |

- **T0** (`tools/mg conformance`, and the `conformance` test binary under `tools/mg core-test`) runs every scenario that is not live against its golden with Meguri's executor, natively on macOS and in the Linux dev container. It fails when any scenario is `invalid` or any active scenario is neither `pass` nor `live`. Other statuses are listed in the report.
- **Freshness** (`tools/mg goldens-check`) fails when any scenario is `invalid`, `missing` or `stale`, when a golden has no scenario, when an `area.yaml` entry matches nothing, when an overlay in `conformance/goldens-apple/` is invalid, stale, orphaned or holds an entry outside the `fp-libm-boundary` class (`float-determinism.md` F7.4), or when a pin in `conformance/oracle.json` differs from its lock file (`lock_sha256` from `oracle/requirements.lock`, `macos_lock_sha256` from `oracle/macos/requirements-macos.lock`; checked where `oracle/` is present). These checks run on every platform. With a base revision it also lists every golden value that changed since that revision (golden, pointer, value before and after), grouped by area and observation kind (the review step of an oracle bump, R1874); the text shows the first 20 per group and the `--report` JSON all of them.
- **T1** (`tools/mg conformance --live`) first re-records missing and stale goldens through `tools/mg oracle record` (the only writer of goldens), then runs T0, then runs every selected scenario on live oracle workers: the oracle replays it against its golden with no deviation applied (deviations describe Meguri, D13), and a live scenario whose replay passes then runs as a chain (D9.2), or as a mixed run of an actor scenario (D9.3), and is compared with the same golden, deviations applied. Workers must answer the pinned `anki_version` and `lock_sha256` and all run the same driver, or the run is an `error`. Any scenario that is not `pass`, active or not, fails T1, except a `pending` one that is not active: an oracle that no longer reproduces its golden is drift to fix before anything else.

## D11. Goldens

A golden is written only by `tools/mg oracle record` (R1885). There is no path that turns Meguri's output into a golden.

```json
{
  "clock": {"mode": "step", "start": "2030-01-15T10:00:00"},
  "observations": {"steps": [ ... ]},
  "oracle": {"anki_version": "26.9.3", "driver_sha256": "<hex>", "lock_sha256": "<hex>"},
  "scenario_id": "smoke/timing-advance",
  "scenario_sha256": "<hex>",
  "tz": "Europe/Paris"
}
```

- `lock_sha256` is the SHA-256 of the oracle's lock file, which pins the `anki` wheel hashes for both CPU architectures; goldens do not depend on the architecture. `driver_sha256` identifies the driver build and never makes a golden stale.
- `clock.mode` is `frozen` when the scenario leaves it out.
- A live scenario's golden comes from the oracle running all its segments (D9.2).
- Text: UTF-8 JSON, keys sorted, two-space indents, an array of scalars on one line with `, ` between items, `": "` after keys, an empty object as `{}`, LF line ends and a final newline.
- `conformance/oracle.json` records the current pin, `{anki_version, lock_sha256, macos_lock_sha256}` (`macos_lock_sha256`: the native lane's lock, `float-determinism.md` F6.1). Adopting an oracle bump updates it together with both lock files, every golden and every overlay, in one change.
- **Overlay goldens.** `conformance/goldens-apple/<id>.json` holds, for a scenario whose observations on the macOS native oracle differ from its golden, the differing values (`float-determinism.md` F7). Only `tools/mg oracle record --platform apple <filter>` writes them, never a person and never Meguri's output. `tools/mg goldens-check` validates them for freshness and for the `fp-libm-boundary` class (F7.4, F8); the runner applies them on macOS only (F9).

## D12. Failure report

For every scenario that is not `pass`, the runner reports (R1888):

- the scenario id and status;
- for `fail`: the index of the first step whose output differs and the differences as JSON pointers into the observations (`/steps/3/next_states/#c1/good/scheduled_days`), each with its expected and actual value (`<absent>` for a missing member), the first 20 in pointer order plus the total count;
- for `pending`: the missing setup sections, ops and observation kinds;
- for `stale`: which hash differs and the command that fixes it (`tools/mg oracle record <id>` for a scenario change, `tools/mg oracle bump <version>` for the oracle pin);
- for a T1 `fail` or `error`, whether the oracle's replay or the live chain differed;
- a one-line repro command, `tools/mg conformance <id>` (`tools/mg conformance --live <id>` for T1 and live scenarios).

`meguri-cli conformance run --report <file>` writes the same as JSON: `{runner, tier: t0|t1, executor, lane, oracle_pin, wall_ms, totals, failed, scenarios: [{scenario, path, area, active, live, status, fails, message, repro, scenario_sha256?, first_divergence?, diffs?, diff_count?, missing?, deviations_applied?, overlay_applied?}]}`, where `fails` follows the report's tier and `scenario_sha256` is the hash of the scenario text the status is for (absent when the file did not load); a T1 run adds the workers' `oracle` metadata. On macOS, `overlay_applied` is the number of overlay entries applied to the scenario's expected values (present when it has an overlay), and `totals` gains `overlay_applied` (entries) and `overlay_scenarios` (scenarios with at least one); the text report adds `overlays: <entries> values in <scenarios> scenarios` (`float-determinism.md` F9).

## D13. Known deviations

`conformance/deviations.yaml` (schema `deviations.schema.json`) lists every deliberate difference from the oracle (R1889):

```yaml
deviations:
  - id: search-unicode-case-folding
    requirement: R1036
    summary: Plain and field searches fold non-ASCII letters, so Meguri can match more notes.
    test: search/folding/unicode-mode        # scenario id or test id asserting Meguri's behavior
    scenarios: [search/folding/umlaut]       # patterns over scenario ids (D9 syntax)
    pointer: /steps/*/search/ids              # pointer pattern (observations.md O3)
    transform: {replace: ["#n1", "#n2"]}
```

- The runner applies an entry's transform to the expected (oracle) value at every pointer matching `pointer` in every scenario matching `scenarios`, before comparing. Applied entries are listed in the report. Only comparisons that hold Meguri's observations apply them (T0, live chains); the oracle's replay of its own golden in T1 never does.
- Transforms: `replace: <value>` makes the expected value `<value>`. `apple_overlay: true` (only the entry `fp-libm-boundary`, R1889, `float-determinism.md` F8.6): in T0 on macOS, each entry of the scenario's overlay whose pointer lies at or under a matching pointer puts its `apple` value there; on Linux, in live chains and for scenarios without an overlay, it does nothing. Areas add transforms in `core/crates/meguri-testkit/src/diff/transforms.rs` with their spec.
- Order: the `apple_overlay` entry applies first, then the other entries in file order, so a deviation describes Meguri against Anki on the same platform.
- No other difference is tolerated: an unlisted difference fails.
- `test` names the scenario or test that asserts Meguri's own behavior; every entry needs one.

## D14. Harvest catalog

Every behavior test in Anki at the pinned tag is inventoried (R1877–R1882). The catalog is `conformance/catalog/harvest.csv`, RFC 4180 CSV with a header row:

| Column | Meaning |
|---|---|
| `harvest_id` | opaque id: `h` and 12 hex digits |
| `language` | `rust`, `python` or `typescript` |
| `source` | `tag`: a test in Anki at the pinned release; `supplementary`: a test added on Anki's main branch after it |
| `area` | a Meguri area (spec area names), or `desktop` and `tooling` for groups with no Meguri counterpart |
| `classification` | `in-scope`, `preservation-only`, `out-of-scope`, `not-applicable` or `unclassified` |
| `reason` | one line in Meguri's words; required unless `unclassified` |
| `scenarios` | scenario ids that cover the test, separated by spaces |
| `status` | `open` (needs classification or scenarios), `deferred` (in scope, scenarios wait for a later unit) or `done` |

- `done` requires a classification other than `unclassified`, and linked scenarios unless the classification is `out-of-scope` or `not-applicable`. `unclassified` rows are `open`.
- The catalog holds no Anki file names, paths or test names. The link from a harvest id to the Anki test lives only on the spec side, in `oracle/harvest/links.csv` (R1881).
- Expected results of harvested scenarios come from the oracle, never from Anki's assertions (R1878).
- Supplementary rows come from `tools/mg harvest-inventory --supplementary` (a checkout of Anki's main branch): tests whose language, file and name the tag does not have. Their spec writer classifies whether the behavior exists at the tag (in scope or preservation only when it does, not applicable otherwise). They stay through re-inventories at the tag, and become `tag` rows with the same id once a bump's release has them. They are not in the counts at the tag.
- `tools/mg harvest-report` writes the coverage report: the counts at the tag per language (supplementary rows apart), rows per language and classification, and for each classification the statuses of the linked scenarios (pass, fail, pending and the rest of D10). A linked scenario's status is its T0 status; for a live scenario, its T1 status when a `tools/mg conformance --live` report is given (`--live-report`), since only T1 runs it. The report must have run at the current oracle pin (`oracle_pin`; another pin is an error), and its status counts only while T0 finds the scenario valid with a fresh golden (`live` or `pending`) and the report ran the scenario's current text (same `scenario_sha256`); otherwise the T0 status stands, so a scenario that is `invalid`, `missing` or `stale` never shows an older T1 pass. Every row appears with its classification or `unclassified`. Where `oracle/harvest/links.csv` is readable, it also checks that no catalog value contains an Anki file name or test name, that each row's source matches links.csv, and that links.csv was inventoried at the oracle pin's release.
- On an oracle bump, new or changed tests at the new tag are inventoried in the same change (R1882): `tools/mg harvest-report` fails until `tools/mg harvest-inventory` has run at the new release. `links.csv` keeps a SHA-256 of each test's source (`body_sha256`; Rust: from `#[test]` to the end of the function, Python: the function or method with its decorators, TypeScript: the test call), never its text. A re-inventory lists every test whose source changed under `changed since <old tag>` and sets its row's status back to `open`, keeping its id, classification, reason and scenarios, so its spec writer reviews it; a row the inventory classifies by path alone (`desktop`, `tooling`) keeps its status.
