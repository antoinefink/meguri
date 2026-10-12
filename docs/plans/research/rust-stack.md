# Rust crate stack for the Meguri core: decisions and pitfalls

Date: 2026-10-12. Versions and licenses come from the crates.io API, queried today. "Verified" means I built or ran it in `rust:1.99-slim` on linux/arm64 in Docker, or read it in the crate source. Everything else comes from the docs or from release notes.

Sources by authority:
- Repo guidance: AGENTS.md, the requirements plan, local-data research 02/04/06/08/09.
- Official docs and crate source: crates.io, docs.rs, the GitHub repos.
- Anki's `Cargo.toml` and `Cargo.lock`. I used these only for **third-party** crate pins, which are interface facts. I used no Anki code.

---

## 0. Findings that change the plan

1. **Pin `unicase = "=2.6.0"`.** Anki's workspace pins exactly this version, with the comment that any change could invalidate SQLite indexes. Newer `unicase` releases (2.7 to 2.10) changed the Unicode folding tables. The `unicase` collation orders the unique indexes on notetype, deck, template and field names, and the tags primary key.
   - A different fold changes the B-tree order. `integrity_check` then fails on the server after a full upload, or locally after a full download.
   - Verified: 2.6.0 builds on Rust 1.99. It folds `ß` equal to `ss`, so it uses **full** case folding, not the "simple case folding" written in research 06 §1.2. Fix the spec.
2. **Reuse Anki's third-party crates for exact behavioural parity.** This is legal: these are not Anki code. Each crate below gives oracle-identical behaviour for free, if the version is pinned to the oracle's lockfile:
   - `unicase` (collation)
   - `regex` (`re:` syntax and case folding, required by R1041/R1202)
   - `htmlescape 0.3.1` (entity decoding inside `strip_html`, which feeds `sfld` and `csum`)
   - `unicode-normalization` (NFC, and NFKD for `nc:`)
   - `sha1` (`csum`, media)
   - `csv` (CSV import)
   - `fsrs =6.6.2`
   - `rand 0.10.x` with `StdRng::seed_from_u64`. Card fuzz can then be **bit-identical** to Anki's (seed = `card.id + reps`, research 02 §5.5).

   This turns fuzz, `sfld`/`csum` and search folding from "same distribution" into exact golden tests. The spec must still describe each algorithm in its own words; only the crate is shared.
3. **fsrs 6.6.2 is pure host code. It has no burn backend at runtime and is very fast.**
   - `burn` is only a dev-dependency. Runtime deps are ndarray, rayon, rand, priority-queue, serde, snafu, strum and itertools.
   - Verified on synthetic data: 11.5k cards, 115k reviews, 103k items, 8 epochs, M3 Max inside Docker.

     | Operation | Time |
     |---|---|
     | `compute_parameters` | **72 ms** |
     | `evaluate` | 34 ms |
     | health check (`evaluate_with_time_series_splits`) | 81 ms |
     | `memory_state_batch`, 100k cards | 44 ms |
     | `next_states` ×100k | 16 ms |
     | abort, flagged after 50 ms | returned `Interrupted` at 53 ms |

   - Real collections have longer histories, and an iPhone is slower, but this is still about one second, not minutes. **The cost is in SQLite**: reading the revlog into items and writing 100k card `data` updates. So the plan should:
     - budget R1849/R1850 against the DB path;
     - make the revlog→`FSRSItem` builder and the batched `UPDATE` the performance work items;
     - drop any BGProcessingTask machinery that exists only for the optimizer.
4. **prost drops unknown protobuf fields.** Verified: a field unknown to the decoder is gone after re-encode.
   - Notetype, deck and deck-config blobs written by a newer Anki would lose data when Meguri rewrites them. Anki has the same limitation, but R1486 ("round-trip unchanged") needs Meguri to do better.
   - Generating code from Anki's `.proto` files is ruled out anyway, because those files are AGPL.
   - Decision: hand-write the message structs from the spec with `prost` derives, and wrap each blob codec with an **unknown-field capture**. Scan tags with `prost::encoding::decode_key`, keep unrecognised `(tag, wire bytes)` and append them on encode.
   - Rejected alternatives: `protobuf` 3.x (stepancheg) is end-of-life. Google's v4 crate is beta and needs protoc built from source.
   - Add a round-trip property test with injected unknown tags.
5. **Disable redirects in the HTTP client.** reqwest follows 307/308 by default and re-sends a buffered body. That would silently follow AnkiWeb's `meta` 308 and lose the `new_endpoint` that must be persisted (research 06 §2.4).
   - Use `redirect::Policy::none()` and handle 308 manually, on `meta` only.
6. **Do not rely on Swift `Task` cancellation through UniFFI.** Verified in the uniffi 0.32.2 Swift template (`Async.swift`): Rust futures are polled with `withUnsafeContinuation` and never cancelled. Cancelling the Swift Task does nothing on the Rust side.
   - Every long operation takes an explicit `CancelToken` object exported from Rust.
7. **The toolchain must move.** The machine has Rust 1.89. Current stable is **1.99.0** (2026-09-28).
   - Several current crates need 1.85 to 1.88 or newer, for example zip 9, proptest 1.12 and the icu crates.
   - Pin `rust-toolchain.toml` to 1.99. Add `aarch64-apple-ios` and `aarch64-apple-ios-sim` on the host. Docker uses the same version.

Verified stack build: rusqlite 0.40.2 (bundled), unicase 2.6.0, fsrs 6.6.2, reqwest 0.13.5 (rustls), tokio 1.53, axum 0.8.9 (ws), zstd 0.14.2, zip 9.0.2, serde_json (preserve_order, float_roundtrip), regex, unicode-normalization, sha1 0.11, prost 0.14, uniffi 0.32.2 and jiff.
- 254 crates in the normal dependency graph.
- Clean release build in 41 s on linux/arm64.

---

## 1. SQLite: rusqlite against schema-18 collections

**Crate:** `rusqlite 0.40.2` (MIT), with `libsqlite3-sys 0.38.2` bundling **SQLite 3.53.2** (verified at runtime).

Features: `bundled`, `functions`, `collation`, `backup`, `hooks`. Optional: `serialize`, `trace` (debug only). The default `cache` feature gives `prepare_cached`.

Always use `bundled` on every target, Apple included:
- the same SQLite version and compile options in Docker and on device;
- Apple's system libsqlite3 has different options and versions;
- bundled LIKE has no ICU, so it is ASCII-only case-insensitive, which matches Anki (research 04 §1);
- FTS5 (with the trigram tokenizer), JSON1 and STAT4 are compiled in.

Pitfall: the bundled build sets `SQLITE_DEFAULT_FOREIGN_KEYS=1`. The Anki schema declares no foreign keys, so this is harmless. Do not add any in the shadow DB unless you mean it.

**Collation and functions**
- Register `unicase` on **every** connection before touching any table:
  `conn.create_collation("unicase", |a, b| UniCase::new(a).cmp(&UniCase::new(b)))`.
  The open path must fail closed if registration fails.
- The schema itself uses no custom functions. Register Meguri's query functions (`regexp`, field-at-index, tag regex, process_text/nc, FSRS data extractors) with `FunctionFlags::SQLITE_UTF8 | SQLITE_DETERMINISTIC | SQLITE_INNOCUOUS`.
- Cache compiled regexes per statement with `Context::get_or_create_aux(0, …)`. That is rusqlite's aux-data API; without it you recompile once per row.
- **Never** put a custom function or a non-standard collation into an index, view or trigger inside `collection.anki2`. Anki and the sync server could no longer open the file. Shadow-DB objects belong in the separate shadow DB.

**Journal and locking.** Anki opens with `locking_mode=exclusive`, `journal_mode=wal` and `busy_timeout=0` (research 06 §1.2).
- Exclusive locking in WAL mode means a **single connection**. Other connections cannot even read.
- Locking mode is per connection, not stored in the file, so Meguri does not need to copy it for file compatibility.
- `journal_mode=wal` **is** persisted in the header. That is fine: Anki writes WAL-mode files, and uploads them too.

Decision: WAL with normal locking:
- **one writer connection**, owned by the engine's write actor;
- **N read-only connections**: open with `SQLITE_OPEN_READ_ONLY`, `query_only=1`, 2 to 4 of them, for search, browser paging, stats and AnkiConnect reads;
- `busy_timeout` of about 5 s on readers;
- `synchronous=NORMAL`, `cache_size=-40960`, `page_size=4096` for new files, `temp_store=MEMORY`.

Ownership of the file:
- Meguri owns its collection file inside its container. It never opens a file that desktop Anki has open; import copies.
- Take an advisory lock file (or `BEGIN IMMEDIATE` discipline) so two Meguri processes cannot both write.

Pitfalls:
- **Checkpoint starvation.** A long-lived read transaction stops WAL checkpoints and the `-wal` file grows. Keep read transactions short, page by id ranges, and never hold a statement open across FFI calls.
- **Full upload.** Do not send the raw file while a WAL exists. Follow Anki's `before_upload` (usn reset, `scm`, vacuum), then `wal_checkpoint(TRUNCATE)` on the writer. Better: `VACUUM INTO '<tmp>'` from the writer, then stream the temp file. You get a consistent snapshot with no WAL and no need to close the live collection, and R2004 is satisfied.
- **Full download.** Write to a temp file, open it with the collation registered, run `integrity_check`, set `ls=mod`, close, then do an atomic `rename` while the engine has every connection closed.
- **Backups.** Use `rusqlite::backup::Backup` or `VACUUM INTO` on a background connection, then zstd-compress. Never read the live file bytes directly.
- **iOS App Group containers.** If a widget or extension ever needs data, give it a small exported snapshot. Do not put the collection in a shared container: holding SQLite locks there when the app is suspended gets the app killed with 0xdead10cc.
- **Shadow DB.** Use a separate file (FTS5, previews, aggregates). You may `ATTACH` it to the reader connections for joins. A transaction across attached WAL databases is atomic per file, not across files, so make shadow updates idempotent and rebuildable.

**Prepared statements.** `prepare_cached` is an LRU per connection with capacity 16 by default. Raise it to about 128 with `set_prepared_statement_cache_capacity` on each connection. Dynamic search SQL varies per query, so do not cache it. Cache the fixed statements: answer path, card and note fetch, revlog insert.

**Bulk writes** (memory-state refresh, import, find/replace):
- one transaction per batch of about 1,000 to 5,000 rows;
- reuse cached statements;
- for very large updates, stage rows in a TEMP table and run one `UPDATE … FROM`.

Rejected: `r2d2` and `deadpool-sqlite` (deadpool needs MSRV 1.95). A fixed set of reader connections owned by the engine is simpler and deterministic.

---

## 2. fsrs 6.6.2

**Crate:** `fsrs = "=6.6.2"` (BSD-3-Clause), edition 2024. Its `priority-queue = "=2.7.0"` is `LGPL-3.0+ OR MPL-2.0`; select MPL-2.0 in cargo-deny and in the notices.

API surface (read in the v6.6.2 tag source):
- **Scheduling:** `FSRS::new(&params)` (empty slice means defaults; parameters are filled and clipped) and `next_states(Option<MemoryState>, desired_retention: f32, days_elapsed: u32) -> NextStates{again, hard, good, easy: ItemState{memory, interval}}`. Also `next_interval`, `current_retrievability(state, days, decay)` and `memory_state_from_sm2(ease, ivl, sm2_retention)`.
- **Memory state:**
  - `memory_state(FSRSItem, Option<MemoryState>)`;
  - `memory_state_batch(Vec<FSRSItem>, Vec<Option<MemoryState>>)`, which is **sequential**, not rayon;
  - `historical_memory_states(_batch)`.
  - A single `FSRSItem{reviews: Vec<FSRSReview{rating, delta_t}>}` holds the whole history of one card.
- **Optimizer:** `compute_parameters(ComputeParametersInput{train_set, card_ids, progress, enable_short_term, num_relearning_steps, training_config}) -> Vec<f32>`.
  - Anki passes `TrainingConfig{num_epochs: 8, ..}`. The crate **default is 5**, so set 8 explicitly.
  - Below 8 items it returns defaults. Below 64 items it returns only initialised stabilities. Callers must treat both as "not optimized".
  - The training loop is **single-threaded** on the calling thread (analytic gradient plus Adam). FSRS-7's two-thread training is unreleased.
- **Evaluation:** `FSRS::evaluate(items, |ItemProgress| -> bool)` returns `ModelEvaluation{log_loss, rmse_bins}`. The health check is `evaluate_with_time_series_splits(input, progress_fn)`.
  - It spawns 5 split trainings on the **global rayon pool**. Abort is checked per batch inside each split, and the outer closure is polled as splits finish.
  - The simulator also uses rayon; Meguri's plan has it out of scope.

Cancellation and progress:
- `CombinedProgressState::new_shared()` gives `Arc<Mutex<…>>`. Set `want_abort = true` to stop. It is checked once per batch of 512 items. Initialisation before training is not abortable, but it is short.
- The result is `Err(FSRSError::Interrupted)`. Map it to the engine's `Cancelled` error and write nothing (R698).
- Bridge it: the engine's `CancelToken` is polled by a small thread that sets `want_abort`, or the engine sets the flag directly when the token fires.

Threading and QoS:
- The optimizer inherits the caller thread's QoS. Call it from a Swift background-QoS thread.
- Configure rayon once with `ThreadPoolBuilder::build_global()`:
  - `num_threads` = performance cores minus 1;
  - a `start_handler` that sets `pthread_set_qos_class_self_np(QOS_CLASS_UTILITY)` through `libc` on Apple. Otherwise health-check threads run at default QoS and compete with the review UI.
- Do this before the first fsrs call. The global pool can be built only once.

Precision: params, desired retention and S/D are `f32` in the crate. Store and serialise them as f32. See §5: widening to f64 changes the JSON text.

---

## 3. HTTP client for sync protocol v11

**Decision:** `reqwest 0.13.5` with `default-features = false` and features `["rustls", "stream", "http2", "system-proxy"]`, on `tokio 1.53`. Do **not** enable reqwest's `zstd` feature, which is Content-Encoding auto-decompression. Anki bodies are raw zstd with no Content-Encoding header, so compress and decompress manually with the `zstd` crate.

TLS:
- reqwest 0.13 made rustls the default. Its `rustls` feature means the aws-lc-rs provider plus **rustls-platform-verifier 0.7**. On macOS and iOS that verifies through Security.framework: OS roots, keychain and MDM-installed certificates, with revocation. That covers users whose self-hosted servers sit behind a reverse proxy with a private CA installed on the device.
- aws-lc-rs needs no CMake for non-FIPS Apple builds, per its docs. All aws-lc-sys licenses are permissive.
- Linux/Docker: the platform verifier reads the system CA bundle. A slim image needs `ca-certificates`, or tests must use plain `http://` against the Docker sync server (the normal case).
- Rejected: `native-tls` (Security.framework through a different code path; adds an OpenSSL dependency in Docker). Rejected: `ureq 3.4`, which is blocking and cannot be interrupted mid-read except by timeouts. That is fine for a CLI, but Meguri needs prompt Cancel (R1860) and has tokio anyway for the Mac servers.

Settings:
- `connect_timeout` 30 s.
- **`read_timeout(io_timeout)`**: per-read idle timeout, reset on each read. It equals Anki's stall timeout (default 60 s on desktop).
- No total timeout on full upload or download.
- `redirect::Policy::none()` (see §0.5).
- `pool_idle_timeout` short. The `anki-sync` header is set per request.
- Response handling: require `anki-original-size`. Stream-decompress with `zstd::stream::read::Decoder` (or the async-compression equivalent), capped at the declared size, and reject anything larger.
- For full upload, stream the temp file through a zstd encoder into `Body::wrap_stream`. Never load 300 MB into RAM (R1852).

Plain HTTP to self-hosted servers: reqwest uses raw sockets, so ATS does not apply. iOS **Local Network privacy** still applies to LAN addresses: add `NSLocalNetworkUsageDescription`. Simulator behaviour differs from devices, so test on a device.

Runtime and footprint:
- One lazily created tokio runtime per process: `rt-multi-thread`, `worker_threads(2)`, `enable_io`, `enable_time`. On iOS it exists only during sync. Shut it down, or let it idle, on background.
- Code-size cost is a few hundred KB; reqwest, hyper and rustls dominate. Measure the stripped staticlib for `aarch64-apple-ios` once, and also with `rustls-no-provider` plus ring, before deciding size work is needed.

Cancellation exposed to Swift:
- Export `CancelToken` (a UniFFI object wrapping `tokio_util::sync::CancellationToken` plus an `AtomicBool` for CPU loops).
- Sync functions are **blocking exports**. They run `rt.block_on(async { select!{ _ = token.cancelled() => Err(Cancelled), r = work => r } })`.
- Swift calls them from its own background thread or actor, never from the cooperative pool for long calls, and calls `token.cancel()` from the UI.
- On cancel, the engine rolls back and fires `abort` without waiting (R1532).
- Progress goes through a UniFFI callback interface throttled to about 10 Hz, or through a pollable progress object. Never one FFI call per object.

Design seam: put transport behind a small `trait SyncTransport { fn post(&self, url, header, body_stream) -> Response }`. reqwest is the only implementation in v1. The seam lets an iOS URLSession background transfer be added later for 300 MB full syncs that must survive app suspension, without touching protocol code.

---

## 4. Local HTTP and WebSocket servers (Mac: 8765 AnkiConnect, 44432 Migaku)

**Decision:** Rust, with `axum 0.8.9` (`ws` feature, which uses tokio-tungstenite internally) on `hyper 1.12` and `tokio`, in a separate crate compiled only for macOS and Linux (cfg/feature `servers`).

Rejected: Network.framework in Swift. It cannot run in Docker, so the R1799/R1800/R1946 transcript contract suites would need a Mac host. It would also put a second implementation of note creation, search and media rules on the Swift side. In Rust, the contract tests bind `127.0.0.1:0` inside the core's integration tests and replay golden transcripts against the same engine the app uses.

Pitfalls (axum defaults that break these protocols):
- **Body limit.** axum's `DefaultBodyLimit` is 2 MB. AnkiConnect `storeMediaFile` base64 data and Migaku `/receive` data-URL audio and images exceed it. Set an explicit limit, for example 200 MB to cover a 100 MiB media cap plus base64 overhead, and stream large bodies.
- **Content-Type.** Do not use `Json<T>` extractors. They reject a missing or `text/plain` Content-Type, which browser clients send to avoid CORS preflight. Read `Bytes` and parse with serde_json. On bad JSON, return AnkiConnect's `{"result":null,"error":…}` with **HTTP 200**.
- **Routing.** AnkiConnect accepts any method on `/`, and a GET with an empty body is a ping. Use a single fallback handler, not method routers. For Migaku, `OPTIONS` returns 204 with the fixed CORS headers.
- **CORS and Host checks.** Hand-roll them as a tower layer. tower-http `CorsLayer` cannot express "echo the allowed origin, 403 with empty body otherwise, except `requestPermission`", or the `Access-Control-Allow-Private-Network` reply. Check `Host` before routing (R1676).
- **Connection reuse.** AnkiConnect closes after each request. hyper keeps connections alive, which clients tolerate. Do not depend on close for framing.
- **WebSocket.** `/anki-connect` must accept several connections, answer pings (tungstenite does automatically) and ignore malformed frames without closing. Set `max_message_size` generously and log, rather than error, on parse failure.
- **Binding.** Bind `127.0.0.1` explicitly; never `0.0.0.0`, and `[::1]` only if needed. tokio and mio set `SO_REUSEADDR` on Unix. On macOS that lets Meguri bind `127.0.0.1:44432` **even while** Anki's Migaku add-on holds `0.0.0.0:44432`; the more specific bind wins. So port-conflict detection (R-"port in use") will not fire in that case. Test the coexistence behaviour deliberately; probe by connecting first if a warning is wanted.
- **Sandbox.** Add `com.apple.security.network.server` (R1671). Loopback-only listeners are not expected to trigger the Local Network or firewall prompt, but R1671 already requires verification on a clean user account.

---

## 5. Other crates

| Need | Pick (version, license) | Notes / pitfalls |
|---|---|---|
| zstd | `zstd 0.14.2` (BSD-3-Clause in crates.io metadata; wraps libzstd), feature `zstdmt` | 0.14.2 was published 2 days ago. If it causes trouble, fall back to 0.13.3 (Anki's). Use streaming `read::Decoder`/`write::Encoder` for collection files, media and sync bodies. `zstdmt` gives multithreaded compression for large colpkg/backup files. |
| zip | `zip 9.0.2` (MIT), **`default-features = false, features = ["deflate"]`** | The defaults pull in aes, bzip2, lzma, xz, ppmd and zstd: big and useless here. The crate has had frequent majors and yanks: pin the exact minor and review upgrades. Anki's media entries and `collection.anki21b` are **stored** entries whose payload is zstd. Do not use zip's own zstd method (93). Legacy `collection.anki21` is deflate. Call `large_file(true)` for entries that may exceed 4 GiB (zip64 media colpkgs). Check names on import (`enclosed_name`); never trust paths. |
| JSON | `serde_json 1.0.154` with **`preserve_order` + `float_roundtrip`**; **not** `arbitrary_precision` | `preserve_order` keeps the key order of unknown config, schema11 notetype and deck JSON across round trips. Anki writes BTreeMap-sorted keys, so preserved order is also sorted. `float_roundtrip` makes parsing exact; the default can be 1 ULP off. Skip `arbitrary_precision`: it breaks `#[serde(flatten)]` (needed for "unknown keys" catch-alls in schema11 structs) and untagged enums. Anki's own serde_json has no such feature, so its values fit i64/u64/f64 anyway. Features unify across the whole graph, so set them in the workspace once. **Precision pitfall:** decode each number into the type Anki uses: f32 for FSRS params, DR and decay; i64 for ids. `0.212f32` widened to f64 serialises as `0.21199999…`. Unknown values stay as `Value` untouched. Test with the oracle's JSON as golden bytes. |
| Protobuf | `prost 0.14.4` (Apache-2.0), derives on hand-written structs | See §0.4: unknown-field capture is required. No `prost-build` from Anki's `.proto` files (AGPL). `prost-reflect` is not needed. |
| Regex | `regex 1.13.1` (MIT/Apache) | Required: R1041 and R1202 define the syntax as Rust `regex`. **Not `fancy-regex`**: it accepts look-around and backrefs that Anki rejects, which would make searches valid in Meguri and invalid on desktop. `(?i)` uses Unicode simple case folding (Cyrillic and Greek fold; `ß`≠`SS`), identical to Anki. Set `size_limit` and `dfa_size_limit` to bound user regexes, and compile once per query (aux-data cache in SQLite functions). Search with `regex::bytes` over `flds` to skip UTF-8 re-validation. `aho-corasick` and `memchr` are already transitive deps; use them for multi-literal scans (media refs). |
| Unicode | `unicode-normalization 0.1.25` (MIT/Apache) for NFC and NFKD/`nc:`; `unicase =2.6.0` | Matches the oracle's crates. ICU4X (`icu_normalizer 2.3`, Unicode-3.0 licence) is heavier and diverges in edge cases. Unicode-version skew between Meguri's and the oracle's crate versions is the residual risk: keep a property test of random strings, including recent code points, against the oracle. |
| HTML strip / entities | No HTML parser for stored data. Use `regex` plus `htmlescape 0.3.1` (Apache-2.0/MIT/MPL-2.0) | `sfld`, `csum`, duplicate checks and `dupe:` must equal Anki's **regex-based** stripping plus htmlescape entity decoding. An HTML5 parser (`html5ever 0.40`, `scraper 0.27`, `lol_html 3`) gives *different* text on malformed HTML, so `csum` and duplicate detection would drift. Differential-fuzz strip/decode against the oracle. Sanitising for display (deck descriptions) can be `ammonia 4.2.3`, or done in the WKWebView with a strict CSP. Editor paste sanitising belongs to the Swift/WebKit side. |
| Hashes | `sha1 0.11.0` (RustCrypto, MIT/Apache, MSRV 1.85); `md-5 0.11.0` for AnkiConnect `skipHash` | sha1 0.11 has a different digest-trait API from 0.10; pick 0.11 from the start. `csum` = the first 4 bytes of SHA1 as a big-endian u32. Media sums are lowercase hex. |
| Time | **Inject `now_ms` and the local UTC offset** from Swift (and from libfaketime in tests). Use `jiff 0.2.38` (Unlicense/MIT) with `default-features=false, features=["std"]` for civil-date math | Anki's day math uses fixed UTC offsets (creation offset, current offset, rollover hour), not a tz database, so **no `chrono-tz`**. Avoid any library's "system timezone" lookup in the core: iOS file-system tz paths and Docker differ. Swift supplies `TimeZone.current.secondsFromGMT(for:)`. Backup file names in local time, and Monday-start weeks for thinning, use the injected offset. `chrono 0.4.45` without default features is an equal choice; pick one. |
| Images / thumbnails | **None in the core**; leave to Swift (ImageIO `CGImageSourceCreateThumbnailAtIndex`, which covers HEIC, WebP and AVIF) | The core only moves bytes and hashes. Image-occlusion masks are drawn in the web view. Image diffing is test tooling in Docker (Python or `image` as a dev-dependency only). |
| Errors / logging | `snafu 0.9` (also used by fsrs) or `thiserror 2`; `tracing 0.1.44` | One error enum per crate. The FFI maps to a small Swift-facing error set. |
| FFI | `uniffi 0.32.2` (MPL-2.0), proc-macro mode, one FFI crate | Generates Swift. Build `staticlib` for `aarch64-apple-darwin`, `aarch64-apple-ios` and `aarch64-apple-ios-sim`, then `xcodebuild -create-xcframework`; `cargo-swift 0.11` can script it. Pitfalls: no Task-cancel propagation (§0.6). Records are serialised through RustBuffer, so page large results: never return 150k rows. Prefer blocking exports run off the cooperative pool over `async` exports, which need `async_runtime = "tokio"` and still ignore cancellation. |
| Licence gate | `cargo-deny 0.20.2`, `cargo-about 0.9.2` | Allow: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0, Unlicense, CDLA-Permissive-2.0, MIT-0. Deny: GPL, AGPL, LGPL-only. Clarify `priority-queue` to MPL-2.0. cargo-about generates the in-app notices (fsrs BSD-3 notice required). |

### Testing crates

- **`proptest 1.12`** (MSRV 1.88): generators for search strings, templates, field HTML and revlog histories.
  - The differential oracle is Python `anki` in Docker, too slow per case. So the generators write **corpora** (thousands of cases in JSON). The oracle evaluates them in one batch and produces golden files. Rust replays them.
  - The same generators feed new rounds.
- **`cargo-fuzz 0.13.2` + `libfuzzer-sys` + `arbitrary 1.5`** (nightly, works on linux/arm64 in Docker): crash and robustness fuzzing for parsers that take untrusted input.
  - Targets: search parser, template parser, cloze, apkg/colpkg/zip import, CSV, sync JSON, AnkiConnect and Migaku request bodies, protobuf blobs.
  - Interesting fuzz inputs get promoted into the oracle corpus.
  - `bolero 0.13` can run the same harness under proptest and libFuzzer if one API is wanted.
- **`insta 1.49`**: snapshot and golden tests.
  - Use redactions for ids and timestamps.
  - Run with `INSTA_UPDATE=no` / `CI=true` in Docker so new snapshots fail rather than auto-accept.
  - Golden oracle outputs live as plain files; use insta only for Meguri-produced text, such as rendered HTML and SQL.
- **Benchmarks (R1864):**
  - **`gungraun 0.20`** (the renamed iai-callgrind, which stopped at 0.16): instruction counts under valgrind in Docker.
  - `criterion 0.8.2` or `divan` for wall-clock on the host.

---

## 6. Workspace layout

**Several crates, with one FFI crate as the only Apple-built artifact.** Reasons:
- parallel and incremental compilation (the core will be large);
- fuzz targets and property tests link only small pure crates;
- tokio, axum and the servers stay out of the iOS binary;
- boundaries are enforced by the compiler.

Do not split to dozens; 9 to 11 crates is right for this size.

```
core/
  Cargo.toml                 # [workspace], shared [workspace.dependencies] with exact pins
  rust-toolchain.toml        # 1.99
  deny.toml
  crates/
    meguri-text/        pure: strip_html, entities, NFC/nc:, unicase helpers, csum, media-ref scan
    meguri-model/       ids, usn, mtime types, protobuf structs + unknown-field codec, schema11 JSON
    meguri-storage/     rusqlite: open/collation/functions, schema, queries, connections (writer + readers), shadow DB
    meguri-search/      parser, canonical writer, SQL writer (depends: text, model)
    meguri-template/    template/cloze/furigana/type-answer rendering (depends: text)
    meguri-sched/       day math, queues, answer path, fsrs glue, optimizer job, fuzz (depends: storage, fsrs, rand)
    meguri-sync/        protocol v11 + media sync; SyncTransport trait; reqwest impl behind feature (depends: storage)
    meguri-package/     apkg/colpkg/backup/CSV import-export (zip, zstd, csv)
    meguri-engine/      Collection facade, ops, undo, change events, CancelToken, progress (depends on all above)
    meguri-servers/     axum AnkiConnect + Migaku (cfg macOS/linux; depends: engine)
    meguri-ffi/         uniffi exports only; crate-type = ["staticlib", "lib"]; features: servers on macOS only
  tools/
    meguri-cli/         headless driver for Docker tests, oracle comparison, benchmarks
  fuzz/                 cargo-fuzz targets (separate workspace member, nightly)
```

Rules worth writing into `core/AGENTS.md`:
- Only `meguri-storage` writes SQL against `collection.anki2`.
- Only `meguri-ffi` knows UniFFI.
- Only `meguri-sync` and `meguri-servers` know tokio, plus the engine's runtime handle.
- `unicase`, `fsrs`, `rand`, `regex`, `htmlescape` and `unicode-normalization` are pinned with `=` in `[workspace.dependencies]`. Change them only together with the oracle's pin, with a note.
- The Docker image needs a C toolchain for bundled SQLite, zstd and aws-lc-sys. My test installed `cmake` and `clang`; I did not test whether `cmake` can be dropped.
- Cache `~/.cargo/registry` and `target/` in named volumes. A linux/arm64 build runs natively on Apple silicon; a clean build of the full dependency stack takes about 40 s.

---

## Sources

- crates.io API (versions, licences, MSRV, features), queried 2026-10-12. Rust stable channel manifest: static.rust-lang.org/dist/channel-rust-stable.toml (1.99.0).
- fsrs-rs v6.6.2 tag source: `Cargo.toml`, `src/lib.rs`, `src/training.rs`, `src/inference.rs`, `src/model.rs` (local `refs/fsrs-rs`, BSD-3).
- rusqlite v0.40.2 `libsqlite3-sys/sqlite3/sqlite3.h` (3.53.2) and `libsqlite3-sys/build.rs` compile flags (github.com/rusqlite/rusqlite).
- UniFFI 0.32.2 Swift template `uniffi_bindgen/src/bindings/swift/templates/Async.swift`; async FFI docs https://mozilla.github.io/uniffi-rs/0.32/internals/async-ffi.html
- reqwest 0.13 announcement https://seanmonstar.com/blog/reqwest-v013-rustls-default/ and release notes https://newreleases.io/project/cargo/reqwest/release/0.13.0
- rustls-platform-verifier README https://github.com/rustls/rustls-platform-verifier
- aws-lc-rs Apple requirements https://aws.github.io/aws-lc-rs/requirements/apple.html
- rust-protobuf end-of-life notice https://github.com/stepancheg/rust-protobuf; prost unknown-fields PR https://github.com/tokio-rs/prost/pull/574
- Anki `Cargo.toml`/`Cargo.lock` at refs/anki 2f62b5e (third-party pins only: `unicase =2.6.0`, `fsrs 6.6.2`, `rand 0.10.1`, `htmlescape 0.3.1`, `regex`, `unicode-normalization`, `zstd 0.13.3`, `zip 4`, `rusqlite 0.36 bundled`).
- Verification scratch projects: `scratchpad/stackcheck` (stack build plus collation/JSON checks) and `scratchpad/fsrsbench` (optimizer timing, abort, prost unknown-field check), both run in `rust:1.99-slim` linux/arm64.
