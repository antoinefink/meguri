# Standalone server dependency assessment

Assessment date: 2026-11-15. Scope: the 14 production and two development direct
dependency edges in this standalone MIT workspace, for `servers/meguri-sync-web#W1,W2,W3,W7`.
This is a purpose, release-maintenance, feature and license assessment, not a
security guarantee or a formal source audit of every dependency. Official catalog
and release facts below were inspected by the infrastructure integrator on that
date and supplied to this filtered fixer; this session used no network.

The standalone resolved graph contains 133 packages, with 14 production and two
development direct edges. It includes resolved development and platform contexts;
this count does not mean that every package links into the Linux runtime. The
integrator completed the MG checks recorded below using Rust 1.99.0 in the Docker development environment.
This filtered documentation author ran no network, compilation, graph resolution
or MG jobs.

The frozen manifest SHA-256 is
`4cc3d8521c5afed32b3f940428ef5c4f97ce48912aeef668c161f28ad60083a4`;
the frozen lock SHA-256 is
`9e4f6785931ddbafb48ca03d35f91a7385ea3aa839fff53ea450c15fde4c28ab`.

## Browser body lifetime dependency

The `httpapi/ingress.rs` implementation directly uses
`http_body::{Body, Frame, SizeHint}` to poll Axum's incoming body without
consuming/dropping it on timeout, and implements the standard response body trait
so the unread receiver, admission permit and service/root owner stay alive through
the closing HTTP/1 response. This narrow ownership requirement justifies one
direct dependency; it is not a new parser or server framework. No direct
http-body-util dependency is needed or present.

Both the manifest and the updated real lock root dependency list now include this
production edge:

```toml
http-body = { version = "=1.1.0", default-features = false }
```

http-body 1.1.0 was already transitive, so the direct edge required no new resolved
package. Its exact locked metadata features are `[]`. Primary official catalogs
supplied by the integrator, inspected 2026-11-15:
[http-body latest](https://docs.rs/crate/http-body/latest) identifies latest stable
1.1.0, released July 13, MIT, Hyperium's typed HTTP body traits; the matching
[http-body-util catalog](https://docs.rs/crate/http-body-util/latest) identifies
0.1.5, August 12, MIT adapters, but it is not directly used or requested here.

Features below describe direct manifest requests. “Defaults” means
`default-features` was not disabled and no extra features were requested unless
listed. The integrator retained actual metadata and separate normal/build/dev
feature trees. Exact resolved metadata records argon2 0.6.0
`[alloc, password-hash]`, password-hash 0.6.1 `[alloc, phc]`, phc 0.6.1 `[alloc]`,
getrandom 0.4.3 `[]` and sha2 0.11.1 `[]`; unused sha2 alloc/OID defaults have been
removed. rusqlite resolves bundled libsqlite3-sys 0.39.1 with features
`[bundled, bundled_bindings, cc, default, min_sqlite_version_3_45_3, pkg-config, vcpkg]`.
These feature names do not establish the underlying SQLite version or a formal
dependency source audit. License expressions and scoped deny/about evidence
support this assessment; metadata alone does not supply notices or select every
transitive license alternative.

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

## Compatibility and ownership

The earlier Argon2 Salt API compile issue is fixed. The actual 0.5.3 fixed-input
synthetic PHC was characterized before the upgrade. Permanent 0.6 verification,
fixed-output byte-for-byte compatibility and new random-salt contract tests pass
in the full suite. No old Argon2 development dependency or capture-only artifact
writer remains.

The full suite includes storage, credential and recovery coverage, the
revision-only rehash restart fixture, JSON/ingress finite-unit checks and ordinary
HTTP cases. W2/W6 compare the exact original revision and generation at issuance;
they impose no relative ordering between these positive counters. The startup
check accepts a ready account whose same-password rehash advances revision while
generation stays fixed. Provisioning identity/proof checks and credential
issuance/revocation are unchanged.

There is no core, app, Anki, Python or Node production dependency in this Rust
workspace. New first-party server code is MIT, authored independently; no Anki
implementation is used. Future public-model reuse from core is a separate audit
boundary: reused MPL modules retain their original MPL terms and are not
relicensed MIT by inclusion here.

## Integrator verification evidence

The integrator retained the following actual MG receipts for the exact formatted
source and frozen manifest/lock:

- Dependency assessment:
  `/private/tmp/meguri-sync-U4-verify/build/mg/sync-server-deps/20261115T153142Z-38556/result.json`.
  Scoped cargo-deny advisories (including unmaintained, unsound and yanked policy),
  bans, licenses and sources all reported zero errors and warnings. Cargo-about
  notices, metadata, normal/build/dev feature trees and exact lock/config artifacts
  were retained. These results are not an absence-of-risk guarantee.
- Full server suite: 83/83 pass, zero skipped,
  `/private/tmp/meguri-sync-U4-verify/build/mg/sync-server-test/20261115T153913Z-41515/result.json`.
- Correctness/suspicious clippy checks across all targets and features, plus the
  default production check: pass,
  `/private/tmp/meguri-sync-U4-verify/build/mg/sync-server-lint/20261115T153543Z-40360/result.json`.
  This does not claim that all cosmetic warnings are zero.
- Focused MG evidence-parser/argument synthetic Bats checks: 13/13 pass,
  `/private/tmp/meguri-sync-U4-verify/build/mg/selftest/20261115T144920Z-3589/result.json`.

Evidence limits: a broad earlier operational pre-fix reproduction was refused by
the model cybersecurity guard and is preserved as unavailable red-proof, never a
passing result. It was not recreated or routed. Authorized actual replacement
proof consists of finite in-process admission/deadline/cancellation/lifetime
checks, controlled local authorization/SQLite fixtures and ordinary single-request
HTTP boundary/matrix tests. Private HTTPS/compiled-browser evidence belongs to U8;
protocol, current-client and device compatibility belong to later units. Controlled
phase faults do not imply physical disk-full proof. Independent final source
review and parent canonical provenance/privacy/similarity/full gate remain pending;
final server acceptance is not claimed.

Read-only MG reproducibility commands for the integrator (not executed by this
filtered documentation author):

```sh
tools/mg sync-server-deps
tools/mg sync-server-test
tools/mg sync-server-lint
```

The normal dependency job is locked/read-only and retains graph/deny/about
evidence. Dependency mutation with `--update` is optional maintenance, not an
integration prerequisite. These exact local receipt paths identify the supplied
evidence; the unit review log is the durable repository acceptance record.
