# specs/sync/transport.md and specs/sync/login-endpoints.md (U105)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to state the transport, endpoint
and login behavior in Meguri's own words; no code was copied. Server facts
marked "verified" were also probed against the U13 rig (26.9.3 image) with
Meguri-shaped requests on 2026-10-15.

| Anki file | Read for | Spec sections |
|---|---|---|
| `rslib/src/sync/http_client/mod.rs` | one session key per client object, base-62 of a random 32-bit number; the header is built per request; 30 s default stall time; the default endpoint | transport T2, T4, T8; login-endpoints L3 |
| `rslib/src/sync/http_client/io_monitor.rs`, `rslib/src/sync/http_client/full_sync.rs` | stall watchdog over request and response bytes, 1 s polling, no total timeout; status checked before `anki-original-size`; a 308 is surfaced with its `Location`; byte counts for progress | transport T6, T7, T8 |
| `rslib/src/sync/http_client/protocol.rs`, `rslib/src/sync/collection/protocol.rs`, `rslib/src/sync/media/protocol.rs` | method names, `sync/` and `msync/` path joining; argument-less methods send `{"_pad":null}` and the server accepts `{}` | transport T1, T5; login-endpoints L2 |
| `rslib/src/sync/request/mod.rs`, `rslib/src/sync/request/header_and_stream.rs` | the `anki-sync` header members and order; zstd request bodies; the server's payload limits (100 MiB, 300 MiB decompressed); the server reads the header as visible ASCII | transport T2, T5 |
| `rslib/src/sync/response.rs` | `anki-original-size` on zstd responses; `upload` answers plain text | transport T6.2 |
| `rslib/src/sync/error.rs` | server error responses: bare status code text; 308 as a permanent redirect | transport T6.1, T14 |
| `rslib/src/sync/version.rs` | supported versions 8 to 11; v11 drops `hostNum` for a redirect | transport T1, T2 |
| `rslib/src/sync/collection/meta.rs` | the redirect is followed once, on `meta` only, with no checks; `cv` in the meta body; the server answers 501 to an unsupported body version | transport T3, T7 |
| `rslib/src/sync/login.rs`, `rslib/src/backend/sync.rs` | `hostKey` body `u`/`p`, answer `key`; endpoint text resolved against `./`; login returns the key and the given endpoint | login-endpoints L1, L5 |
| `rslib/src/sync/http_server/mod.rs`, `rslib/src/sync/http_server/user.rs`, `rslib/src/sync/http_server/handlers.rs`, `rslib/src/sync/http_server/routes.rs` | 403 for unknown users, wrong passwords and unknown host keys; host key derivation; 409 for a different session key; 400 for a missing one; media `begin` needs a non-empty `v`; routes at `/sync` and `/msync` | transport T2, T4, T14; login-endpoints L6, L7 |
| `rslib/src/sync/media/begin.rs` | media `begin` body member `v` carries the client version | transport T3 |
| `rslib/src/version.rs` | Anki's client version strings (short `c`, long `cv`) | transport T3 |
| `rslib/src/error/network.rs` | how Anki's client classifies statuses and network failures; the URL is stripped from error text | transport T11 |
| `rslib/src/backend/mod.rs` | HTTP/1.1-only client with library defaults (automatic redirects on, system proxy variables honored) | transport T1, T7, T10 |
| `qt/aqt/profiles.py`, `qt/aqt/sync.py`, `qt/aqt/preferences.py`, `qt/aqt/forms/preferences.ui` | redirected endpoint stored per profile and cleared with the key on auth failure; user name trimmed; 60 s default and 30 to 99999 s range of the network timeout | transport T8; login-endpoints L4, L5 |
| `rslib/src/sync/collection/tests.rs` (host key, redirect tests) | harvest classification of the transport rows | catalog |
