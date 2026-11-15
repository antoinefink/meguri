# Meguri Sync native server transport

Spec: sync/server-transport
Requirements: Meguri Sync U2; R4, R7–R9, R14, R18, R25, R28, R30; KTD5, KTD10, KTD12
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/server/ (planned; probe contract conformance/meguri-sync-server#C3)

## T1. Authority and fact classes

**B** means an existing neutral 26.09.3 baseline fact, with its owning spec named. **M** means original Meguri Sync policy. **P** means an unresolved current-client observation, assigned a probe in `conformance/meguri-sync-server`. B is not evidence for Desktop 26.09.4 or any AnkiMobile build. A compatibility implementation may depend on B only after the corresponding P is sealed for its supported target. M can be implemented independently now. These classes apply to every rule below and to the other new server contracts.

The server is a separate service. Nothing here changes the Meguri application's transfer or iCloud policy. Browser methods belong only to `servers/meguri-sync-web`.

## T2. Native routes and request forms

**B:** `sync/transport#T1–T6`, `sync/login-endpoints#L2,L5` and `sync/wire-format` own the protocol v11 route names, header fields, zstd frames, integer types and bodies. Required native routes are POST `/sync/hostKey`, `/sync/meta`, `/sync/start`, `/sync/applyGraves`, `/sync/applyChanges`, `/sync/chunk`, `/sync/applyChunk`, `/sync/sanityCheck2`, `/sync/finish`, `/sync/abort`, `/sync/upload`, `/sync/download`, and POST `/msync/begin`, `/msync/mediaChanges`, `/msync/uploadChanges`, `/msync/downloadFiles`, `/msync/mediaSanity`.

**M:** Only observed current-client forms are required. Initially that is v11 with raw zstd and `anki-sync` authentication. No speculative multipart or legacy-protocol support is specified. Accept either valid Content-Length framing or HTTP chunked framing; do not reject an otherwise valid HTTP version because the baseline client happens to use HTTP/1.1. The HTTP server rejects ambiguous framing. Native routes never accept a browser cookie as authentication. They do not redirect. Unknown routes return 404, unsupported HTTP methods 405 with `Allow: POST`; no SPA fallback may answer a native route.

**B:** The `anki-sync` object carries `v`, `k`, `c`, `s`. Unsupported protocol, malformed header or missing header gives 400; unknown hostkey gives 403. Extra header object members are ignored. A round request has a nonempty session key. Non-ASCII header values must use JSON escaping. **M:** Read each known header member once; duplicate known members are malformed. Client identity is diagnostic, not authority, filesystem input, or a version-based access grant. Bind native credentials to an enabled ready account and its current credential generation.

**P:** C3 probes actual target request/header forms, HTTP framing, no-argument `{}` versus `{"_pad":null}`, protocol version, successful response decoding, malformed headers, and whether any additional form is actually required. Do not infer these from a release number.

## T3. Responses and errors

**B:** Successful responses carry a single decimal `anki-original-size` equal to the decompressed byte length and a zstd body; the empty-payload exception and decoding are `sync/transport#T6.2`. Bodies and result envelopes are `sync/wire-format#W2–W17`. `hostKey` returns a nonempty opaque `key`; `upload` succeeds only with the exact payload `OK`. Normal mutators returning no value return JSON `null`. Media download is a ZIP, without a media envelope.

**M:** Emit nonempty successful bodies as valid zstd frames and always emit the size header. Never include HTTP content coding in addition to the protocol compression. Errors are uncompressed short plain text without secrets or collection excerpts. Return 400 for malformed/invalid input, 403 for failed native authentication or revoked authority, 409 for a wrong/stale collection round or missing media download target, 413 for a configured body limit, 429 for exhausted admission budgets, and 503 for unavailable storage or service admission. Internal failures give 500 without implementation details. Do not claim an operation failed before publication if publication may already have committed; connection loss can leave the client uncertain. No new acknowledgement extension is introduced.

**P:** C3/C5/C7 verify error status/body acceptance by each target; 429/503 handling remains a server policy rather than a claimed reference fact.

## T4. Resource and exposure policy

**M:** All endpoints apply bounded streaming reads, bounded decompression and nesting, and bounded concurrent work before materializing attacker-supplied lengths. Default native compressed-body limit is 104,857,600 bytes; decompressed-body limit is 314,572,800 bytes. Configuration may raise either limit explicitly for self-hosted collections; limits must be positive and recorded in evidence. Stop at the first byte above a limit. Media-specific entry limits remain `server-media#M2`. Limits do not justify buffering a full upload in memory. A 60-second no-progress timeout applies to incoming/outgoing bytes; activity can extend a transfer without a total-duration limit. Parsing JSON uses exact integer tokens and the strict/loose exceptions of `wire-format#W1,W8–W11`; binary64 is not an intermediate for strict IDs or stamps.

Production deployment uses the configured HTTPS public origin behind a proxy; backend HTTP is supported. Only configured proxy peers supply trusted forwarded client-address/protocol information. A spoofed forwarding header cannot alter credentials, browser Origin policy, or rate-limit identity. Logs and retained evidence never contain passwords, hostkeys, cookies, capability values, request bodies, note content, or real usernames. The sole exception is operator console output of the setup code on successful first initialization, under `servers/meguri-sync-web#W4`. Use synthetic identities in fixtures. Diagnostic route, status, elapsed time and byte counts are sufficient.

**B:** Baseline default server body limits are recorded in `sync/transport#T5`; this server's configurable limits and timeout policy are M. **P:** C3/C7 probe boundary encodings and current-client treatment; a refusal is not evidence that a client sent an unsupported legacy form.
