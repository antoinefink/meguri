# Server transcripts: format, normalization and comparison

Spec: servers/transcripts
Requirements: R1799, R1800 (harness), R1801, R1945, R1948, R1949 (runner hook); supports R1888, R1889, R1944, R1946
Anki: 26.09.3 (the oracle runs Anki 26.9.3 with the AnkiConnect add-on; Anki's own behavior is reached only through the specs cited)
Scenarios: conformance/recordings/servers/ankiconnect/harness/ (T18; T2 gives the location of every transcript scenario). Protocol scenarios are listed in the protocol specs and in `servers/ankiconnect-clients`.

A server transcript scenario is a list of HTTP and WebSocket exchanges with one of Meguri's local servers, with optional collection observations between them. For the AnkiConnect server, the expected replies are recorded from the real AnkiConnect add-on running in headless Anki in Docker (the transcript oracle). For the Migaku server, which has no oracle, and for Meguri-only behavior (the `Host` check, binding, limits), the expected replies are written in the scenario itself. Meguri's executor replays the same requests against its own server, normalizes both sides the same way and compares.

Targets: AnkiConnect API version 6 as served by the add-on build of 2025-11-09 (the build the user runs, pinned by SHA-256 in the oracle image), inside Anki 26.9.3; the Migaku browser extension 1.30.16 and 1.31.0.17 payload format.

This file defines the scenario file, the golden file, the normalization, the comparison, the deviation transforms, the equivalence check of R1948 and the failure report. It does not define how the recorder works inside its container (oracle-maintainer work, U136), only what it must produce. The general conformance rules (`conformance/dsl`, `conformance/observations`) apply wherever this file cites them.

## T1. Terms

- **Transcript scenario**: a file under `conformance/recordings/servers/` (T2) in this file's form. It is not a `conformance/dsl` scenario: it has its own schema (`conformance/schema/server-transcript.schema.json`) and its own runner job (`tools/mg servers-contract`).
- **Source**: where the expected values come from: `oracle` (recorded by the transcript oracle into a golden) or `declared` (written in the scenario's `expect` members).
- **Exchange**: one HTTP request sent on a connection and the one reply read back.
- **Executor**: the side that runs a scenario: the **recorder** (the transcript oracle) or **Meguri's executor** (U136's testkit, which runs Meguri's host in-process against an engine opened on the scenario's collection, in the bind mode T20 gives).
- **Entity tokens**: the stable names that replace real ids in requests and replies (T10.1).

## T2. Files and ids

- Scenarios: `conformance/recordings/servers/<server>/<path>.yaml`, where `<server>` is `ankiconnect`, `migaku` or `host` (both servers). The id is `servers/<server>/<path>`, the path under `conformance/recordings/` without the suffix, for example `servers/ankiconnect/envelope/version-default`; otherwise ids follow `conformance/dsl` D1. They are not under `conformance/scenarios/`: the DSL suite loads every YAML file there as a `conformance/dsl` scenario, and `tools/mg goldens-check` checks every golden under `conformance/goldens/`, so a transcript there would be read as a DSL scenario or golden. Moving them there needs both tools to skip `servers/` first, and changes this section.
- Goldens (oracle scenarios only): next to the scenario, `conformance/recordings/servers/<server>/<path>.json`, written only by the recorder (`oracle/ankiconnect/record.sh` until the job `tools/mg oracle record servers/ankiconnect` of T16 exists) (R1885). No path turns Meguri's output into a golden. Declared scenarios have no golden.
- Fixtures (request bodies and media too large or too binary for YAML): `conformance/fixtures/servers/<server>/...`. Fixtures are written by spec writers, contain only synthetic content (no text, image or audio from the user's collection or any third party), and are at most 1 MiB each; larger bodies use the `fill` body form (T6.3).
- YAML rules, the scenario hash and the clock are `conformance/dsl` D2, D3 and D5. The hash covers the whole parsed scenario, fixtures excluded; a fixture's SHA-256 is part of the scenario through its `fixture` reference (T6.3), so changing a fixture's bytes makes the golden stale.

## T3. Top-level keys

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | T2 |
| `description` | no | one line for people |
| `tags` | no | as `conformance/dsl` D4; `client:<name>` marks a scenario of the client inventory (`servers/ankiconnect-clients`) |
| `server` | yes | `ankiconnect` or `migaku`: the server the exchanges go to; or `host`: both servers exist and each exchange names its server (T6.1). `host` scenarios are `declared` and run in the fixed bind mode (T20) |
| `source` | yes | `oracle` (only with `server: ankiconnect`) or `declared` |
| `clock` | yes | `conformance/dsl` D5 |
| `compare` | no | `conformance/dsl` D5 (`set` pointer patterns, applied to the step outputs of T9) |
| `server_config` | no | T4 |
| `host_env` | no | T4.1; only with `server: host` |
| `setup` | no | T5 |
| `steps` | yes | T6 and T7; at least one exchange |
| `equivalence` | no | T14; only with `source: declared` |

No other key is allowed.

## T4. `server_config`

The server's settings at the start of the scenario, in neutral names that both executors map to their own configuration. Members not given take the defaults below.

| Member | Default | Recorder (AnkiConnect add-on config) | Meguri (`servers/host-security` H10.2) |
|---|---|---|---|
| `cors_origins` | `["http://localhost"]` | `webCorsOriginList` | `ankiconnect.cors_origins` |
| `deny_origins` | `[]` | `ignoreOriginList` | `ankiconnect.deny_origins` |
| `api_key` | `null` | `apiKey` | the applied key, and `ankiconnect.api_key_required` true when not null |
| `read_only` | `false` | not available: a scenario that sets it must be `declared` | `ankiconnect.read_only` |
| `extra_origins` | `[]` | not available (`declared`) | `migaku.extra_origins` |
| `media_dir` | `media-dir` (below) | not available: the add-on always reports Anki's media folder | `ankiconnect.media_dir` |
| `readable_folders` | `[]` | not available | `ankiconnect.readable_folders` |
| `enabled` | the scenario's `server`; `[]` for `server: host` | not available | `ankiconnect.enabled`, `migaku.enabled`: the servers listed are on |
| `port` | 8765 | not available | `ankiconnect.port`; used only in the fixed bind mode (T20) |

- The recorder always runs AnkiConnect with `webBindAddress` `127.0.0.1`, `webBindPort` 8765 and no `apiLogPath`. The scenario cannot change those.
- For `server: ankiconnect` or `migaku`, the server under test is enabled and the other server is off; `enabled` is not allowed.
- Paths in `media_dir` and `readable_folders` are relative to the scenario's working directory, which the executor creates empty for each run. Meguri's executor maps `media_dir` to `ankiconnect.media_dir` `{path: <absolute path>, bookmark: ""}` (Linux has no bookmarks) and creates the folder; by default it uses the folder `media-dir` of the working directory. `media_dir: null` (declared only) leaves `ankiconnect.media_dir` unset, as before the user picks a folder (`servers/ankiconnect-media` X8). The recorder's `getMediaDirPath` is Anki's own media folder; both normalize to `<media-dir>` (T10.3).

### T4.1 `host_env`

| Member | Default | Meaning |
|---|---|---|
| `ipv6` | `true` | `false` runs the scenario in a network namespace with no IPv6 loopback |
| `start` | `ready` | `before_open`: setup builds the collection file, then the engine starts with nothing open and the host starts; the step `lifecycle: open` (or `open_fail`) makes the first open attempt (`servers/host-security` H3) |
| `store_user_version` | none | the integrations store is created before the host starts with this `user_version` and the given `server_config` as its document (`servers/host-security` H10.1) |

## T5. `setup`

`setup` is `conformance/dsl` D6, unchanged: the same sections, order and rules, run on a new collection created at `clock.start`. On both sides that collection holds what a new collection holds (`storage/new-collection` B11 for Meguri's executor; the recorder's is the collection of the new Anki profile): the default deck (id 1), the default preset and the six stock note types at ids `now_ms` to `now_ms` + 5 (`storage/new-collection` B7). Notes and cards made by setup get logical ids `#nN` and `#cN` (`conformance/dsl` D7.1); decks and note types get the tokens of T10.1.

The recorder applies setup through Anki's library inside the running Anki (its driver add-on) before AnkiConnect receives the first request. Meguri's executor applies it through the engine before its server starts. Neither side counts setup as an exchange.

## T6. Exchanges

### T6.1 Step form

A step with the key `http` is an exchange:

```yaml
- http:
    method: POST            # default POST
    target: /               # default "/"; the raw request target
    headers:                # ordered list of [name, value]; default []
      - [Origin, "chrome-extension://abcdefghijklmnopabcdefghijklmnop"]
    body: {json: {action: deckNames, version: 6}}
    connection: fresh       # default fresh; or a name (T6.4)
  expect: {...}             # declared scenarios only (T13)
```

Other members of `http`:

| Member | Default | Meaning |
|---|---|---|
| `server` | the scenario's `server` | `ankiconnect` or `migaku`; required in `host` scenarios, not allowed in others |
| `address` | `ipv4` | `ipv4` (`127.0.0.1`), `ipv6` (`[::1]`) or `non_loopback` (the namespace's own non-loopback interface address); the last two only in `host` scenarios |
| `http_version` | `"1.1"` | `"1.0"` or `"1.1"`, written in the request line (declared only for `"1.0"`) |
| `raw` | none | `{text: <string>}` or `{base64: <string>}`: these bytes are the whole request, sent as given; no request line, header or framing is added, and `method`, `target`, `headers` and `body` are not allowed with it (declared only) |
| `expect_continue` | none | `wait`: send the head, wait up to 1 s for an interim `100 Continue` or a final reply, then send the body only when the interim came (declared only); the head must list `Expect: 100-continue` |
| `async` | none | a name: the exchange runs in the background and the next step starts at once; `{await: <name>}` waits for it and outputs its result (T7.1). An async exchange uses its own fresh connection |
| `no_host` | `false` | T6.2 |
| `chunks`, `declared_length` | none | T6.3 |

A header value in `headers` may be `{fill: {size: <n>, byte: <0..255>}}` instead of a string: `byte` repeated `n` times (declared only).

### T6.2 Request bytes

Both executors write the request as raw bytes on a TCP socket, never through an HTTP client library, so that both send exactly the same bytes:

1. `<method> <target> HTTP/<http_version>` and CRLF. `method` is sent as written (it may be any token, `OPTIONS` included). `target` is sent after T10.2's substitution.
2. `Host: 127.0.0.1:<port>` and CRLF, first, unless `headers` contains a `Host` entry (any case), in which case only the given entries are sent, or unless `no_host: true` is set in `http`, in which case no `Host` is sent. `<port>` is the bound port of the exchange's server in this run (8765 for the recorder; T20 for Meguri's executor). The substitution `${port}` (T10.2) gives the same value inside header values.
3. Each `headers` entry as `<name>: <value>` and CRLF, in order, name and value exactly as given after T10.2's substitution (one space after the colon: the add-on's parser needs it).
4. `Content-Length: <n>` and CRLF when the body has bytes and `headers` holds no `Content-Length` and no `Transfer-Encoding` entry. A request with no body, or an empty body, sends no `Content-Length` unless given.
5. CRLF, then the body bytes; with a `Transfer-Encoding: chunked` entry, the body is sent as one chunk of its full length followed by the last chunk (`0` CRLF CRLF), unless `chunks` gives the sizes (T6.3).

No other header is added: no `User-Agent`, `Accept`, `Connection` or `Content-Type` unless the scenario lists it. Real clients' header sets are reproduced by listing them.

### T6.3 Body forms

| Form | Bytes sent |
|---|---|
| absent | none |
| `{empty: true}` | none; same as absent but explicit |
| `{json: <value>}` | the value serialized as compact JSON in UTF-8: no white space between tokens, object members in the order the YAML gives them, strings with only `"`, `\` and U+0000 to U+001F escaped (`\n`, `\t`, `\r`, `\b`, `\f`, otherwise `\u00xx`), every other character as UTF-8, integers in decimal, other numbers in their shortest round-trip form |
| `{text: <string>}` | the string as UTF-8, unchanged |
| `{base64: <string>}` | the decoded bytes |
| `{fixture: {path: <path under conformance/fixtures/servers/>, sha256: <hex>}}` | the file's bytes; a run whose file hash differs is `error` |
| `{fill: {size: <n>, byte: <0..255>, prefix: <string>?}}` | `prefix` as UTF-8 (default empty), then `byte` repeated until the body is `size` bytes |

- `chunks: [<n>, ...]` (inside `http`, with a `Transfer-Encoding: chunked` header) splits the body into chunks of these sizes; their sum must equal the body length.
- `declared_length: <n>` (inside `http`, declared scenarios only) sends `Content-Length: <n>` and then only the head, for checks that the server refuses before reading a body (`servers/host-security` H8).
- Inside `json` values, T10.2's substitution applies to strings before serialization.

### T6.4 Connections

- `connection: fresh` (the default) opens a new connection for the exchange and closes it after reading the reply. Oracle scenarios use only `fresh`: the add-on closes every connection after one reply.
- `connection: <name>` (declared scenarios only) uses the named connection, opening it on first use and keeping it open between exchanges. A step `{close: <name>}` closes it from the client side. When the server has closed a named connection, the next exchange on it is `error` unless its `expect` says `connection_closed_before: true`.
- The executor reads a reply up to the end of its `Content-Length`, or, when the reply has no `Content-Length`, until the server closes the connection. A reply not complete within 60 s (or the exchange's `expect.within_ms`, T13) is a run error, recorded as `timeout`.
- The executor reads while it writes. When the server replies before the request is fully written (an over-limit body, a refusal), a write error the executor then meets (broken pipe, reset) is ignored, and the reply is read and recorded. A reset before any reply byte is the outcome `reset` (T7.4).
- `{pipeline: [<http>, ...]}` (declared only) writes every request of the list back to back on one connection (a fresh one, or the named one of the first item) before reading, then reads the replies in order; its output is the list of the replies' outputs.
- `{open: {name, server?, address?, count?}}` (declared only) opens a connection and sends nothing. With `count: n` it opens `n` connections named `<name>-1` to `<name>-n`. Its output is the outcome: `open`, `refused`, or, when the expectation asks for it, `closed_without_reply` (T13).
- `{close_wait: {name, within_ms, not_before_ms?}}` (declared only) waits, sending nothing, until the server closes the named connection; it fails when the close comes after `within_ms` or before `not_before_ms`, counted from the end of the last reply on it (or from its opening when it had none), or when bytes arrive.

## T7. Other steps

### T7.1 Steps for every scenario

| Step | Form | Meaning |
|---|---|---|
| `advance` | duration (`conformance/dsl` D7.1) | moves the scenario clock as the DSL op does. In Meguri's executor this is also the host's test clock (`servers/host-security` H21), so due host timers (retry, restart waits, interaction timeouts, collection waits) fire during it, in deadline order |
| `observe` | list of observation requests (`conformance/observations` O2) | the collection observations of `conformance/observations` O4, on the server's collection, after every earlier exchange completed, `async` exchanges not yet awaited excepted. `db_dump` (O8) and `media_dir` are the usual kinds (`media_trash` and `media_pending` exist too); T10.6 replaces the ids in them |
| `await` | an `async` name (T6.1) | waits for that exchange and outputs its result as an `http` step would |
| `close` | a connection name | T6.4 (declared only) |
| `media_dir_write` | `{name, text?, base64?, fixture?}` (exactly one content member; `fixture` as the T6.3 body form) | an outside process writes the file `name` with these bytes into the folder that `getMediaDirPath` reports (the recorder: Anki's media folder; Meguri: the `media_dir` of T4), as mpvacious does (`servers/ankiconnect-clients` C2). `name` is a plain file name; the step does not wait for any pickup |

### T7.2 Permission prompts (`requestPermission`)

`permission: {answer: allow | deny | deny_ignore | none}` scripts the answer to the next permission prompt the server raises (R1749, R1750). It must come before the exchange that raises the prompt and applies to that prompt only. It is the same as `interaction {kind: PermissionPrompt, ...}` (T7.5) with these answers.

| Answer | Recorder (the add-on's dialog) | Meguri (the interaction of `servers/host-security` H11.2) |
|---|---|---|
| `allow` | the Yes button | Allow |
| `deny` | the No button, ignore box clear | Deny, checkbox clear |
| `deny_ignore` | the No button, ignore box checked | Deny, checkbox checked |
| `none` | not available: the add-on waits for the dialog forever and answers no other request meanwhile, the ping included (declared only) | no answer; the interaction times out (120 s on the host's timer clock, which `advance` moves; run the exchange `async`, then `advance`) |

An exchange that raises a prompt with no scripted answer is a run error on both sides. A scripted answer that no prompt consumed by the end of the scenario is a run error too. The recorder's driver add-on answers the dialog itself (T19.1): `allow` adds the origin to the add-on's allow-list, `deny_ignore` to its ignore list, and `deny` changes nothing, so a later request from that origin prompts again. A `requestPermission` that ends without a prompt consumes no answer: an allowed origin, a deny-listed (ignored) origin, and in the recorder one with a `params` member, which fails before any prompt.

### T7.3 Meguri-only host steps (declared scenarios only)

| Step | Form | Meaning and output |
|---|---|---|
| `apply_config` | a partial config document in `servers/host-security` H10.2's nested form (`{ankiconnect: {enabled: false}}`), optionally with `api_key` | merges it into the current document and applies the result with the key through H10.3. Output `{}`, `{refused: {member: "<dotted name>"}}` or `{refused: store_read_only}` |
| `config` | `{}` | output `{config: <the current document>}` |
| `lifecycle` | `exclusive`, `ready`, `long_op`, `long_op_end`, `close`, `open`, `open_fail`, or `{replace: <setup>}` | puts the engine in that state (`engine/lifecycle`): `exclusive` enters an exclusive phase; `long_op` takes the long-operation slot as Check Database would; `open_fail` makes an open attempt that ends `Failed{reason}`; `{replace: ...}` completes a replacement with a new collection built from that `conformance/dsl` D6 setup, entity tokens continuing (T10.1) |
| `restart_host` | `{}` | stops the host as at app quit and builds it again on the same store and collection |
| `helper` | `{listen: "<addr>:<port>"}`, `{release: "<addr>:<port>"}` or `{accepted: "<addr>:<port>"}` | a helper socket outside Meguri listens on, or releases, that address; `accepted` outputs `{accepted: <n>}`, the connections that helper accepted so far. A helper never reads or writes |
| `fault` | `{listener_crash: {server}}`, `{handler_panic: {server, target}}`, `{drop_ipv4_socket: {server}}` | the host's test hooks (`servers/host-security` H22); `handler_panic` makes the next request to that route panic in its handler |
| `revalidate` | `{}` | calls the host's revalidate operation |
| `resolve` | `{id, outcome}` | calls the host's resolve operation (H11.2) with that id and JSON outcome; output `{resolved: <bool>}` |
| `events` | `{}` | output `{events: [...]}`: the host events posted since the scenario started or since the previous `events` step, in order, as `{type, ...}` objects with H11.1's payloads; status events are left out |
| `sockets` | `{}` | output `{sockets: [...]}`: the listening TCP addresses of the process, as `127.0.0.1:<port>` and `[::1]:<port>` strings, sorted |
| `logs` | `{}` | output `{activity: <n>, received: <n>}`: the number of AnkiConnect activity-log entries (U141) and of Recently received rows (U140) |
| `media_dir_scan` | `{}` | runs one scan of the external media folder (`servers/ankiconnect-media` X11.3), as Linux tests do instead of the file-system watcher |
| `wait` | duration | real time passes; nothing else is done |
| `ws_open` | `{name, server?, target?, headers?}` | opens a WebSocket on a new connection (Migaku); `expect` holds the handshake reply |
| `ws_send` | `{name, text?, base64?, ping?, close?}` | sends one frame or message |
| `ws_expect` | `{name, frames, within_ms}` | the frames the client must have received by then, in order: `{pong: <payload>}`, `{close: <code>}`, `{text: ...}`; `frames: []` means none |
| `status` | `{}` or `{until: {<member>: <value>, ...}, within_ms: <n>, server?}` | output `{status: [<ankiconnect status>, <migaku status>]}` with the values of `servers/host-security` H12, `next_retry_at` as an offset in seconds from `clock.start` on the host's wall clock. With `until`, the executor reads the status again (every 50 ms of real time) until every listed member of the named server's status (`server` as in T6.1) has the value, or `within_ms` passes, which fails the step |
| `parallel` | a list of `http` steps | sends them on separate fresh connections at the same instant and waits for all replies; outputs in list order |

### T7.4 What a step outputs

Each step outputs one value, at its index in the run's `steps` output (T9):

- `http`: `{reply: <normalized reply>}` (T8, T10), or `{outcome: <kind>}` when no reply was read: `refused` (the connection was refused), `closed_without_reply` (the server closed it before any reply byte), `reset` (reset before any reply byte). Without a matching expectation (T13), an outcome is a run error of that kind.
- `observe`: `{<kind>: <value>, ...}` as `conformance/observations` O1, then T10.6.
- `await`, `pipeline`, `open`, `status` and the T7.3 steps with an output: as their rows say.
- every other step: `{}`.

A run error ends the run with the status `error` (T16) and no golden; the report names its kind and step (T15). The kinds:

| Kind | Executor | When |
|---|---|---|
| `refused`, `closed_without_reply`, `reset` | both | an outcome above with no expectation that asks for it |
| `timeout` | both | a reply not complete in time (T6.4); for the recorder also Anki's start, its collection, a setup, snapshot or observe request that does not finish, or Anki's Browse window still running a search 10 s before an exchange (T16) |
| `prompt_not_answered` | both | a permission prompt with no scripted answer (T7.2) |
| `permission_unused` | both | a scripted permission answer that no prompt consumed by the end of the scenario (T7.2) |
| `bad_framing` | both | a reply whose head does not parse, or that ends before its `Content-Length` (T8) |
| `unknown_ref`, `reused_ref` | both | T10.2 |
| `fixture_unavailable` | both | a `fixture` body or `media_dir_write` whose file is missing, or no fixtures folder is mounted |
| `fixture_mismatch` | both | a fixture whose bytes do not have the given SHA-256 (T6.3) |
| `unsupported_step` | recorder | a valid step form the recorder does not run yet (today `async` exchanges and `await`); Meguri's executor reports such a form as `pending` instead (T15) |
| `clock_escaped` | recorder | a clock read inside Anki escaped the scenario clock: after setup or an exchange, a new id of at least 1,000,000,000 outside [now in ms, now in ms + 10^7), or a note or card `mod` outside [`clock.start`, now]; or Anki's clock did not reach the instant an `advance` wrote (T16) |
| `anki_exited` | recorder | Anki stopped during the run |

A scenario that breaks this file's rules (a key, step or member it does not allow, or one not allowed in its `source`) is `invalid` (`conformance/dsl` D10); the recorder reports it as the kind `invalid_scenario` and writes no golden.

### T7.5 Scripted interactions (Meguri's executor)

`interaction: {kind, answer}` queues one answer for the next interaction of that kind that the host posts (`servers/host-security` H11.2, H22). `kind` is the requesting unit's kind name (`servers/ankiconnect-gui` G8, `PermissionPrompt` of `servers/ankiconnect-envelope` E9); `answer` is that kind's answer in the JSON form of `servers/ankiconnect-gui` G8 (or E9's for `PermissionPrompt`), with T10.2's substitution (`{notes: ["#n2"]}`), or `none` for no answer, so the interaction times out on the host's timer clock.

A kind with no queued answer gets its test default:

| Kind | Default |
|---|---|
| `GuiBrowse`, `GuiEditNote`, `BrowseNote` | `done` |
| `GuiAddCards` | `presented` |
| `GuiSelectCard` | `{selected: true}` |
| `GuiSelectedNotes` | `{notes: []}` |
| `PermissionPrompt`, `SyncNow` | none: T7.2's run error for `PermissionPrompt`; `SyncNow` must be scripted, or it is a run error |

In oracle scenarios the recorder ignores `interaction` steps (it drives Anki's real windows, `servers/ankiconnect-clients` C7). There an answer stands in for the window on Meguri's side and must describe what the recording shows (for `GuiSelectedNotes`, the notes the recorded reply lists); when it does not, the difference shows in the reply, and the oracle maintainer corrects the answer, not the golden.

## T8. What a reply holds

A normalized reply is:

```json
{
  "status": 200,
  "headers": {"access-control-allow-headers": "*", "access-control-allow-origin": "*", "content-type": "application/json"},
  "body": {"json": {"result": ["Default"], "error": null}},
  "closed": true
}
```

- `status`: the status code as an integer. The reason text after the code is ignored.
- `headers`: an object of the **compared headers** present in the reply, keyed by lowercase name, each value with spaces and tabs removed at both ends; repeated headers are joined with `, ` in order. The compared headers are `content-type`, `access-control-allow-origin`, `access-control-allow-headers`, `access-control-allow-methods`, `access-control-allow-private-network`, `allow`, `upgrade` and `sec-websocket-accept`. Every other header (`date`, `content-length`, `connection`, `keep-alive`, `vary`, `server`, `transfer-encoding`) is not recorded in goldens: connection handling is Meguri's own (`servers/host-security#H7`) and is checked by declared scenarios, which may name any header (T13).
- `body`: `{"empty": true}` when the body has no bytes; `{"json": <value>}` when the bytes are valid UTF-8 and one JSON value (RFC 8259, white space allowed around it); otherwise `{"text": <string>}` when valid UTF-8, else `{"base64": <string>}`.
- `closed`: whether the server closed the connection right after the reply (before the executor's next action on it). Recorded for information; not compared unless a declared `expect` names it (T11).
- `interim`: the status codes of interim (1xx) responses read before the final reply, in order; present only for an exchange with `expect_continue` (T6.1), `[]` when none came. Declared scenarios only.
- Content-Length consistency is checked, not recorded: a reply whose `Content-Length` differs from its body length is a run error (`bad_framing`) for Meguri's executor; the recorder accepts the add-on's 403 reply, which has none. A reply to `HEAD` is exempt: it has no body, and its `Content-Length` may be absent or give the length of the omitted body, as HTTP allows (`servers/ankiconnect-envelope` E2, `servers/migaku-protocol` P4); Meguri's executor reads no body bytes after the head of a `HEAD` reply.
- JSON integers are kept exactly at any size (field and template ids are 64-bit); they are never turned into floating-point numbers. Numbers with a fraction or exponent keep their kind (`conformance/observations` O6, O7).

## T9. Goldens

```json
{
  "scenario_id": "servers/ankiconnect/envelope/version-default",
  "scenario_sha256": "<hex>",
  "oracle": {
    "anki_version": "26.9.3",
    "lock_sha256": "<hex>",
    "ankiconnect_lock_sha256": "<hex>",
    "addon": {"build": "2025-11-09", "sha256": "<hex of the add-on source archive>"},
    "image": "<oracle-ankiconnect image tag>",
    "driver_sha256": "<hex>"
  },
  "tz": "Europe/Paris",
  "clock": {"mode": "frozen", "start": "2030-01-15T10:00:00"},
  "observations": {"steps": [{}, {"reply": {...}}]}
}
```

- Written only by the recorder (T2); text format as `conformance/dsl` D11 (sorted keys, two-space indents, LF, final newline).
- `oracle.addon.sha256` is the SHA-256, in lower-case hex, of the pinned upstream source archive of the add-on (upstream release 25.11.9.0), the value that `ankiconnect_addon_sha256` in `conformance/oracle.json` will hold once that key is added (see the staleness rule below); until then the current pin is `archive_sha256` in `oracle/ankiconnect/ADDON_PIN`, which the image's `ADD --checksum` repeats. The image build checks the archive against it and each installed add-on file against its own pinned hash, so one value names the exact add-on code. `oracle.addon.build` is the build date `2025-11-09`, for reading only.
- `oracle.lock_sha256` is the base oracle image's lock, `oracle/requirements.lock`, as in `conformance/dsl` D11 (the value of `lock_sha256` in `conformance/oracle.json`). `oracle.ankiconnect_lock_sha256` is the SHA-256, in lower-case hex, of the recorder image's own lock, `oracle/ankiconnect/requirements.lock`, which pins Anki 26.9.3's desktop app and its Qt on top of the base lock; the current pin is that file's hash until `conformance/oracle.json` gains `ankiconnect_lock_sha256`.
- `oracle.driver_sha256` names the recorder build. It is the lower-case hex SHA-256 of a list of lines, one per file, each `<label>`, a NUL byte, the lower-case hex SHA-256 of the file's bytes and LF: first the base oracle driver's files (those its own `driver_sha256` covers, `conformance/dsl` D11), labelled `driver/<path in the driver folder>`, in the order of those paths; then, labelled `ankiconnect/<path in oracle/ankiconnect/>` and sorted by that path, every file of `recorder/` except files whose name starts with `test_` and compiled caches, every file of the driver add-on `addons21/meguri_recorder/`, `launch.py` and `entrypoint.sh`. The pinned AnkiConnect files are not in it: `oracle.addon.sha256` names them.
- `observations.steps` holds one output per scenario step (T7.4), already normalized by T10. A golden never holds a real id that T10.1 maps (in replies at T10.5's positions, in observations at T10.6's), a wall-clock time, a host path or a random id.
- A golden is **stale** when `scenario_sha256` differs from the scenario's hash, or when `oracle.lock_sha256`, `oracle.ankiconnect_lock_sha256`, `oracle.addon.sha256` or `oracle.anki_version` differs from the current oracle pin, or a member of these is missing (`conformance/oracle.json` gains `ankiconnect_addon_sha256`, `ankiconnect_lock_sha256` and `ankiconnect_image`). `driver_sha256` and `image` never make a golden stale on their own.
- Two recordings of the same scenario at different wall-clock times and on different machines give byte-identical goldens (T17).

## T10. Normalization

Both executors apply exactly these rules, in this order, to every reply and observation, and the same substitution to every request.

### T10.1 Entity tokens

Real ids differ between runs and between Anki and Meguri; tokens name entities by their order of creation.

| Entity | Token | Numbered |
|---|---|---|
| note | `#nN` | `conformance/dsl` D7.1 |
| card | `#cN` | `conformance/dsl` D7.1 |
| deck | `#dN` | from 1, in ascending id order among decks whose id is at least 1,000,000,000, at the end of setup; later decks continue the numbering |
| note type | `#mN` | the same rule among note types |

- **Stock note types count.** The note types numbered at the end of setup include the six stock kinds of the new collection (T5), which both executors hold at ids `now_ms` to `now_ms` + 5 in `notetypes/stock-kinds` K1 order, below every note type setup adds (`storage/new-collection` B7; Meguri's ids by `storage/ids-usn-mtime` I2). So they are `#m1` to `#m6` on both sides (`Basic` is `#m1`, `Image Occlusion` `#m6`), and the first note type of setup is `#m7`. No deck other than the default deck (id 1, never a token) exists before setup, so setup's decks start at `#d1`. Meguri's executor needs no other collection than `storage/new-collection` B11's for this; a Meguri executor whose new collection lacks the stock kinds gives every later note type another token and fails such scenarios, as it would fail `notetypes/stock-kinds` K2.
- After every exchange, and before that exchange's reply is normalized, notes, cards, decks and note types that have no token yet get the next ones, in ascending id order per entity. Cards are numbered by ascending card id too, never by template order, as `adopt` does (`conformance/normalized-dump` N5); only setup's cards follow `conformance/dsl` D7.1. The reference creates the cards of one cloze note in no set id order, so a scenario never relies on the tokens of a cloze note's cards when an exchange makes more than one card for that note. So the reply of the `addNote` that created a note already shows that note's token. Entities deleted before they got a token never get one.
- Ids below 1,000,000,000 (the Default deck and the default preset, id 1) are never tokens: both sides use the same small ids, and a count, an ord or a version must never be mistaken for an id.
- **How the recorder's ids fall.** Under the frozen clock every new id of a kind is the clock's milliseconds plus the lowest offset not used by that kind yet (observed: the stock note types get `<ms>` to `<ms>+5`, and the first deck, note and card each get `<ms>`). So ids of different kinds are often equal, and an id freed by a deletion is given to the next entity of that kind (observed for notes, their cards and decks). Two recordings of one scenario give the same ids. Meguri's ids need not follow this scheme; tokens make the two agree.
- **A token names an entity, not an id.** After each exchange, a tokened entity that no longer exists is retired (below): its token is never given to another entity, and a later entity with the same id gets the next token as a new entity. An entity found at a tokened id whose identity changed within one exchange is new too: a note whose `guid` differs, a card whose note entity or template ord differs, a deck or note type whose name differs. Notes are resolved first: a card's note is compared by the note's token after this exchange's notes got theirs, not by the note id, so a card whose note was deleted and replaced by a new note at the same id within one exchange (a `multi` with `deleteNotes` then `addNote`) is a new card even when its id and ord are unchanged.
- **Retired tokens.** A token whose entity no longer exists is **retired**. It keeps the entity's last real id, and it stays usable in later requests: T10.2 substitutes that id (never `unknown_ref`; `reused_ref` when a live entity of the kind now has the id). Retiring, like giving new tokens, happens after the exchange and before its reply is normalized, so the reply of the deleting exchange already sees the entity as retired. A live entity always wins over a retired one with the same id; among retired entities of one kind with the same last id (the reference can reuse an id, delete it again and so on), the one retired last wins.
- **Where ids are replaced.** A JSON integer is replaced by an entity's token only at a position that T10.5 declares to hold an id of that entity, and only when it equals the real id of a live entity with a token. At such a position, an integer that matches no live entity becomes `"#unknown"` when it is at least 1,000,000,000 (a deleted or never-existing id), and stays as it is otherwise; retired tokens are not used at positions.
- **Inside error strings.** In the `error` member of an AnkiConnect error reply, each run of 10 or more digits is replaced by the token of the tokened entity whose real id it equals. Because ids of different kinds coincide, the kind is taken from the message when it names one: a message that starts with `Note` or `note` looks among notes, `Card` or `card` among cards, `deck` among decks, `model` among note types (`Note was not found: <id>`, `model was not found: <id>`). Any other message is replaced only when exactly one tokened entity of any kind has that id. Live entities are searched first; when none has the id, retired ones are, with the same kind rule, so `Note was not found: <id>` for a deleted note reads `Note was not found: #n1` on both sides even though Meguri's deleted id differs from the reference's. Other runs stay.
- U+2068 and U+2069 inside an error string are kept: T10.3 removes them only from `nextReviews`. An error text that differs from Meguri's only by them is a whole-string `replace` deviation like any other text difference (T12).
- Integers equal to an entity id at positions T10.5 does not name stay as they are. The recorder reports each such case as a warning naming the scenario, step and pointer, so the table can be completed.
- A reused id also moves an entity in replies that the reference orders by storage position rather than by id: observed, after a note is deleted and another added, `findNotes` and `findCards` list the new note and its card last, although their ids are now the lowest. Scenarios whose replies list notes or cards do not delete before adding unless the order is compared as a set (T11).

### T10.2 Substitution in requests

- A JSON string value that is exactly a token (`"#n3"`, `"#c1"`, `"#d2"`, `"#m1"`) is replaced by that entity's real id as a JSON integer.
- Inside any string (in `json` bodies, `text` bodies, `target` and header values), `${#n3}` is replaced by the decimal real id, `${port}` by the bound port of the exchange's server, and `${fixture_base}` by the base URL of the scenario's HTTP fixtures (`conformance/dsl` D6.9) as this executor serves them, with no final slash (`http://127.0.0.1:<fixture port>`). Each executor serves the fixtures on its own loopback, so the value differs between them. `$${` writes a literal `${`. Any other `${...}` is a scenario error.
- A token that names no entity yet is a scenario error (`unknown_ref`), as in `conformance/dsl` D7.1.
- A retired token is replaced by its entity's last real id (`notesInfo` `["#n1"]` after `deleteNotes` asks about the deleted id on both sides). When that id now belongs to a live entity of the same kind (the reference reuses freed ids, T10.1), the substitution is a run error (`reused_ref`): the request would name another entity on one side only. Scenarios therefore use a retired token only before the next entity of its kind is created.

### T10.3 Times and environment values

| Where | Rule |
|---|---|
| in `reply` outputs only, a member named `mod` whose value is an integer, at any depth | `"@<n>"`, where n is the value minus the epoch seconds of `clock.start` (may be negative). `observe` outputs are normalized by their own rules (`conformance/observations`, `conformance/normalized-dump`), by the next two rows and by T10.1 at T10.6's positions only |
| in every output, `reply` and `observe` alike (a failed download's text lands in a field), every occurrence of the executor's `${fixture_base}` value inside a string | `<fixture>` (so `http://127.0.0.1:41234/missing.png download failed with return code 404` becomes `<fixture>/missing.png download failed with return code 404`) |
| the result of `getMediaDirPath`, and any string equal to the executor's media folder path | `"<media-dir>"` |
| the result of `getActiveProfile`, items of the result of `getProfiles`, and members named `profile` in `notesInfo` items, when equal to the executor's profile name | `"<profile>"` |
| members named `id` inside items of `tmpls` and `flds` of a note type object (`findModelsById`, `findModelsByName`, `createModel` results) | `"#rand"` when an integer |
| items of `nextReviews` (`cardsInfo`) | the characters U+2068 and U+2069 removed |
| members `question` and `answer` of `cardsInfo` items | `conformance/render-ops#Q5.4`, nothing else |

Times not named here (review `due` days, revlog ids, intervals) are left unchanged: the clock is the same on both sides.

### T10.4 Order

The rules apply in the order T10.3 (environment), then T10.1 (ids, at T10.5's positions in replies and T10.6's in observations). Pointers in reports and deviations address the normalized values.

### T10.5 Id positions

Positions are given per action as pointers into the reply's result (the bare result when the request's `version` is 4 or less, `/result` otherwise; for `multi`, each sub-reply by its own version) and into request-independent shapes. `*` matches any one segment.

| Action | Note ids | Card ids | Deck ids | Note type ids |
|---|---|---|---|---|
| `findNotes`, `cardsToNotes` | `/*` | | | |
| `findCards`, `guiBrowse` | | `/*` | | |
| `getDecks` (members named by deck, each a list of card ids) | | `/*/*` | | |
| `addNote`, `guiAddCards` | the result | | | |
| `addNotes` | `/*` | | | |
| `guiSelectedNotes` | `/*` | | | |
| `notesInfo` | `/*/noteId` | `/*/cards/*` | | |
| `notesModTime` | `/*/noteId` | | | |
| `cardsInfo` | `/*/note` | `/*/cardId` | | |
| `cardsModTime` | | `/*/cardId` | | |
| `deckNamesAndIds` | | | `/*` | |
| `createDeck` | | | the result | |
| `getDeckStats` | | | the member names (`/<id>`, replaced as keys) and `/*/deck_id` | |
| `modelNamesAndIds` | | | | `/*` |
| `findModelsById`, `findModelsByName` | | | `/*/did` | `/*/id` |
| `createModel` | | | `/did` | `/id` |

- An object member name that is a decimal id (`getDeckStats`) is replaced by its token as a member name.
- Actions not in the table have no id positions; their integers are compared as they are.
- U141 to U143 add rows when they add actions to scenarios, in this file, with their spec change.

### T10.6 Id positions in observations

`conformance/observations` reports real ids in some kinds (O1, O8: Meguri's DSL executor allocates ids as Anki does). In a transcript the reference's ids follow the add-on's run and Meguri's need not (T10.1), so the ids at these positions of an `observe` output are replaced too, after the kind's own normalization:

| Kind | Positions and the entity each names |
|---|---|
| `db_dump` `notes` | the row key and column `id`: note; `mid`: note type |
| `db_dump` `cards` | the row key and column `id`: card; `nid`: note; `did` and `odid`: deck |
| `db_dump` `decks` | the row key and column `id`: deck |
| `db_dump` `notetypes` | the row key and column `id`: note type |
| `db_dump` `fields`, `templates` | the `<ntid>` part of the row key `<ntid>,<ord>` and column `ntid`: note type |
| `db_dump` `revlog` | `cid`: card (the row key and `id` are review instants and stay) |
| `db_dump` `graves` | the `<oid>` part of the row key `<oid>,<type>` and column `oid`: by the row's `type`, card for 0, note for 1, deck for 2 |
| `decks` | `/<name>/id`: deck |

- A value at a position is replaced as in replies (T10.1 "Where ids are replaced"): the token of the live entity with that id; an id of at least 1,000,000,000 that no live entity has becomes `"#unknown"`, a smaller one (the default deck 1, `odid` 0) stays. Two differences: a `graves` `oid` is looked up among retired entities of its kind first (a grave names a deleted entity; among several, the one retired last), then among live ones; and in a row key an id with no token stays in decimal digits, so that two rows never get the same key (the rows the add-on adds and deletes inside one `addNotes`, `servers/ankiconnect-actions` A18 `ankiconnect-addnotes-rollback`, have no tokens). A key replaced by a token is written as the token text (`"#n1"`, `"#m7,0"`, `"#n2,1"`).
- Ids inside BLOB digests, inside `config` keys (`_nt_<id>_lastDeck`) and in other columns are not replaced. Transcript scenarios therefore dump `notes`, `cards`, `revlog`, `graves`, `decks` and `tags` only: `col` and `config` hold stamps and id-named keys of the add-on's run (`servers/ankiconnect-actions` A18 `ankiconnect-add-defaults`), and `notetypes`, `fields` and `templates` hold the stock note types' rows, which stay out of goldens (R1959; `conformance/notetype-ops` keeps the same rule).
- `ids` is not used in transcript scenarios: its values are real ids by definition. Other kinds that name notes and cards (`card`, `revlog`, `search`) already use the logical ids `#nN` and `#cN`, which are the tokens of T10.1 for notes and cards (the executor gives the driver each token's real id, a retired token's last one).

## T11. Comparison

The runner compares the expected outputs (the golden's for `oracle`, the scenario's `expect` for `declared`) with Meguri's, after normalization and after the deviation transforms (T12):

- Step outputs compare by `conformance/observations` O7: member by member, item by item, numbers by kind, strings exactly, `compare.set` for unordered arrays.
- For an oracle scenario, every member of every reply is compared: `status`, each compared header present on either side (a header on one side only is a difference, shown as `<absent>`), and `body`. `closed` is never compared for oracle scenarios (`servers/host-security#H16`: the add-on always closes).
- Error strings compare exactly. An error that Meguri words differently on purpose needs a deviation entry: `replace` with Meguri's text, or `any_string` (T12) where the original's text is its language runtime's message and only the shape is the interface (`servers/ankiconnect-envelope#E18`). No other pattern matching is ever applied.
- The first divergence is the smallest step index with a difference.

## T12. Deviations

Deliberate differences from the oracle are entries of `conformance/deviations.yaml` (`conformance/dsl` D13), with pointers into the normalized step outputs: `/steps/<i>/reply/headers/access-control-allow-origin`, `/steps/*/reply/body/json/error`. `scenarios` patterns select transcript scenarios by id as for other scenarios. An entry for transcript scenarios may also carry `tags: [<tag>, ...]`: it then applies only to the selected scenarios that carry every listed tag (T3 `tags`).

Transforms: `replace` (D13), and two transforms added by this file:

- `absent: true`: the member at the pointer is removed from the expected value (a header or member the oracle sends and Meguri deliberately does not). A pointer that matches nothing applies nothing.
- `any_string: true`: the expected value at the pointer, which must be a string, becomes a matcher that equals any JSON string of at least one character on the actual side. An actual value that is not a string, or is empty, is a difference reported against the original expected value. Used only for error texts that the original takes from its language runtime.

Expected deviation entries, each with the scenario that asserts Meguri's side. The complete list for AnkiConnect is `servers/ankiconnect-envelope#E18` (and the difference sections of `servers/ankiconnect-actions`, `servers/ankiconnect-media`, `servers/ankiconnect-gui`); the host's own are:

| Requirement | What differs | Typical transform |
|---|---|---|
| R1676 | a request with a refused `Host` gets 403 with an empty body; the add-on answers it normally | `replace` of the whole reply |
| R1739 | with no `Origin`, the add-on sends `access-control-allow-origin: http://localhost`, Meguri `*` | `replace` on that header, with `tags: [no-origin]`; every oracle scenario whose requests carry no `Origin` has the tag `no-origin` |
| R1744 | `Origin: null` is refused on both sides (the add-on refuses it unless the list holds `null` or `*`); with `*` in the list the add-on allows it and Meguri does not | `replace` of the whole reply, only in scenarios that list `*` |
| R1745 | the 403 for a disallowed origin carries no CORS header in Meguri; the add-on sends `access-control-allow-origin` and `access-control-allow-headers` | `absent` on both headers |
| R1747 | a supplied `key` with no key configured is ignored; the add-on refuses it | `replace` |
| R1740 | unknown `params` members are ignored and a missing one is named in Meguri's words; the add-on reports Python's argument errors | `replace` of `/body/json` |
| R1777 | Unicode case folding in search | `replace` of the result |

A deviation entry suppresses exactly its declared pointer in its declared scenarios: any other difference still fails (T17's self-test checks this).

Oracle scenarios never send `Transfer-Encoding`: the add-on frames a request body by `Content-Length` alone and answers a chunked request as an empty-body ping (`servers/host-security#H16`). Chunked bodies are tested only in declared scenarios, so they need no deviation entry. Likewise keep-alive needs none, since `closed` is not compared in oracle scenarios (T11).

## T13. Declared scenarios

A declared scenario gives each exchange's expected reply in `expect`:

```yaml
- http: {method: GET, target: /srs-check}
  expect:
    status: 200
    headers: {access-control-allow-origin: "*", content-type: "application/json; charset=UTF-8"}
    body: {json: {ok: true, version: "4"}}
    within_ms: 200
```

- `status` is required, except with `outcome`. `body` is optional: when absent, the body is not compared; `{empty: true}` requires no bytes.
- `expect` may also follow an `open` step (`outcome` and its times) and an `apply_config` or `restart_host` step (`no_effect` only). A step's own output checks (`status` `until`, `close_wait`) need no `expect`.
- `headers` lists only the headers checked. A name with the value `null` must be absent; a listed name must be present with that value. Header names not listed are not compared. Any header may be named, not only T8's compared set (the executor records all headers for declared scenarios).
- `closed: true | false` checks whether the server closed the connection after the reply. `connection_closed_before: true` is T6.4's case.
- `outcome: refused | closed_without_reply | reset | open` expects that connection-level outcome (T7.4) instead of a reply; `status` is then not given. For `closed_without_reply`, `within_ms` bounds when the close must come and `not_before_ms` (optional) when it may come at the earliest, counted from the last request byte sent, or from the connection's opening for `open`.
- `interim: [<status>, ...]` checks T8's `interim` list.
- `within_ms` fails the exchange when the whole reply took longer, measured from the last request byte sent to the last reply byte read.
- Body values in `expect` use T10's tokens: a declared expectation never holds a real id.
- `no_effect: true` checks that the step changed nothing in the collection: the `db_dump` of every table and the media folder listing are the same before and after it. It is allowed on exchanges and on the `apply_config` and `restart_host` steps.

Declared scenarios are the specs' statements of Meguri's behavior. Their expectations change only with the spec that owns them.

## T14. Equivalence with Anki (R1948)

For notes that Meguri creates through a server where no oracle records the same request (all Migaku notes; AnkiConnect adds are already covered by `observe` steps in oracle scenarios):

```yaml
equivalence:
  notes: ["#n1"]
```

1. After the last step, Meguri's executor reads each listed note and builds its **add record**: note type name, deck full name of each card, field values in field order, tags in stored order, and the media files the fields reference (name and SHA-256 of the bytes).
2. The runner builds a `conformance/dsl` scenario from the transcript's `clock` and `setup` plus, for each record in order, the media files (`media_files`, D6.8) and one `add_note` step with the record's note type, deck and field values and tags (D7.2), then `observe: [{db_dump: {tables: [notes, cards]}}, media_dir]`.
3. The anki oracle runs that scenario. The runner compares its `db_dump` of the added notes and their cards with Meguri's own `db_dump` of the same notes after the transcript, with `conformance/normalized-dump` N2's masks: fields, `sfld`, `csum`, tags, note type, card ords and decks, card queue and type must match; ids, GUIDs, `mod` and `usn` are masked. Media files must match by name and SHA-256.
4. A difference fails the scenario with the pointer into the dump, as T15 reports. An add record the oracle refuses (a step rejection) fails it too.

This checks that a note made by the server is the note Anki would have made from the same fields; the routing and mapping that produced the fields are the protocol specs' own scenarios.

## T15. Failure report (R1888)

For every scenario that is not `pass`, `tools/mg servers-contract` reports, in text and in the JSON report of `conformance/dsl` D12 (with `runner: "servers"`):

- the scenario id and status;
- for `fail`: the first divergence as the step index and, for an exchange, the request's method and target; then each difference as a pointer into the step outputs (`/steps/3/reply/body/json/result/0`), with expected and actual values (`<absent>` for a missing member), the first 20 in pointer order and the total count; the deviation entries applied;
- for `error`: the run error kind (T7.4) and the step;
- for `stale`: which hash or pin differs and the command `tools/mg oracle record <id>`;
- for `pending`: the step kinds, actions or observation kinds Meguri's executor does not implement yet;
- the repro command `tools/mg servers-contract <id>`.

## T16. Statuses and the job

- Statuses are those of `conformance/dsl` D10: `pass`, `fail`, `error`, `stale`, `pending`, `missing` (an oracle scenario without a golden). A declared scenario cannot be `stale` or `missing`.
- `tools/mg servers-contract [<filter>] [--tag <tag>]` runs the transcript scenarios in Docker against Meguri's executor and writes the JSON report. `<filter>` is a scenario id or an id prefix ending in `/` (`servers/ankiconnect/clients/yomitan/`); `--tag` keeps only the scenarios that carry that tag (`client:yomitan`); with both, a scenario must match both. Meguri's executor binds as T20 says, on loopback only (R1949); the job also runs the loopback check of `servers/host-security` H17 `docker-non-loopback-refused`.
- `tools/mg oracle record servers/ankiconnect[/<filter>]` runs the recorder for oracle scenarios and writes their goldens. It never runs declared scenarios. Until that job exists, `oracle/ankiconnect/record.sh` is the recorder's only entry point, under the same rules.
- The recorder runs each scenario in a fresh container state: a new Anki profile, the frozen scenario clock through libfaketime with real monotonic time (so Qt timers fire), setup, then the exchanges in order, one at a time, each sent only after the previous reply was read and the add-on closed the connection.
- Before each exchange, the recorder waits until Anki's windows are settled: the Browse window, when open, has run the search its search box shows, and its editor has no save pending. AnkiConnect's `guiBrowse` replies before Anki runs that search (Anki runs it after the editor's save), so without the wait a `guiSelectedNotes` that follows reads the selection from before or after the search, by timing (observed: `[]` in 1 recording of 8, `servers/ankiconnect-clients` C9.12). The wait sends no request and changes no state, so it changes no step's output. Meguri's executor does not wait: its GUI interactions are scripted (T7.5).
- **Record twice.** Anki's desktop app reads the clock from many threads, and libfaketime does not hold every read: a read can escape the scenario clock (observed before the recorder's fix: about one recording in a dozen gave a setup card an id from real time). The recorder checks every snapshot and fails such a run as `clock_escaped` (T7.4), but that check covers only new ids and note and card `mod` values; an escaped read that lands anywhere else (another stamp, a time inside a reply) is not seen. So a golden is accepted only when two recordings of the scenario, each in its own fresh container, give byte-identical golden text. A difference writes no golden and fails the record run, naming the scenario; it is recorder drift, to fix in the recorder, never settled by keeping one of the two.

## T17. Harness self-tests

| Check | Expected |
|---|---|
| `servers/ankiconnect/harness/version` (`{"action":"version","version":6}`) and `servers/ankiconnect/harness/ping` (empty body) recorded, then replayed against the recorder and against a stub server that answers each request with the golden's reply | `pass` both |
| two recordings of `harness/version` an hour apart (wall clock) | byte-identical goldens |
| a stub that answers `harness/version` with `{"result":5,"error":null}` | `fail`, first divergence step 0, pointer `/steps/0/reply/body/json/result`, expected 6, actual 5, and the repro command |
| a deviation entry `absent` on `/steps/0/reply/headers/access-control-allow-headers` for a stub that drops that header and changes `content-type` | `fail` with only the `content-type` difference reported; the entry listed as applied |
| a Migaku equivalence scenario whose resulting note is replayed into the anki oracle | `pass`; with one field of the add record mutated before the replay, `fail` on that field's pointer and on `sfld`/`csum` when it is the sort field |
| an oracle scenario whose request body names `"#n9"` with only 2 notes | `error`, `unknown_ref` |
| `addNote` (`#n1`), `deleteNotes ["#n1"]`, then `notesInfo ["#n1"]` | both executors send the deleted id; the reply matches the golden |
| as above with an `addNote` (`#n2`) before the `notesInfo`, recorded | `error`, `reused_ref` (the reference gave `#n2` the freed id) |
| `updateNoteFields` on a deleted `#n1` | the error reads `Note was not found: #n1` on both executors |
| a reply id at a position T10.5 does not list | recorded unchanged; the recorder's warning names the step and pointer |
| the record job with a recorder stub whose second recording differs from its first by one byte | no golden written; the run fails naming the scenario (T16 "Record twice") |
| `harness/tokens` replayed on Meguri's executor | `pass`: the dump's keys and ids are tokens (T10.6), its note type `#m7` (T10.1) |

## T18. Planned harness scenarios

| Scenario | Source | Steps | Golden or expectation |
|---|---|---|---|
| `servers/ankiconnect/harness/version` | oracle | `http` `{json: {action: version, version: 6}}` | recorded: 200, `content-type: application/json`, `access-control-allow-origin: http://localhost`, `access-control-allow-headers: *`, body `{"result": 6, "error": null}` |
| `servers/ankiconnect/harness/ping` | oracle | `http` with no body | recorded: 200, body `{"apiVersion": "AnkiConnect v.6"}` |
| `servers/ankiconnect/harness/tokens` | oracle | setup: one deck `Lang`, note type and two notes; `http` `findNotes` with `query: "deck:Lang"`; `http` `notesInfo` with `notes: ["#n2"]`; `http` `addNote`; `observe: [{db_dump: {tables: [notes, cards]}}]` (not a bare `db_dump`: the note type tables hold the stock note types' rows, R1959) | ids appear as `#n1`, `#n2`, `#c1`…, new note as `#n3`; `mod` as `@0`; in the dump, row keys and `id`, `nid` and `mid` as tokens (`mid` `#m7`, the first setup note type, T10.1), `did` `#d1` (T10.6) |
| `servers/migaku/harness/declared-srs-check` | declared | `http` `GET /srs-check` | 200, `{"ok":true,"version":"4"}` |

(The header values above are what T8 records; the recorded golden is the authority.)

## T19. Open questions

1. Settled: the recorder's driver add-on answers the add-on's permission dialog (T7.2). Observed: `allow`, `deny` and `deny_ignore` each give the reply and the list change T7.2 states, and an ignored origin is denied with no dialog. A dialog with no queued answer holds every request, so `none` stays declared-only.
2. Whether the add-on build of 2025-11-09 is byte-identical to a public upstream commit. The oracle pins upstream release 25.11.9.0, the release named for that date and the last one before it, by its source archive's hash (T9). The user's installed copy no longer exists, so byte identity with it cannot be checked. Its 121 actions match the pinned release exactly (`servers/ankiconnect-envelope` E12). Still open, as a fact that no probe can settle.
3. Settled: two fresh recorders at the same clock start gave byte-identical replies, ids included. Ids follow T10.1's "How the recorder's ids fall": they coincide across kinds and a deleted id is reused, which T10.1's entity rule and error-string rule handle. `#unknown` still needs ids of deleted entities to be at least 1,000,000,000, which holds for any clock after 2001.
4. Settled: the add-on's `cardsInfo` `question` and `answer` start with the note type's CSS in a `<style>` element (Anki 26.9.3 renders them that way), and `servers/ankiconnect-actions` gives Meguri the same start. T10.3 normalizes nothing beyond `conformance/render-ops#Q5.4`, so any remaining difference becomes a deviation entry.
5. Whether the AnkiConnect transcript suite should also record replies with `Origin` values of real client extensions (their extension ids). The inventory uses synthetic extension ids, since the add-on treats every extension id alike (`servers/ankiconnect-clients` C2).

## T20. Meguri's executor

- **Bind modes** (`servers/host-security` H22). Scenarios with `server: ankiconnect` or `migaku`, oracle or declared, run in the **ephemeral** mode: the server under test binds `127.0.0.1:0` only, with no probe and no `[::1]`; the `Host` default of T6.2 and `${port}` use that bound port, which the `Host` check also compares with. Scenarios with `server: host` run in the **fixed** mode: the real binding path of `servers/host-security` H4 on the configured ports (8765 unless T4's `port` says otherwise, and 44432), with the probe and `[::1]`. Each `host` scenario runs in its own network namespace (its own container network with its own loopback), so fixed ports never collide with another run, and `address: non_loopback` reaches that namespace's own interface address. The host is built with the test construction of `servers/host-security` H22, never through the FFI.
- **Clock.** The host's clock is a test clock (`servers/host-security` H21) that is the scenario clock: `advance` moves it; real-time limits (connection timeouts, the 1 s bound of `servers/host-security` H10.4, probe and fetch limits) run in real time, which `wait` lets pass.
- **Interactions.** The host's interactions go to a `TestHost` that T7.2 and T7.5 script, with T7.5's defaults.
- **Order.** Steps run in order. A step starts after the previous one completed, except after an `async` exchange, which runs on while later steps run, until its `await`. A scenario that ends with an `async` exchange not awaited is a run error.

