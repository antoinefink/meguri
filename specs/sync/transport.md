# Sync transport (protocol v11)

Spec: sync/transport
Requirements: R1504, R1505, R1506, R1507, R1508, R1509, R1510, R1511, R1512, R2030; core parts of R1501, R1561 (status codes), R1852 (bounded bodies); AE70 (transport half)
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/transport/

How one request of the sync protocol travels between Meguri and a sync server: the URL, the `anki-sync` header, the client identity and the session key, compressed bodies, how a response is checked, the one redirect that may be followed, the stall timeout, cancellation, TLS and proxies, and the error categories the transport reports. Every sync path uses it: login, the status check, the normal sync, one-way upload and download, and media sync.

Other files: endpoints, server choices and login are `login-endpoints.md`; the JSON of each method's arguments and results is `wire-format.md` (U106); turning transport errors into the kinds the user sees is `errors.md` (U106). Words: a *job* is one sync operation the app starts (a login, a status check, a normal sync, a one-way sync, a media sync); a *request* is one HTTP exchange inside it; the *base* is the job's endpoint URL, which always ends with `/` (`login-endpoints.md` L1).

Facts marked "verified" were observed against the official sync server of Anki 26.09.3 (the U13 rig) on 2026-10-15; T14 lists them.

## T1. Requests

- Meguri speaks only protocol version 11 (R1505). Every request is an HTTP/1.1 `POST`, never HTTP/2. Anki's own client is HTTP/1.1-only too, because servers often sit behind reverse proxies that only pass HTTP/1.1.
- The URL is the base followed by `sync/<method>` for collection methods and by `msync/<method>` for media methods. Because the base ends with `/`, this is plain text concatenation: base `https://h/anki/` and method `meta` give `https://h/anki/sync/meta`.
- Method names are case-sensitive and spelled exactly:

  | Path | Methods | Used by |
  |---|---|---|
  | `sync/` | `hostKey` | login (`login-endpoints.md` L5) |
  | `sync/` | `meta` | status check (U106); the only method that may follow a redirect (T7) |
  | `sync/` | `start`, `applyGraves`, `applyChanges`, `chunk`, `applyChunk`, `sanityCheck2`, `finish`, `abort` | normal sync (U107) |
  | `sync/` | `upload`, `download` | one-way sync (U109) |
  | `msync/` | `begin`, `mediaChanges`, `uploadChanges`, `downloadFiles`, `mediaSanity` | media sync (U111) |

- Request headers: `anki-sync` (T2) and `Content-Type: application/octet-stream`, plus what HTTP/1.1 itself needs (`Host`, and `Content-Length` or `Transfer-Encoding: chunked`). No `Accept-Encoding`, no cookies, no `Authorization`: credentials travel only inside the login body and the header's `k`.
- A request body may be sent with a `Content-Length` or chunked; the official server accepts both (verified). Anki's client always sends chunked bodies. Streamed bodies (T5) are chunked.

## T2. The `anki-sync` header

- Every request, login included, carries the header `anki-sync` (R1506). Its value is a compact JSON object (no white space) with exactly these four members, in this order:

  | Member | Value |
  |---|---|
  | `v` | the number `11` |
  | `k` | the host key (`login-endpoints.md` L7); the empty string `""` for `hostKey` |
  | `c` | the client identity (T3) |
  | `s` | the job's session key (T4) |

  Example: `{"v":11,"k":"0123456789abcdef0123456789abcdef01234567","c":"meguri,1.0.0 (3f2a9c1d),macos","s":"4gfFC3"}`.
- The value is printable ASCII only. Strings are written as JSON requires (`"` and `\` escaped), and every character outside U+0020 to U+007E is written as a `\uXXXX` escape, with a surrogate pair for characters above U+FFFF. The official server answers 400 to a header that holds raw non-ASCII bytes and accepts the same characters escaped (verified). Meguri's own `c` and `s` are ASCII anyway; the rule matters for a host key a server might issue.
- How the official server treats the header (verified, for the stub and for U106): no header, or a value that is not JSON, gives 400; a `v` it does not support gives 400 (it supports 8 to 11; 12 was tried); extra members are ignored; a `k` it does not know gives 403; an empty `s` on `start`, `applyGraves`, `applyChanges`, `chunk`, `applyChunk`, `sanityCheck2` or `finish` gives 400 (tried on `start`).

## T3. Client identity (R1509, R2030)

- Meguri names itself with one identity string, used everywhere a sync request carries a client version:
  - the header member `c` of every request (T2);
  - the `cv` member of the `meta` body (`wire-format.md`, U106);
  - the `v` member of the media `begin` body (U111).
- Form: `meguri,<version> (<build>),<platform>`, for example `meguri,1.0.0 (3f2a9c1d),macos`.

  | Part | Value |
  |---|---|
  | `meguri` | fixed, lower case |
  | `<version>` | Meguri's version, the core's workspace version (for example `1.0.0` or `1.1.0-beta.2`): characters `0-9 A-Z a-z . -` only |
  | `<build>` | the first 8 lowercase hex digits of the commit the core was built from, or `dev` when the build has none |
  | `<platform>` | `macos`, `ios` (iPhone and iPad alike) or `linux` (tests), from the target the core was compiled for |

- The string never names an Anki client or Anki's version, and is never empty.
- For comparison, Anki's desktop client sends `c` as `<version>,<build>,<platform>` (no client name) and `cv` as `anki,<version> (<build>),<platform>`. Meguri keeps the shape of the second form, with its own name first, in all three places, so a server that splits on commas still finds three parts.
- Verified: the official server answers `meta` with `cont: true` for this identity and runs every method with it; it does not inspect `c` or `cv` (it logs `c`), and it requires only a non-empty `v` in the media `begin` body. How AnkiWeb treats a client that is not Anki cannot be tested (R1566, R1876).

## T4. Session key

- A job creates one session key when it starts and sends it as `s` in every request of the job: the `meta` retry after a redirect (T7) and the media requests the job makes included. Every job, a login included, has a new key.
- Form: the base-62 representation of a fresh unsigned 32-bit value drawn from the core's process-wide random source (`meguri_model::rng`) when the job starts. Digits are `0-9 A-Z a-z` in that order, most significant first, with no leading zeroes; zero is `0`. The result has one to six characters and decodes to at most `4294967295`. Seed the source once per process, never per job. Each job takes a fresh draw; a seeded run repeats its keys.
- Why it matters: the server binds the state of an incremental sync to the key that called `start`. A later request of that sync with a different key gets 409 (verified), and a new `start` with another key ends the old sync on the server. Anki's client uses a random 32-bit number written in base 62 (at most 6 characters). The official server also accepts other non-empty ASCII keys; that acceptance does not relax the bounded form Meguri sends.

## T5. Request bodies (R1507)

- A method with arguments sends the zstd compression of the compact UTF-8 JSON text of its arguments (`wire-format.md`). A method without arguments (`chunk`, `finish`, `abort`, `download`) sends the zstd compression of `{}`. Anki's client sends `{"_pad":null}` for these; the server accepts both and answers 400 to any other member (verified).
- `upload` sends the zstd compression of the raw bytes of the collection file, and `uploadChanges` the zstd compression of the media zip (U111). These bodies are streamed: compressed while they are sent, never held twice in memory.
- The compressed body is a standard zstd stream (RFC 8878). Any compression level works; Meguri uses level 3. The official server answers 400 to an empty body and to an uncompressed one (verified).
- The official server refuses a request body larger than 100 MiB as sent, or larger than 300 MiB once decompressed (its defaults). One-way upload limits are U109's.

## T6. Responses

### T6.1 Order of checks

1. The status is read first.
2. A 2xx status goes on to T6.2.
3. A 3xx status is a redirect: T7 decides. It never reaches T6.2.
4. Any other status (4xx, 5xx) is a `status` error (T11) carrying the code and a reason text: up to the first 16 KiB of the body, read as UTF-8 with invalid bytes replaced and surrounding white space trimmed. The body is not decompressed, and `anki-original-size` is neither required nor read. If reading the body fails, the error keeps the code with an empty reason. The stall timeout (T8) applies while it is read.

The official server's error bodies are the bare status code as text (`403`) or a short message (verified). Other servers and reverse proxies may send HTML. The reason is for logs and diagnostics only: classification uses the code alone (U106).

### T6.2 Successful responses (R1508)

- A 2xx response must carry exactly one `anki-original-size` header. Its value is the size in bytes of the decompressed body: ASCII decimal digits only (no sign, no space), at most 20 of them, and a value that fits in an unsigned 64-bit integer. A missing header is the `protocol` error `missing_original_size`; a repeated or malformed one is `bad_original_size`. This check happens before any body byte reaches the caller.
- The body is decompressed as it arrives. A body of zero bytes is the empty payload. Otherwise it is one or more complete zstd frames (skippable frames allowed); anything else, a truncated frame included, is `bad_compression`.
- The decompressed length must equal the declared size:
  - The first decompressed byte past the declared size makes the response `size_exceeded`. That byte and anything after it are never buffered or handed to the caller, and the connection is closed.
  - A body that ends before the declared size is `size_short`.
- Memory grows only with bytes that actually arrived: the transport never reserves memory from the declared size alone, so a large declared size cannot exhaust memory by itself (R1852).
- `Content-Type` and `Content-Encoding` of responses are ignored. Meguri asks for no HTTP content coding and undoes none.
- What successful bodies hold (decoded by the callers): JSON for most methods; the JSON text `null` for methods with no result (`applyGraves`, `applyChunk`, `abort`); for `upload`, the plain text `OK` or a refusal text (U109); for `download`, the collection file's bytes; for `downloadFiles`, a zip (U111). The official server always sends 2xx bodies chunked, with `anki-original-size` set (verified).

## T7. Redirects (R1510, R1511)

- No request ever follows a redirect automatically.
- Only `meta` may follow one, and only a `308` with a `Location` header. The new base is computed and checked:
  1. `Location` must be an absolute `http` or `https` URL with a host, no user name or password, no query and no fragment. A relative reference fails.
  2. It is normalised like an endpoint (`login-endpoints.md` L1): `https://sync20.example.net` becomes `https://sync20.example.net/`.
  3. When the current base is `https`, the new base must be `https`.
  4. Host rule, by the job's server kind (`login-endpoints.md` L3):
     - `ankiweb` (only with feature `ankiweb`): the new host is `ankiweb.net` or ends with `.ankiweb.net`, compared in ASCII lower case. An IP address never passes.
     - `self_hosted`: the new host equals the current one (names compared in ASCII lower case, addresses as addresses). The port may change.
- When every check passes, the job sends `meta` again, once, to `<new base>sync/meta`, with the same session key. Every later request of the job, collection and media, goes to the new base. The job's result carries the new base, exactly as normalised, as `newEndpoint`. The core keeps nothing: the app stores it as the account's endpoint and passes it as the base of later jobs until the user changes server or logs out (R1502, `login-endpoints.md` L4).
- A job follows at most one redirect. A 3xx answer to the repeated `meta` is a `redirect` error.
- Every other case is a `redirect` error (T11) that stops the job at once: a `308` on `meta` without `Location` or failing a check; any other 3xx on `meta`; any 3xx on any other method (R1511), `hostKey` included. Nothing is retried, and the result has no `newEndpoint`. The error keeps the status and the `Location` text as received, for logs.
- Anki's client follows a `308` on `meta` without checking scheme or host; the checks above are Meguri's own (R1510).

| Current base, kind | Response to `meta` | Outcome |
|---|---|---|
| `https://sync.ankiweb.net/`, `ankiweb` | 308, `Location: https://sync20.ankiweb.net/` | follow; `newEndpoint` `https://sync20.ankiweb.net/` (AE70) |
| `https://sync.ankiweb.net/`, `ankiweb` | 308, `Location: http://sync20.ankiweb.net/` | `redirect` error (https lost) |
| `https://sync.ankiweb.net/`, `ankiweb` | 308, `Location: https://sync20.example.org/` | `redirect` error (host) |
| `https://h.example/anki/`, `self_hosted` | 308, `Location: https://h.example:8443/v2` | follow; new base `https://h.example:8443/v2/` |
| `https://h.example/`, `self_hosted` | 308, `Location: https://other.example/` | `redirect` error (host) |
| `http://192.168.1.5:8080/`, `self_hosted` | 308, `Location: https://192.168.1.5:8443/` | follow (http may become https) |
| `https://h.example/`, `self_hosted` | 308, `Location: /v2/` | `redirect` error (relative) |
| `https://h.example/`, `self_hosted` | 301 or 307, any `Location` | `redirect` error |
| any | 308 on `start` (or any method but `meta`) | `redirect` error; not followed |
| any | 308 on `meta`, then 308 on the repeated `meta` | `redirect` error |

The AnkiWeb rows hold only with feature `ankiweb`; tests use a `.test` stand-in for the AnkiWeb names (T12).

## T8. Stall timeout (R1504) and connecting

- Each job carries a stall timeout from the caller: 60 seconds by default (R1504); the app's setting supplies other values (U151). Any duration above zero is accepted; zero is an `invalid_input` error.
- Progress is any of:
  - a byte of the request body handed to the connection;
  - the response's status line and headers arriving;
  - a byte of the response body read from the connection (counted before decompression).
- Setting up the connection (name lookup, TCP, TLS, a proxy tunnel) is not progress.
- The watchdog starts when the request starts. When no progress has happened for the whole stall timeout, the request ends with a `timeout` error. It fires no earlier than the timeout after the last progress and no later than one second after that.
- There is no total timeout: a transfer that keeps moving bytes never times out, however long it takes.
- So the watchdog catches a server that never answers, a server that reads the whole body and then stays silent, a response that stops in the middle, and an upload that stops because the server or network stopped taking bytes.
- Connecting (name lookup, TCP connection, TLS handshake and proxy tunnel together) may take at most 30 seconds; past that, the request ends with a `network` error (`connect_timeout`). With a stall timeout under 30 seconds the watchdog fires first, as a `timeout` error.
- Both clocks are the process's monotonic time, never the collection clock seam.
- Anki's client watches the same two directions with a 30-second default that the desktop app raises to 60 and lets the user set from 30 to 99999 seconds.

## T9. Cancellation

- Every request runs under the job's cancel token (U24). Once the token is cancelled, the job returns a `cancelled` error within 200 milliseconds, whatever the request is doing, and the connection is closed at once: no further body bytes are sent, and a partly received body is dropped.
- A failure that happens after the cancellation is still reported as `cancelled`.
- The blocking entry points behind the FFI block their calling thread until the job ends (U37, U114 call them off the main thread).

## T10. TLS, plain HTTP and proxies (R1501, R1512)

- TLS uses the platform's certificate verification and trust store (R1512), TLS 1.2 or 1.3. A certificate the system does not trust, an expired one or a host name mismatch is a `tls` error. Meguri has no setting for a custom certificate; a self-hosted server with a private certificate authority works once the user installs that authority in the system's trust store (an iOS configuration profile, the macOS keychain).
- Plain `http` is allowed for a self-hosted server the user entered (R1501). The transport opens its own sockets, so App Transport Security does not apply. On iOS and iPadOS the system may hold the first connection to a local-network address until the user answers its Local Network prompt; the transport reports a failure there like any other `network` error, and the app explains the permission (U151, U160).
- Proxy: a job carries either no proxy or one proxy that the app resolved from the system settings for the job's base, PAC files included (U114). The transport uses exactly that, for every request of the job, a redirected `meta` and later requests included. It never reads proxy environment variables (`http_proxy` and the like) or system settings itself.
  - A proxy is an `http://<host>:<port>` URL plus optional user name and password, which the transport sends as basic proxy credentials. An `https` target goes through a `CONNECT` tunnel; an `http` target is sent to the proxy in absolute form.
  - A proxy URL with another scheme is an `invalid_proxy` error before any connection. The transport never falls back to a direct connection.
  - A 407 from the proxy, either as the response or as the answer to `CONNECT`, is a `status` error with code 407.

## T11. Errors

The transport reports one of these categories, with the details listed. U106 maps them to the kinds the user sees (`errors.md`).

| Category | When | Details |
|---|---|---|
| `invalid_endpoint` | a base or redirect target fails `login-endpoints.md` L1 | which rule |
| `invalid_input` | a caller value is out of range: a zero stall timeout, an empty user name or password at login | which value |
| `invalid_proxy` | the job's proxy cannot be used (T10) | |
| `network` | no response head arrived: name lookup failed, connection refused, host or network unreachable, connecting took over 30 s (`connect_timeout`), or the connection was reset or closed; or the connection broke after the response head, while the body was read | the cause, and whether the response head had arrived |
| `tls` | the TLS handshake failed: untrusted, expired or mismatched certificate, no common protocol version | the cause |
| `timeout` | the stall watchdog fired (T8) | |
| `status` | a 4xx or 5xx response, or a 407 from the proxy (T6.1, T10) | the code and the reason text |
| `redirect` | a redirect that is not followed (T7) | the code and the `Location` text |
| `protocol` | a 2xx response that breaks T6.2: `missing_original_size`, `bad_original_size`, `size_exceeded`, `size_short`, `bad_compression` | which |
| `cancelled` | the job's cancel token (T9) | |
| `endpoint_refused` | builds with `test-hooks` only: the endpoint guard refused a URL (T12) | the guard's reason |

- A 403 and a 407 are `status` errors even though they carry no `anki-original-size`: status comes before the header check (T6.1).
- Anki reports a connection that breaks after the response head as a sync error rather than a network error; U106 decides how Meguri presents it.
- No error text, log line or diagnostic ever contains the host key, the password or a request body. Meguri never puts the host key in a URL.

## T12. Test hooks (feature `test-hooks`)

- Endpoint guard (R1566, R1876): before connecting, every request's URL (after a redirect too) and the job's proxy pass the endpoint guard (`meguri_sync::endpoint_guard`, U13). A refused URL ends the job with `endpoint_refused` before any name lookup or connection.
- AnkiWeb stand-in: with features `ankiweb` and `test-hooks`, the AnkiWeb host suffix of T7 and the AnkiWeb default endpoint (`login-endpoints.md` L3) come from one value that tests override with a name under the reserved `.test` domain, for example suffix `aw.test` and default endpoint `https://sync.aw.test/`. Tests can also pin host names to local socket addresses, and a test root certificate authority is trusted in addition to the platform's in `test-hooks` builds only, so the scripted stub (T13) can serve `https://sync20.aw.test/` with a certificate for that name. No test, script or scenario holds a real AnkiWeb host name (U19).
- Builds with `test-hooks` (Debug and Profile app builds among them) can therefore sync only with local servers.

## T13. The scripted stub

`meguri-testkit`'s `sync_stub` stands in for a server in transport tests. It needs to be able to:

- answer with a chosen status, headers and body, per request: zstd bodies with or without `anki-original-size`, a declared size above or below the real one, invalid zstd, a `308` or other 3xx with or without `Location`;
- pause before the response head, pause in the middle of the body, trickle bytes slowly, stop reading the request body, and read the whole body and then stay silent;
- serve TLS with a self-signed certificate outside the test root, and with a certificate for a `.test` name signed by the test root;
- act as an HTTP proxy (absolute-form requests and `CONNECT`), and answer 407;
- record each request: method, path, decoded `anki-sync` header, decompressed body.

## T14. Verified server behavior

The official sync server of Anki 26.09.3 (the U13 rig, real clock), probed with Meguri-shaped requests:

| Request | Response |
|---|---|
| `hostKey`, right credentials, `k` empty, Meguri identity | 200, chunked, `anki-original-size: 50`, body `{"key":"<40 lowercase hex>"}` |
| `hostKey`, wrong password, or a user that does not exist | 403, `content-type: text/plain; charset=utf-8`, body `403`, no `anki-original-size` |
| `meta` with the Meguri identity in `c` and `cv` | 200, `cont: true` |
| `meta` with `c` and `cv` empty | 200 (the server does not check them) |
| `meta` with an unknown `k` | 403 |
| header `v` 12 | 400; body `v` 12 or 7 in `meta`: 501 |
| header with an extra member | accepted |
| header with raw non-ASCII bytes in `c` or `s` | 400; the same characters as `\u` escapes: accepted |
| no `anki-sync` header, or a value that is not JSON | 400 |
| body sent with `Content-Length` or chunked | both accepted |
| empty body, or uncompressed JSON | 400 |
| `chunk` with `{}` and no sync started | 409; with `{"x":1}`: 400 |
| `start` with empty `s` | 400 |
| `chunk` with a key other than `start`'s | 409; with `start`'s key: 200 |
| `abort` with `{}` | 200, body `null` |
| `download` with `{}` | 200, the collection file's bytes, `anki-original-size` their count |
| media `begin` with body `{"v":"<Meguri identity>"}` | 200, `{"data":{"usn":0,"sk":"<host key>"},"err":""}` |
| an unknown method name | 400 with a text listing the valid names |
| `GET` on a method | 405 |

Recorded Anki-client exchanges with the same server are in `conformance/recordings/scenarios/smoke/sync/` (U13).

## T15. Transfer progress

- For each request the transport counts the request body's bytes before compression, as the compressor takes them, against the uncompressed total the caller gives (the collection file's size for `upload`), and the response's bytes after decompression against the declared `anki-original-size`.
- It reports both pairs through the job's progress callback (U24), at U24's throttling. One-way sync (U109) and media sync (U111) show them; Anki's client counts the same uncompressed bytes.
- Counting never waits for the network and never changes what is sent.
