# Server dependency assessment

New first-party server code is MIT. The public model dependency remains
MPL-2.0. No Anki implementation or reference runtime belongs to this graph.

The accepted account-service baseline used 133 resolved packages, fourteen
production edges and two development edges. Those historical results do **not**
audit the changed collection graph. The manager supplied the measured current
graph below; this filtered wrapper session did not resolve, compile or audit it.
These dirty-source graph checks do not establish a final clean artifact,
Desktop interoperability, independent review or a release notice/source bundle.

## Collection additions and public API boundary

| Dependency | Concrete purpose and selection | Measured graph and remaining assessment |
| --- | --- | --- |
| `meguri-model`, path `../core/crates/meguri-model`, defaults disabled, no features | Public records, `schema11` conversions, `proto::ProtoBlob` and blob messages, and `json_text::parse`. SQL, HTTP, account authority and file publication are independently authored. No storage, text, engine, scheduler, sync client, randomness or stock-content convenience import. | Measured model features are `[]`; its only four production dependencies are ryu, serde, serde_json and unicode-normalization. serde_json unifies raw_value; exact versions/features are below. No model test hooks, development peers, engine, scheduler, client, storage or text dependency enters production. |
| `unicase =2.6.0` | Required Unicode 12.1 SQLite collation, including the specified three-character folding quirk. This compatibility pin intentionally differs from current Unicode. | Measured 2.6.0, features `[]`, MIT/Apache-2.0. Actual scoped deny/about passed on the supplied manifest/lock; this is no future advisory or release-notice guarantee. |
| `zstd =0.14.4`, defaults disabled | Required streaming raw protocol compression, concatenated frames and a bounded decoder window, without HTTP content coding. Prefer the latest compatible exact patch from the supplied official catalog, released 2026-11-12. The catalog lists BSD-3-Clause for the binding and normal zstd-safe ^8.3.0; these facts are not a security guarantee. | Measured zstd 0.14.4, zstd-safe 8.3.0 and zstd-sys 2.1.3+zstd.1.5.7; exact features/licenses below. Actual deny/about passed; native distribution inputs and final release notices/source remain release work. |
| `regex =1.11.2`, defaults disabled; `std`, `perf`, `unicode` | Neutral pinned Unicode pattern semantics for note derived values, including malformed markup. No HTML parser or browser rendering. | Measured 1.11.2 and its automata/syntax graph below; MIT OR Apache-2.0. Unicode tables retain their separate notices. Supplied primary catalog records MSRV 1.65. Actual deny/about passed; final distribution notices remain required. |
| `htmlescape =0.3.1`, defaults disabled | Neutral pinned all-or-nothing HTML 4 entity decoding through the Result API. Independent composition of public rules, without meguri-text dependencies. | Measured 0.3.1, features `[]`; actual metadata license expression is Apache-2.0 / MIT / MPL-2.0. Actual deny/about passed with the approved selection; do not generalize the sole package-scoped model MPL allowance to another package. |
| `sha1 =0.11.0`, defaults disabled | RustCrypto legacy first-field note checksum only. Never used for security and no locally authored primitive. | Measured 0.11.0, features `[]`, MIT OR Apache-2.0; actual deny/about passed. Supplied primary catalog identifies stable release 2026-03-27 and normal cfg-if/digest 0.11/cpufeatures 0.3. |
| `rusqlite =0.41.1`, defaults disabled, `bundled`, `backup`, `collation` | Existing pin with SQLite backup for complete committed snapshots and the collection collation. Independently authored WAL/FULL lifecycle, without app storage policy. | Measured rusqlite/libsqlite3-sys features and licenses below; actual deny/about passed. Final native SQLite compile-option evidence, including statistics support, is separate from graph resolution. |

The model's four intended production dependencies are ryu, serde, serde_json
with raw_value and exact unicode-normalization 0.1.24. Its measured features are
`[]`; optional randomness, test hooks and development peers are absent. The
standalone production graph has no model engine/scheduler/client/storage/text
edge. These are supplied measured graph facts, not a claim of a smaller runtime.

## Measured collection graph

Manager receipt: `build/mg/sync-server-deps/20261115T194209Z-83856/result.json`,
status `pass`: 149 packages, 20 production direct edges and two development
direct edges. Actual cargo-deny and cargo-about passed on manifest SHA-256
`b1ca395a5b6b6fbaae92d5a88b083541639ed94abd5294c42e38d57c438e6403`
and lock SHA-256
`e6ff1a10840e84fac2a83621a1c4d4dbe21cf6915215ff59deb4d4532b3b8036`.
The following selected metadata is attached to that receipt; it is not a new
dependency or feature choice by this wrapper fixer. Feature lists are the actual
resolved lists, including transitive unification.

| Package | Version | Metadata license | Resolved features |
| --- | --- | --- | --- |
| htmlescape | 0.3.1 | Apache-2.0 / MIT / MPL-2.0 | `[]` |
| libsqlite3-sys | 0.39.1 | MIT | bundled, bundled_bindings, cc, default, min_sqlite_version_3_45_3, pkg-config, vcpkg |
| meguri-model | 0.0.0 | MPL-2.0 | `[]` |
| regex | 1.11.2 | MIT OR Apache-2.0 | perf, perf-backtrack, perf-cache, perf-dfa, perf-inline, perf-literal, perf-onepass, std, unicode, unicode-age, unicode-bool, unicode-case, unicode-gencat, unicode-perl, unicode-script, unicode-segment |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | alloc, dfa-onepass, hybrid, meta, nfa-backtrack, nfa-pikevm, nfa-thompson, perf-inline, perf-literal, perf-literal-multisubstring, perf-literal-substring, std, syntax, unicode, unicode-age, unicode-bool, unicode-case, unicode-gencat, unicode-perl, unicode-script, unicode-segment, unicode-word-boundary |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | std, unicode, unicode-age, unicode-bool, unicode-case, unicode-gencat, unicode-perl, unicode-script, unicode-segment |
| rusqlite | 0.41.1 | MIT | backup, bundled, collation, modern_sqlite |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | `[]` |
| serde | 1.0.228 | MIT OR Apache-2.0 | default, derive, serde_derive, std |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | default, raw_value, std |
| sha1 | 0.11.0 | MIT OR Apache-2.0 | `[]` |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | alloc, default |
| unicase | 2.6.0 | MIT/Apache-2.0 | `[]` |
| unicode-normalization | 0.1.24 | MIT/Apache-2.0 | default, std |
| zstd | 0.14.4 | BSD-3-Clause | `[]` |
| zstd-safe | 8.3.0 | BSD-3-Clause | alloc, default |
| zstd-sys | 2.1.3+zstd.1.5.7 | BSD-3-Clause AND BSD-2-Clause | std |

Passing scoped checks describe these exact dirty-source manifest/lock inputs,
not a permanent advisory guarantee. Final clean build and candidate receipts
must independently bind the committed source, actual lock and running binary.
Release assembly still retains MPL covered source, its availability instructions
and all selected notices; this assessment does not supply that release bundle.

## Retained account-service assessment (historical)

The exact unchanged-dependency purpose, maintenance and license assessment below
is restored from the own-source account baseline. Any resolved feature/count or
passing check language in these tables describes that historical graph only;
it does not certify the current changed graph. In particular, rusqlite now also
requests backup/collation, assessed above. The manager's later account baseline
was 95/95 at source a48fd7472b9761e04e1b57e14e4147dab197784c; it characterizes
account behavior only, not collection or client compatibility.

## Production dependencies

| Pin and selected-version documentation | Purpose and requested features | License | Maintenance and selection rationale |
| --- | --- | --- | --- |
| [tokio 1.53.3](https://docs.rs/crate/tokio/1.53.3) | Runtime, sockets, synchronization, timers, signals and bounded `spawn_blocking` password workers. Defaults off; `rt-multi-thread`, `macros`, `net`, `sync`, `time`, `signal`. No full/process/fs extras. | MIT | Official [release policy](https://docs.rs/crate/tokio/latest) lists 1.53.x LTS through September 2027; 1.53.3 is its October 13 latest patch. Retain supported LTS despite newer 1.54.1. |
| [axum 0.8.8](https://docs.rs/crate/axum/0.8.8) | HTTP routing/framing and typed JSON over Hyper, avoiding a custom HTTP parser. Defaults off; `http1`, `tokio`, `json` only. | MIT | Latest 0.8.9 (April 14) [release notes](https://raw.githubusercontent.com/tokio-rs/axum/axum-v0.8.9/axum/CHANGELOG.md) address disabled WebSocket/multipart paths, unused CONNECT handling and MSRV. Retain tested 0.8.8 for enabled service paths; the resolved graph's scoped deny checks and full service suite pass. |
| [http-body 1.1.0](https://docs.rs/crate/http-body/1.1.0) | Asynchronous typed HTTP-body trait/Frame facility retaining the incoming body, admission permit and root owner through typed response handling. Defaults off; no optional features requested. | MIT | Hyperium's latest stable July 13 release per the supplied [primary catalog](https://docs.rs/crate/http-body/latest), inspected 2026-11-15. Already transitive in the 133-package graph; its direct root edge is recorded in the real lock, with exact features `[]` and passing scoped MG dependency checks. |
| [rusqlite 0.41.1](https://docs.rs/crate/rusqlite/0.41.1) | Shared real SQLite state. Defaults off; `bundled` only. WAL/FULL/foreign-key policy has own tests. | MIT | Published October 28. [Catalog](https://docs.rs/crate/rusqlite/latest) lists 0.42 on November 11 and 0.43.0 released on this assessment date. Retain recent tested, core-compatible SQL baseline instead of an unreviewed new minor. The locked bundled dependency is libsqlite3-sys 0.39.1 with resolved features recorded above; no underlying SQLite version is asserted here. |
| [serde 1.0.228](https://docs.rs/crate/serde/1.0.228) | Strict typed requests/results. Default `std` plus `derive`. | MIT OR Apache-2.0 | Latest 1.0.229 (July 18) changes derive infrastructure to syn 3 per [release notes](https://github.com/serde-rs/serde/releases/tag/v1.0.229). Retain tested current-contract/core-compatible baseline; no service need for that unrelated derive toolchain change. |
| [serde_json 1.0.154](https://docs.rs/crate/serde_json/1.0.154) | JSON encoding and typed deserialization; default `std`. Own request tests exercise unknown/duplicate members and depth limits. | MIT OR Apache-2.0 | Latest stable in the supplied catalog, released October 11. Retain the tested JSON boundary. |
| [argon2 0.6.0](https://docs.rs/crate/argon2/0.6.0) | Password hashing/PHC parsing and verification through RustCrypto. Defaults off; `alloc`, `password-hash` only per [feature documentation](https://docs.rs/crate/argon2/0.6.0/features). Service supplies a 16-byte OS-random salt; no getrandom, rand_core, parallel/rayon or kdf extras. There is no `std` feature in 0.6. | MIT OR Apache-2.0 | Deliberate upgrade to current supported stable, released August 27 versus 0.5.3 on January 20, 2024. [Release notes](https://raw.githubusercontent.com/RustCrypto/password-hashes/argon2-v0.6.0/argon2/CHANGELOG.md) describe allocation-failure handling, parameter-validation ordering fixes and updated password-hash/digest family. Bounded API adaptation and memory/error handling justify adoption; this is not evidence that 0.5.3 was unsafe. |
| [getrandom 0.4.3](https://docs.rs/crate/getrandom/0.4.3) | OS CSPRNG for secret and salt bytes via `fill`; defaults, no optional custom/JS/rand_core backend requested. | MIT OR Apache-2.0 | Deliberate upgrade to latest stable June 17 release from selected 0.3.4 (October 14, 2025). [Release notes](https://raw.githubusercontent.com/rust-random/getrandom/v0.4.3/CHANGELOG.md) include earlier 0.4.2 error/OS-source fixes. Prefer current OS-random boundary; existing fill API remains suitable. |
| [sha2 0.11.1](https://docs.rs/crate/sha2/0.11.1) | SHA-256 digest of high-entropy bearer secrets, not password hashing. Defaults off: resolved 0.11.1 defaults enable `alloc` and `oid`, neither needed for fixed-size SHA-256 bearer digests. Retains crate-provided crypto. | MIT OR Apache-2.0 | Latest stable RustCrypto hash family in the supplied catalog. Same pin; actual metadata confirms features `[]` after removing unused defaults, with retained feature trees, notices and passing scoped deny checks and full service tests. |
| [subtle 2.6.1](https://docs.rs/crate/subtle/2.6.1) | Constant-time bearer-digest comparison. Defaults off; no direct transitive dependencies enabled. | BSD-3-Clause | [Catalog](https://docs.rs/crate/subtle/latest) still lists June 24, 2024 as latest stable. Low release frequency alone is not an unmaintained finding. Retain this narrow primitive; scoped advisory and maintenance-policy checks on the full resolved graph report zero errors and warnings. |
| [base64 0.23.1](https://docs.rs/crate/base64/0.23.1) | Standard unpadded URL-safe bearer representation; default std/alloc. | MIT OR Apache-2.0 | Current stable August 4 release; catalog reports no production dependencies. Retain instead of writing an encoder/decoder. |
| [url 2.5.8](https://docs.rs/crate/url/2.5.8) | Vetted configured HTTPS-origin parser. Defaults including std and the IDNA graph are justified by origin parsing. Request headers cannot choose the origin. | MIT OR Apache-2.0 | Latest stable in supplied catalog. Retain established parser rather than inventing origin parsing; its transitive graph is included in the completed scoped dependency checks. |
| [time 0.3.55](https://docs.rs/crate/time/0.3.55) | RFC3339 UTC expiry results. Defaults off; `std`, `formatting` only. No local-offset, parsing, macros, serde or random extras. | MIT OR Apache-2.0 | Latest stable in supplied catalog. Retain narrowly configured time formatting. |
| [libc 0.2.190](https://docs.rs/crate/libc/0.2.190) | Unix inode, lock, nofollow and terminal primitives, not a service framework. Defaults; no explicit extras. | MIT OR Apache-2.0 | Retain reviewed October 2 stable Linux ABI also used by core. Latest 0.2.191 (October 16) [release notes](https://raw.githubusercontent.com/rust-lang/libc/0.2.191/CHANGELOG.md) change non-Linux bindings/platforms, not the Linux primitives used here. No need for 1.0 alpha. |

## Development dependencies

| Pin and selected-version documentation | Purpose and requested features | License | Maintenance and selection rationale |
| --- | --- | --- | --- |
| [reqwest 0.13.5](https://docs.rs/crate/reqwest/0.13.5) | Actual loopback HTTP assertions; defaults off, `json` only. No TLS/cookies or production network client. | MIT OR Apache-2.0 | Latest stable in supplied catalog; retain real HTTP test coverage without expanding service production dependencies. |
| [tempfile 3.27.0](https://docs.rs/crate/tempfile/3.27.0) | Isolated real SQLite/filesystem/process tests with RAII temporary directories. Defaults; no explicit extras. | MIT OR Apache-2.0 | Current stable in supplied catalog. Retain tested filesystem isolation; production does not rely on this temporary-file helper. |

## Covered files, provenance and distribution

The sole MPL allowance in cargo-deny is package `meguri-model`. Cargo-about may
emit MPL notices; cargo-deny still rejects an unapproved second MPL package.
Covered owned paths are `core/crates/meguri-model/Cargo.toml` and all files under
`core/crates/meguri-model/src/`, together with inherited declarations in
`core/Cargo.toml` and root `LICENSE`. Do not copy their bodies into MIT files.
Public module documentation cites neutral model specs and identifies the
protobuf codec as Meguri's own. The supplied assessment does not establish
exclusive ownership or complete chain of title.

The coherent batch base supplied by the manager is commit
`aa8565719af435051de7d5f068c0c817286b568e`. The model reuse source revision is
`8ead750f295558c1f74362a47bd927ae2c876e51`, scope
`core/crates/meguri-model`. The spec writer inspected path-limited Git
attribution/role trailers and the session ledger without source patches or
quarantined/private content; all recorded session IDs were present. This is
process evidence, not exclusive ownership or relicensing proof. The candidate receipt must bind the
actual reused source revision and exact covered bytes. Release assembly must
retain original covered source and MPL license/notices. Executable distributions
must explain how recipients obtain covered source under MPL-2.0. The larger
service may remain MIT without relicensing covered files. This document is not
a release notice bundle.

## Manager verification

Use `tools/mg sync-server-deps --resolve-existing` once for the manifest change;
it retains the prior lock and explicitly resolves against the existing lock.
Inspect every lock change, then use ordinary locked dependency, test and lint
jobs. Do not import core/Cargo.lock. Record actual versions, features,
advisories, maintenance conclusions and notices before accepting the graph.
The supplied initial resolution at
`build/mg/sync-server-deps/20261115T192927Z-71678/result.json` used zstd 0.14.3;
the later measured 149-package receipt above supersedes that graph. This filtered
wrapper fixer leaves the supplied Rust, manifests and lock frozen. The supplied
seed's server checks passed 116/116 with zero skips/retries and its scoped MG
fixtures passed 14/14; these are parent observations, not executions here or
Desktop interoperability evidence. The manager's initial focused attempt failed
before tests at schema.rs:17 (unavailable load-extension DbConfig under selected
features). Preserve that failure as evidence, not a preimplementation red or a
test pass. No observed preimplementation red exists for the new tests.
