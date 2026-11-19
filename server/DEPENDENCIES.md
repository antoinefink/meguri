# Server dependency assessment

Current policy (2026-11-17): independently authored Meguri material, including the model crate, now uses MIT under `LICENSING.md`. The assessment and receipts below describe the earlier source revisions and retain their original MPL-2.0 statements. This note does not rerun or replace dependency or packaging verification. Third-party terms remain unchanged.

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
| `sha1 =0.11.0`, defaults disabled | RustCrypto legacy first-field note checksum and media protocol digest. Never an authorization proof; no locally authored primitive. | Measured 0.11.0, features `[]`, MIT OR Apache-2.0; actual deny/about passed. Supplied primary catalog identifies stable release 2026-03-27 and normal cfg-if/digest 0.11/cpufeatures 0.3. |
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
the later measured 149-package receipt above supersedes that graph. That earlier filtered
collection wrapper left its supplied Rust, manifests and lock frozen. The supplied
seed's server checks passed 116/116 with zero skips/retries and its scoped MG
fixtures passed 14/14; these are parent observations, not executions here or
Desktop interoperability evidence. The manager's initial focused attempt failed
before tests at schema.rs:17 (unavailable load-extension DbConfig under selected
features). Preserve that failure as evidence, not a preimplementation red or a
test pass. No observed preimplementation red exists for the new tests.

## Media additions: measured graph

The media manifest adds exact `zip =9.0.5`, defaults disabled, with only
`deflate-flate2-zlib-rs`, and a direct `unicode-normalization =0.1.24` edge
(the latter already appeared in the measured model graph). Existing
`sha1 =0.11.0` also supplies the required legacy media protocol digest. Existing
`sha2 =0.11.1` supplies an internal content check; generated account-private
object IDs and actual-byte comparison determine storage reuse, never SHA-1.

`zip` initially supplied upload reads and stored download generation. The
targeted replacement below removes its untrusted upload reader; the exact writer
and test-fixture pin remains. No remote archive name is extracted or used as a
path, and no remote permissions are applied. NFC validation compares the
original spelling with its normalized character sequence; it never silently
renames a supplied name.

Primary facts supplied by the parent for this selection:

- [zip 9.0.5 documentation](https://docs.rs/crate/zip/9.0.5) states MIT,
  MSRV 1.88, active 9.x maintenance and stored/deflate read/write capability.
  General `deflate` adds zopfli; `deflate-flate2-zlib-rs` selects the required
  backend alone. The catalog includes 9.6.0 on November 15; that is no reason
  to float this specifically vetted pin.
- [RUSTSEC-2026-0367](https://rustsec.org/advisories/RUSTSEC-2026-0367) and
  [RUSTSEC-2026-0429](https://rustsec.org/advisories/RUSTSEC-2026-0429.html)
  describe extraction-permission and symlink-allocation flaws fixed in versions
  >=9.0.0. Avoiding extraction/mode application narrows our use; these facts do
  not establish an advisory-free resolved graph or prove our bounds.
- The prior measured unicode-normalization version was 0.1.24, MIT/Apache-2.0,
  with default/std and its tinyvec graph. The current supplied audit also
  records unicode-normalization 0.1.24 as MIT/Apache-2.0.

Manager-supplied `MG sync-server-deps --resolve-existing` receipt
`20261116T030209Z-41348` is PASS; both scoped cargo-deny and cargo-about ran
successfully on server lock SHA-256
`a6566ec28fe707892909590223ac8753effe0fef1abd1b1fdd64836a69c8fe8f`.
Existing dependencies were retained. Ten packages were added: cfg_aliases
0.2.2, crc32fast 1.5.5, equivalent 1.0.3, flate2 1.1.11, hashbrown 0.17.1,
indexmap 2.14.2, nix 0.31.3, typed-path 0.12.4, zip 9.0.5 and zlib-rs 0.6.8.
Relevant supplied licenses are zip MIT, zlib-rs Zlib, and flate2/crc32fast
MIT OR Apache-2.0. The exact resolved zip feature
`deflate-flate2-zlib-rs` includes `deflate-flate2`, `_deflate-any` and flate2's
zlib-rs backend; no zopfli, default ZIP crypto or other compression is enabled.
The core lock remained SHA-256
`4cd1fbf8107a078ecc344cb83a54346dd5260c38a1a49a00a633d3b870678bb5`.

The receipt above is retained as **superseded graph evidence**. It predates the
rawzip and direct flate2 edges below; the manager-supplied
`MG sync-server-deps --resolve-existing` receipt `20261116T040827Z-95770`
is PASS for the actual resolved graph, including scoped cargo-deny/cargo-about
and normal/build/dev feature trees. Only one package was added: rawzip 0.5.2,
MIT, with resolved node features exactly `alloc`, `std` and an empty dependency
list. Its own declared development dependencies are not production graph edges.
Existing flate2 remains 1.1.11 with zlib-rs; the direct edge adds no new packages.
Existing zip 9.0.5 and its writer features remain unchanged. The server lock
SHA-256 is `867679c49c310e081f89e4c09750d57fc03e17165ddbd06b9c362816f5a923dd`;
the core lock remains
`4cd1fbf8107a078ecc344cb83a54346dd5260c38a1a49a00a633d3b870678bb5`.
This is graph/advisory/license evidence, not proof of adapter correctness.
No resolver, build or audit ran in this filtered fixer session; the supplied
resolved locks and exact manifests remain unchanged.

### Targeted archive reader replacement: measured graph, proofs pending

| Exact edge | Purpose, features and resource boundary | Maintenance, license and outstanding assessment |
| --- | --- | --- |
| `rawzip =0.5.2`, defaults disabled, `std` only | File-backed untrusted ZIP locator, lending directory iterator, local headers, compressed ranges, descriptors and EOF CRC/size verification. Caller-owned fixed buffers and its documented streaming, zero-index architecture avoid allocations sized by declared counts or ZIP64 metadata. Application count, extent, name, duplicate, size, overlap and mapping limits remain independent checks. | [Primary crate documentation](https://docs.rs/crate/rawzip/latest) identifies 0.5.2 released 2026-10-04, following 0.5.1 in July and 0.4.4 in March; it reports no normal dependencies. The primary repository identifies MIT and has tests/fuzzing. This is a small project, **not widely vetted**; these supplied facts do not establish a security audit. Receipt `20261116T040827Z-95770` measures MIT, exactly `alloc`, `std` and no dependency edges, with passing scoped graph/advisory/license checks. Adapter proofs remain pending. |
| `flate2 =1.1.11`, defaults disabled, `zlib-rs` only | Direct edge to the already selected raw-DEFLATE implementation, wrapped in rawzip's EOF verifier. A fixed-buffer adapter requires StreamEnd and exact consumed compressed extent; fixed-block staging and actual expanded-byte limits apply independently of size hints. | Existing superseded receipt measured flate2 1.1.11 (MIT OR Apache-2.0) and zlib-rs 0.6.8 (Zlib). Receipt `20261116T040827Z-95770` confirms the retained version and zlib-rs backend; the direct edge adds no packages. No other version/backend change is authorized. |
| `zip =9.0.5`, existing features unchanged | Stored download writer and existing test fixture reader/writer only. Untrusted production uploads no longer construct its `ZipArchive` or use its sequential reader. | Receipt `20261116T040827Z-95770` covers the combined graph; the exact writer pin and features are unchanged. |

The selected reader uses a bounded, single locator attempt and does not retry
false-footer candidates. The small extent adapter reads only bounded metadata
at rawzip's selected footer and record offsets; it does not select another
footer, index the upload in memory or decode ZIP structure independently.
Declared directory extent/count and actual record extents must agree exactly.
At most 26 members (including `_meta`) are admitted; directory metadata and
aggregate local-header metadata are each at most 128 KiB, `_meta` at most
50 KiB and each addition 1..100 MiB. Directory/local-header caller buffers are
64 KiB each, the staging block is 64 KiB, and DEFLATE adds one fixed 64 KiB
input block plus backend state. Metadata byte storage reserves a fixed 50 KiB,
not a hint-derived capacity. JSON allocations are bounded by that metadata
input cap; identifier/member/extent collections are bounded by 26 records.
Existing temporary-budget reservation bookkeeping scales with actual fixed-block
writes under the configured byte cap; this is not a constant total-memory claim. Actual expanded
bytes include metadata and must fit the configured request limit. ZIP input and
staging remain file-backed with temporary reservations before every write.
Hashes use actual decoded blocks, outside the publication barrier. All member
bytes, local/central agreement, CRCs, descriptors, extents, logical names and
mapping must validate before any staged content is durably published.

Supported input is deliberately bounded to classic single-disk stored/deflate
archives with contiguous local-member extents, version-needed at most 2.0,
and no local/central extra fields, self-extracting prefix, inter-record padding
or trailing data. ZIP64 forms and oversized header records (over the 64 KiB
caller buffers) are unsupported. Flags are limited to
UTF-8, descriptors and DEFLATE options; encryption and other unsupported flag
forms are rejected. The qualified descriptor limitation in the
[rawzip descriptor documentation](https://docs.rs/rawzip/latest/rawzip/struct.ZipDataDescriptor.html)
remains: an omitted signature with CRC `0x08074b50` can be falsely rejected.
This is an unsupported structural form, **not a media-payload byte ban**. No
exhaustive ZIP variant compatibility or optional/mobile acceptance is claimed.
The parent measured Desktop 26.09.4 finalized stored headers only, without
extras, descriptors or ZIP64; that evidence applies to the measured runtime.

The source change targets three existing HTTP reds without modifying proofs:
`stored_data_descriptor_upload_preserves_exact_bytes_and_listing_after_restart`
and `deflated_data_descriptor_upload_preserves_exact_bytes_and_listing_after_restart`
were HTTP 400 versus expected 200 in `20261116T032009Z-49447`;
`malformed_small_central_directory_and_zip64_metadata_reject_without_committed_effects`
was HTTP 200 versus expected 400 for declared directory size 97 in
`20261116T033105Z-54262`. The latter red predates the prior archive actor's edit;
the parent retains the SHA evidence. All three reds remain unverified after
the compile correction, not waived. No pass for these tests is asserted here.
The superseded speculative zip-index envelope/reader-wrapper proposal is removed;
the new production reader owns structural parsing and the adapter checks the
same library-selected metadata. Compilation, focused proofs, full server/lint,
independent review and provenance/privacy/similarity checks remain orchestrator
work, as does SDK reference acceptance. The measured graph receipt above does
not replace any of these checks. The manager's compile attempt
`20261116T040857Z-96092` failed with E0369 at archive.rs:168,171,188 because
rawzip's `EntryFlags` cannot be masked directly with integers. This fixer uses
the documented `bits(self) -> u16` accessor for the existing numeric masks and
local/central comparison, and replaces deprecated rawzip compression constants
with the identical `STORE`/`DEFLATE` constants. No compilation ran here.

The parent reports physical OS ENOSPC proof PASS 1/1 in
`20261116T035504Z-85991`, after correcting the test directory's 0700 setup.
The earlier `035005` attempt observed zero ENOSPC events and is not passing
ENOSPC evidence. The private 16 MiB mount and 32 MiB maximum fill fixture,
existing assertions, locks and all other source are unchanged by this fixer.

## Collection MCP card positions

The server pins `rand` 0.9.4 with defaults disabled and `std_rng` enabled to
match `notes/deck-placement-and-position#P4`. The seed is the stored position
counter. OAuth and collection secrets still use the existing OS randomness
path. `sync-server-deps/20261119T151112Z-72539` records the locked graph,
advisory check and package notices for this change. The MCP adapter uses the
existing HTTP stack. Its SDK trial was rejected by the dependency audit.
