# Oracle harness, scenario format, sync rig, fuzzing, similarity scan: design recommendations

Date: 2026-10-12. Scope: R1868–R1960 (compatibility oracle, harvested tests, scenarios, sync, fuzzing, clean room, tiers).
This document builds on private research note 13 (conformance oracle). It does not repeat that note. It adds decisions and fresh probe results.
All probes ran today in Docker 29.8.2 on arm64 (M3 Max). Probe files are in the session scratchpad and are not committed. No probe code is meant for the repo.

Source labels: **[probe]** = measured today. **[refs]** = read in `refs/anki`. **[docs]** = upstream docs or release pages. **[judgment]** = recommendation.

---

## 0. Findings that change the plan

1. **Apple libm and glibc give different f32 results, so exact next states on Apple are at risk.** [probe] fsrs-rs 6.6.2 calls `f32::powf`, `exp` and `ln`. I ran these on 1M FSRS-range inputs. About 0.6% of `powf`/`expf` results differ by 1 ulp between macOS (Apple libm) and Linux (glibc). The oracle runs on glibc. Meguri ships on Apple libm. Next-state intervals therefore differ in rare boundary cases. R1890 ("match exactly") will fail on a few cards out of tens of thousands. The `libm` crate is not a fix: it differs from both Apple libm and glibc in about 10% of cases. What the plan should do:
   - Run the T0 golden suite **natively on macOS**, not only in Linux Docker. Linux-only conformance tests a platform Meguri never ships on.
   - Measure early. In milestone 1, run the 14.4k-card next-state snapshot on macOS.
   - Fallback if mismatches appear: patch fsrs-rs (BSD, a fork is allowed) so it calls a vendored port of ARM optimized-routines `expf`/`logf`/`powf` (MIT/Apache). glibc's flt-32 routines come from that code, so the port would likely be bit-identical to glibc on both platforms. Verify this before relying on it. The other option is a documented known deviation (R1889) for float-boundary cases.
2. **`refs/anki` is not at the pinned release.** [refs] HEAD is `26.09.3-36-g2f62b5e`, which is 36 commits past the tag on `main`. Those commits include the `{{type:ci:}}` filter, a fix for clozes inside MathJax, a `deck_for_adding` fallback fix, and **rand 0.9.4 → 0.10.1**. Spec writers must read the tag (`git worktree add refs/anki-26.09.3 26.09.3`). Otherwise specs describe behavior the 26.9.3 oracle does not have.
   - Correction to note 13: at the tag, rslib's fuzz uses **rand 0.9.4**, not 0.10.1. fsrs 6.6.2 pulls in 0.10.1.
   - [probe] Over 20,000 seeds, `StdRng::seed_from_u64(s).random_range(0f32..1)` and the u32 range draws were bit-identical between rand 0.9.4 and 0.10.1. The risk is low. Still pin the version the tag uses, and keep the 10k-pair table (R1892).
3. **libfaketime traps.** [probe]
   - **`DONT_FAKE_MONOTONIC=1` is mandatory.** With a frozen clock and a faked monotonic clock, `sync_login` fails with `NetworkError` because tokio/hyper timers never advance. With the flag set, full upload and download under a frozen clock work, taking 0.36 s.
   - **Calling `setenv("FAKETIME")` at runtime does nothing.** Only `FAKETIME_TIMESTAMP_FILE` with `FAKETIME_NO_CACHE=1` moves the clock inside a process. With that setup, writing the file moved `days_elapsed` from 0 to 5.
   - The timestamp file costs about 1.1 µs per clock read, against 0.06 µs without it. That is negligible.
4. **Goldens do not depend on CPU architecture.** [probe] 435 FSRS next-state snapshots (load balancer on, fuzz on, 40 simulated days) plus memory states gave the same hash on arm64 and on amd64. amd64 ran under qemu, so confirm once on a real x86 CI runner. What this changes:
   - CI can use x86_64 runners.
   - R1885's "wheel hash" should be the **hash of the lock file**, which records both per-arch wheel sha256 values. The two wheels have different hashes but produce the same output:
     - aarch64: `d994c996dcb5a619049291d829500f5bb0bbae5fe36058f1d43daadd59b5b804`
     - x86_64: `7b3dcb6c9e06e8087ed66bb70f35356e5c60686f51e3bde25f6328433072132f`
5. **The harvest counts in R1877 are off.** [refs] At tag 26.09.3 I count:
   - 673 Rust `#[test]`. 660 of them are in rslib: scheduler 132, import_export 103, notetype 67, search 64, decks 45, card_rendering 31, sync 25, notes 24, tags 22, media 20, cloze.rs 19, typeanswer.rs 12, and so on.
   - 225 Python tests: pylib 155, qt 64, tools 6.
   - 34 TS tests.
   - `main` has 768 Rust tests. The 95 tests added after the tag (sync upload/login, simulator review orders, storage migration, deckconfig) mostly cover behavior that already exists in 26.9.3. Harvest them too. Their expected values come from the oracle anyway, so this coverage costs nothing extra.
   - Estimate: about 500 in-scope Rust tests and about 110 pylib tests. Nearly all qt tests are N/A.
   - Plan wording: "count at the pinned tag; record the counts in the coverage report" rather than fixed numbers.
6. **Meguri→Anki directions cannot be golden files.** [judgment] When Meguri's output is the oracle's input, there is nothing to precompute:
   - Meguri exports an apkg and Anki imports it.
   - Meguri does a full upload and Anki downloads it.
   - Anki runs Check Database on a Meguri-written collection (R1925).
   - Meguri syncs against the server.

   These suites are **live only**. That is what defines tier T1. Everything that runs Anki on scenario inputs alone is a committed golden in T0.

---

## 1. Oracle container

### Image [probe + judgment]
```dockerfile
FROM python:3.12-slim-trixie@sha256:<pin>   # trixie: glibc 2.41, libfaketime 0.9.10 (verified)
RUN apt-get update && apt-get install -y --no-install-recommends libfaketime tzdata \
 && rm -rf /var/lib/apt/lists/*
COPY requirements.lock /tmp/
RUN pip install --no-cache-dir --require-hashes -r /tmp/requirements.lock
COPY driver/ /opt/driver/
ENV DONT_FAKE_MONOTONIC=1 PYTHONDONTWRITEBYTECODE=1
```
- **Use trixie, not bookworm.** Debian 12 left regular security support in mid-2026. Both work. The image builds in about 12 s and is 267 MB.
- **`requirements.lock`** is generated with `uv pip compile --generate-hashes` in Docker. It hashes anki 26.9.3 (both Linux wheels) plus protobuf, orjson, markdown, requests, decorator, distro and typing-extensions. The probe resolved protobuf 7.36.2 and orjson 3.13.0.
- **The libfaketime path depends on the architecture.** Set `LD_PRELOAD` in an entrypoint script that uses `dpkg --print-architecture`. Do not hard-code it.
- Tag the image as `meguri-oracle:26.9.3-<sha256 of oracle/ tree>`. Build it locally and in CI. Never push it (R1869). A BuildKit layer cache is enough.

### Measured cost of oracle operations (frozen clock, arm64) [probe]
| Operation | Cost |
|---|---|
| Container start (`docker run --rm`) | 0.39 s |
| `import anki` | 80–115 ms |
| New `Collection()` (create, then close) | 24–48 ms |
| Copy a cached template `.anki2`, then open and close | 1–2 ms |
| Add note | 2–5 ms (frozen clock adds id-collision bumping) |
| Get queue + answer | 2–4 ms |
| `get_scheduling_states`, render, `find_cards` | < 0.1 ms each |
| Two-client full sync through `anki.syncserver` | 0.36 s |

A typical 10-op scenario costs about 100 ms on a warm worker. **Long-lived workers are required.** Paying container start plus import for every scenario would be 5× slower. Template-copy creation is a later optimization: key the cache by (start instant, TZ), because `crt` and ids come from the clock.

### How a scenario runs [judgment]
- **One worker = one long-lived process = one TZ = one clock file.** The runner keeps a pool keyed by TZ and starts workers on demand. Each worker runs:

  ```sh
  docker run -i --rm --tmpfs /work -e TZ=… -e FAKETIME_TIMESTAMP_FILE=/work/clock -e FAKETIME_NO_CACHE=1 meguri-oracle:<tag>
  ```

  It speaks JSON lines on stdin and stdout. No ports are needed.
- **Each scenario gets a fresh collection in `/work/<uuid>/`** (tmpfs). Never put SQLite on a bind mount: virtiofs locking is slow and fragile on macOS. After the scenario, close the collection and delete the directory.
- The driver itself writes the clock file at `clock.start` and at each `advance`. Use frozen values only, never the `@` form, so ids and fuzz stay deterministic. The driver passes `answered_at_millis` from the scenario clock, not from `time.time()`.
- **Protocol (Meguri-owned, MPL schema in `conformance/schema/protocol.json`):**
  - `hello`: returns `{protocol, anki_version, lock_sha256, driver_sha256, tz}`.
  - `run`: takes `{scenario}` and returns `{observations[], error?}`. This is batch mode: one round trip per scenario.
  - `begin` / `step` / `end`: interactive mode for agents debugging one step.

  Batch mode is the default, including for fuzzing (§4). It is simpler, and replaying a 0.5 s scenario to find the first divergence costs little.
- **Do not use a CLI that is invoked once per scenario.** That costs 0.5 s per invocation, against about 0.1 s for a scenario on a warm worker. Do not use HTTP JSON-RPC either: stdio needs no port management and maps onto `docker run -i`.

### AGPL quarantine layout [judgment]
```
oracle/                    AGPL-3.0-or-later (LICENSE here). Spec/infra side only.
  LICENSE  AGENTS.md  Dockerfile  entrypoint.sh  requirements.lock
  driver/                  Python package: protocol loop + DSL interpreter (imports anki)
conformance/               MPL-2.0. Shared by both sides.
  schema/                  scenario.schema.json, observation.schema.json, protocol.json
  scenarios/<area>/…yaml   inputs only
  goldens/<area>/…json     oracle output
  fixtures/                oracle-produced .apkg/.colpkg (small, data)
  deviations.yaml          R1889 known deviations, each with a test id
  runner/                  Rust crate meguri-conformance (implementer side)
  compose.sync.yaml
specs/                     prose specs (MPL). Provenance notes stay spec-side (R1954).
```
- **Implementer isolation:**
  - Implementer worktrees use `git sparse-checkout` to leave out `oracle/` and `refs/`. The files are never on disk.
  - Deny rules add a second layer: `Read(oracle/**)`, `Read(refs/**)`, and Bash patterns for `git show`/`git log -p` on those paths.
  - Implementers run the oracle only as a prebuilt image tag.
  - Git history still holds the driver. That is acceptable: the clean room is evidence, not perfection.
  - A stronger option, if wanted later: move the driver to a separate private repo pinned by image digest.
- **The DSL semantics are spec, not driver.** The schema and the prose that define what each op and observation means live in `conformance/schema` and `specs/conformance/`. Both adapters implement them.
- **Scenarios define their own note types and CSS.** Never use Anki's stock "Basic" etc., except in tests that are explicitly about interop with stock note types. This keeps Anki's stock template and CSS text out of the goldens (R1959).

---

## 2. Neutral scenario format and runner

### Format [judgment + docs]
- **Authored scenarios:** YAML restricted to a JSON-compatible subset (no tags, no merge keys), validated against a JSON Schema.
- **Machine-generated scenarios** (matrices, fuzz regressions): JSON or JSONL.
- **Rust parsing:** `serde-saphyr` 1.3 (typed, no unsafe, maintained). Do **not** use `serde_yaml`, which was archived in 2024, or `serde_yml`, which has advisory RUSTSEC-2025-0068. Python side: PyYAML `safe_load`.
- **Scenario hash** = sha256 of the canonical JSON of the *parsed* scenario, so whitespace and comment edits do not invalidate goldens.

Sketch:
```yaml
id: sched/fsrs/lapse-then-good
clock: {tz: Europe/Paris, start: "2030-01-15T10:00:00"}
setup:
  notetypes: [{name: QA, fields: [Q, A], templates: [{name: C1, qfmt: "{{Q}}", afmt: "{{FrontSide}}<hr>{{A}}"}], css: ".c{}"}]
  presets: [{name: P, fsrs: true, desired_retention: 0.9, learn_steps: [1m, 10m], load_balancer: false}]
  decks: [{name: A, preset: P}]
  notes: [{notetype: QA, deck: A, fields: [q1, a1]}]
steps:
  - observe: [next_states(#c1), card(#c1)]
  - answer: {card: "#c1", rating: good}
  - advance: 3d
  - answer: {card: "#c1", rating: again}
  - observe: [next_states(#c1), card(#c1), revlog(#c1), queue(deck: A), timing]
```
- **Op set v0:** `add_note`, `edit_note`, `delete_notes`, `answer`, `advance`, `set_clock`, `bury`/`suspend`/`forget`/`set_due`/`reposition`, `update_preset`, `build_filtered`, `import`/`export` (fixture paths), `check_db`, `db_exec`, `sync`.
  - `db_exec` is raw SQL on the schema-18 tables. It is needed for corruption injection (R1924) and for the "time travel by data" that harvested Anki tests use. The schema is an interop fact, so this is allowed.
- **Observations:**
  - `next_states`: per button `{kind, interval_secs | interval_days, memory:{s,d}}`. No labels.
  - `card` and `revlog` rows.
  - `queue`: ordered logical ids, queue kind, counts.
  - `deck_tree` counts and `timing`.
  - `render`: q/a HTML, AV tags, type marker, error kind.
  - `search`: ordered ids or `parse_error`.
  - `db_dump`: normalized tables, with protobuf blobs compared as bytes and decoded only for the diff display.
  - `stats` subset, `check_db` counters, `sync` result kind, and `digest`, a cheap hash of all card rows used for fuzzing.
- **Logical ids:** `#cN`/`#nN` are assigned in creation order, and both systems record that order. Fuzz seeds are `card_id + reps`, so any scenario with fuzz on needs **identical real card ids**. Meguri must reproduce Anki's id allocation: millisecond timestamp from the injected clock, bumped by +1 on collision. Add an `ids` observation so a seed mismatch shows up as an id diff, not as a confusing interval diff.
- **Float trap:** both sides must print f32 values as the shortest round-trip *f32* text. In the probe, protobuf JSON gave `11.262237`. Widening to f64 before printing would give `11.26223659…`. Tolerances apply only where a scenario declares them (memory states 1e-5, stats 1e-6).
- **Golden file:** canonical JSON (sorted keys, LF) containing:
  - `{scenario_id, scenario_sha256, oracle:{anki_version, lock_sha256, driver_sha256}, tz, clock, observations}`
  - **Goldens are written only by the oracle** (`just goldens regen <filter>`). Never accept Meguri output as a golden: no insta-style "bless" flow.

### Rust runner [judgment + docs]
- **Test harness:** crate `meguri-conformance` with **datatest-stable 0.3** (file-driven, built on libtest-mimic), run under **cargo-nextest**. That gives one test per scenario file, parallelism, per-test timeouts and JUnit output.
  - `MeguriExecutor` links `meguri-core` in-process. It feeds the injected `Clock` and RNG from the scenario, and a run takes milliseconds.
  - `OracleExecutor` manages the worker pool.
- **Thin `meguri-cli conformance` front end over the same library**, for agents:
  - `run <filter> [--live]`
  - `exec <file>` (prints observations)
  - `diff <file>`
  - `capabilities` (lists the ops Meguri supports, used by the fuzzer)
- **Failure output (R1888):** a JSON report with scenario id, first divergent step, JSON-pointer diff, and the repro command.
- **Where tests run:**
  - **T0 runs both natively on macOS** (finding 1) **and in Linux Docker** (`rust:1.99-slim`, which is already present locally).
  - The T1 live runner defaults to Docker. It also has a `--host` mode that drives the dockerized oracle over `docker run -i` stdio.

---

## 3. Sync conformance rig

- **The sync server is a child of the oracle worker, not its own compose service.** The driver op `sync_server.start` runs `python -m anki.syncserver` with the worker's own `FAKETIME_TIMESTAMP_FILE`. That gives the shared clock (R1926) and per-scenario isolation for free. The probe ran exactly this setup.
  - Give each scenario a fresh `SYNC_BASE`.
  - The server reads `SYNC_USER1..N` only at start. Restart it per scenario (about 0.2 s), or pre-provision about 100 users and give each scenario a fresh one.
- **Clock-skew scenario (R1932):** start the server with a second clock file set 301 s ahead.
- **Topology:** `compose.sync.yaml` defines a network with:
  - `oracle` workers, which own the server and also act as "other device" clients;
  - `toxiproxy` (v2.12) between Meguri and the server, for drops, latency, `limit_data` and `reset_peer`;
  - spec side only: `mitmdump --mode reverse:http://oracle:8080` to record exchanges (R1933). Recordings are decoded from zstd and committed as data.

  The Meguri runner joins the same network. For iOS-simulator sync end-to-end tests, the oracle container publishes the server port on 127.0.0.1.
- **Simulating AnkiMobile and desktop:** AnkiMobile and Anki desktop run the same Rust backend as pylib, so one or more oracle clients are faithful protocol stand-ins. Only the client-version string differs. A scenario script looks like this:
  1. Client A (oracle) adds and studies, then syncs.
  2. Meguri syncs.
  3. Both edit, then both sync.
  4. Assert that the normalized dumps of A, Meguri and a fresh client C after a full download all converge.

  Make convergence across three parties the main assertion. It also covers conflict rules (newer mtime wins).
- **Media (NFC/NFD, the 28.8k-file set):** synthetic sets in T1 and nightly. The user's real media set runs only in the private T3 suite.
- **Never contact AnkiWeb (R1876).** Enforce it: the runner refuses any endpoint that is not a private address, and the containers run on an `internal: true` network except where a test needs the host.

---

## 4. Differential fuzzing

- **Generator:** Rust, using `proptest-state-machine` (the reference state machine is a cheap model of ids, decks and cards, used to produce *valid* ops) or a custom generator over a seeded ChaCha. It emits scenario-DSL JSON. Programs have 20–200 ops on 5–200 cards. Time advances are biased to rollover ±1 s and DST edges. The generator draws only ops in `meguri-cli conformance capabilities`, so fuzzing can start early and grow with the op set.
- **Execution:** run the program once on Meguri (in-process, milliseconds) and once on the oracle (batch, about 0.2–1 s). Auto-insert a `digest` observation after every op, then compare the digest lists. The first mismatch is the divergence. Replay up to that step with full observations to produce a readable diff.
- **Shrinking:** ddmin over the op list. Each candidate is a fresh oracle run of about 0.5 s, and a typical shrink takes 50–200 runs, so about 1–2 minutes per failure.
  - Dedupe failures by signature (first differing observation path + op kind), so one bug does not produce 500 regressions.
  - Save each minimal case to `conformance/scenarios/regressions/` and have the oracle generate its golden.
- **Throughput:** 12 oracle workers on the M3 Max gives roughly 20–25 programs/s, so a 30-minute night covers about 40k programs (R1937). Log the seed. Search-query fuzzing (R1918) is cheaper still, at under 0.1 ms per `find_cards`.
- **Keep the oracle out of coverage-guided fuzzing.** Use cargo-fuzz/libFuzzer only for Meguri-only robustness of the parsers (search, template, apkg/colpkg reader, sync JSON) with invariant checks (R1936). The oracle is about 1000× too slow for coverage-guided exec rates.
- **Non-comparable areas:** filtered decks with Random order use SQLite `random()`. The DSL marks that observation as `compare: set`.
- **Weekly sync fuzz:** 2–3 parties doing random edits and syncs, asserting convergence. About 1–2 s per program.

---

## 5. Code-similarity scan (R1957)

[probe] Corpus = every unique `.rs/.py/.ts/.svelte/.js` blob across all **54 tags from 2.1.51 to 26.09.3**: 6,149 blobs, 49 MB. The 2.1.50 tag is not in the local clone. Deduplicate by git blob sha, so 54 tags cost little more than one tag.

I wrote a small winnowing scanner as a probe. It uses three fingerprint channels:
- `struct`: tokens with identifiers abstracted, k=40, w=10.
- `lex`: identifier words split from camel/snake case, plus string and number literals, k=12. This channel is language-agnostic and catches transliteration that keeps names.
- `com`: comment-word 10-grams. LLM recall often reproduces comments.

Results:
- Indexing the whole corpus took **13.6 s** in Docker. Cache the index by tag-list hash.
- Scanning about 30 files took **0.2 s**.
- Calibration:
  - An Anki `review.rs` with identifiers renamed and comments stripped scored **100% struct / 100% lex**.
  - Unrelated Rust (fsrs-rs, 30 files) scored **0–1%**, with a maximum of 5% on a 37-fingerprint file.

Recommendations:
- **Per-change gate: the custom scanner** (in Docker, seconds). It is the only practical way to cover 54 tags, cross-language identifier and literal reuse, and comments in one pass.
  - Flag a file if `struct` ≥ 10% or `lex` ≥ 15% (each with ≥ 30 fingerprints), or if any `com` 10-gram matches.
  - Also flag runs of ≥ 6 consecutive matched struct fingerprints, which is about 100 tokens. That catches a copied function inside a large file.
  - Fail CI unless an allowlist entry gives a reason and a reviewer.
  - Allowlist interop matches up front: schema-18 DDL, protobuf field declarations (Meguri writes its own `.proto` with only the needed fields), sync endpoint and JSON field names, config keys, search keywords.
  - Add `.sql` and `.proto` files to the corpus. Consider adding the AnkiConnect and Migaku add-on sources too (R1956).
- **Nightly and milestone second opinion: JPlag 6.3** (Java 25, in Docker). Use `--old` with the Anki tags and `--new` with Meguri. It supports Rust (mature), Swift/TS/JS (beta) and Python, and produces a reviewable HTML report. Run it against the latest tag plus a few major tags (2.1.66, 23.10, 24.06, 25.02, 25.09).
- **Do not rely on:**
  - **Dolos**: no Swift parser; it has rust, python, typescript, javascript.
  - **jscpd** (v5.4.1): literal clones only, and no old-versus-new corpus mode.
  - **Simian**: commercial licence.
- **Swift has no Anki counterpart** (AnkiMobile is closed). The Swift risk is transliteration of TS or Python, which only the `lex` and `com` channels can catch.

---

## 6. Tiers and timings

| Tier | What | Where | When | Time budget |
|---|---|---|---|---|
| T0 | All committed goldens replayed on Meguri in-process. Freshness check: every scenario has a golden whose `scenario_sha256` and `lock_sha256` match. Similarity scan. | macOS native **and** Linux Docker | every change | < 60 s (about 10–20k scenarios at 1–5 ms each, parallel; scan < 20 s) |
| T1 | Live-only suites: Meguri output fed to the oracle (apkg/colpkg/CSV export → Anki import; Anki Check Database on Meguri-written collections; Meguri↔server sync subset). Golden generation for new or changed scenarios. Live re-run of the touched area. | Docker | every change to core/ | 2–5 min. Full live re-run of all goldens: 15k × ~0.1 s / 12 workers ≈ 2–3 min |
| T2 | Differential fuzz (30–60 min). Search-grammar fuzz. Full sync + media + fault matrix. 365-day DST simulations. JPlag. Meguri/Anki perf ratio (R1865). One x86_64 run. | Docker (+ macOS for the perf ratio) | nightly; sync fuzz weekly | 60–90 min |
| T3 | Private user-collection suite (R1942). | local Mac only | milestone, release | ~10–30 min (next states for 57.6k transitions and 14.4k renders: seconds on both sides; the full sync with 1.41 GB of media dominates) |
| T4 | On-device performance. | host, devices | milestone, release | per R1863 |

Bump procedure (R1874/R1882): bump the lock file → regenerate all goldens → have the oracle generate goldens for newly harvested tests → review a behavior-diff report that groups changed observations by area. The 26.9.3 vs `main` diff in finding 2 is a preview of what one bump contains.

---

## 7. Suggested first-milestone order

1. Create the oracle image and lock file, a driver that supports `hello` and `run`, and a protocol schema.
2. Build the runner skeleton with datatest-stable and nextest, a stub Meguri executor, and an `injected Clock` in the core.
3. Write the first goldens:
   - the fuzz table (10k `(id, reps)` pairs);
   - id allocation;
   - SM-2 and FSRS transitions with the load balancer off;
   - the rollover × TZ matrix.
4. **Check the macOS-vs-oracle float behavior** on FSRS next states (finding 1) before more scheduler work builds on it.
5. Set up the similarity-scan CI job and the implementer sparse-checkout profile.

## Sources
- PyPI JSON API for `anki` 26.9.3 (wheels, hashes, requires_dist), queried 2026-10-12.
- Release pages (via `gh api`): JPlag v6.3.0 (2025-12), Dolos v2.9.3 (2025-11) plus its parsers directory, jscpd v5.4.1 (2026-10), datatest-stable 0.3.3, libtest-mimic 0.8.2, cargo-nextest 0.9.148, toxiproxy v2.12.0, mitmproxy v12.2.3, libfaketime v0.9.13 (Debian trixie ships 0.9.10), serde-saphyr 1.3.0.
- JPlag README (languages, `--old`/`--new`): https://github.com/jplag/JPlag
- serde_yaml archived; serde_yml advisory: https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868, https://api.osv.dev/v1/vulns/RUSTSEC-2025-0068, https://github.com/bourumir-wyngs/serde-saphyr
- `refs/anki` at tag 26.09.3 vs HEAD (Cargo.toml, Cargo.lock, test counts).
- `~/.cargo/registry` fsrs-6.6.2 source (`powf`/`exp`/`ln` usage).
