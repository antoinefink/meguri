# Server host, binding and security layer

Spec: servers/host-security
Requirements: R1663, R1665, R1666, R1668, R1670, R1672 (status model), R1673, R1674, R1675, R1676, R1677, R1678, R1789 (write path), R1790 (read concurrency), R1949; AE78; R1659; supports R1664, R1667, R1669, R1671, R1679, R1748, R1750, R1797
Anki: 26.09.3 (no Anki behavior: both protocols are add-on protocols, and Anki's own collection behavior is reached only through the specs cited)
Scenarios: none yet in `conformance/`. The server transcript format is `servers/transcripts`; until its runner exists (U136), H17 lists the planned scenarios of `conformance/scenarios/servers/host/` as request and reply tables.

What both of Meguri's local servers share on the Mac: where and when they listen, how they find out that another app holds their port, the `Host` check that runs before anything else, HTTP/1.1 framing, keep-alive and body limits, how requests reach the collection, how the servers survive sleep, wake and collection replacement, the status each server reports, the local integrations store, and the channel between the Rust host and Swift.

The two servers are the AnkiConnect-compatible server (default port 8765) and the Migaku-compatible server (port 44432). The external interfaces they target are AnkiConnect API version 6 as served by the AnkiConnect add-on build of 2025-11-09, and the Migaku browser extension 1.30 (build 1.30.16) and 1.31 (build 1.31.0.17) connected to the Migaku Anki add-on 0.6.6. Neither interface defines the behavior in this file: neither original checks `Host`, the Migaku add-on binds every interface, and the AnkiConnect add-on binds `127.0.0.1` alone (IPv4 only) and sets no body limit. H16 lists the differences.

Other files own the protocols:

- Migaku routes, origins, CORS, preflight and replies: `servers/migaku-protocol` (U137); `/receive`: `servers/migaku-receive` (U139); routing: `servers/migaku-routing` (U138).
- AnkiConnect envelope, origins, API key and errors: `servers/ankiconnect-envelope` (U141); actions: `servers/ankiconnect-actions` (U142), `servers/ankiconnect-media` (U143), `servers/ankiconnect-gui` (U146).
- The transcript format and comparison rules: `servers/transcripts` (U136). The client call inventory: `servers/ankiconnect-clients`.

## H1. Terms

- **Server**: one of the two integrations, `ankiconnect` or `migaku`. Each has its own on/off setting, both off by default (R1663).
- **Port**: the TCP port of a server. Migaku: always 44432 (R1675). AnkiConnect: the `ankiconnect.port` setting (H10), default 8765. In the test-only ephemeral bind mode (H22) it is the port the operating system gave the IPv4 socket. **Bound port**: the port the server's listener set is bound to; in the app it always equals the port.
- **Listener set**: the listening sockets of one server, at most two: `127.0.0.1:<port>` and `[::1]:<port>`.
- **Host**: the part of the engine side that owns both servers, their runtime, the integrations store and the channel to Swift (H11). There is one host per process.
- **Available**: the collection is available in the sense of `engine/lifecycle` L6.
- **Head hook**, **error responder**, **handler**: the three parts each protocol supplies to the host (H19). The error responder turns a host-level refusal (H8's over-limit body, H9.4's caught failure) into that server's own error reply. The host never writes a protocol body itself, except the empty-body replies of H6 and H7.
- **Config snapshot**: the immutable copy of the integrations config (H10) that a request reads from its first byte to its reply.

## H2. Platforms

- The servers exist only in the macOS app (R1664) and in Linux builds used for tests in Docker. On iPhone and iPad, every server operation of the FFI returns `Unsupported` and changes nothing; the FFI surface is the same on every slice (KTD9), so Swift code that calls it compiles everywhere.
- The servers run on their own async runtime, separate from any runtime the sync code uses. Nothing in the host runs on the main thread, and nothing on a request's path waits for the main thread, except the interactions of H11 (which a request awaits without holding any lock or reader).
- When the Meguri process is not running, nothing listens on either port (R1668). Meguri ships no helper process, launch agent or queue for that case; a client then sees a refused connection, exactly as when Anki is closed.

## H3. When a server listens

| Condition | Server state (H12) |
|---|---|
| the server's setting is off | `off`; no socket |
| on, and no open attempt of this process has ended yet | `waiting_for_collection`; no socket |
| on, after the first open attempt of this process ended, whatever its outcome | binding (H4), then `listening` or `port_in_use` |

- The host is created when the app launches, but it opens no socket and does no network work until the first open attempt of the process ends: the engine reports the collection `Ready`, `Failed{Corrupt}` or `Failed{reason}` (`engine/lifecycle` L1, R1666). The servers therefore never delay launch or the first frame. When that event arrives, every enabled server starts binding at once, in parallel. After a failed first open the servers listen with `collection_available: false`, so Settings shows "Collection unavailable" (R1672) and clients get their protocol's unavailable error rather than a refused connection (H18.7).
- `NoCollection` before any open attempt (the process has not tried to open a collection yet) counts as "not ended": the servers wait.
- Once a server listens, it keeps listening until its setting is turned off, its port changes (H4.4), or the process ends. A later exclusive phase, a collection replacement, a close and reopen, a failed open or sleep never stop it (R1670); while the collection is not available, requests that need it follow their protocol's wait rules (H9) and the status shows `collection_available: false`.
- Turning a server on after the first open attempt ended starts binding at once, whether the collection is available or not.

## H4. Binding (R1673, R1674, R1675, AE78)

### H4.1 Addresses

- The bind addresses are constants: `127.0.0.1` first, then `[::1]`. No setting, environment variable or launch argument changes them. A server never binds `0.0.0.0`, `::`, a LAN or VPN interface address, or a host name.
- `[::1]` is bound only after `127.0.0.1` was bound successfully on the same port. While `127.0.0.1` is not bound, nothing is bound on `[::1]` either.
- The listening sockets allow address reuse, so a restart of Meguri rebinds at once while old connections linger in TIME_WAIT. They never allow port sharing between processes (no `SO_REUSEPORT`).
- IPv6 sockets are IPv6-only (no IPv4-mapped addresses).

### H4.2 The probe

Address reuse on macOS lets a bind to `127.0.0.1:<port>` succeed even while another process listens on the wildcard address of that port, and the more specific socket then takes the connections. Anki's Migaku add-on listens on the wildcard address of 44432. A bind error alone therefore does not tell Meguri that the port is taken, and Meguri must never silently take connections away from another app. Before every bind attempt the host probes:

1. Open a TCP connection to `127.0.0.1:<port>` with a 500 ms timeout. Send nothing.
2. If the connection is established, close it at once: the port is **in use**. If the connection is refused, the port is **free**. A timeout, or any other error, counts as in use.
3. If free, bind `127.0.0.1:<port>` and listen. A bind error of any kind (in use, permission, address not available) counts as in use, and the socket is closed.
4. If the IPv4 bind succeeded, bind `[::1]:<port>` and listen. If that fails (no IPv6 loopback, in use, any other error), the server listens on IPv4 alone: status `listening` with one address. The IPv6 bind is retried on each retry tick (H4.3) and each revalidation (H9.3) while it is missing; the IPv4 socket is never closed for it.

The probe and the bind of one attempt run back to back with no wait between them. The probe connection never reaches Meguri's own listener: a server whose listener set already holds `127.0.0.1:<port>` does not probe that port again (H9.3 checks its own sockets differently).

### H4.3 Port in use and retry

- When the port is in use, the server's state is `port_in_use{port}` and it holds no socket. It tries again with the full probe of H4.2 each time. Attempts start 10 s apart, measured from the start of the previous attempt (a slow attempt, such as a 500 ms probe timeout, does not push the next one back). The retry period runs on the host's timer clock (H21), so tests advance it without waiting.
- When an attempt finds the port free and the bind succeeds, the state becomes `listening` within that attempt. A free port is found at once (the probe is refused) and the bind takes no wait, so the server listens on a port freed by Anki quitting within 10 s of it being freed, plus the few milliseconds of the probe and the bind (AE78).
- A revalidation (H9.3) while `port_in_use` starts an attempt at once; the next attempts then start 10 s apart from it.
- Settings shows, from this status, "Port <n> is in use by another app — quit Anki or change the port" (R1674). The text is U144's; the host only reports the state and the port. For the Migaku server, whose port cannot change, U144 shows its own variant.
- Retrying continues until the server is turned off or its port changes. It never stops on its own and never falls back to another port.

### H4.4 Changing the AnkiConnect port

- `ankiconnect.port` takes an integer from 1024 to 65535, other than 44432 (the Migaku port, whether Migaku is on or not). The config apply (H10.3) refuses any other value. In the ephemeral bind mode (H22) the member is ignored for binding.
- A changed port takes effect when the config is applied: the server stops as when turned off (H10.4) on the old port, then binds the new port from H4.2 step 1. Connections on the old port, idle or not, are closed after their in-flight reply.
- Applying a config with the same port changes nothing on the sockets.

## H5. Loopback only (R1949)

- The listener set never holds a socket on any address but `127.0.0.1` and `::1` (H4.1). In Docker, a client that connects to the container's own non-loopback interface address on either port gets a refused connection at the TCP level.
- As a second line, each accepted connection's peer address is checked before any byte is read: a peer outside `127.0.0.0/8` and `::1` is closed at once, with no reply. (A loopback-only listener cannot normally receive such a peer.)
- The status lists exactly the bound addresses (H12), so tests can assert that no wildcard or LAN address ever appears.

## H6. The `Host` check (R1676)

The first check of every request, on both servers, before origin checks, routing, the body, WebSocket upgrades and preflight. It uses only the request head.

### H6.1 Accepted values

A request passes when it has no `Host` header, or exactly one `Host` header whose value, after removing spaces and tabs at both ends, is `<name>:<port>` where:

- `<name>` is one of `127.0.0.1`, `localhost`, `[::1]` or `host.docker.internal`, compared ASCII case-insensitively (`LOCALHOST` passes, `[::1]` has no case);
- `<port>` is the bound port of this server (H1) in decimal, as digits only, with no sign, no leading zero, and nothing after it.

A request passes whichever of its server's sockets it arrived on: `localhost:8765` is accepted on `[::1]:8765` and `[::1]:8765` on `127.0.0.1:8765`.

### H6.2 Refused values

Every other case is refused. In particular:

| `Host` value | Why refused |
|---|---|
| `evil.example:8765` | name not in the list |
| `127.0.0.1:8766` (on port 8765) | other port |
| `localhost` | no port |
| `localhost:` | empty port |
| `localhost:08765` | leading zero |
| `localhost.:8765` | trailing dot: another name |
| `127.1:8765`, `2130706433:8765`, `0x7f000001:8765` | other spellings of the address are not in the list |
| `[0:0:0:0:0:0:0:1]:8765`, `[::ffff:127.0.0.1]:8765`, `::1:8765` | other spellings of the IPv6 loopback |
| `user@localhost:8765` | userinfo |
| empty value | not a listed name |
| two `Host` headers, equal or not | ambiguous |

- When the request target is in absolute form (`POST http://localhost:8765/ HTTP/1.1`), the target's authority must also pass H6.1; when it does not, the request is refused even if `Host` passes.
- A refused request gets status 403 with an empty body and these headers only: `Content-Length: 0` and `Connection: close`. It carries no `Content-Type` and no CORS header. The connection closes after the reply, without reading the body. No handler runs, the collection is not touched, and nothing is logged in a protocol log (the AnkiConnect activity log or Recently received). A WebSocket upgrade refused here gets the same reply and no 101.
- A missing `Host` header is accepted so that plain HTTP/1.0 clients and tools that build requests by hand keep working. The DNS-rebinding protection rests on the check above: a browser that reached the server through a rebinding name always sends that name in `Host`.

## H7. HTTP framing and connections (R1677)

### H7.1 Protocol

- Both servers speak HTTP/1.1, and accept HTTP/1.0 requests (answered with HTTP/1.1 status lines, as HTTP allows). HTTP/2, prior-knowledge HTTP/2 and TLS are not offered; a connection that starts with the HTTP/2 preface or a TLS handshake is closed without a reply.
- Replies always carry `Content-Length` (also `Content-Length: 0` for empty bodies) and are never chunked. The host may add `Date`. It never adds `Server`, `Keep-Alive` or any CORS header; CORS headers come from the protocol specs.
- Header names compare ASCII case-insensitively. Unknown request headers are ignored.

### H7.2 Malformed requests

Before the `Host` check can run, the request head must parse. These are answered by the host, on both servers, with an empty body, `Content-Length: 0` and `Connection: close`, and the connection then closes:

| Case | Status |
|---|---|
| request line or a header line not valid HTTP/1.x, a header name with a space, a bare CR or LF | 400 |
| both `Content-Length` and `Transfer-Encoding`, two different `Content-Length` values, a non-decimal `Content-Length`, or a `Transfer-Encoding` other than `chunked` | 400 |
| request head larger than 65,536 bytes, or more than 100 header fields | 431 |
| an HTTP version other than 1.0 and 1.1 | 505 |

A malformed chunked body found while reading the body is answered the same way with 400. No handler runs.

### H7.3 Keep-alive and timeouts

| Rule | Value |
|---|---|
| idle time before Meguri closes a connection: from accept until the first byte of a request, and from the end of each reply until the first byte of the next request | 5 s |
| time from the first byte of a request head to its last byte | 10 s; then the connection is closed without a reply |
| time without any body byte while a body is being read | 30 s; then the connection is closed without a reply |
| time to send a reply | not limited; a peer that stops reading is closed after 30 s without progress |
| time a request may wait for the collection or an interaction | the protocol's own rule; the idle timeout does not run while a request is in progress |

- A connection serves any number of requests in order. A pipelined request (sent before the previous reply) is answered after the previous reply, in order.
- A request with `Connection: close`, or an HTTP/1.0 request without `Connection: keep-alive`, gets its reply and then the connection closes cleanly (the reply carries `Connection: close`). Clients that close after each request (as AnkiConnect clients expect, since the add-on always does) work unchanged.
- A reply sent before its request body was read to the end (H6, H7.2, H8, and the protocol replies that P2 of `servers/migaku-protocol` names) carries `Connection: close`, and the connection closes after it; the unread body is discarded.
- Upgraded WebSocket connections (Migaku only) leave these rules once the 101 is sent; `servers/migaku-protocol` P10 owns them.

### H7.4 `Expect: 100-continue`

When a request carries `Expect: 100-continue` and has a body, the host sends `100 Continue` only after the `Host` check passed, the protocol's head hook returned Continue (H19; a Migaku 403, 404 or 405 is therefore sent without it), and the declared `Content-Length`, if any, is within the limit (H8). Otherwise the final reply is sent without `100 Continue`. Other `Expect` values are ignored.

### H7.5 Connection limit

Each server serves at most 128 HTTP connections at once (upgraded WebSockets do not count; Migaku caps those itself). A connection accepted beyond that is closed at once, before any byte is read. Connections over the limit never block the accept loop.

## H8. Body limits (R1678)

| Server | Limit |
|---|---|
| AnkiConnect | 268,435,456 bytes (256 MiB) |
| Migaku | 134,217,728 bytes (128 MiB) |

- A body is the request's content after HTTP framing: the `Content-Length` bytes, or the decoded data of a chunked body (chunk sizes, extensions and trailers do not count).
- **By `Content-Length` first.** When `Content-Length` is larger than the limit, the request is refused before any body byte is read.
- **By counted bytes otherwise.** A chunked body is counted while it is read; when the count passes the limit, reading stops and the request is refused.
- A body exactly at the limit is read whole and handed on. The check is on bytes only; nothing parses the body before it.
- A refused request gets the server's over-limit reply from its error responder (H19, refusal `OverLimit`: `servers/migaku-protocol` P8.3 for Migaku; `servers/ankiconnect-envelope` E3 row 3 for AnkiConnect), with `Connection: close`. The handler never runs; nothing is written to the collection, the media folder or the integrations store.
- Handlers receive the body as bytes. No request is ever refused for its `Content-Type`, missing or not.
- These limits also cap every remote download the shared fetcher makes for that server (H13).

## H9. Collection access

### H9.1 Handles and generations

- In the app, the host uses the engine session that the FFI opened: `servers_start` (H20) takes no collection argument and builds the host on the `CoreHost` that the FFI already holds. In headless runs (Docker, conformance), the test construction (H22) is given a `CoreHost` opened on the scenario's collection (`engine/host` H1).
- A handler gets a collection handle of the current generation (`engine/lifecycle` L3) for each request, never one kept from an earlier request. After `CollectionReplaced{g+1}` (`engine/change-stream` S3), the next request works on generation g+1.
- A read request that fails with `StaleGeneration` because a replacement completed while it waited takes a new handle and runs once more, within its protocol's remaining wait time. A write request follows its protocol's rule (for Migaku, `servers/migaku-receive` P14).

### H9.2 Reads, writes and waiting (R1789, R1790, R1659)

- Reads use the two server readers (`engine/lifecycle` L11). Two read requests run at the same time; a third waits for a reader. Reads never take the interactive or query readers, so they never delay the UI's reads.
- Writes are ops with source `LocalApi` (`engine/op-model` O1). They share the interactive queue with UI writes and commit in arrival order there (R1790), each request's write as one atomic op (`servers/ankiconnect-envelope` E14 and `servers/ankiconnect-actions` A3 say which AnkiConnect actions write; `servers/migaku-receive` P16 is Migaku's). They mark `mod` and `usn` like UI writes and keep unknown data byte for byte (R1789).
- **The waiter.** A request that needs the collection while it is not available waits with the engine's availability wait (`engine/lifecycle` L6), with the protocol's timeout: 5 s for Migaku (`servers/migaku-receive` P14), 30 s for AnkiConnect (R1797, U141). On timeout the protocol replies with its unavailable error. The timeout runs on the host's timer clock (H21). While a request waits it holds no reader, no lock and no op slot.
- Protocols can read the current lifecycle state and whether the long-operation slot is held (and by what kind of operation) at any time, without waiting (`engine/lifecycle` L1, L5); `servers/migaku-protocol` P9 uses it for `/srs-check`. A normal sync does not make the collection unavailable (L6), so a request that arrives during it is queued with the other writes and is never lost (R1659).
- During an exclusive phase both listeners keep accepting, and requests that need no collection (preflight, `/srs-check`, WebSocket upgrades, the AnkiConnect ping) are answered at once (R1670).

### H9.3 Sleep, wake and network changes (R1670)

- Swift calls the host's revalidate operation on wake from sleep and on every network path change. The host then checks each socket of each listening server: it opens a connection from the host to that socket's own address with a 500 ms timeout and closes it at once. Such a self-check connection is recognized by the host and never reaches a handler or a log.
- A socket that fails its check is closed. If the IPv4 socket failed, the IPv6 one is closed too and the server goes back to H4.2 step 1. If only the IPv6 socket failed, it is rebound as in H4.2 step 4.
- Revalidation also runs on `CollectionReplaced`, though a replacement never touches the sockets.
- Per state: `listening` is checked as above; `port_in_use` starts an attempt at once (H4.3); `binding` (an attempt or a restart wait in progress), `off` and `waiting_for_collection` do nothing.
- Open keep-alive connections that the operating system dropped end with a transport error; that is not a failure of the server.

### H9.4 Failures inside the host (R1670)

- A failure inside one request's handler that unwinds (a panic) is caught for that request. The request gets its server's internal-error reply from the error responder (for Migaku, 500 with `Meguri hit an internal error.`, `servers/migaku-protocol` P8.2), the connection closes, and the failure is logged without request content. Other connections and the listener carry on.
- A listener task that ends unexpectedly is restarted by the host: its sockets are closed and the server goes back to H4.2 step 1 after 1 s, then 2 s, 4 s and 8 s for repeated failures within 60 s, and 10 s after that. The waits and the 60 s window run on the host's timer clock (H21). During the wait the state is `binding`. Each restart is logged. Status returns to `listening` once the bind succeeds.
- No request, body or connection pattern makes the host stop listening or close another connection. The fuzz targets of the protocol units cover the parsers; the host's own fuzz target feeds arbitrary bytes as a connection's input and checks that the listener survives.

## H10. The integrations store (R1665, R1748)

### H10.1 The file

- `meguri-integrations.db`, a SQLite file in the app-private Application Support directory, beside the collection's folder, never inside it (KTD12). Files that later units keep with it (pending `/receive` payloads, `servers/migaku-receive` P15) go in the folder `meguri-integrations/` beside it.
- Nothing in it is ever written to the collection: no config key, no column, no `col.mod` change. Sync, `.apkg` and `.colpkg` export never carry it (R1665). Changing any integration setting leaves the collection file byte for byte as it was.
- The schema has a version in `PRAGMA user_version`. U135 creates version 1 with one table for the config document. Units that need tables (U138 profiles and routing state, U139 the received log, the receive index, the intents and the pending queue, U140 the move history, U141 the activity log, U143 the external-folder index; `servers/migaku-received-log` L1) add migrations in unit order; each migration raises the version by one and runs in one transaction at open.
- At open, a version higher than this build knows (a newer Meguri wrote it) is not migrated down: the store opens read-only, every server stays `off`, and the status reports `store_error: "newer"` until a matching build runs. A file that fails SQLite's quick check, or cannot be opened, is renamed `meguri-integrations.db.corrupt-<UTC yyyyMMddTHHmmssZ>` and a new store with defaults is created; the status reports `store_error: "recreated"` once, until the next apply.
- The API key is never in the store, the logs or the status. Swift keeps it in the Keychain and passes it with each apply (H10.3).

### H10.2 The config document

One JSON object. Members and defaults:

| Member | Type | Default | Owner of the meaning |
|---|---|---|---|
| `migaku.enabled` | bool | `false` | H3 |
| `migaku.extra_origins` | list of strings | `[]` | `servers/migaku-protocol` P3.2 |
| `ankiconnect.enabled` | bool | `false` | H3 |
| `ankiconnect.port` | integer | `8765` | H4.4 |
| `ankiconnect.cors_origins` | list of strings | `["http://localhost"]` | `servers/ankiconnect-envelope` E7 (R1742, R1743) |
| `ankiconnect.deny_origins` | list of strings | `[]` | `servers/ankiconnect-envelope` E9 (R1749, R1750) |
| `ankiconnect.api_key_required` | bool | `false` | `servers/ankiconnect-envelope` E8 (R1747) |
| `ankiconnect.read_only` | bool | `false` | `servers/ankiconnect-envelope` E13 (R1779) |
| `ankiconnect.keep_log` | bool | `false` | `servers/ankiconnect-envelope` E16 (R1780) |
| `ankiconnect.bring_to_front` | bool | `true` | `servers/ankiconnect-gui` G2 (R1787) |
| `ankiconnect.readable_folders` | list of `{path, bookmark}` | `[]` | `servers/ankiconnect-media` X1, X3.3 (R1772) |
| `ankiconnect.media_dir` | `{path, bookmark}` or null | `null` | `servers/ankiconnect-media` X1, X8 (R1770) |

- The names are written here with dots for reading; the document nests them (`{"migaku": {"enabled": false, "extra_origins": []}, "ankiconnect": {...}}`). `path` is an absolute path; `bookmark` is the security-scoped bookmark data that Swift created, base64, opaque to Rust.
- Members added by later units (Migaku profiles live in their own tables, not here) get a default here when they are added. A document read from the store with a missing member takes its default; an unknown member is kept unchanged when the document is written back, so a downgrade and upgrade does not lose it.

### H10.3 Applying a config

- Swift applies a whole document, together with the API key (a string, or none). The host validates the members in the order of H10.2's table, and refuses the document at the first member that fails, naming it by its dotted name (`ankiconnect.port`); nothing changes. The checks:

  | Member | Refused when |
  |---|---|
  | `migaku`, `ankiconnect` | present and not an object |
  | each bool member | present and not a JSON boolean |
  | `ankiconnect.port` | present and not an integer, or outside H4.4's range, or 44432 |
  | `migaku.extra_origins`, `ankiconnect.cors_origins`, `ankiconnect.deny_origins` | present and not a list whose items are all strings. The core does not check the entries' form: Settings refuses bad entries before applying (`servers/migaku-protocol` P3.2, U144), and an entry that is not an origin simply never matches |
  | `ankiconnect.readable_folders` | present and not a list of objects each with a string `path` that is an absolute path and a string `bookmark` |
  | `ankiconnect.media_dir` | present and neither null nor such an object |

  The document root must be an object (else the refusal names `document`). Missing members take their defaults; unknown members are never refused (H10.2).
- While the store is open read-only (`store_error: "newer"`, H10.1), every apply is refused with `StoreReadOnly` (H20) and nothing changes.
- A valid document is written to the store in one transaction, then becomes the new config snapshot in one atomic swap. Every request that starts after the swap uses it; a request already in progress keeps the snapshot it started with (R1748: changes take effect on the next request).
- The core's own changes to the document (`servers/ankiconnect-envelope` E9.1's list appends) are written and swapped exactly as an apply; applies and such changes run one at a time, in the order they reach the host.
- **The API key.** Each apply replaces the key held in memory; the key itself is held only in memory. The flag decides:

  | `ankiconnect.api_key_required` | Key given with the apply | AnkiConnect key state (`servers/ankiconnect-envelope` E8) |
  |---|---|---|
  | false | none, empty or any string | no key set; a given string is discarded |
  | true | none or empty string | set but unknown: every action except `requestPermission` gets the key error |
  | true | a non-empty string | set to that string |

  An apply without a key while the flag is true therefore clears a key applied earlier. Before Swift's first apply of the process, the flag read from the store alone decides: true gives "set but unknown".
- After the swap the host starts, stops or rebinds servers to match (H3, H4.4, H10.4), and posts a status event.
- At launch the host reads the stored document as the first snapshot; the servers then start as H3 says, once the first open attempt of the process has ended. The API key arrives with Swift's first apply.

### H10.4 Turning a server off

Within 1 s of the apply that turns it off (real time, H21):

- its listening sockets are closed (state `off`, no address in the status);
- idle keep-alive connections are closed;
- a request still waiting for the collection or an interaction is answered as if its wait had timed out, and its connection closes after the reply;
- open Migaku WebSockets get a close frame 1001 (`servers/migaku-protocol` P10.4).

The 1 s bound does not cover handlers already running (an action's collection work, a remote download): each finishes within its own limits (H13's time limits, its protocol's rules), sends its reply with `Connection: close`, and its connection then closes. They are not cancelled.

Turning it on again starts from H4.2 step 1.

## H11. The host channel to Swift

### H11.1 Events (Rust to Swift)

One callback interface that Swift implements, following KTD9: calls into it never block the caller and return at once; the host calls it from one dispatcher thread, never from a request's worker, never while holding a lock or a reader, and in the order the events were posted.

| Event | Payload | Posted by |
|---|---|---|
| status changed | the status of both servers (H12) | the host, on every status change |
| received log changed | none | `servers/migaku-receive` P20, `servers/migaku-received-log` L6 |
| received | received-log row id, note id, deck full name | `servers/migaku-receive` P20 |
| activity appended | the new activity-log entries | `servers/ankiconnect-envelope` E16 (R1780) |
| media ingest notices | the new ingest notices of the external media folder | `servers/ankiconnect-media` X11.9 |
| interaction request | `{id, kind, payload}` | H11.2 |
| interaction cancelled | `{id}` | H11.2 |

- There is no separate toast event: Swift shows the Migaku toast (U144) from the received event.
- The events cross the FFI as the `HostEvent` enum of H20.
- Status events coalesce: when several are waiting to be delivered, only the latest is delivered. No other event coalesces.
- Events posted before Swift registers its listener wait and are delivered once it does.

### H11.2 Interactions (Rust asks, Swift answers)

Some requests need the user or the UI: AnkiConnect's permission alert (R1749, R1750) and the GUI actions (U146).

- The requester posts an interaction with a kind, a JSON payload, a timeout and a timeout outcome, all defined by the requesting unit (for the permission alert: 120 s, outcome denied, nothing persisted; R1750). The host gives it an id unique in the process (`i-` followed by a decimal counter from 1) and posts the interaction request event. Timeouts run on the host's timer clock (H21).
- An interaction is either **awaited** (the request waits for its outcome; the rules below) or **detached** (the requester does not wait: it posts and goes on, as `servers/migaku-received-log`'s `searchNote` does with `BrowseNote`). A detached interaction has the same id, timeout, resolve and cancelled rules; its outcome, or its timeout, is only logged and reaches no request.
- Swift answers with the host's resolve operation `(id, outcome)`. The outcome is a JSON value whose shape the kind defines. The first valid answer resolves the interaction, and the waiting request continues with it.
- An answer for an unknown id, an id already resolved, an id whose interaction timed out, or an outcome the kind does not accept changes nothing and returns `false` (a valid resolution returns `true`).
- When the timeout passes first, the request continues with the timeout outcome, and the host posts interaction cancelled with the id so Swift can dismiss its alert.
- When the server is turned off or the app quits, every open interaction ends with its timeout outcome and is cancelled the same way.
- A request awaiting an interaction holds no reader, lock or op. Interactions of different requests may be open at once; each kind says whether Swift shows them one at a time.
- Headless runs use a test host instead of Swift (H22): it answers each interaction with a scripted outcome, a default outcome per kind, or no answer so that it times out (`servers/transcripts` T7.2, T7.5).

### H11.3 The audio transcoder

The one blocking callback into Swift (named in KTD9): `AudioTranscoder`, which `servers/migaku-receive` P11 calls to re-encode received audio. It exists on every slice; only the macOS app implements it (U144), and Linux test builds use a pass-through. It is called only from a blocking thread registered with the engine (`engine/jobs`), never from a server worker or the dispatcher thread.

## H12. Status (R1672)

Each server reports a status value:

| Member | Value |
|---|---|
| `server` | `ankiconnect` or `migaku` |
| `state` | `off`, `waiting_for_collection`, `binding`, `listening` or `port_in_use` |
| `port` | the port from the config snapshot (Migaku: 44432), in every state; in the ephemeral bind mode (H22), the bound port while `listening` and 0 otherwise |
| `addresses` | the bound socket addresses as `127.0.0.1:<port>` and `[::1]:<port>`, IPv4 first; `[]` unless `listening` |
| `collection_available` | in every state, `true` exactly when the collection is available (H1) at the time of the status; Settings uses it only in `listening` |
| `next_retry_at` | in `port_in_use`, the wall time (H21) of the next attempt's start, UTC, ISO 8601 with seconds and `Z`, for example `2030-01-15T10:00:10Z`; null in every other state |
| `store_error` | null, `"newer"` or `"recreated"` (H10.1); a property of the one integrations store, so always the same on both servers |
| `migaku` | Migaku only: the block of `servers/migaku-protocol` P10.5, in every state; absent on the AnkiConnect server |

- `binding` covers the probe and bind attempt of H4.2 and the restart wait of H9.4; it normally lasts well under a second and Settings may show it as Off.
- Settings maps the status to R1672's wording (U144):

  | Status | Shown |
  |---|---|
  | `off`, `waiting_for_collection`, `binding` | Off (or nothing) |
  | `listening`, `collection_available: true` | Listening on 127.0.0.1:<port> |
  | `listening`, `collection_available: false` | Collection unavailable |
  | `port_in_use` | Port in use by another app (R1674's sentence) |

- A status event (H11.1) carries both servers' values. One is posted on every change of any member except `next_retry_at` alone, including every change of availability (`Available` and each loss of availability, `engine/lifecycle` L6).

## H13. The shared fetcher

Both protocols download remote media through one fetcher (`servers/migaku-receive` P8; U143 for AnkiConnect's `url` source).

- Only `http` and `https` URLs, on the first request and on every redirect hop. Both directions between them are followed (an `https` page may redirect to `http`).
- Each caller sets a per-item time limit and a redirect limit: Migaku 8 s and 5 redirects (R1701), AnkiConnect 30 s and 5 redirects (R1773). The time limit runs in real time (H21) from the start of the fetch, name resolution included, to the last body byte. The redirect limit counts followed redirects: the redirect response that would need one more fails the item.
- Redirects: statuses 301, 302, 303, 307 and 308 with a `Location`, resolved against the current URL; every hop is a GET without a body. A 3xx response without `Location` is a final response.
- The body is capped at the calling server's body limit (H8) while it streams; a larger body fails the item. No content decoding is asked for (`Accept-Encoding: identity`) and none is applied.
- TLS uses rustls with the operating system's certificate verifier. No cookies are kept and no credentials are sent. The `User-Agent` is `Meguri/<app version>`. No proxy is used (H18.3).
- No destination host is refused: loopback and LAN addresses are fetched like any other. The originals refuse none either (`servers/migaku-receive` P8; the AnkiConnect add-on downloads any URL its HTTP client accepts).
- The fetcher does not judge the final status: it returns it, and each caller decides (both callers accept any 2xx: Migaku, `servers/migaku-receive` P8; AnkiConnect, `servers/ankiconnect-media` X3.4, where the add-on accepts exactly 200, H16).

The fetcher's result, which the callers map to their own messages:

| Result | When |
|---|---|
| `Response{status, content_type, body}` | a final response was read to its end within the limits: `status` the final status code (any, 2xx or not), `content_type` the response's `Content-Type` value or none, `body` the bytes |
| `UnsupportedUrl` | the URL is not an absolute `http` or `https` URL with a host, or a redirect's `Location` names another scheme or does not resolve to such a URL |
| `TooManyRedirects` | a redirect response arrived when the redirect limit was already reached |
| `Transport{reason}` | name resolution, connection, TLS or a transport error before the last body byte; `reason` is a short English text for logs and for AnkiConnect's `(<reason>)` |
| `TimedOut` | the time limit passed first |
| `TooLarge` | the body passed the cap |

When two could apply, the one met first in time counts.

## H14. Platform behavior that U144 provides

These are Swift's half of R1666 to R1671 and are listed so the host's contract is complete:

- While any server is `listening`, the app holds a process activity that prevents App Nap from delaying socket handling, so trivial requests are answered within 200 ms while Meguri is hidden or in the background (R1669).
- While either server is enabled, closing the main window does not quit the app (R1667).
- The sandboxed build carries the network-server and network-client entitlements. Loopback-only listening must not trigger the Local Network or firewall prompts, checked on a clean user account (R1671, U147).
- Swift calls the revalidate operation (H9.3) on wake and on network path changes, and applies the config with the API key from the Keychain at launch and on every settings change.

## H15. Ordering of the checks

For every request on either server, in order; the first that answers ends it:

1. H5 peer check (at accept).
2. H7.2 head parse.
3. H6 `Host` check.
4. The protocol's head hook (H19), on the head alone: it may answer at once (Migaku's origin, preflight, route and method checks, `servers/migaku-protocol` P2 steps 2 to 6; AnkiConnect's preflight, `servers/ankiconnect-envelope` E3 row 2) or continue. A protocol may leave part of its origin decision to its handler (AnkiConnect decides after the envelope is parsed, because `requestPermission` is exempt, E3 rows 4 to 6).
5. H7.4's `100 Continue`, then H8 body limit while the body is read; an over-limit body is answered by the error responder.
6. The handler, with H9's collection access.

Steps 1 to 3 and 5 belong to the host and are the same on both servers; steps 4 and 6 and the error responder are supplied by each protocol, so the host has no knowledge of either protocol's rules.

## H16. Differences from the originals

| Behavior | AnkiConnect add-on (2025-11-09) | Migaku add-on 0.6.6 | Meguri | Requirement |
|---|---|---|---|---|
| Bind address | `127.0.0.1` by default, any address by setting | every interface, IPv4 and IPv6 | `127.0.0.1` and `[::1]` only, not configurable | R1673 |
| Port in use | no probe; the bind outcome decides | no probe | probe, `port_in_use`, retry every 10 s | R1674 |
| `Host` check | none | none | H6 | R1676 |
| Keep-alive | closes every connection after one reply | the web framework's default | keep-alive, 5 s idle | R1677 |
| Body framing | `Content-Length` only; a chunked body is not read | the web framework's default | `Content-Length` or chunked | R1677 |
| Body limit | none | the web framework's default | H8 | R1678 |
| Concurrency | one request at a time, polled from the UI thread | the add-on's own thread | concurrent reads, ordered writes, never on the main thread | R1790, R1791 |
| Remote downloads (`url` sources) | Anki's HTTP client: a request timeout of `webTimeout` (10,000 ms) on each wait rather than a total limit, the HTTP library's default redirect handling and proxy settings, final status exactly 200 | not established | H13: total limits of 30 s (AnkiConnect) and 8 s (Migaku), 5 redirects, no proxy; status rule per caller | R1701, R1773 |

How the AnkiConnect transcript suite meets these differences (`servers/transcripts` T11, T12): the `Host` refusal is a deviation entry (T12, R1676); keep-alive needs none, since `closed` is never compared in oracle scenarios (T11); chunked request bodies appear only in declared scenarios, since the add-on frames by `Content-Length` alone and answers a chunked request as an empty-body ping (T12); remote downloads use only fixtures that answer at once with 200 or 404, where the two fetchers agree. Migaku has no oracle.

## H17. Planned scenarios

Each row is one declared scenario (`servers/transcripts` T3, `source: declared`) of `conformance/scenarios/servers/host/`, with `server: host`, so it runs in the fixed bind mode on its own network namespace (`servers/transcripts` T20) and every exchange names its server. Unless a row says otherwise, the server named is enabled at the start (`servers/transcripts` T4 `enabled`), the collection is `Ready`, requests use a new connection to `127.0.0.1`, carry `Host: 127.0.0.1:<p>` and no `Origin`, and the request is the protocol's simplest one that needs no collection: AnkiConnect `POST /` with an empty body (the ping, answered `{"apiVersion":"AnkiConnect v.6"}`), Migaku `GET /srs-check`. "OK" means that protocol's normal reply. `<p>` is the server's port. "Advance" is the `advance` step, which moves the host's timer and wall clock (H21); "wait" is the real-time `wait` step. Step names are those of `servers/transcripts` T6 and T7.

| Scenario | Setup and requests | Expected |
|---|---|---|
| `bind-both-addresses` | enable AnkiConnect; a ping to `address: ipv4`, then to `address: ipv6` | status `listening`, `addresses` `["127.0.0.1:8765", "[::1]:8765"]`; both pings OK |
| `bind-no-wildcard` | enable both servers; `sockets` | exactly `["127.0.0.1:44432", "127.0.0.1:8765", "[::1]:44432", "[::1]:8765"]`; none on `0.0.0.0`, `::` or another address |
| `port-in-use-ipv4-holder` (AE78) | `helper {listen: "127.0.0.1:8765"}`; enable AnkiConnect; `open` on `[::1]:8765` | status `port_in_use`, port 8765, `addresses` `[]`; the `open` outcome `refused` |
| `port-in-use-release` (AE78) | as above, then `helper {release: "127.0.0.1:8765"}`; advance 10 s | status `listening` on both addresses; ping OK on both |
| `port-in-use-wildcard-holder` | `helper {listen: "0.0.0.0:44432"}`; enable Migaku; `open` on `127.0.0.1:44432`; `helper {accepted: "0.0.0.0:44432"}` | status `port_in_use` with port 44432; `sockets` lists nothing on 44432; the `open` outcome `open` and the helper's accepted count 1 |
| `port-in-use-retry-time` | helper holds 8765; enable AnkiConnect at clock start; `status`; advance 10 s; `status` | `next_retry_at` 10, then 20 (offsets from clock start, `servers/transcripts` T7.3) |
| `ipv6-unavailable` | `host_env.ipv6: false`; enable AnkiConnect | status `listening`, `addresses` `["127.0.0.1:8765"]`; ping OK on IPv4 |
| `port-change` | AnkiConnect listening on 8765; `apply_config {ankiconnect: {port: 8766}}`; `status {until: {state: listening, port: 8766}, within_ms: 1000}`; `open` on `127.0.0.1:8765`; ping to `Host: 127.0.0.1:8766` on 8766 | status reached; the `open` outcome `refused`; ping OK |
| `port-refused-values` | `apply_config` with `ankiconnect.port` 80, 1023, 44432, 65536 and `"8765"` | each refused with member `ankiconnect.port`; `status` unchanged |
| `docker-non-loopback-refused` (R1949) | both servers on; an exchange to `address: non_loopback` on each | outcome `refused` on both |
| `host-accepted` | one request each with `Host`: `127.0.0.1:<p>`, `localhost:<p>`, `LocalHost:<p>`, `[::1]:<p>`, `host.docker.internal:<p>`, no `Host` header (`no_host`), and `localhost:<p>` sent to `address: ipv6`; on both servers | OK each |
| `host-refused` | one request each with `Host`: `evil.example:<p>`, `127.0.0.1:<p+1>`, `localhost`, `localhost:`, `localhost:0<p>`, `localhost.:<p>`, `127.1:<p>`, `[0:0:0:0:0:0:0:1]:<p>`, `user@localhost:<p>`, empty, and two `Host` headers `localhost:<p>`; on both servers; then `logs` | 403 each, empty body, `content-length: 0`, `connection: close`, no `content-type`, no `access-control-*` header, `closed: true`; `logs` `{activity: 0, received: 0}` |
| `host-absolute-target` | `POST http://evil.example:<p>/ HTTP/1.1` with `Host: localhost:<p>` | 403 as above |
| `host-before-origin-and-route` | Migaku `GET /nope`, `Host: evil.example:44432`, `Origin: https://evil.example`; AnkiConnect `{"action":"deckNames"}` with `Host: evil.example:8765` | 403 empty each (never 404, never the origin's 403 with its own headers, never a JSON reply) |
| `host-websocket` | `ws_open` Migaku `/anki-connect` with `Host: evil.example:44432` | handshake reply 403, no 101 |
| `malformed-head` | `raw` requests: `GARBAGE\r\n\r\n`; a header line `X Bad: 1`; `Content-Length: 5` and `Transfer-Encoding: chunked` together; `Content-Length: abc` | 400 each, empty body, `closed: true` |
| `head-too-large` | a request with one header whose value is `fill {size: 70000, byte: 0x61}` | 431, `closed: true` |
| `body-at-limit` | AnkiConnect: a body `fill {size: 268435456, byte: 0x20}` with `Content-Length`; Migaku: `POST /srs-import` with `fill {size: 134217728, byte: 0x20}` | the handler runs: AnkiConnect replies with its JSON-parse error envelope (a body of spaces is not empty and not JSON), Migaku 501 |
| `body-over-limit-length` | the same with limit + 1 in `declared_length` | the server's over-limit reply at once, before any body byte is sent, `connection: close`; the handler never runs |
| `body-over-limit-chunked` | limit + 1 bytes sent with `Transfer-Encoding: chunked`, no `Content-Length` | the over-limit reply after the limit is passed (the executor ignores its own write error, `servers/transcripts` T6.4), `connection: close`; `no_effect: true` |
| `expect-continue` | AnkiConnect `Expect: 100-continue`, `expect_continue: wait`, body `{}`; then the same with `declared_length` over the limit | first: `interim: [100]`, then the reply; second: the over-limit reply, `interim: []` |
| `keepalive-two-requests` | two pings on one named connection | both OK, `closed: false` after the second |
| `keepalive-idle-close` | ping on a named connection; wait 4 s; ping on it; `close_wait {name, not_before_ms: 4500, within_ms: 6000}` | the second ping OK on the same connection, `closed: false`; the server closes it about 5 s after the second reply |
| `keepalive-idle-before-first` | `open {name: c}` and send nothing | outcome `closed_without_reply`, `not_before_ms: 4500`, `within_ms: 6000` |
| `connection-close` | a ping with `Connection: close` | one reply with `connection: close`, `closed: true` |
| `http10` | a ping with `http_version: "1.0"` and no `Connection` header | reply, `closed: true` |
| `pipelined` | `pipeline` of two pings on one connection | two replies in order |
| `connection-limit` | `open {name: c, count: 128}`, then `open {name: x}` | `x` outcome `closed_without_reply` within 1000 ms; a ping on `c-1` and on `c-128` OK |
| `exclusive-phase-listening` | `lifecycle: exclusive`; AnkiConnect ping; Migaku `GET /srs-check` | both OK, `within_ms: 200` (`/srs-check` with `ok` false); status `listening`, `collection_available: false` |
| `generation-change` | `lifecycle: {replace: <setup with one deck Other>}`; AnkiConnect `{"action":"deckNames","version":6}` | `{"result":["Default","Other"],"error":null}` |
| `listener-crash` | `fault {listener_crash: {server: ankiconnect}}`; `status`; advance 1 s; `status {until: {state: listening}, within_ms: 1000}` | first `binding`; then `listening`; ping OK |
| `handler-panic` | `fault {handler_panic: {server: migaku, target: /srs-check}}`; `parallel` of Migaku `GET /srs-check` and an AnkiConnect ping; then Migaku `GET /srs-check` | the first `/srs-check` gets 500 `{"success":false,"error":"Meguri hit an internal error."}` and `closed: true`; the ping OK; the later `/srs-check` OK |
| `start-before-open` | `host_env.start: before_open`; AnkiConnect enabled; `status`; `sockets`; `lifecycle: open`; `status {until: {state: listening}, within_ms: 1000}` | first `waiting_for_collection` and `sockets` `[]`; then `listening` |
| `failed-open-listens` | `host_env.start: before_open`; AnkiConnect enabled; `lifecycle: open_fail`; `status {until: {state: listening}, within_ms: 1000}`; `{"action":"deckNames","version":6}` with `async: d`; advance 30 s; `await: d` | `listening` with `collection_available: false`; the awaited reply is the `collection is not available` envelope |
| `revalidate-after-wake` | `fault {drop_ipv4_socket: {server: ankiconnect}}`; `revalidate`; `status {until: {state: listening}, within_ms: 1000}` | `listening` on both addresses; ping OK on both |
| `toggle-off` | a named idle connection with one ping done; `lifecycle: exclusive`; `{"action":"deckNames","version":6}` with `async: w`; `apply_config {ankiconnect: {enabled: false}}`; `await: w`; `status {until: {state: off}, within_ms: 1000}`; `close_wait` on the idle connection | the awaited reply is the `collection is not available` envelope; status `off`, `addresses` `[]`; the idle connection closed within 1000 ms |
| `config-persist` | `apply_config {ankiconnect: {port: 8800}, migaku: {extra_origins: ["https://example.org"]}}` with `no_effect: true`; `restart_host`; `config` | `config` returns the same two values; the collection unchanged by both steps |
| `store-newer-version` | `host_env.store_user_version: 99`; both servers enabled in the store | status `off` on both, `store_error: "newer"`; an `apply_config` refused with `store_read_only` |
| `interaction-unknown-id` | `resolve {id: "i-999", outcome: "done"}` | `{resolved: false}`; `events` lists nothing |
| `interaction-timeout` | `interaction {kind: PermissionPrompt, answer: none}`; AnkiConnect `{"action":"requestPermission","version":6}` from `Origin: https://new.example` with `async: p`; advance 120 s; `await: p`; `events`; `resolve` with the posted id | the reply `{"result":{"permission":"denied"},"error":null}` (the timeout outcome, `servers/ankiconnect-envelope` E9); `events` holds the interaction request and then interaction cancelled with the same id; `{resolved: false}` |

Non-transcript checks (U135 Rust and Swift tests, not transcripts): the probe count of the retry schedule (one at enable, none 9 s later, the next at 10 s); the probe never connects to Meguri's own socket (`port-in-use-*` with Meguri already listening on the other server's port); the launch path runs no probe or bind before the first open attempt ends (R1666); the store file of `store-newer-version` is unchanged byte for byte; the host fuzz target of H9.4 runs 10 minutes clean; the status never lists an address outside `127.0.0.1` and `::1` across every scenario above; and on an iOS simulator build every operation of H20 returns `Unsupported` while the same generated bindings start the servers on the Mac (U6 symbol check).

## H18. Open questions

These could not be settled from the permitted sources. Until they are, Meguri follows the rule stated in the section named.

1. Docker Desktop on macOS: whether a container's connection to `host.docker.internal:<p>` reaches a listener bound only to the host's loopback addresses. R1676 accepts the name, so scripts in Docker are expected to work; if Docker's forwarding arrives from a non-loopback address, H4.1 and H5 refuse it and R1676's Docker case cannot work without a wider bind, which R1673 forbids. A manual check on the user's Mac settles it (U147) (H4.1, H6.1).
2. Whether the probe should also connect to `[::1]:<port>`. A process that listens on `[::1]:<port>` alone (and not on IPv4) is not detected; Meguri then binds IPv4, fails the IPv6 bind and serves IPv4 only (H4.2).
3. Whether the shared fetcher should use the system's HTTP proxy settings. KTD14's proxy handling is for manual Anki transfers. The AnkiConnect add-on downloads through Anki's HTTP client, which follows the HTTP library's default and so uses the environment's and system's proxy settings; what the Migaku add-on does is not established. Meguri connects directly (H13), so a user who needs a proxy for media downloads gets `Transport` failures where the add-on succeeded.
4. Which `Host` value AnkiConnect clients actually send. Browser extensions send the URL's authority (`127.0.0.1:8765` or `localhost:8765`); a client configured with another loopback spelling, such as `http://127.1:8765`, is refused by H6.2 (H6).
5. Whether any client depends on AnkiConnect closing the connection after each reply (for example by reading the body until close rather than by `Content-Length`). Meguri always sends `Content-Length`, so such a client still gets the whole body but then waits up to 5 s for the close (H7.3).
6. The bound of 128 connections per server and the 10 s head timeout are Meguri choices with no source; a client that opens many parallel connections (a `multi`-less batch) has not been observed (H7.3, H7.5).
7. Whether the servers should listen after a failed first open (H3). R1666 says a server begins listening "as soon as the collection is open" and does not say what happens when the first open fails; R1672 has a "Collection unavailable" state. Meguri starts binding when the first open attempt ends, whatever its outcome, so that state is shown and clients get an unavailable error rather than a refused connection.

## H19. Protocol hooks

Each server is the host's shared steps (H15 steps 1 to 3 and 5) plus three parts that its protocol unit supplies (U137 for Migaku, U141 for AnkiConnect). None of them sees the socket.

- **Head hook.** Input: the parsed request head (method, target, the header list in order, HTTP version), the server's bound port, whether the request announces a body and its declared `Content-Length`, and the config snapshot. Output, one of:
  - `Reply(response)`: the host sends it; when the request has unread body bytes, the host adds `Connection: close` and closes after it (H7.3).
  - `Upgrade(handler)`: a WebSocket upgrade the protocol accepts (`servers/migaku-protocol` P10); the protocol sends its 101 and owns the connection from then on, outside H7's rules.
  - `Continue(context)`: the host goes on with H15 step 5. `context` is an opaque protocol value (for example an origin verdict) that the host passes unchanged to the error responder and the handler.
- **Error responder.** Input: the head, the context, and the refusal kind, `OverLimit` (H8) or `Panic` (H9.4). Output: the reply. It never reads the body. Because it has the context, AnkiConnect answers `OverLimit` with its error envelope or with 403 by the origin (`servers/ankiconnect-envelope` E3 row 3), and Migaku adds its CORS headers (`servers/migaku-protocol` P6, P8.3).
- **Handler.** Input: the head, the context, the body bytes, collection access (H9), the clock (H21) and the interaction channel (H11.2). Output: the reply. It makes every decision that needs the body (AnkiConnect E3 rows 4 to 14).
- The host writes the framing: `Content-Length`, optionally `Date`, and `Connection: close` where H7 says. A protocol never sets framing headers.
- A panic in the head hook or in the error responder itself is answered by the host with status 500, an empty body, `Content-Length: 0` and `Connection: close`, the only reply that ignores the protocol, besides those of H6 and H7.

## H20. FFI surface

The host's operations in `core/crates/meguri-ffi/src/servers.rs`, identical on every slice (KTD9). Config documents and interaction payloads and outcomes cross as JSON text, because the document is open (unknown members are kept, H10.2) and payloads are defined per kind; statuses and events cross as typed records.

| Operation | Input | Output | Errors |
|---|---|---|---|
| `servers_start` | `listener: HostEventListener`, `transcoder: AudioTranscoder` | none. Creates the process's one host on the engine session the FFI holds (H9.1), opens the store and reads the first snapshot (H10.3), registers the listener (events posted before are delivered then, H11.1) | `AlreadyStarted`, `NoEngine` (the FFI holds no engine session yet), `Unsupported` |
| `servers_apply_config` | `document: String`, `api_key: Option<String>` | none (H10.3) | `InvalidConfig { member }`, `StoreReadOnly`, `NotStarted`, `Unsupported` |
| `servers_config` | none | `String`: the current document, every default filled in, unknown members kept | `NotStarted`, `Unsupported` |
| `servers_status` | none | `Vec<ServerStatus>`: AnkiConnect first, then Migaku | `NotStarted`, `Unsupported` |
| `servers_revalidate` | none | none (H9.3) | `NotStarted`, `Unsupported` |
| `servers_resolve_interaction` | `id: String`, `outcome: String` | `bool` (H11.2) | `NotStarted`, `Unsupported` |

- `ServersError`: `Unsupported`, `NotStarted`, `AlreadyStarted`, `NoEngine`, `InvalidConfig { member: String }` (H10.3's dotted name, or `document`), `StoreReadOnly`, `Internal { message: String }` (an unexpected store failure; the message is for logs and holds no key or request content).
- `HostEventListener`: a callback interface with one method, `on_event(event: HostEvent)`, which returns at once (H11.1).
- `HostEvent`: `StatusChanged { statuses: Vec<ServerStatus> }`; `ReceivedLogChanged`; `Received { row_id: i64, note_id: i64, deck: String }`; `ActivityAppended { entries: String }` (a JSON list of `servers/ankiconnect-envelope` E16 entries); `MediaIngestNotices { notices: String }` (a JSON list of `servers/ankiconnect-media` X11.9 notices); `InteractionRequest { id: String, kind: String, payload: String }`; `InteractionCancelled { id: String }`.
- `ServerStatus`: `server: ServerKind` (`AnkiConnect`, `Migaku`), `state: ServerState` (`Off`, `WaitingForCollection`, `Binding`, `Listening`, `PortInUse`), `port: u16`, `addresses: Vec<String>`, `collection_available: bool`, `next_retry_at: Option<String>`, `store_error: Option<StoreError>` (`Newer`, `Recreated`), `migaku: Option<MigakuStatus>` (the record of `servers/migaku-protocol` P10.5, owned by U137). The values are H12's.
- `AudioTranscoder`: the foreign trait of H11.3; its method is `servers/migaku-receive` P11's.
- On iPhone and iPad every operation returns `Unsupported` and changes nothing (H2); the callback interfaces exist and are never called.
- Other units add their own operations to the same module: the profile and routing operations (`servers/migaku-routing` D2.3, U138), the received-log calls (`servers/migaku-received-log` L16, U140), the activity log and the agent snippet (`servers/ankiconnect-envelope` E16, E17, U141), and the external media folder (`servers/ankiconnect-media` X11.8, U143). Each returns `NotStarted` before `servers_start` and `Unsupported` on iPhone and iPad.

## H21. Clocks

The host has one injectable clock with two faces: **wall time** (UTC) and a **timer** source (deadlines and sleeps). In the app both are the system's. In tests (H22) a test clock stands in: its wall time starts at the scenario's `clock.start` (`conformance/dsl` D5) and moves only when the test advances it; a timer is due when the clock reaches its deadline, and due timers fire in deadline order during the advance. In Meguri's transcript executor the test clock is the scenario clock the engine also uses.

| Timer or time | Clock |
|---|---|
| retry period (H4.3); listener restart waits and their 60 s window (H9.4) | host timer |
| interaction timeouts, awaited and detached (H11.2) | host timer |
| collection waits (H9.2: 5 s Migaku, 30 s AnkiConnect) and the other request waits that the protocol specs name | host timer |
| request arrival times (`servers/migaku-routing` D18), `next_retry_at` (H12), activity-log and received-log times, the `.corrupt-<UTC>` file name (H10.1) | host wall time |
| probe timeout and self-check timeout, 500 ms (H4.2, H9.3) | real monotonic time |
| connection timeouts: idle 5 s, head 10 s, body and send stalls 30 s (H7.3) | real monotonic time |
| the 1 s bound of H10.4 | real monotonic time |
| fetch time limits (H13) | real monotonic time |

## H22. Construction for tests

Rust tests and U136's testkit build the host directly, never through H20, with these parameters:

| Parameter | Values | In the app |
|---|---|---|
| `collection` | a `CoreHost` session (`engine/host` H1) | the FFI's (H9.1) |
| `store` | the path of the integrations store | Application Support (H10.1) |
| `bind` | `Fixed` (H4 as written) or `Ephemeral` | `Fixed` |
| `clock` | the system clock or a test clock (H21) | system |
| `interactions` | the Swift channel or a `TestHost` | Swift |
| `transcoder` | U144's or the pass-through (H11.3) | U144's |
| `migaku_pending_switch` | bool (`servers/migaku-receive` P14) | `false` |

- **Ephemeral bind mode.** Each enabled server binds `127.0.0.1:0` and nothing else: no probe, no `[::1]`, and `ankiconnect.port` is ignored for binding (an apply still validates it, H10.3). The bound port is the port the operating system gave; the `Host` check compares with it (H6.1) and the status reports it (H12). Used for protocol scenarios, which do not test binding (`servers/transcripts` T20).
- **`TestHost`.** Answers each interaction from a queue of scripted answers per kind; when the kind's queue is empty, with the kind's test default (`servers/transcripts` T7.5) or, without one, not at all, so the interaction times out. It records every event the host posts, in order, for the transcript `events` step.
- **Test hooks.** A listener task failure for one server; a panic in the handler of the next request to one server's route; closing one server's IPv4 socket from underneath; the list of the process's listening sockets.
- **Socketless serving.** The host can serve one request on one server without a socket: H15 steps 3 to 6 on a given head and body, with the host's collection and clock. `servers/migaku-routing` D14.3's replay uses it.
