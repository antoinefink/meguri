# Migaku server protocol

Spec: servers/migaku-protocol
Requirements: R1680, R1681, R1682, R1683, R1684, R1685, R1686, R1687, R1688, R1689, R1690; R1672 (extension connection state), R1678 (Migaku over-limit reply), R1679 (Migaku side), R1669 (Migaku trivial requests)
Anki: 26.09.3
Scenarios: conformance/recordings/servers/migaku/endpoints/ (declared, `servers/transcripts` T13; 33 of P16's 39 rows). `srs-check-corrupt`, `srs-check-cloud-adopt`, `ws-oversized`, `ws-invalid-utf8`, `ws-unmasked` and `ws-last-received` cannot be written with `servers/transcripts` T7.3's steps yet and stay U137 Rust tests (`core/crates/meguri-servers/tests/migaku_endpoints.rs`). The transcript runner (U136) does not exist yet; until it lands, U137 verifies every P16 row through those Rust tests, one test per row.

The HTTP and WebSocket surface that Meguri's Mac app serves on port 44432 so that the Migaku browser extension can send cards to Meguri instead of the Migaku Anki add-on. It targets the extension versions 1.30 (build 1.30.16) and 1.31 (build 1.31.0.17, "Early Access"). The Migaku Anki add-on 0.6.6 is the compatibility reference for routes, reply shapes and headers; where Meguri differs on purpose, P15 says so. Anki itself has no part in this protocol: nothing here is observable through the `anki` oracle, and no deviation entry is needed.

This file owns the route table, the origin policy, the CORS headers, preflight, the reply and error shapes, `/srs-check`, the `/anki-connect` WebSocket, `/srs-import-info`, `/srs-sample-card`, `/srs-import` and `/anki-hello`. Other files own the rest:

- Binding, the port-in-use probe and retry, the `Host` check, keep-alive, the body-size limit and the server status states: `servers/host-security` (U135). P18 lists what this file relies on from it, with the section that gives each guarantee.
- `POST /receive`: `servers/migaku-receive` and `servers/migaku-syntax` (U139). Destination choice: `servers/migaku-routing` (U138).
- `POST /create`: `servers/migaku-received-log` (U140).

## P1. Terms

- **Migaku server**: the listener set of `servers/host-security` for the Migaku integration. Its port is always 44432; no setting changes it (R1675).
- **Origin value**: the value of the request's `Origin` header, exactly as sent (no trimming). A request has **no Origin** when it has no `Origin` header at all.
- **Path**: the path part of the request target, before any `?`, compared byte for byte: no percent-decoding, no case folding, no removal of a trailing `/`. An absolute-form target (`http://127.0.0.1:44432/srs-check`) has the path of its URL. The query is ignored everywhere in this file.
- **Available**: the collection is available as `engine/lifecycle` L6 defines it for the local servers: the lifecycle state is `Ready` and the long-operation slot is free, or held only by a `CloudAdopt` that has not entered its exclusive phase. Every other case is unavailable: any other lifecycle state (`NoCollection`, `Opening`, `Upgrading`, `ExclusiveBusy`, `Failed{reason}`, `Failed{Corrupt}`) and a slot held by anything else, Check Database included. This is the same test that `servers/migaku-receive` P14 uses for a busy collection.
- **JSON reply**: a reply whose body is one JSON value encoded in UTF-8, with `Content-Type: application/json; charset=UTF-8`. Meguri writes compact JSON (no white space between tokens), members in the order this file lists them. Clients and transcripts compare JSON values, not bytes.
- **Error shape**: the JSON reply body `{"success":false,"error":<message>}`. The message strings of this file are interface strings (P8.2).

## P2. Requests in order

Every HTTP request on the Migaku server goes through these steps, in this order. The first step that answers ends the request.

1. **Host** (`servers/host-security` H6): a rejected `Host` gets 403 with an empty body and no CORS header.
2. **Origin** (P3): a disallowed origin gets 403 with an empty body and no CORS header, whatever the method and path, `OPTIONS` and WebSocket upgrades included.
3. **Preflight**: any `OPTIONS` request, on any path, known or not (an `OPTIONS *` request too), gets P5's reply.
4. **Route** (P4): a path not in the route table gets 404.
5. **Method** (P4): a method the route does not list gets 405.
6. **WebSocket**: `GET /anki-connect` continues in P10. It never reads a body.
7. **Body**: the body is read under the Migaku size limit of `servers/host-security` H8 (128 MiB, 134,217,728 bytes, R1678). A body over the limit gets P8.3's reply and the handler never runs.
8. **Handler**: the route's handler answers.

Steps 1 to 6 use only the request head. A request **has unread body bytes** when its head announces a body (a `Content-Length` other than `0`, or `Transfer-Encoding: chunked`) and the server answers before reading that body to its end. A reply from steps 2 to 6, or the over-limit reply, to such a request carries `Connection: close`, and the connection closes after it. Every other reply, including a step 2 to 6 reply to a request with no body, follows the keep-alive rules of `servers/host-security`.

Every reply from steps 3 to 8 carries the CORS headers of P6. The two 403 replies carry none.

## P3. Origin policy (R1683, R1679)

### P3.1 Allowed origins

A request is allowed when one of these holds:

| Rule | Allowed when | Examples allowed | Examples refused |
|---|---|---|---|
| no Origin | the request has no `Origin` header | (curl, scripts) | — |
| browser extension | the origin value is `chrome-extension://`, `moz-extension://` or `safari-web-extension://` followed by a non-empty extension identifier; the scheme compares ASCII case-insensitively; the identifier holds no `/`, `?`, `#`, `@`, `:` or white space | `chrome-extension://abcdefghijklmnopabcdefghijklmnop`, `moz-extension://4b1c9a0e-7d2f-4f0a-9d5e-1a2b3c4d5e6f`, `safari-web-extension://A1B2C3D4-0000-1111-2222-333344445555` | `chrome-extension://`, `chrome-extension://abc/`, `chrome-extension://abc:80` |
| Migaku web | the origin is `https://` followed by a host that is `migaku.com` or one or more labels followed by `.migaku.com`, compared ASCII case-insensitively, with no port, path, user or trailing `/`. Every label is non-empty and holds only ASCII letters, digits and `-` | `https://migaku.com`, `https://app.migaku.com`, `https://study.migaku.com`, `https://a.b.migaku.com` | `http://app.migaku.com`, `https://app.migaku.com:8443`, `https://migaku.com.evil.example`, `https://evilmigaku.com`, `https://.migaku.com`, `https://a..migaku.com`, `https://a_b.migaku.com` |
| user-added | the origin value equals an entry of the Migaku extra-origins list, `migaku.extra_origins` of the integrations config (`servers/host-security` H10.2), ASCII case-insensitively | (whatever the user added) | — |

Everything else is refused, in particular:

- `Origin: null` (the literal four letters, any case);
- an `Origin` header with an empty value;
- a request with more than one `Origin` header;
- an origin value with a byte outside printable ASCII;
- any other web origin, including `http://localhost` and `http://127.0.0.1:<any port>`;
- the origin of Meguri's own card webview (R1679). That origin is not an extension origin and not a Migaku web origin, so the built-in rules never allow it. Card JavaScript therefore cannot call this server unless the user adds that origin to the extra-origins list.

R1683 names `https://*.migaku.com`. Meguri reads that pattern as browser extension host patterns are read, where `*.` also covers the bare domain, so `https://migaku.com` is allowed on purpose.

### P3.2 The extra-origins list

- It lives in the integrations store, never in the collection (R1665). It is empty by default.
- An entry is stored as the user typed it, after removing white space at both ends. Settings refuses `null`, an empty entry, and an entry with a path, query or fragment (U144 shows the reason). The comparison of P3.1 is exact apart from ASCII case: `https://example.org` matches `https://EXAMPLE.org` and does not match `https://example.org:443` or `https://example.org/`.
- A change applies from the next request (the config snapshot of `servers/host-security` H10.3). It never closes an open WebSocket.

## P4. Route table (R1680, R1689)

| Path | Methods | Handler |
|---|---|---|
| `/srs-check` | `GET` | P9 |
| `/receive` | `POST` | `servers/migaku-receive` (U139) |
| `/create` | `POST` | `servers/migaku-received-log` (U140) |
| `/anki-connect` | `GET` | P10 (WebSocket) |
| `/srs-import-info` | `GET`, `POST` | P11 |
| `/srs-sample-card` | `GET`, `POST` | P11 |
| `/srs-import` | `GET`, `POST` | P11 |
| `/anki-hello` | `GET`, `POST` | P12 |

- Method names compare exactly (HTTP methods are case-sensitive). `HEAD` is not listed anywhere, so it gets 405. A reply to `HEAD` never has a body (HTTP forbids it): it has the status and headers that the same reply to `GET` would have, `Content-Length` excepted, which may be absent or give the length of the omitted body. Scenarios do not compare `Content-Length` on `HEAD` replies.
- Every other path gets 404 with the error shape and the message `Not found.` This includes `/`, `/receive/`, `/RECEIVE`, `/srs-check.json` and the add-on's older routes that the current extension does not use (`/info`, `/profile-data`, `/learning-statuses`, `/search`, `/sendcard`, `/condense`).
- A listed path with an unlisted method gets 405 with the error shape, the message `Method not allowed.`, and an `Allow` header naming the route's methods followed by `OPTIONS`, separated by `, `: `Allow: GET, OPTIONS` for `/srs-check` and `/anki-connect`; `Allow: POST, OPTIONS` for `/receive` and `/create`; `Allow: GET, POST, OPTIONS` for the other four.

## P5. Preflight (R1682)

- Any allowed `OPTIONS` request gets status 204, no body and no `Content-Type`, with P6's headers.
- When the request has a header `Access-Control-Request-Private-Network` whose value is `true` (ASCII case-insensitively, after trimming white space), the reply also carries `Access-Control-Allow-Private-Network: true`. No other reply carries that header.
- The values of `Access-Control-Request-Method` and `Access-Control-Request-Headers` do not change the reply; the browser compares them with P6's fixed lists.
- A preflight never reads the collection and never waits for it.

## P6. CORS headers (R1681)

Every reply that P2 sends from step 3 on (204, 200, 400, 404, 405, 500, 501, 503 and the WebSocket 101) carries:

| Header | Value |
|---|---|
| `Access-Control-Allow-Origin` | the request's origin value, unchanged, when it has one; `*` when it has no Origin |
| `Access-Control-Allow-Headers` | `access-control-allow-origin,content-type` |
| `Access-Control-Allow-Methods` | `POST, GET, OPTIONS` |
| `Vary` | `Origin`, only when `Access-Control-Allow-Origin` echoes an origin |

No reply carries `Access-Control-Allow-Credentials`. The two 403 replies of P2 carry none of these headers.

## P7. Concurrency and timing (R1669)

- `OPTIONS`, `/srs-check`, `/anki-hello`, the three `/srs-import*` routes, the error replies of P4 and P8, and WebSocket upgrades never read the collection, never wait for it and never queue behind a write. They answer within 200 ms while the app is hidden, in the background or under App Nap, while a `/receive` commit is running and during an exclusive phase.
- Any number of requests are served at the same time. `/receive` ordering is `servers/migaku-receive`'s.

## P8. Replies and errors

### P8.1 Shapes

- Success replies are JSON replies with the bodies this file and the handler files name.
- Failures are JSON replies with the error shape, with one status per failure (P8.2).
- A reply that a route's handler file defines as plain text (`/create`'s acknowledgements, `servers/migaku-received-log`) has `Content-Type: text/plain; charset=UTF-8`.

### P8.2 Statuses and messages of this file

| Case | Status | Body |
|---|---|---|
| rejected `Host` (`servers/host-security`) | 403 | empty |
| disallowed origin (P3) | 403 | empty |
| unknown path (P4) | 404 | `{"success":false,"error":"Not found."}` |
| unlisted method (P4) | 405 | `{"success":false,"error":"Method not allowed."}` |
| body over the Migaku limit (P8.3) | 400 | `{"success":false,"error":"Request body is too large."}` |
| `GET /anki-connect` without a valid upgrade (P10.1) | 400 | `{"success":false,"error":"Expected a WebSocket upgrade."}` |
| too many open WebSockets (P10.1) | 503 | `{"success":false,"error":"Too many open connections."}` |
| `/srs-import-info`, `/srs-sample-card`, `/srs-import` (P11) | 501 | `{"success":false,"error":"Importing into Migaku Memory is not supported by Meguri."}` |
| a handler fails unexpectedly (a caught panic, `servers/host-security`) | 500 | `{"success":false,"error":"Meguri hit an internal error."}` |

The messages of `/receive` (`No fields to map to.`, `No space left on device` and the others) are `servers/migaku-receive`'s and `servers/migaku-routing`'s.

The 500 row applies on every route, `/receive` and `/create` included. For `/receive` this is a deliberate exception to R1702's HTTP 400: a caught panic is a Meguri defect, not a problem with the card, and the reply keeps the error shape. How the extension shows a status other than 200 and 400 is not known (P17 item 4). This file guarantees only the reply. Whether a panic caught after the body was parsed also writes a Failed row with the payload kept is `servers/migaku-receive`'s (P20; P17 item 7).

### P8.3 Over-limit bodies (R1678)

- The limit and its two checks (by `Content-Length` before reading, by counted bytes while reading a body without one) are `servers/host-security`'s. A body exactly at the limit is read and handled.
- The over-limit reply is HTTP 400 with the message `Request body is too large.`, the add-on's ordinary failure status, so the extension shows it as a failed card. It carries `Connection: close`. The handler never runs, and nothing is written to the collection, the media folder or the integrations store.
- Over-limit `/receive` bodies are not added to Recently received (the payload was never read).

## P9. `GET /srs-check` (R1684)

- Reply: 200, JSON reply `{"ok":<bool>,"version":"4"}`. `version` is always the string `"4"`; some extension code paths require it to be at least 4.
- `ok` is `true` exactly when the collection is Available (P1), and `false` in every other case: an exclusive phase (full sync, restore, collection replacement), a long operation holding the slot (Check Database), `Failed{Corrupt}`, an open or upgrade in progress, and a closed collection. The server only listens while the integration is on and after the collection first opened (`servers/host-security`), so the closed and opening cases occur only after that first open.
- R1684 says `ok` is true when "the collection is open". Meguri reads "open" as Available, the test `/receive` uses: while `ok` is `true`, a `/receive` does not wait for the collection (`servers/migaku-receive` P14). Check Database keeps the file open but makes `/receive` wait, so `ok` is `false` during it.
- `ok` does not depend on any WebSocket: it is `true` with no `/anki-connect` socket open. It does not depend on routing either: it stays `true` when no destination profile is set up, and the problem shows when a card is sent (`servers/migaku-routing` D7).
- A request body, if any, is read under the limit and ignored. The handler reads only the lifecycle state and the slot holder, both held in memory: no collection query, no wait (P7).

| State | Reply body |
|---|---|
| `Ready`, slot free, no WebSocket open | `{"ok":true,"version":"4"}` |
| `Ready`, slot free, two WebSockets open | `{"ok":true,"version":"4"}` |
| `Ready`, slot held by a `CloudAdopt` before its exclusive phase | `{"ok":true,"version":"4"}` |
| `Ready`, Check Database holding the long-operation slot | `{"ok":false,"version":"4"}` |
| `ExclusiveBusy` (one-way download, restore, collection replacement) | `{"ok":false,"version":"4"}` |
| `Failed{Corrupt}`, Check Database holding the slot or not | `{"ok":false,"version":"4"}` |
| `Opening`, `Upgrading`, `NoCollection`, `Failed{reason}` while listening | `{"ok":false,"version":"4"}` |

## P10. `/anki-connect` WebSocket (R1685, R1686, R1687)

### P10.1 Upgrade

- A `GET /anki-connect` that passed P2's steps 1 to 5 is a WebSocket opening handshake under RFC 6455: it needs `Upgrade: websocket` (ASCII case-insensitive), a `Connection` header listing `upgrade` (ASCII case-insensitive token), `Sec-WebSocket-Version: 13` and a `Sec-WebSocket-Key`. A request missing any of them gets the P8.2 400 `Expected a WebSocket upgrade.`; a wrong version also gets `Sec-WebSocket-Version: 13` in the reply, as RFC 6455 asks.
- A valid handshake gets 101 with `Sec-WebSocket-Accept` computed as RFC 6455 says, plus P6's headers. Meguri selects no subprotocol (a `Sec-WebSocket-Protocol` offer is ignored and not echoed) and negotiates no extension (no per-message compression).
- The upgrade does not need the collection: it succeeds during an exclusive phase.
- At most 64 sockets are open at once. An upgrade that would open the 65th gets the P8.2 503 instead of 101.

### P10.2 Frames from the client

A **well-formed message** is a complete data message (text or binary, possibly fragmented) whose frames follow RFC 6455 and, for text, whose bytes are valid UTF-8.

- Well-formed text messages are parsed as JSON. A message that is an object with the members `id`, `msg` and `data` is logged at debug level with its `id` and `msg` only; `data` is never logged, because it can carry card content. Every other well-formed text message (invalid JSON, a non-object, missing members, members of any type) is logged at debug level as malformed, without its content. No message gets a reply, and no well-formed message within the size limit closes the socket.
- Well-formed binary messages are ignored the same way.
- Ping frames get the pong frame RFC 6455 requires, with the same payload. Pong frames are ignored.
- Fragmented messages are reassembled under the same limit.
- Only these close the socket from Meguri's side, each with a close frame and then the end of the connection:

  | Fault | Close status |
  |---|---|
  | a message or frame larger than the Migaku body limit | 1009 |
  | a text message whose bytes are not valid UTF-8 | 1007 |
  | a framing fault: an unmasked client frame; a reserved bit set (no extension is in use); an unknown opcode; a control frame over 125 bytes or fragmented; a continuation frame with no message started; a new data frame while a fragmented message is open | 1002 |
  | a close frame with an invalid payload (P10.4) | 1002 |

  None of them affects another socket.

### P10.3 What Meguri sends

Nothing but pongs and the close frames of P10.4. Meguri sends no data frames and no pings in this version (R1686). The add-on's server-to-extension commands (`Migaku-Search`, `Migaku-Play-Audio`, `Migaku-Syntax`, `Migaku-Definitions`) are not sent.

### P10.4 Closing

- A close frame from the client gets a close frame back, then the connection ends. The reply depends on the client's close payload:

  | Client close payload | Meguri's close frame |
  |---|---|
  | empty (no status code) | empty (no status code) |
  | a status code from 1000 to 1003, 1007 to 1014, or 3000 to 4999, with or without a valid UTF-8 reason | the same status code, no reason |
  | one byte, a code outside those ranges (1004, 1005, 1006, 1015, below 1000, 1016 to 2999, 5000 and above), or a reason that is not valid UTF-8 | 1002 |
- When the Migaku integration is turned off or the app quits, every open socket gets a close frame with status 1001 (going away), and the listener stops (`servers/host-security` H10.4).
- A collection replacement, an exclusive phase, sleep and wake never close a socket by themselves. If the operating system drops the connection, the socket ends without a close frame.
- The HTTP keep-alive idle timeout (`servers/host-security`) does not apply to an upgraded connection. An open socket with no traffic stays open.

### P10.5 Connection tracking (R1672, R1687)

- A socket counts as open from the 101 reply until its connection ends (close handshake done, or the transport closed or failed).
- The Migaku status block of the server status (`servers/host-security` H12, `MigakuStatus` of H20) holds:

  | Member | Value |
  |---|---|
  | `extension_connected` | `true` while at least one socket is open |
  | `open_sockets` | the number of open sockets |
  | `last_received_at` | the time, in milliseconds since the epoch, of the most recent note committed through `/receive`'s pipeline; null before the first one |

- `last_received_at` is set each time `servers/migaku-receive` writes a Created row (its P20): a direct commit, a drained pending entry, or a Retry from Recently received that commits through that pipeline. The value is the commit's time on the host's wall clock (`servers/host-security` H21). A retry answered with an existing note (no row), a failure, a Pending row, `/create` and a move never change it. It is kept in the integrations store, so it survives a restart.
- A status event goes to Swift whenever `open_sockets` or `last_received_at` changes. The connection state feeds only the status display; no other behavior depends on it (`/srs-check` P9 included).

## P11. `/srs-import-info`, `/srs-sample-card`, `/srs-import` (R1688)

These routes move Anki decks into Migaku Memory in the add-on. Meguri does not support that.

- `GET` or `POST` on any of the three gets 501 with `{"success":false,"error":"Importing into Migaku Memory is not supported by Meguri."}`.
- The body, if any, is read under the limit and ignored. No collection read, no collection write, no wait, no Recently received row.

## P12. `/anki-hello` (R1690)

`GET` or `POST /anki-hello` gets 200 with `{"status":"connect","port":44432}`; `port` is the JSON number 44432, the constant of R1675, also when a test binds another port (`servers/host-security` H22). It does not read the collection. Older clients use it; the current extension does not.

## P13. `/receive` and `/create`

This file guarantees, before handing a request to `servers/migaku-receive` or `servers/migaku-received-log`: the `Host` and origin were accepted, the method is `POST`, the body is complete and within the limit. Those files own everything after that, including statuses, bodies and waiting for a busy collection. `/receive` ignores `Content-Type` (R1691). Their replies carry P6's headers like every other reply.

## P14. Robustness

- No request, body or frame makes the server panic, stop listening or close another connection (`core/fuzz` target `migaku_ws_message`, U137). A caught failure inside a handler gets P8.2's 500. For WebSocket input the fuzz target checks that every frame sequence either leaves the socket open or closes it with a status of P10.2 or P10.4, and that other sockets stay open.
- Header names compare ASCII case-insensitively, as HTTP requires. Unknown request headers are ignored.

## P15. Differences from the Migaku add-on 0.6.6

| Behavior | Add-on | Meguri | Requirement |
|---|---|---|---|
| Bind address | all interfaces | loopback only (`servers/host-security`) | R1673 |
| `Host` check | none | refused names get 403 (`servers/host-security`) | R1676 |
| Origins | every origin accepted | P3 | R1683 |
| `Access-Control-Allow-Origin` | always `*` | echoes the origin, `*` with no Origin | R1681 |
| Private-network preflight | no special header | `Access-Control-Allow-Private-Network: true` when asked | R1682 |
| `/srs-check` `ok` | needs the collection loaded and a WebSocket connected | needs only the collection Available (P1) | R1684 |
| WebSocket | the add-on pushes commands to the extension | Meguri sends nothing | R1686 |
| `/srs-import*` | migrates decks into Migaku Memory | 501 | R1688 |
| Unknown paths, wrong methods | the web framework's own replies | 404 and 405 with the error shape | R1689 |
| Older routes (`/info`, `/search`, …) | served | 404 | R1680 |

No `conformance/deviations.yaml` entry is needed: there is no Migaku oracle (U136), and the scenarios of P16 state Meguri's replies directly.

## P16. Planned scenarios

Each row is one scenario of `conformance/recordings/servers/migaku/endpoints/`, id `servers/migaku/endpoints/<row>`. Every row is declared (`servers/transcripts` T13) at `conformance/recordings/servers/migaku/endpoints/<row>.yaml`, except the six rows marked "not expressible (U137 test only)": `servers/transcripts` T7.3 has no step yet that sets up their state or sends their frames, so only U137's Rust tests check them. Unless a row says otherwise: the Migaku integration is on, the collection is Available, every request carries the default `Host` of `servers/transcripts` T6.2 (`127.0.0.1:<bound port>`: 44432 in the app, the ephemeral port in these scenarios, T20), and "CORS(x)" means P6's headers with `Access-Control-Allow-Origin: x` (and `Vary: Origin` when x is not `*`). `EXT` stands for `chrome-extension://abcdefghijklmnopabcdefghijklmnop` (an invented identifier of the Chrome form: 32 letters `a` to `p`).

| Scenario | Request | Expected reply |
|---|---|---|
| `preflight-extension` | `OPTIONS /receive`, `Origin: EXT`, `Access-Control-Request-Method: POST`, `Access-Control-Request-Headers: content-type` | 204, CORS(EXT), no body, no `Access-Control-Allow-Private-Network` |
| `preflight-private-network` | as above plus `Access-Control-Request-Private-Network: true` | 204, CORS(EXT), `Access-Control-Allow-Private-Network: true` |
| `preflight-unknown-path` | `OPTIONS /nope`, no Origin | 204, CORS(`*`) |
| `srs-check-no-origin` | `GET /srs-check`, no Origin | 200, CORS(`*`), `{"ok":true,"version":"4"}` |
| `srs-check-extension` | `GET /srs-check`, `Origin: EXT` | 200, CORS(EXT), `{"ok":true,"version":"4"}` |
| `srs-check-query` | `GET /srs-check?t=1715` | 200, `{"ok":true,"version":"4"}` |
| `srs-check-busy` | lifecycle in an exclusive phase; `GET /srs-check` | 200, `{"ok":false,"version":"4"}` within 200 ms |
| `srs-check-long-op` | Check Database holds the long-operation slot; `GET /srs-check` | 200, `{"ok":false,"version":"4"}` within 200 ms |
| `srs-check-corrupt` (not expressible, U137 test only) | lifecycle `Failed{Corrupt}`; `GET /srs-check` | 200, `{"ok":false,"version":"4"}` |
| `srs-check-cloud-adopt` (not expressible, U137 test only) | a `CloudAdopt` holds the slot before its exclusive phase; `GET /srs-check` | 200, `{"ok":true,"version":"4"}` |
| `srs-check-with-sockets` | two `/anki-connect` sockets open; `GET /srs-check` | 200, `{"ok":true,"version":"4"}` |
| `origins-allowed` | `GET /srs-check` with each `Origin`: `https://app.migaku.com`, `https://migaku.com`, `https://Study.Migaku.com`, `moz-extension://4b1c9a0e-7d2f-4f0a-9d5e-1a2b3c4d5e6f`, `safari-web-extension://A1B2C3D4-0000-1111-2222-333344445555` | 200 each, CORS echoing that origin |
| `origins-refused` | `GET /srs-check` with each `Origin`: `https://evil.example`, `null`, `NULL`, empty value, `http://migaku.com.evil.example`, `https://migaku.com.evil.example`, `http://app.migaku.com`, `https://app.migaku.com:8443`, `https://evilmigaku.com`, `chrome-extension://`, `chrome-extension://abc/`, `http://localhost`, `http://127.0.0.1:44432`; and one request with two `Origin` headers (`EXT` twice) | 403 each, empty body, no CORS header |
| `origins-refused-preflight` | `OPTIONS /receive`, `Origin: https://evil.example` | 403, empty body, no CORS header |
| `origins-refused-websocket` | `GET /anki-connect` upgrade, `Origin: https://evil.example`; then `Origin: null` | 403 each, no 101, `open_sockets` stays 0 |
| `origins-user-added` | extra origins `["https://example.org"]`; `GET /srs-check` with `Origin: https://example.org`, then `https://EXAMPLE.org`, then `https://example.org:443`, then `https://example.org/` | 200, 200 (each echoing its own value), 403, 403 |
| `order-origin-before-route` | `GET /nope`, `Origin: https://evil.example` | 403, empty body |
| `order-host-before-origin` | `GET /srs-check`, `Host: evil.example:44432`, `Origin: EXT` | 403, empty body, no CORS header |
| `not-found` | `GET /nope`; `GET /receive/`; `GET /RECEIVE`; `GET /`; `GET /learning-statuses` | 404 each, CORS(`*`), `{"success":false,"error":"Not found."}` |
| `method-not-allowed` | `GET /receive`; `POST /srs-check`; `PUT /srs-import`; `DELETE /anki-hello` | 405 each, CORS(`*`), `{"success":false,"error":"Method not allowed."}`, `Allow`: `POST, OPTIONS`; `GET, OPTIONS`; `GET, POST, OPTIONS`; `GET, POST, OPTIONS` |
| `method-head` | `HEAD /srs-check` | 405, CORS(`*`), `Allow: GET, OPTIONS`, `Content-Type: application/json; charset=UTF-8`, no body (`Content-Length` not compared) |
| `keepalive-after-404` | on one connection: `GET /nope`, then `GET /srs-check` | 404, then 200 on the same connection; neither reply carries `Connection: close` |
| `close-after-unread-body` | `POST /nope` with `Content-Length: 10` and a 10-byte body | 404 with `Connection: close`; the connection then closes |
| `srs-import-routes` | `GET /srs-import-info`; `POST /srs-sample-card` with body `{}`; `POST /srs-import` with body `{"decks":[1]}` | 501 each, CORS(`*`), `{"success":false,"error":"Importing into Migaku Memory is not supported by Meguri."}`; nothing changes in the collection or the media folder (`no_effect`, `servers/transcripts` T13). That nothing is read stays a U137 test |
| `anki-hello` | `GET /anki-hello`; `POST /anki-hello` | 200 each, `{"status":"connect","port":44432}` |
| `body-over-limit` | `POST /receive` with `Content-Length` one byte over the Migaku limit; then the same body sent chunked without `Content-Length` | 400 each, CORS(`*`), `{"success":false,"error":"Request body is too large."}`, `Connection: close`; the `/receive` handler never runs; no Recently received row |
| `body-at-limit` | `POST /srs-import` with a body exactly at the limit | 501 (the body was accepted and ignored) |
| `ws-no-upgrade` | `GET /anki-connect` without `Upgrade` | 400, CORS(`*`), `{"success":false,"error":"Expected a WebSocket upgrade."}` |
| `ws-open-two` | two upgrades with `Origin: EXT`, then both closed by the client | 101 each; status `extension_connected` false → true (`open_sockets` 1, then 2) → after both close, 0 and false; three status events in total after the first |
| `ws-ignored-frames` | one socket; send text `not json`, text `{"id":1}`, text `[1,2]`, binary `00 ff`, text `{"id":"a1","msg":"Migaku-Search","data":{"q":"x"}}`, then ping with payload `p` | socket stays open; the only frame received is a pong with payload `p` |
| `ws-busy-collection` | exclusive phase; upgrade with `Origin: EXT` | 101 |
| `ws-limit` | 64 sockets open; a 65th upgrade | 503, `{"success":false,"error":"Too many open connections."}`; the 64 stay open |
| `ws-oversized` (not expressible, U137 test only) | one socket; a text message one byte over the Migaku limit | close frame 1009; `open_sockets` back to 0 |
| `ws-server-off` | one socket open; the Migaku integration is turned off | close frame 1001; status Off with `extension_connected` false |
| `ws-wrong-version` | upgrade with `Sec-WebSocket-Version: 8` | 400 `Expected a WebSocket upgrade.` with `Sec-WebSocket-Version: 13` |
| `ws-invalid-utf8` (not expressible, U137 test only) | one socket; a text frame with payload `C3 28` | close frame 1007; a second socket opened before stays open |
| `ws-unmasked` (not expressible, U137 test only) | one socket; an unmasked text frame `hi` | close frame 1002 |
| `ws-close-codes` | three sockets; the client closes them with an empty payload, with 1000 `bye`, and with 1005 | close frames back: empty, 1000 with no reason, 1002 |
| `ws-last-received` (not expressible, U137 test only) | one `/receive` commits at time t | status `last_received_at` = t; one status event; a later failed `/receive` leaves it at t. A committing `/receive` needs a confirmed Migaku profile (`servers/migaku-routing` D7.1), and profiles are saved only through `servers/migaku-routing` D2.3's FFI operations, which no `servers/transcripts` step reaches. Proposed: a T7.3 step that saves Migaku profiles through D2.3 (or a T4 or T5 member that seeds them), which `servers/migaku-routing`'s planned rows need too. Until it exists, this row is a U137 Rust test only |

Non-transcript checks (U137 tests): the fuzz target of P14 runs 10 minutes clean; P7's 200 ms answer holds while 10 `/receive` commits are queued.

## P17. Open questions

These could not be settled from the permitted sources. Until they are, Meguri follows the rule stated in the section named.

1. Which `Origin` the extension's requests carry. If 1.30 or 1.31 sends `/receive` or `/srs-check` from a content script, the origin is the page's (for example a video site), and P3 refuses it unless the user adds that origin. A capture of the extension's real request headers would settle it (P3). If the default policy refused the real extension, cards could not be sent at all, so this capture is needed before release, with U147's manual checks.
2. Which HTTP methods the extension uses for `/srs-import-info`, `/srs-sample-card`, `/srs-import` and the old `/anki-hello` route. Meguri accepts both `GET` and `POST` (P4).
3. Whether the extension waits for a message on `/anki-connect` before it counts itself connected. The extension is known to open the socket and retry every 10 s; nothing shows that it waits for a message. Meguri sends nothing (P10.3).
4. Whether any extension code reads the body or status of a 500, 501, 404 or 405 beyond the `error` member (P8.2).
5. Whether the extension compares any reply as bytes rather than as JSON. Meguri writes compact JSON; the add-on's framework writes `": "` and `", "` separators (P1).
6. Whether the current extension calls `/learning-statuses`. The add-on serves it with each word's known or learning status, which an extension can use to colour words in the browser; whether 1.30 or 1.31 still asks for it is not established. Meguri answers 404, as R1680 does not list the route (P4). If the extension does call it, word colouring in the browser stops working with Meguri, which a user would notice; a capture of the extension's requests would settle it.
7. Whether a panic caught inside `/receive` after the body was parsed writes a Failed row with the payload kept for Retry. That is `servers/migaku-receive`'s decision (its P20); this file fixes only the 500 reply (P8.2).
8. How the extension shows `ok: false` during a short Check Database. Meguri reports `false` whenever `/receive` would have to wait (P9); if the extension then stops sending cards for the whole session, R1684's wording ("the collection is open") may need `true` during a long operation instead.

## P18. What this file relies on from `servers/host-security`

Each guarantee this file relies on, and the section of `servers/host-security` (U135) that gives it. A change there that drops one changes this file too.

| Relied on | Given by | Used by |
|---|---|---|
| The `Host` check runs first, before the per-server origin policy, and refuses with 403, an empty body and no CORS header. | H6, H15 steps 3 and 4 | P2 step 1, P16 `order-host-before-origin` |
| The origin check is part of the protocol's head hook that U137 supplies; the host runs it after the `Host` check and before routing, on every request, WebSocket upgrades included. | H15 step 4, H19 (head hook, `Upgrade`) | P2 steps 2 to 6, P3, P10.1 |
| A per-server body limit (Migaku: 134,217,728 bytes), checked by `Content-Length` before reading and by counted bytes while reading; the over-limit reply comes from the error responder that U137 supplies, without the body being parsed. | H8, H19 (`OverLimit`) | P2 step 7, P8.3 |
| Keep-alive and its idle timeout for HTTP connections, `Connection: close` after a reply that leaves body bytes unread, none of it applied to upgraded connections. | H7.3, H19 (`Reply`) | P2, P10.4 |
| A config snapshot, swapped atomically, that holds the Migaku extra-origins list; a change applies from the next request. | H10.2 `migaku.extra_origins`, H10.3 | P3.2 |
| The lifecycle state and the long-operation slot holder, readable without a wait. | H9.2 | P1 Available, P9 |
| A server status value per server with a Migaku block holding P10.5's members, and a status event to Swift when it changes. | H12, H11.1, H20 `MigakuStatus` | P10.5 |
| Per-request catching of handler panics, answered by the server's error responder. | H9.4, H19 (`Panic`) | P8.2, P14 |
| The integrations store, where `last_received_at` and the extra-origins list are kept. | H10.1, H10.2 | P3.2, P10.5 |
| Turning the server off within 1 s, open WebSockets closed with 1001. | H10.4 | P10.4 |
