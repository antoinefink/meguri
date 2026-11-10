# specs/servers/host-security.md (U135)

Spec-writer session s-20261110T085429Z-165d83d3. Written in Meguri's own
words; no code of AnkiConnect, the Migaku add-on, their web frameworks or Anki
was copied or translated. Anki 26.09.3 (`refs/anki-26.09.3`) was read only for its HTTP
client (H16): the host has no Anki behavior, and collection access is
cross-referenced to `engine/lifecycle`, `engine/op-model`, `engine/host` and
`engine/change-stream`.

| Source | Read for | Spec sections |
|---|---|---|
| plan, Product Contract "Mac integrations" R1663-R1679, R1789-R1797, R1949, AE78; U135 (Approach, Test scenarios), KTD9, KTD12, KTD13, KTD14 | requirements, status model, store contents, interaction channel, AudioTranscoder exception, fetcher limits | all |
| `docs/plans/research/rust-stack.md` §4 | SO_REUSEADDR lets a specific-address bind win over a wildcard holder on macOS, hence the connect probe; no `Json` extractors; Host check before routing; AnkiConnect closes after each request | H4.2, H7, H8, H16 |
| `local-data/research/09-ankiconnect.md` §0, §1.1, §1.2, §3.1, §5 | add-on bind addresses (`127.0.0.1:8765`, Migaku `*:44432` IPv4+IPv6 from `lsof`); AnkiConnect transport (one request per connection, Qt-timer polling); recommendation to block DNS rebinding | H4, H6, H16 |
| AnkiConnect upstream repository (git.sr.ht/~foosoft/anki-connect), commit 532a3c8 (2025-11-06, the last before the 2025-11-09 build): `plugin/web.py`, `plugin/util.py` (defaults) | transport facts only: socket bound with SO_REUSEADDR to `webBindAddress`, body length taken from `Content-Length` alone (chunked bodies not read), connection closed after each reply, no Host check, no body limit | H16 |
| `specs/servers/migaku-protocol.md`, `migaku-receive.md`, `migaku-routing.md` (sibling drafts of this session) | which host sections they cite (Host check, limits, keep-alive, status block, extra-origins list, waiter, transcoder, fetcher, events) | H1, H8, H10.2, H11, H12, H13 |
| `specs/engine/lifecycle.md` L1-L11, `engine/change-stream` S3, `engine/op-model` O1 | states, generations, availability wait, readers, `LocalApi` source | H3, H9, H12 |
| `specs/render/media-serving.md` | card webview origin (for R1679 cross-reference only) | — |
| AnkiConnect upstream repository, commit 532a3c8: `plugin/util.py` (`download` through Anki's HTTP client, timeout from `webTimeout`, status must be 200; `DEFAULT_CONFIG` `webBindAddress` `127.0.0.1`), `plugin/web.py` (body length from `Content-Length` only, empty body answered as the ping) | the remote-download row of H16; the intro's bind statement; chunked requests answered as a ping | intro, H16, H18.3 |
| Anki 26.09.3 (`refs/anki-26.09.3`): `pylib/anki/httpclient.py` (the HTTP client is a `requests` session with a per-request timeout) and the `requests` library's documented defaults (redirects followed, environment and system proxy settings used) | the add-on's download behavior | H16, H18.3 |
| `specs/servers/ankiconnect-envelope.md` E3, E8, E9.1, E16; `specs/servers/migaku-protocol.md` P2, P6, P8, P10.5, P18; `specs/servers/migaku-receive.md` P8, P11, P14; `specs/servers/migaku-routing.md` D2.3, D14.3, D18; `specs/servers/migaku-received-log.md` C3 (`searchNote`); `specs/servers/ankiconnect-media.md` X3.4, X8, X11; `specs/servers/ankiconnect-gui.md` G8 | the order of checks each protocol needs (head hook, responder), the key states, core-made config changes, the fetcher's callers and their status rules, the construction switch, the replay server and request clock, detached interactions, interaction kinds | H10.3, H11.2, H13, H15, H19-H22 |

Meguri decisions made here (no external source): the 500 ms probe timeout
and "timeout counts as in use"; any bind error counts as in use; IPv6
failure leaves IPv4 serving with IPv6 retried; IPv6-only sockets; the exact
Host grammar (case-insensitive names, no leading zero, refused alternative
spellings, absolute-form authority also checked, duplicate Host refused);
the 403 reply headers; malformed-request statuses 400/431/505 and limits
(64 KiB head, 100 fields); timeouts (5 s idle before the first request too,
10 s head, 30 s body and send stalls); 128 connections per server;
`Expect: 100-continue` handling; the byte values of the limits (256 MiB and
128 MiB, matching `migaku-receive`'s 128 MiB); AnkiConnect port range
1024-65535 excluding 44432; config member names and nesting; store versioning,
"newer" and corrupt-file handling; event coalescing for status only;
interaction ids `i-<n>` and the `false` return for stale answers; listener
restart backoff; status member names and `binding` state; fetcher
`Accept-Encoding: identity`, User-Agent, redirect statuses.

Added after review (same session): the head hook, error responder and
handler interface (H19); the FFI operation names, error enum, event enum and
status record (H20); the clock table, which timers use the host's injected
timer and wall clock and which use real time (H21); the test construction
parameters, ephemeral bind mode, `TestHost` defaults and socketless serving
(H22); binding after the first open attempt ends, whatever its outcome (H3,
H18.7); retries measured from the start of each attempt (H4.3); revalidate
per state; the core validation table and member order, the API-key table
and the read-only-store refusal (H10.3); the 1 s bound's scope (H10.4);
detached interactions (H11.2); per-state status values (H12); the fetcher
result table, name resolution inside the time limit, status left to the
caller (H13); the rewritten H17 scenario list. All are Meguri decisions with
no external source beyond the sibling specs listed above.
