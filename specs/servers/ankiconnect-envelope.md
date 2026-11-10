# AnkiConnect envelope, policy and action registry

Spec: servers/ankiconnect-envelope
Requirements: R1734, R1735, R1736, R1737, R1738, R1739, R1740, R1741, R1742, R1743, R1744, R1745, R1746, R1747, R1748, R1749, R1750, R1751, R1752, R1753, R1775, R1779, R1780, R1781, R1790 (one action, one op), R1797; AE75, AE76, AE77; supports R1659, R1679, R1795
Anki: 26.09.3
Scenarios: none yet in `conformance/`. The transcript format is `servers/transcripts`; until its runner exists (U136), E19 lists every planned scenario as a request and reply table, to become `conformance/recordings/servers/ankiconnect/{envelope,policy,multi}/` (`servers/transcripts` T2; ids `servers/ankiconnect/<dir>/<name>`).

What the AnkiConnect-compatible server on the AnkiConnect port (default 8765) does with one HTTP request before and after an action runs: the request and reply envelope, API versions, the ping, origin gating and CORS, the API key, `requestPermission`, `apiReflect`, `multi`, read-only mode, the collection wait, the action registry, the activity log and the connection snippet for agents.

Targets: AnkiConnect API version 6, as served by the AnkiConnect add-on build of 2025-11-09 (upstream release 25.11.9.0, 121 actions), which is the reference that U136 records against (R1799). "The reference" below means that build. Everything not marked as a difference behaves as the reference does; E18 lists every difference. The statements about what the reference does in E2 to E12, E15 and E18 were checked against that build running in the transcript oracle, except that E6's "same names for every version" was checked only for the versions E19 uses; where a check changed the text, the section says "observed".

Other files: binding, the `Host` check, body limits, keep-alive and listener status are `servers/host-security` (U135), which also owns the parts this file builds on: the config document and its apply (H10.2, H10.3), the events to Swift (H11.1), the interaction channel (H11.2) and caught handler failures (H9.4). The collection actions and their parameters are `servers/ankiconnect-actions` (U142), the media actions and attachment objects `servers/ankiconnect-media` (U143), the GUI actions and `sync` `servers/ankiconnect-gui` (U146). Transcript format and normalization are `servers/transcripts` (U136).

## E1. Terms

- **Request**: one HTTP request that reaches this server after the `Host` check (`servers/host-security`).
- **Envelope**: the JSON object in the request body: `action`, `version`, `params`, `key` (E4).
- **Action**: a name the registry knows (E12). A **sub-action** is one element of `multi`'s `actions` list (E11).
- **Top-level action**: the action named by the envelope of the request itself.
- **Allowed origin**: a request whose origin passes E7.
- **Bare reply** and **wrapped reply**: E5.
- **Key set**: E8.
- **Collection action**: an action whose registry entry has the `collection` flag (E12); it waits for the collection (E14).

## E2. Transport

- Every HTTP method is accepted and handled the same way, except `OPTIONS` (E7.4). `GET`, `POST`, `PUT` or any other token: only the body matters. A `HEAD` request is handled as a `GET` and its reply carries no body, as HTTP requires (difference: the reference sends the body, E18). This is HTTP framing, not special handling of the method (R1734): the action runs as for any other method, and the reply has the status and headers the same request would get with `GET`, except that `Content-Length` may be absent or give the length of the omitted body. Scenarios do not compare `Content-Length` on `HEAD` replies (`servers/transcripts` T8).
- The request target is ignored: `/`, `/anything` and `/?x=1` are the same request (as in the reference).
- The `Content-Type` request header is ignored. Clients send `application/json` (a browser then sends the E7.4 preflight first; Yomitan does this), `text/plain` (no preflight) or none.
- The body is the bytes HTTP/1.1 delivers: by `Content-Length`, or decoded from `Transfer-Encoding: chunked`. A body longer than the AnkiConnect limit of `servers/host-security` H8 (256 MiB, R1678) is not read further and not parsed: the reply is the error envelope `Meguri: the request body is too large` when the origin is allowed (E7), 403 otherwise (E5.3). Either reply carries `Connection: close` and the connection closes after it (`servers/host-security` H7.3, H8).
- Keep-alive, the 5 s idle timeout and `Connection: close` follow `servers/host-security`. The reference closes the connection after every reply and never answers a second request sent on the same connection (observed with two pipelined requests: one reply, then close); clients of both work with either.

## E3. The order of checks

A request is answered by the first row that applies, top to bottom. Later rows assume the earlier rows did not apply.

| # | Condition | Reply |
|---|---|---|
| 1 | `Host` header refused (`servers/host-security`) | 403, empty body |
| 2 | method `OPTIONS` | the preflight reply (E7.4) |
| 3 | body over the limit (E2) | origin allowed: error envelope `Meguri: the request body is too large`; otherwise 403 |
| 4 | body is empty (zero bytes) | origin allowed: the ping (E5.1); otherwise 403 |
| 5 | body is not a valid envelope (E4) | origin allowed: error envelope with the E4 message; otherwise 403 |
| 6 | origin not allowed and the action is not `requestPermission` | 403 |
| 7 | action `requestPermission` | E9; no key, no read-only gate, no wait |
| 8 | key set and the envelope's `key` does not match (E8) | error envelope `valid api key must be provided` |
| 9 | action not in the registry | error envelope `unsupported action` |
| 10 | read-only mode on and the action is mutating (E13) | error envelope `Meguri: the API is in read-only mode` |
| 11 | a parameter is missing or has the wrong type (E12.2) | error envelope naming the parameter |
| 12 | collection action and the collection is not available within 30 s (E14) | error envelope `collection is not available` |
| 13 | the action fails | error envelope with the action's message (its spec) |
| 14 | otherwise | the action's result, bare or wrapped by `version` (E6) |

- A disallowed origin never learns more than 403, whatever the body holds (rows 3 to 6), except through `requestPermission`.
- The key is checked before the action is looked up (rows 8, 9): with a key set, `{"action": "nope"}` without a key gets `valid api key must be provided`, as in the reference.
- `multi`'s sub-actions go through rows 7 to 14 each (E11); rows 1 to 6 are the outer request's.

## E4. The request envelope

The body is decoded as UTF-8 JSON (R1734). It is valid when all of these hold; otherwise the first failing check gives the message.

| Check | Message (Meguri's wording, E15.2) |
|---|---|
| the bytes are valid UTF-8 and one JSON value, with at most 128 levels of nesting | `invalid request: the body is not valid JSON` |
| the value is an object | `invalid request: the body must be a JSON object` |
| member `action` is present and is a string of at least one character | `invalid request: "action" must be a non-empty string` |
| member `version`, when present, is an integer (E4.1) | `invalid request: "version" must be an integer` |
| member `params`, when present, is an object (`null` is not) | `invalid request: "params" must be an object` |

- `key` is not checked here; it may be any JSON value (E8).
- Unknown members of the envelope are ignored.
- A member repeated in one object counts with its last occurrence, as most JSON decoders do.
- `params` absent means `{}`. `version` absent means 4 (R1736).
- White space around the JSON value is allowed. A body of white space only is not empty: it fails the first check (row 5), it is not a ping.

### E4.1 Integers

- An **integer** is a JSON number with no fractional part: `6`, `6.0`, `6e0` and `-1` are integers; `6.5`, `"6"`, `true` and `null` are not. The reference accepts the same set.
- A number written with digits only (and an optional leading `-`) is an integer whatever its length. A number written with a fraction or an exponent is read as the nearest IEEE 754 double: it is an integer when that double is finite and whole. So `1e400` (it overflows the double range) and `-1e400` are not integers, and `6.0000000000000001` (nearest double 6) is one. The reference decides the same way.
- `version` compares by value, whatever its size: any integer at most 4 is "4 or less", any other is "5 or more" (`99999999999999999999999` is "5 or more").
- Integer parameters of actions (ids, counts) are integers in this sense within the signed 64-bit range; outside it they have the wrong type (E12.2).

## E5. Replies

### E5.1 Successful replies

Every reply in this section is HTTP 200 with these headers (R1739), in addition to what HTTP/1.1 framing needs (`Content-Length`):

| Header | Value |
|---|---|
| `Content-Type` | `application/json` |
| `Access-Control-Allow-Origin` | E7.3 |
| `Access-Control-Allow-Headers` | `*` |

| Reply | Body |
|---|---|
| ping (empty body, R1735) | `{"apiVersion": "AnkiConnect v.6"}`, whatever the method |
| action succeeded, `version` 4 or less | the bare result: `6`, `["Default"]`, `null` |
| action succeeded, `version` 5 or more | `{"result": <result>, "error": null}` |
| any error (E3 rows 3, 5, 8 to 13), every version | `{"result": null, "error": "<message>"}` (R1737) |

- Errors are always wrapped and always HTTP 200, also with `version` 4 or less and for envelope failures (R1737). A bare reply therefore never says whether it failed: clients that need errors send `version` 5 or 6.
- The ping needs no key and works in read-only mode and while the collection is busy.

### E5.2 JSON form

- Bodies are UTF-8 JSON. Members appear in the order this spec writes them (`result` before `error`; `permission`, `requireApikey`, `version`; `scopes`, `actions`).
- The reference writes every non-ASCII character as a `\uXXXX` escape and puts one space after each `,` and `:`. Meguri may write raw UTF-8 and any white space: both decode to the same value, and transcripts compare decoded values (`servers/transcripts`).

### E5.3 403

- A refused request (E3 rows 1, 3 to 6) gets status 403, an empty body (`Content-Length: 0`) and no `Content-Type` and no `Access-Control-*` header (R1745, AE76). Difference: the reference's 403 carries `Access-Control-Allow-Origin` with its fallback value `http://localhost` and `Access-Control-Allow-Headers: *` (E18); observed: no `Content-Type`, no `Content-Length`, an empty body and a closed connection, whatever the request's origin.

## E6. Versions (R1736, R1741)

| Constant | Value |
|---|---|
| default `version` of an envelope or a sub-action | 4 |
| result of the `version` action | `6` |
| `version` in a granted `requestPermission` result | `6` |
| ping text | `AnkiConnect v.6` |

- Action names do not depend on `version`: every action of E12 has the same name and parameters for every version, as in the reference build.
- `version` (action): no parameters; result `6`.

## E7. Origins and CORS

### E7.1 The lists

- **Allow-list** (R1742): a list of origin strings, default `["http://localhost"]`, stored as `ankiconnect.cors_origins`. **Deny-list** (R1749): default `[]`, stored as `ankiconnect.deny_origins`. Both are members of the local config document (R1665, `servers/host-security` H10.2), edited in Settings (R1748) and by `requestPermission` (E9.1). A change applies from the next request on (R1748); a request in progress, `multi` and all its sub-actions included, keeps the config snapshot it started with (`servers/host-security` H10.3): lists, key, read-only mode.
- Entries are compared with the request's `Origin` value exactly: same characters, case included. Settings stores what the user typed, trimmed of surrounding white space; it does not normalize (`https://site.example/` with a slash never matches a browser origin).

### E7.2 Allowed origins

A request's origin is decided by the first matching row:

| # | Request | Allowed |
|---|---|---|
| 1 | no `Origin` header | yes, always (R1744) |
| 2 | more than one `Origin` header | no, whatever the lists hold |
| 3 | `Origin` is the literal `null`, or empty | no, whatever the lists hold, `*` included (R1744) |
| 4 | the allow-list holds `*` | yes (R1742) |
| 5 | `Origin` equals an allow-list entry | yes |
| 6 | the allow-list holds `http://localhost` and `Origin` matches the localhost rule (below) | yes (R1743) |
| 7 | otherwise | no |

The **localhost rule** matches an origin that is one of `http://127.0.0.1`, `https://127.0.0.1`, `http://localhost`, `https://localhost`, optionally followed by `:` and one to five ASCII digits; or that starts with `chrome-extension://`, `moz-extension://` or `safari-web-extension://` (anything may follow).

| Origin | Default allow-list | Allow-list `[]` | Allow-list `["*"]` |
|---|---|---|---|
| (none) | allowed | allowed | allowed |
| `chrome-extension://abc` | allowed | no | allowed |
| `moz-extension://4b1f-…` | allowed | no | allowed |
| `safari-web-extension://x` | allowed | no | allowed |
| `http://127.0.0.1` | allowed | no | allowed |
| `http://127.0.0.1:3000` | allowed | no | allowed |
| `https://127.0.0.1:8443` | allowed | no | allowed |
| `http://localhost` | allowed | no | allowed |
| `http://localhost:8080` | allowed | no | allowed |
| `https://localhost` | allowed | no | allowed |
| `http://127.0.0.1:x` | no | no | allowed |
| `http://[::1]:3000` | no | no | allowed |
| `https://evil.example` | no | no | allowed |
| `null` | no | no | no |

- The deny-list never makes a request disallowed; it only changes `requestPermission`'s answer (E9).
- Card pages in Meguri's card webview do not load under an origin that this rule allows (R1679, U66/U67); the user may add that origin to the allow-list.

### E7.3 `Access-Control-Allow-Origin`

The first matching row gives the value:

| # | Reply | Value |
|---|---|---|
| 1 | 403 | no header (E5.3) |
| 2 | any other reply while the allow-list holds `*` | `*` |
| 3 | request with an allowed `Origin` (E7.2 rows 5, 6) | the request's `Origin`, as received |
| 4 | request with no `Origin` | `*` (R1739; the reference sends `http://localhost`, E18) |
| 5 | request with more than one `Origin` header | `http://localhost` |
| 6 | `requestPermission` from an origin that is not allowed | the request's `Origin`, as received, so the page can read the answer |
| 7 | preflight (E7.4) from an origin that is not allowed | `http://localhost` (the reference's fallback; a browser then refuses the page's request) |

Examples: `Origin: null` with `["*"]` and `deckNames` gets 403 (row 1, no header); `Origin: null` with `["*"]` and `requestPermission` gets `*` (row 2); a preflight from `Origin: null` with `["*"]` gets `*` (row 2).

### E7.4 Preflight (R1746)

An `OPTIONS` request, whatever its origin, target and body, gets HTTP 200 with an empty body (`Content-Length: 0`) and the headers of E5.1: `Content-Type: application/json`, `Access-Control-Allow-Origin` (E7.3) and `Access-Control-Allow-Headers: *`.

- When the request carries `Access-Control-Request-Private-Network` with the value `true`, the reply also carries `Access-Control-Allow-Private-Network: true`. The value is compared exactly, as in the reference (observed: `TRUE` or `false` gets no such header).
- The reply has no `Access-Control-Allow-Methods`, as in the reference; browsers then allow the simple methods, `POST` among them.
- No key, read-only or collection check applies, and the activity log records nothing.

## E8. API key (R1747)

- The setting is empty by default. A **key is set** when Settings holds a non-empty key, from the moment the user saves it: Swift keeps it in the Keychain, sets the non-secret flag `ankiconnect.api_key_required` and passes the key with each apply (`servers/host-security` H10.2, H10.3). While the flag is on and no key was applied in this process (or the Keychain read failed), the key is set but unknown: every action except `requestPermission` gets `valid api key must be provided`, whatever `key` the request sends.
- With a key set, every action except a top-level `requestPermission` must carry a `key` member that is a string equal to the key, character for character. The comparison takes the same time whatever the position of the first difference. Otherwise the reply is `valid api key must be provided` (E3 row 8). `multi` checks the outer envelope's key and each sub-action's own key (E11).
- With no key set, `key` is ignored whatever it holds (`"whatever"`, `null`, `5`). Difference: the reference refuses a request that sends a non-null key when none is configured (E18).
- The ping and the preflight never need a key.
- The key never appears in a reply, the activity log or the agent snippet.

## E9. `requestPermission` (R1749, R1750)

- Only a top-level action (inside `multi` it fails, E11). It needs no key, is not gated by read-only mode and does not wait for the collection. Client `params` are ignored (difference: the reference fails a `requestPermission` that has a `params` member, before any prompt, with its unexpected-keyword text, E18).
- The answer is decided by the first matching row:

| # | Request origin | Result | Lists changed |
|---|---|---|---|
| 1 | allowed (E7.2), no `Origin` included | `{"permission": "granted", "requireApikey": <key set>, "version": 6}` | none |
| 2 | more than one `Origin` header | `{"permission": "denied"}`, no prompt | none |
| 3 | `null` or empty `Origin` | `{"permission": "denied"}`, no prompt | none |
| 4 | on the deny-list | `{"permission": "denied"}`, no prompt | none |
| 5 | otherwise | the prompt (below) | per outcome (E9.1) |

- `requireApikey` is `true` while a key is set (E8), `false` otherwise. The member is spelled `requireApikey` (lower-case `k`) as the reference sends it.
- The result is bare or wrapped by `version` (E6) like any result; `Access-Control-Allow-Origin` is E7.3's.
- **The prompt** (row 5): the core posts one `PermissionPrompt` interaction (`servers/host-security` H11.2) with the payload `{"origin": <the Origin value>}`, a timeout of 120 s on the host's injected clock from the moment it is posted, and the timeout outcome "no outcome" below; the request waits for its outcome, holding no reader, lock or op (H11.2). The app shows a native in-window alert, "<origin> wants to use Meguri through the AnkiConnect API", with Allow, Deny and an "Ignore further requests from this origin" checkbox; while Meguri is in the background it requests attention (Dock bounce) without taking focus (R1749, R1750; UI strings and layout are U144's).

| Outcome | Result | Lists changed |
|---|---|---|
| Allow (checkbox ignored) | granted, as row 1 | origin appended to the allow-list, unless already there |
| Deny with the checkbox | `{"permission": "denied"}` | origin appended to the deny-list, unless already there |
| Deny without the checkbox | `{"permission": "denied"}` | none |
| no outcome within 120 s of posting | `{"permission": "denied"}`; the host posts interaction cancelled, so the app dismisses the alert (H11.2) | none |
| server turned off or app quitting | `{"permission": "denied"}` (the timeout outcome, H11.2, H10.4) | none |

- The `PermissionPrompt` outcome is a JSON object `{"answer": "allow" | "deny", "ignore": <bool>}`. Any other value is not accepted: the resolve returns `false` and the prompt stays open (H11.2).
- One prompt per origin at a time: a `requestPermission` from an origin whose prompt is open joins it. Every joined request ends with that prompt's outcome, its timeout included: the 120 s run from the first request's posting, and a joined request starts no timer and no alert of its own. Prompts of different origins are separate interactions, shown one at a time by the app (U144).
- A waiting client that closes its connection does not remove the prompt. The prompt stays until an outcome or its timeout, and the outcome still changes the lists (E9.1); replies to closed connections are dropped.
- Only the waiting requests wait. Every other request, from any origin, is served meanwhile. Difference: the reference's modal dialog stops all of its request handling until answered (E18).
- An origin appended by Allow is stored exactly as the request sent it, and is allowed from the next request on.

### E9.1 Who changes the lists

- The core makes the change, while it handles the app's resolve call for Allow, or for Deny with `ignore` true, and before that call returns `true`: it takes the current config document, appends the origin to `ankiconnect.cors_origins` (Allow) or `ankiconnect.deny_origins` (Deny with the checkbox) unless the entry is already there, leaves every other member as it is, and writes and swaps the document exactly as an apply does (`servers/host-security` H10.3). Applies and these appends run one at a time, in the order they reach the core.
- Swift, when its resolve returns `true` for such an answer, makes the same append to its own copy of the document (Settings › AnkiConnect, U144) before it applies anything else. A Settings edit made while the prompt was open is therefore applied on top of the append, and the next whole-document apply keeps the entry. An apply that Swift sent before its resolve is processed before the append.
- No outcome, a cancel, Deny without the checkbox and an Allow for an origin already listed write nothing. The reference ends the same way, by another route: it handles one request at a time, so a second `requestPermission` from an origin waits behind the first prompt and is then granted from the updated list without a prompt (observed: two concurrent requests, one prompt, one list entry).

## E10. `apiReflect` (R1751)

Parameters: `scopes` (a list of any values) and `actions` (absent, `null` or a list). Both have the registry type "any" (E12.2): the generic checks never name them, and this table owns their errors.

The first matching row gives the answer:

| # | Input | Result or error |
|---|---|---|
| 1 | `scopes` absent, `null` or not a list | error `scopes has invalid value` |
| 2 | `actions` present and neither `null` nor a list | error `actions has invalid value` |
| 3 | `scopes` does not hold the string `actions` | `{"scopes": []}` (no `actions` member), whatever the items of `actions` are |
| 4 | `actions` a list holding an item that is not a string | error `actions has invalid value` (the reference fails with a language error text, E18) |
| 5 | `actions` absent or `null` | `{"scopes": ["actions"], "actions": [<every registry name>]}`, names sorted by code point (E12.3) |
| 6 | `actions` a list of strings | `{"scopes": ["actions"], "actions": [<the list's items that are registry names, in the list's order, repeats kept>]}` |

- Names are matched exactly; names the reference has and Meguri does not (E12.4) never appear (R1775).
- Examples: `{"scopes": ["actions"], "actions": ["addNote", "guiReviewActive"]}` → `{"scopes": ["actions"], "actions": ["addNote"]}`; `["version", "version", "Version"]` → `["version", "version"]`; `{"scopes": ["other"]}` → `{"scopes": []}`; `{"scopes": ["other"], "actions": [5]}` → `{"scopes": []}` and `{"scopes": ["other"], "actions": "x"}` → `actions has invalid value`, as in the reference. Repeated or extra items of `scopes` do not change the answer: `["actions", "actions", 5]` gives `"scopes": ["actions"]` there too.

## E11. `multi` (R1752, R1753)

Parameter: `actions` (required, a list). Missing: `missing parameter: actions`. Not a list, or an element that is not an object: `invalid parameter: actions`, and no sub-action runs (the reference also fails the whole call for a non-object element, with another text, but only when it reaches that element: the elements before it have already run and their changes stay, E18).

- Each element is a sub-action envelope with the members `action`, `version`, `params` and `key`, read as E4 reads a body. The sub-actions run one after another in list order; each starts after the previous one's reply is final, and sees everything earlier sub-actions committed.
- Each sub-action is answered by the first row that applies:

| # | Condition | Its slot in the result |
|---|---|---|
| 1 | `version` present and not an integer, or `params` present and not an object | `{"result": null, "error": "<E4 message>"}`; it does not run |
| 2 | `action` is `requestPermission` | `{"result": null, "error": "requestPermission cannot be used inside multi"}` |
| 3 to 9 | E3 rows 8 to 14, with the sub-action's own `key`, `action`, `params` and `version` | as there |

- An absent, non-string or empty `action` gets `unsupported action` at row 9 (after the key check), as in the reference.
- A sub-action's reply follows its own `version`, default 4, so a sub-action without `version` gives a bare result (R1752). The outer result, the list of slots, is bare or wrapped by the outer `version`.
- A failing sub-action does not stop the others (R1753). `multi` is not atomic: each sub-action that changes the collection is its own op (E14) and stays committed whatever later sub-actions do.
- `multi` itself is not mutating and is not a collection action; read-only mode and the wait apply to each sub-action separately (E13, E14). The outer request's origin covers every sub-action; there is no origin check per sub-action. Every sub-action uses the outer request's config snapshot (E7.1): its allow-list, key and read-only setting, even when an apply lands while the `multi` runs.
- A sub-action whose handler fails unexpectedly (a panic, E14) gets `{"result": null, "error": "Meguri: internal error"}` in its slot; its op, if any, rolls back, and the following sub-actions run as usual.
- `multi` may contain `multi`; the JSON nesting limit (E4) bounds the depth. A nested `multi` is a sub-action like any other: its slot holds its own result list.
- `{"actions": []}` gives `[]`.

AE75: no key set, the request `{"action": "multi", "version": 6, "params": {"actions": [{"action": "deckNames"}, {"action": "nope", "version": 6}]}}` gets HTTP 200 `{"result": [["Default"], {"result": null, "error": "unsupported action"}], "error": null}` on a collection whose only deck is Default. The first slot is bare because its version defaults to 4.

## E12. The action registry

### E12.1 Entries

One registry lists every action Meguri implements; dispatch, `apiReflect` (E10), read-only mode (E13), the wait (E14) and the agent snippet (E17) all read it, so an action absent from it is unsupported everywhere (R1775). An entry holds:

- the name (case-sensitive);
- its parameters: name, required or optional, type, default, and whether `null` counts as absent (the owning spec lists them);
- flags: `mutating` (refused in read-only mode), `collection` (waits for the collection), `gui` (raises a GUI interaction, `servers/ankiconnect-gui`);
- the spec that owns its behavior.

### E12.2 Parameters (R1740)

- Parameters are the members of `params`. Members the entry does not name are ignored (difference: the reference fails with an "unexpected keyword argument" text, E18).
- A required parameter that is absent: `missing parameter: <name>`. When several are absent, the first in the entry's order is named.
- A parameter whose value has the wrong type: `invalid parameter: <name>`. `null` counts as the wrong type unless the owning spec allows it for that parameter, either as a value or as "absent" (the parameter then takes its default). For a member inside an object parameter the name is the dotted path, in missing and invalid errors alike: `missing parameter: note.deckName`, `invalid parameter: note.fields`.
- Types: integer (E4.1), string, boolean (`true`/`false` only), list (of a stated element type), object, and **any**. No coercion: `"5"` is not an integer, `1` is not a boolean. A parameter of type any is never missing and never of the wrong type for these checks: the action checks it itself and owns the error text (`apiReflect`'s `scopes` and `actions`, E10; `guiBrowse`'s `reorderCards`, `servers/ankiconnect-gui` G3).
- These checks run before the collection wait (E3 row 11). Checks that need the collection (a deck that does not exist) belong to the action and run after it.

### E12.3 The v1 set

73 actions. `m` = mutating, `c` = collection, `g` = gui. Sorted by code point, as `apiReflect` lists them.

| Action | Flags | Spec |
|---|---|---|
| `addNote` | m c | ankiconnect-actions, ankiconnect-media |
| `addNotes` | m c | ankiconnect-actions, ankiconnect-media |
| `addTags` | m c | ankiconnect-actions |
| `apiReflect` | | this file, E10 |
| `areDue` | c | ankiconnect-actions |
| `areSuspended` | c | ankiconnect-actions |
| `canAddNote` | c | ankiconnect-actions |
| `canAddNoteWithErrorDetail` | c | ankiconnect-actions |
| `canAddNotes` | c | ankiconnect-actions |
| `canAddNotesWithErrorDetail` | c | ankiconnect-actions |
| `cardsInfo` | c | ankiconnect-actions |
| `cardsModTime` | c | ankiconnect-actions |
| `cardsToNotes` | c | ankiconnect-actions |
| `changeDeck` | m c | ankiconnect-actions |
| `clearUnusedTags` | m c | ankiconnect-actions |
| `createDeck` | m c | ankiconnect-actions |
| `createModel` | m c | ankiconnect-actions |
| `deckNameFromId` | c | ankiconnect-actions |
| `deckNames` | c | ankiconnect-actions |
| `deckNamesAndIds` | c | ankiconnect-actions |
| `deleteDecks` | m c | ankiconnect-actions |
| `deleteMediaFile` | m c | ankiconnect-media |
| `deleteNotes` | m c | ankiconnect-actions |
| `findCards` | c | ankiconnect-actions |
| `findModelsById` | c | ankiconnect-actions |
| `findModelsByName` | c | ankiconnect-actions |
| `findNotes` | c | ankiconnect-actions |
| `forgetCards` | m c | ankiconnect-actions |
| `getActiveProfile` | | ankiconnect-actions |
| `getDeckStats` | c | ankiconnect-actions |
| `getDecks` | c | ankiconnect-actions |
| `getIntervals` | c | ankiconnect-actions |
| `getMediaDirPath` | c | ankiconnect-media |
| `getMediaFilesNames` | c | ankiconnect-media |
| `getNoteTags` | c | ankiconnect-actions |
| `getNumCardsReviewedByDay` | c | ankiconnect-actions |
| `getNumCardsReviewedToday` | c | ankiconnect-actions |
| `getProfiles` | | ankiconnect-actions |
| `getTags` | c | ankiconnect-actions |
| `guiAddCards` | m c g | ankiconnect-gui |
| `guiBrowse` | c g | ankiconnect-gui |
| `guiEditNote` | c g | ankiconnect-gui |
| `guiSelectCard` | g | ankiconnect-gui |
| `guiSelectedNotes` | g | ankiconnect-gui |
| `loadProfile` | | ankiconnect-actions |
| `modelFieldDescriptions` | c | ankiconnect-actions |
| `modelFieldFonts` | c | ankiconnect-actions |
| `modelFieldNames` | c | ankiconnect-actions |
| `modelFieldsOnTemplates` | c | ankiconnect-actions |
| `modelNameFromId` | c | ankiconnect-actions |
| `modelNames` | c | ankiconnect-actions |
| `modelNamesAndIds` | c | ankiconnect-actions |
| `modelStyling` | c | ankiconnect-actions |
| `modelTemplates` | c | ankiconnect-actions |
| `multi` | | this file, E11 |
| `notesInfo` | c | ankiconnect-actions |
| `notesModTime` | c | ankiconnect-actions |
| `reloadCollection` | | ankiconnect-actions |
| `removeTags` | m c | ankiconnect-actions |
| `replaceTags` | m c | ankiconnect-actions |
| `replaceTagsInAllNotes` | m c | ankiconnect-actions |
| `requestPermission` | | this file, E9 |
| `retrieveMediaFile` | c | ankiconnect-media |
| `setDueDate` | m c | ankiconnect-actions |
| `storeMediaFile` | m c | ankiconnect-media |
| `suspend` | m c | ankiconnect-actions |
| `suspended` | c | ankiconnect-actions |
| `sync` | m c | ankiconnect-gui |
| `unsuspend` | m c | ankiconnect-actions |
| `updateNote` | m c | ankiconnect-actions, ankiconnect-media |
| `updateNoteFields` | m c | ankiconnect-actions, ankiconnect-media |
| `updateNoteTags` | m c | ankiconnect-actions |
| `version` | | this file, E6 |

- `guiAddCards` is mutating because it stores attachment media before the panel opens (`servers/ankiconnect-gui`). `sync` is mutating because the exchange applies incoming changes and uploads local ones.
- `guiBrowse` is not mutating, but its `reorderCards` writes the Browse sort keys; read-only mode refuses a `guiBrowse` that carries `reorderCards` inside the action (`servers/ankiconnect-gui` G3), with the E13 text.
- `getProfiles`, `getActiveProfile`, `loadProfile` and `reloadCollection` answer without touching the collection (their results are `servers/ankiconnect-actions`').

### E12.4 Reference actions Meguri does not implement

These 48 names of the reference get `unsupported action` and never appear in `apiReflect` (R1775): `answerCards`, `cardReviews`, `cloneDeckConfigId`, `exportPackage`, `findAndReplaceInModels`, `getCollectionStatsHTML`, `getDeckConfig`, `getEaseFactors`, `getLatestReviewID`, `getReviewsOfCards`, `guiAnswerCard`, `guiCheckDatabase`, `guiCurrentCard`, `guiDeckBrowser`, `guiDeckOverview`, `guiDeckReview`, `guiExitAnki`, `guiImportFile`, `guiPlayAudio`, `guiReviewActive`, `guiSelectNote`, `guiShowAnswer`, `guiShowQuestion`, `guiStartCardTimer`, `guiUndo`, `importPackage`, `insertReviews`, `modelFieldAdd`, `modelFieldRemove`, `modelFieldRename`, `modelFieldReposition`, `modelFieldSetDescription`, `modelFieldSetFont`, `modelFieldSetFontSize`, `modelTemplateAdd`, `modelTemplateRemove`, `modelTemplateRename`, `modelTemplateReposition`, `relearnCards`, `removeDeckConfigId`, `removeEmptyNotes`, `saveDeckConfig`, `setDeckConfigId`, `setEaseFactors`, `setSpecificValueOfCard`, `updateModelStyling`, `updateModelTemplates`, `updateNoteModel`.

## E13. Read-only mode (R1779)

- A setting, off by default. While it is on, a mutating action (E12.3) gets `Meguri: the API is in read-only mode` at E3 row 10 and changes nothing: no collection write, no media file written.
- Non-mutating actions, the ping, the preflight and `requestPermission` (which edits Meguri's settings, not the collection) are unaffected.
- Inside `multi` only the mutating sub-actions fail (E11). The setting is read from the request's config snapshot (E7.1), so every sub-action of one `multi` sees the same value.

## E14. The collection, concurrency and atomicity

- A collection action first waits for the collection (`engine/lifecycle#L6`, wrapped by `servers/host-security`) up to 30 s (R1797). When the wait ends without it: `collection is not available`, and the action did nothing. Each sub-action of `multi` waits on its own.
- A request is answered in every case (R1659): it waits, then succeeds or gets an error. It is never dropped.
- An action that changes the collection is exactly one op with source `LocalApi` (`engine/op-model#O1`): it commits whole or not at all (R1790). When the op is refused at commit because the collection became unavailable or changed generation, the reply is `collection is not available` and nothing changed.
- Read actions may run at the same time as each other and as writes (R1790). Write ops commit one at a time, in arrival order, in the same queue as UI writes (`engine/op-model#O2`).
- `LocalApi` ops are not recorded in the undo history and truncate it where they intersect it (`engine/undo#M7`, R1795, R1796).
- A handler that fails unexpectedly (a panic) is caught for that request only (`servers/host-security` H9.4): the AnkiConnect error responder answers the error envelope `Meguri: internal error` (HTTP 200, E5.1 headers, `Connection: close`) and the server keeps serving. Inside `multi` the failure is caught for that sub-action's slot only (E11).

## E15. Error strings

### E15.1 Strings clients key on (R1738)

Exact text, shared by every action that can produce them. The ones this file produces:

| Message | When |
|---|---|
| `unsupported action` | E3 row 9, E11, E12.4 |
| `valid api key must be provided` | E3 row 8, E8 |
| `collection is not available` | E14 |
| `scopes has invalid value` | E10 |
| `actions has invalid value` | E10 |

The other R1738 strings (`model was not found: <name>`, `deck was not found: <name>`, `note was not found: <id>`, `cannot create note because it is a duplicate`, `cannot create note because it is empty`) belong to the actions that produce them (`servers/ankiconnect-actions`, `servers/ankiconnect-gui`). R1738 writes the note string in lower case; the reference build writes `Note was not found: <id>` (and `Card was not found: <id>`) with a capital letter. `servers/ankiconnect-actions` uses the reference's capital form for its note actions, pending its A21 Q2. `guiEditNote` is not affected by that decision: the reference answers an unknown id there with Anki's own not-found message, not with this text, so Meguri uses R1786's lower-case literal (`servers/ankiconnect-gui` G7, G11). Observed: `updateNoteFields` with an unknown id replies `Note was not found: <id>`, and `modelNameFromId` with an unknown id `model was not found: <id>`. See Open questions.

### E15.2 Meguri's own strings

| Message | When |
|---|---|
| `Meguri: the API is in read-only mode` | E13 (R1779) |
| `Meguri: the request body is too large` | E2, E3 row 3 |
| `Meguri: internal error` | E14 |
| `invalid request: the body is not valid JSON` | E4 |
| `invalid request: the body must be a JSON object` | E4 |
| `invalid request: "action" must be a non-empty string` | E4 |
| `invalid request: "version" must be an integer` | E4, E11 row 1 |
| `invalid request: "params" must be an object` | E4, E11 row 1 |
| `missing parameter: <name>` | E12.2 |
| `invalid parameter: <name>` | E12.2, E11 |
| `requestPermission cannot be used inside multi` | E11 row 2 |

- The reference's texts for the E4 and E12.2 cases are its language runtime's messages (a JSON decoder's, a schema validator's, a function-call error). Meguri does not reproduce them; transcripts compare these replies by shape only (an error envelope with a non-empty string, E18).

## E16. Activity log (R1780)

- The core keeps the newest 1,000 entries in memory, oldest dropped first. One entry is added for each top-level request answered with an envelope or with a 403 of E3 rows 3 to 6 (not for the ping, the preflight or a `Host` refusal), and one for each sub-action of a `multi`.
- **When entries are added.** An entry is added when its reply is final: a sub-action's when its slot is final, a `multi`'s own entry when the `multi`'s reply is ready. So the entries of one `multi` appear in execution order, each sub-action before the `multi` that contains it, at every nesting level: `multi[version, multi[deckNames]]` adds `version`, `deckNames`, the inner `multi`, the outer `multi`. Settings shows the entries in whatever order U144 chooses.
- The log holds request entries only. It has no other kind of entry: the ingest notices of the external media folder are a separate host event (`servers/ankiconnect-media` X11.9, `servers/host-security` H11.1); whether the log should also list them is `servers/ankiconnect-media` X15 Q8.
- An entry holds, as a JSON object (the members are Meguri's):

| Member | Value |
|---|---|
| `seq` | an integer, 1 for the first entry ever added and one more for each next entry; kept in the store with Keep log on, so it keeps increasing across launches |
| `time_ms` | Unix time in milliseconds, wall clock: when the request's body had been read, or, for a body that is not read (E2's over-limit refusal), when the request head had been read; for a sub-action, when it started |
| `origin` | the `Origin` value (the first one when there are several), or `null` when the request had none (shown as "no origin") |
| `action` | the envelope's `action` when it is a string, otherwise `""` (a 403 or envelope error before an action was read) |
| `outcome` | `ok`, `error` or `refused` (403) |
| `duration_ms` | milliseconds from `time_ms` to the reply being ready, waits included |
| `affected` | an integer: the number of collection objects (notes, cards, decks, note types, tags, media files) the action changed, as the owning spec's table gives it for a successful write (`servers/ankiconnect-actions` A3, `servers/ankiconnect-media` X2); 0 for reads, errors and refusals |

- `affected` for the actions of this file and `servers/ankiconnect-gui`: `multi`'s own entry 0 (its sub-actions count in their own entries); `version`, `apiReflect`, `requestPermission` 0; `guiAddCards` the number of attachment files it stored; the other GUI actions 0; `sync` 0 (what an exchange changes is reported by sync status, not here).
- A `multi` whose own reply is its result list is `ok`, whatever its slots hold; it is `error` only when it failed as a whole (E11's parameter errors, the key).
- No field content, no parameters, no key and no error text are stored, so nothing from the collection appears in the log.
- Each added entry is posted to the app in the host event "activity appended" (`servers/host-security` H11.1), whose payload is the list of entries added since the previous such event, in the order added, each in the shape above.
- While "Keep log" (`ankiconnect.keep_log`) is off, the default, the log lives only in memory and is empty at the next launch. While it is on, each entry is also written to the integrations store, in a table that U141's migration adds (`servers/host-security` H10.1) holding the members above, and the newest 1,000 are loaded at launch. Turning it on writes the entries already in memory to the store at once. Turning it off deletes the stored entries; the in-memory log stays until quit.

## E17. Connection info for agents (R1781)

The core builds the Markdown snippet that "Copy connection info for agents" copies, from the current settings and the registry. Its lines, in order (`<…>` filled in):

```
## Meguri AnkiConnect API

- Endpoint: `http://127.0.0.1:<port>` (send JSON in the request body; this Mac only)
- API version: 6 (put `"version": 6` in every request)
- API key: <required: ask the user for it and send it as "key" | not required>
- Read-only mode: <on: actions that change the collection fail | off>
- Actions (<count>): `<name>`, `<name>`, …
- Notes:
  - Changes made through the API cannot be undone in Meguri; backups and the activity log are the way back.
  - Only clients on this Mac can connect (127.0.0.1 and [::1]).
  - <media folder set: `getMediaDirPath` returns a folder outside the app; files written there become collection media within 2 s. | no media folder set: `getMediaDirPath` fails until the user chooses a media folder in Meguri Settings › AnkiConnect.>
```

- `<port>` is the configured AnkiConnect port; the action list is every registry name in E12.3 order. The key itself is never included.
- The media line depends on whether `ankiconnect.media_dir` is set (`servers/host-security` H10.2, `servers/ankiconnect-media` X1, X8), read when the snippet is built.
- The snippet is an English, user-facing string assembled by the core; its wording may change without a spec change, but the facts listed (endpoint, version 6, key requirement, read-only state, the action names, the three notes) must be present.

## E18. Differences from AnkiConnect

Each needs an entry in `conformance/deviations.yaml` once the transcripts exist (U136), naming the test that asserts Meguri's behavior.

| Case | Reference | Meguri | Req |
|---|---|---|---|
| 403 headers | `Access-Control-Allow-Origin: http://localhost`, `Access-Control-Allow-Headers: *` | no CORS headers | R1745, AE76 |
| `Access-Control-Allow-Origin` with no `Origin` | `http://localhost` | `*` | R1739 |
| localhost rule | only `http://127.0.0.1`, `https://127.0.0.1` and `http://127.0.0.1:` followed by anything, plus the extension prefixes | also `http(s)://localhost[:port]` and `https://127.0.0.1:port`; a port must be digits | R1743 |
| `Origin: null` or an empty `Origin` (`Origin: `) with `*` in the allow-list | allowed | 403 | R1744 |
| more than one `Origin` header | the last value counts | 403; `requestPermission` denied without a prompt (E9 row 2) | R1744 |
| `requestPermission` with `Origin: null` (no `*`) | the prompt; Allow appends `null` to its allow-list, after which every request with `Origin: null` is allowed (observed); Deny with the checkbox deny-lists `null`; `Access-Control-Allow-Origin: null` | denied, no prompt (E9 row 3) | R1744, R1749 |
| `requestPermission` with an empty `Origin` (no `*`), sent as `Origin: ` with one space after the colon | the prompt; Allow appends the empty string to its allow-list, after which every request with an empty `Origin` is allowed, with an empty `Access-Control-Allow-Origin` (observed); Deny with the checkbox appends nothing; `Access-Control-Allow-Origin` empty | denied, no prompt (E9 row 3) | R1744, R1749 |
| an `Origin` header line without a space after the colon (`Origin:` with nothing after it, `Origin:https://evil.example`) | the line is not read (`servers/host-security` H16), so the request counts as having no `Origin`: it is served, `requestPermission` is granted with no prompt, and `Access-Control-Allow-Origin` is `http://localhost` (observed); a value after two spaces keeps one leading space and matches no entry (observed: `Origin:  http://localhost` gets 403) | the header is read as HTTP defines it, without the white space around the value: `Origin:` is an empty `Origin` (E7.2 row 3) and `Origin:https://evil.example` is that origin | R1744, R1677 |
| `requestPermission` with `Origin: null` while the allow-list holds `*` | granted | denied, no prompt (E9 row 3) | R1744 |
| JSON tokens `NaN`, `Infinity`, `-Infinity`, and nesting deeper than 128 levels | accepted by its decoder | `invalid request: the body is not valid JSON` | R1734 |
| sub-action check order | key, then action lookup, then the call (a bad `version` or `params` shows only when the action runs) | `version` and `params` first (E11 row 1), then key and lookup | R1752 |
| `key` other than `null` sent while no key is configured (`""`, `false` and `0` included) | `valid api key must be provided` | ignored | R1747 |
| unknown `params` members (also on `requestPermission`, before any prompt) | error (unexpected keyword argument) | ignored | R1740 |
| texts of envelope and parameter errors (E15.2) | language runtime messages | Meguri's strings | R1737, R1740 |
| non-string item in `apiReflect` `actions` while `scopes` holds `actions` | language runtime error text | `actions has invalid value` | R1751 |
| sub-action with a non-integer `version` | the action runs, then the reply fails | refused before it runs | R1752 |
| non-object element of `multi`'s `actions` | whole call fails, runtime text, after the elements before it ran (their changes stay) | whole call fails, `invalid parameter: actions`; nothing runs | R1753 |
| `requestPermission` inside `multi` | fails with a missing-argument text | `requestPermission cannot be used inside multi` | R1749 |
| E12.4 actions | implemented | `unsupported action` | R1775 |
| permission prompt | modal; every request waits, the ping included; no timeout | per request; 120 s; others served | R1750 |
| `HEAD` request | the reply carries the body | no body | R1734 |
| chunked request body | ignored (read as empty: ping) | decoded | R1734 |
| connection reuse | closed after each reply | keep-alive (`servers/host-security`) | R1677 |
| read-only mode, activity log, agent snippet, body limit | none | E13, E16, E17, E2 | R1678, R1779–R1781 |

## E19. Scenarios

Planned transcript scenarios. "Rec." means the reply must equal the reference recording, after `servers/transcripts` normalization (T10) and comparison (T11), with the E18 entries named in the row as `deviations.yaml` transforms (T12): "ACAO per E18" is a `replace` of the `access-control-allow-origin` header, "headers per E18" an `absent` on both CORS headers, "text per E18" an `any_string` on `/body/json/error`. "Meguri" means the expectation is Meguri's own (a deviation, a check of Meguri's own state such as the config document, or an app interaction the recorder cannot drive: no answer, timeouts, joined prompts, closed waiters, resolve calls): a declared scenario (`servers/transcripts` T13, with T7.2 and T7.3 steps) or a test in `core/crates/meguri-servers/tests/` (U141). Unless a row says otherwise: default settings (allow-list `["http://localhost"]`, empty deny-list, no key, read-only off), method `POST`, no `Origin`, a fresh collection whose only deck is `Default`, and replies HTTP 200 with the E5.1 headers. `ACAO` is `Access-Control-Allow-Origin`.

### E19.1 `envelope/`

| Id | Request body (or note) | Reply | Kind |
|---|---|---|---|
| `ping-get` | `GET`, no body | `{"apiVersion": "AnkiConnect v.6"}` | Rec. (ACAO per E18) |
| `ping-post` | empty body | `{"apiVersion": "AnkiConnect v.6"}` | Rec. |
| `ping-whitespace` | ` ` (one space) | `{"result": null, "error": <E4 JSON message>}` | Rec. (text per E18) |
| `version-default` | `{"action": "version"}` | `6` | Rec. |
| `version-4` | `{"action": "version", "version": 4}` | `6` | Rec. |
| `version-5` | `{"action": "version", "version": 5}` | `{"result": 6, "error": null}` | Rec. |
| `version-6` | `{"action": "version", "version": 6}` | `{"result": 6, "error": null}` | Rec. |
| `version-float` | `{"action": "version", "version": 6.0}` | `{"result": 6, "error": null}` | Rec. |
| `version-negative` | `{"action": "version", "version": -1}` | `6` | Rec. |
| `version-string` | `{"action": "version", "version": "6"}` | error envelope, `"version" must be an integer` | Rec. (text) |
| `invalid-json` | `{"action":` | error envelope, not valid JSON | Rec. (text) |
| `not-utf8` | the bytes `FF FE` | error envelope, not valid JSON | Rec. (text) |
| `root-array` | `[]` | error envelope, must be a JSON object | Rec. (text) |
| `action-missing` | `{"version": 6}` | error envelope, `"action"` message | Rec. (text) |
| `action-empty` | `{"action": ""}` | error envelope, `"action"` message | Rec. (text) |
| `action-number` | `{"action": 5}` | error envelope, `"action"` message | Rec. (text) |
| `params-list` | `{"action": "version", "params": []}` | error envelope, `"params"` message | Rec. (text) |
| `params-null` | `{"action": "version", "params": null}` | error envelope, `"params"` message | Rec. (text) |
| `unsupported-v6` | `{"action": "nope", "version": 6}` | `{"result": null, "error": "unsupported action"}` | Rec. |
| `unsupported-v4` | `{"action": "nope"}` | `{"result": null, "error": "unsupported action"}` | Rec. |
| `unsupported-case` | `{"action": "DeckNames", "version": 6}` | `unsupported action` envelope | Rec. |
| `unsupported-reference` | `{"action": "guiReviewActive", "version": 6}` | `unsupported action` envelope | Meguri (E12.4) |
| `extra-param` | `{"action": "version", "version": 6, "params": {"foo": 1}}` | `{"result": 6, "error": null}` | Meguri (E18) |
| `missing-param` | `{"action": "multi", "version": 6, "params": {}}` | `{"result": null, "error": "missing parameter: actions"}` | Rec. (text) |
| `target-ignored` | `POST /some/path?x=1`, `{"action": "version", "version": 6}` | `{"result": 6, "error": null}` | Rec. |
| `method-put` | `PUT`, `{"action": "version", "version": 6}` | `{"result": 6, "error": null}` | Rec. |
| `text-plain` | `Content-Type: text/plain`, `{"action": "version", "version": 6}` | `{"result": 6, "error": null}` | Rec. |
| `json-content-type` | `Origin: chrome-extension://abc`: first `OPTIONS` with `Access-Control-Request-Headers: content-type`, then `POST` with `Content-Type: application/json`, `{"action": "version", "version": 2}` (Yomitan's shape) | the preflight of E7.4 with ACAO echoed; then bare `6`, ACAO echoed | Rec. |
| `version-big-digits` | `{"action": "version", "version": 99999999999999999999999}` | `{"result": 6, "error": null}` | Rec. |
| `version-overflow` | `{"action": "version", "version": 1e400}` | error envelope, `"version" must be an integer` | Rec. (text) |
| `nan-token` | `{"action": "version", "version": 6, "x": NaN}` | error envelope, not valid JSON | Meguri (E18) |
| `chunked` | chunked body `{"action": "version", "version": 6}` | `{"result": 6, "error": null}` | Meguri (E18) |
| `head-request` | `HEAD`, `{"action": "version", "version": 6}` | 200, E5.1 headers, no body; `Content-Length` absent or the length of the body the same request gets with `GET`, not compared | Meguri (E18) |
| `duplicate-member` | `{"action": "nope", "action": "version", "version": 6}` | `{"result": 6, "error": null}` | Rec. |
| `body-too-large` | `Content-Length` of the limit + 1 | `{"result": null, "error": "Meguri: the request body is too large"}`, body not read | Meguri |
| `deck-names-v6` | `{"action": "deckNames", "version": 6}` | `{"result": ["Default"], "error": null}` | Rec. |

### E19.2 `policy/`

| Id | Setup and request | Reply | Kind |
|---|---|---|---|
| `origin-extension` | `Origin: chrome-extension://abc`, `deckNames` v6 | 200, ACAO `chrome-extension://abc` | Rec. |
| `origin-none` | no `Origin`, `deckNames` v6 | 200 | Rec. (ACAO per E18) |
| `origin-evil` | `Origin: https://evil.example`, `deckNames` v6 | 403, empty body, no CORS headers | Rec. (headers per E18) |
| `origin-null` | `Origin: null`, `deckNames` v6 | 403 | Rec. (headers) |
| `origin-127-port` | `Origin: http://127.0.0.1:3000`, `deckNames` v6 | 200, ACAO echoed | Rec. |
| `origin-safari` | `Origin: safari-web-extension://x`, `deckNames` v6 | 200, ACAO echoed | Rec. |
| `origin-https-localhost` | `Origin: https://localhost`, `deckNames` v6 | 200, ACAO echoed | Meguri (E18) |
| `origin-localhost-port` | `Origin: http://localhost:8080`, `deckNames` v6 | 200, ACAO echoed | Meguri (E18) |
| `origin-127-junk-port` | `Origin: http://127.0.0.1:x`, `deckNames` v6 | 403 | Meguri (E18) |
| `list-empty` | allow-list `[]`; `Origin: chrome-extension://abc` | 403 | Rec. (headers) |
| `list-empty-no-origin` | allow-list `[]`; no `Origin` | 200 | Rec. (ACAO) |
| `list-star` | allow-list `["*"]`; `Origin: https://evil.example` | 200, ACAO `*` | Rec. |
| `list-star-null` | allow-list `["*"]`; `Origin: null` | 403 | Meguri (E18) |
| `list-star-empty-origin` | allow-list `["*"]`; `Origin: ` (one space after the colon, then nothing) | 403 | Meguri (E18) |
| `origin-twice` | two headers `Origin: chrome-extension://abc` and `Origin: https://evil.example`; `deckNames` v6 | 403, no CORS headers | Meguri (E18) |
| `permission-origin-twice` | the two headers of `origin-twice`; `requestPermission` v6 | `{"result": {"permission": "denied"}, "error": null}`, ACAO `http://localhost`, no prompt | Meguri (E18) |
| `list-star-permission-null` | allow-list `["*"]`; `Origin: null`; `requestPermission` v6 | denied, ACAO `*`, no prompt | Meguri (E18) |
| `list-exact` | allow-list `["http://localhost", "https://site.example"]`; `Origin: https://site.example` | 200, ACAO `https://site.example` | Rec. |
| `refused-ping` | `Origin: https://evil.example`, empty body | 403 | Rec. (headers) |
| `refused-bad-json` | `Origin: https://evil.example`, body `{` | 403 | Rec. (headers) |
| `preflight` | `OPTIONS`, `Origin: chrome-extension://abc` | 200, empty body, `Content-Type: application/json`, ACAO echoed, `Access-Control-Allow-Headers: *`, no Allow-Methods | Rec. |
| `preflight-pna` | as `preflight` plus `Access-Control-Request-Private-Network: true` | as `preflight` plus `Access-Control-Allow-Private-Network: true` | Rec. |
| `preflight-disallowed` | `OPTIONS`, `Origin: https://evil.example` | 200, empty body, ACAO `http://localhost` | Rec. |
| `preflight-pna-case` | as `preflight` plus `Access-Control-Request-Private-Network: TRUE` | as `preflight`, no `Access-Control-Allow-Private-Network` | Rec. |
| `permission-allowed` | `Origin: chrome-extension://abc`, `{"action": "requestPermission", "version": 6}` | `{"result": {"permission": "granted", "requireApikey": false, "version": 6}, "error": null}` | Rec. |
| `permission-v4` | no `Origin`, `{"action": "requestPermission"}` | `{"permission": "granted", "requireApikey": false, "version": 6}` | Rec. |
| `permission-key` | key `k1`; no `Origin`; `requestPermission` v6 without `key` | granted with `"requireApikey": true` | Rec. |
| `permission-deny-listed` | deny-list `["https://evil.example"]`; that origin; `requestPermission` v6 | `{"result": {"permission": "denied"}, "error": null}`, ACAO `https://evil.example`, no prompt | Rec. |
| `permission-null` | `Origin: null`; `requestPermission` v6 | `{"result": {"permission": "denied"}, "error": null}`, no prompt | Meguri (E18) |
| `permission-empty-origin` | `Origin: ` (one space after the colon, then nothing); `requestPermission` v6 | `{"result": {"permission": "denied"}, "error": null}`, no prompt | Meguri (E18) |
| `permission-params` | `Origin: https://site.example`; `requestPermission` v6 with `params: {"x": 1}`; test host answers Allow | granted | Meguri (E18) |
| `permission-allow` | `Origin: https://site.example`; `permission: allow`; `requestPermission` v6; then `deckNames` v6 from it | granted; then 200 `["Default"]`, ACAO `https://site.example` | Rec. |
| `permission-allow-lists` | as `permission-allow`, then a `config` step (`servers/transcripts` T7.3) | the allow-list ends with `https://site.example`; the deny-list is unchanged | Meguri (Meguri's config document) |
| `permission-deny-ignore` | `Origin: https://site.example`; `permission: deny_ignore`; `requestPermission` v6; then `requestPermission` again with no scripted answer | both `{"permission": "denied"}`; the second raises no prompt (a prompt would be a run error, T7.2) | Rec. |
| `permission-deny-ignore-lists` | as `permission-deny-ignore`, then a `config` step | the deny-list holds the origin (AE76); the allow-list is unchanged | Meguri (Meguri's config document) |
| `permission-deny` | `Origin: https://site.example`; `permission: deny` before each of two `requestPermission` v6 | both denied; each consumed its scripted answer, so the second raised a prompt of its own (T7.2) | Rec. |
| `permission-timeout` | no outcome; injected clock passes 120 s | denied; prompt removed; lists unchanged | Meguri |
| `permission-coalesced` | two `requestPermission` from one origin at once; Allow | one prompt; both granted | Meguri |
| `permission-coalesced-timeout` | `requestPermission` from one origin; clock +100 s; a second one from it; clock +20 s | both denied at 120 s after the first; one prompt, cancelled once | Meguri |
| `permission-closed-waiter` | `requestPermission` from `https://site.example`; the client closes; Allow | the allow-list ends with `https://site.example`; no reply is sent | Meguri |
| `permission-bad-outcome` | a prompt open; the test host resolves with `{"answer": "maybe"}`, then Allow | the first resolve returns `false`, the prompt stays; then granted | Meguri |
| `permission-apply-race` | a prompt open; Settings applies an edit adding `https://other.example`; then Allow from `https://site.example`; Swift applies its copy again | the allow-list holds both origins | Meguri |
| `permission-others-served` | a prompt open; `deckNames` from `chrome-extension://abc` | answered at once | Meguri |
| `key-missing` | key `k1`; `deckNames` v6 without `key` | `valid api key must be provided` (AE77) | Rec. |
| `key-wrong` | key `k1`; `"key": "k2"` | `valid api key must be provided` | Rec. |
| `key-number` | key `k1`; `"key": 1` | `valid api key must be provided` | Rec. |
| `key-right` | key `k1`; `"key": "k1"` | `{"result": ["Default"], "error": null}` | Rec. |
| `key-before-lookup` | key `k1`; `{"action": "nope", "version": 6}` | `valid api key must be provided` | Rec. |
| `key-unset-supplied` | no key; `deckNames` v6 with `"key": "whatever"` | `{"result": ["Default"], "error": null}` (AE77) | Meguri (E18) |
| `key-pending` | "key required" on, no key passed to the core; `deckNames` v6 with any `key` | `valid api key must be provided`; `requestPermission` still granted | Meguri |
| `read-only` | read-only on; `addNote` v6 with a valid note; then `deckNames` v6 | `Meguri: the API is in read-only mode`, no note added; then `["Default"]` | Meguri |
| `busy-timeout` | collection held `ExclusiveBusy` 31 s; `deckNames` v6 | after 30 s, `collection is not available` | Meguri |
| `busy-short` | `ExclusiveBusy` 3 s; `deckNames` v6 | after about 3 s, `["Default"]` | Meguri |
| `slot-held` | Check Database holds the long-operation slot 31 s; `deckNames` v6 | `collection is not available` | Meguri |
| `busy-non-collection` | `ExclusiveBusy`; `version` v6 | `6` at once | Meguri |

### E19.3 `multi/` and `apiReflect`

| Id | Request (`params` of `multi` unless noted) | Reply | Kind |
|---|---|---|---|
| `ae75` | outer v6; `[{"action": "deckNames"}, {"action": "nope", "version": 6}]` | `{"result": [["Default"], {"result": null, "error": "unsupported action"}], "error": null}` | Rec. |
| `outer-v4` | outer no version; `[{"action": "version", "version": 6}]` | `[{"result": 6, "error": null}]` | Rec. |
| `empty` | outer v6; `[]` | `{"result": [], "error": null}` | Rec. |
| `not-atomic` | outer v6; `[{"action": "createDeck", "params": {"deck": "A"}}, {"action": "nope"}, {"action": "deckNames"}]` | slots: A's id, `unsupported action` envelope, `["A", "Default"]`; deck A exists after | Rec. |
| `sub-key` | key `k1`; outer `"key": "k1"`; `[{"action": "version"}, {"action": "version", "key": "k1"}]` | `[{"result": null, "error": "valid api key must be provided"}, 6]` | Rec. |
| `outer-key` | key `k1`; outer without key | `valid api key must be provided` | Rec. |
| `sub-readonly` | read-only on; `[{"action": "deckNames"}, {"action": "createDeck", "params": {"deck": "A"}}]` | `[["Default"], {"result": null, "error": "Meguri: the API is in read-only mode"}]` | Meguri |
| `sub-snapshot` | `[{"action": "deckNames"}, {"action": "createDeck", "params": {"deck": "A"}}]` with a test hook that applies read-only on between the two sub-actions | both run; deck A exists; the next request is refused by read-only | Meguri |
| `sub-panic` | `[{"action": "version"}, <test-hooks action that panics>, {"action": "version"}]` | `[6, {"result": null, "error": "Meguri: internal error"}, 6]` | Meguri |
| `sub-bad-version` | `[{"action": "createDeck", "version": "6", "params": {"deck": "A"}}]` | slot `"version" must be an integer`; no deck A | Meguri (E18) |
| `sub-permission` | `[{"action": "requestPermission"}]` | slot `requestPermission cannot be used inside multi` | Meguri (E18) |
| `non-object-entry` | outer v6; `[1]` | `{"result": null, "error": "invalid parameter: actions"}` | Meguri (E18) |
| `non-object-entry-later` | outer v6; `[{"action": "createDeck", "params": {"deck": "B"}}, 1]` | `{"result": null, "error": "invalid parameter: actions"}`; no deck B | Meguri (E18) |
| `nested` | outer v6; `[{"action": "multi", "params": {"actions": [{"action": "version"}]}}]` | `{"result": [[6]], "error": null}` | Rec. |
| `reflect-all` | `{"action": "apiReflect", "version": 6, "params": {"scopes": ["actions"], "actions": null}}` | `{"scopes": ["actions"], "actions": [<the 73 names of E12.3 in order>]}` wrapped | Meguri (E12.4) |
| `reflect-filter` | `apiReflect` v6, `actions: ["addNote", "guiReviewActive"]` | `{"scopes": ["actions"], "actions": ["addNote"]}` wrapped | Meguri (E12.4) |
| `reflect-repeat` | `actions: ["version", "version", "Version"]` | `["version", "version"]` in `actions` | Rec. |
| `reflect-no-scope` | `scopes: ["other"]` | `{"scopes": []}` wrapped | Rec. |
| `reflect-no-scope-bad-item` | `scopes: ["other"], actions: [5]` | `{"scopes": []}` wrapped | Rec. |
| `reflect-no-scope-bad-actions` | `scopes: ["other"], actions: "x"` | `actions has invalid value` | Rec. |
| `reflect-scopes-repeat` | `scopes: ["actions", "actions", 5], actions: ["version"]` | `{"scopes": ["actions"], "actions": ["version"]}` wrapped | Rec. |
| `reflect-scopes-null` | `scopes: null` | `scopes has invalid value` | Rec. |
| `reflect-scopes-string` | `scopes: "actions"` | `scopes has invalid value` | Rec. |
| `reflect-scopes-missing` | `params: {}` | `scopes has invalid value` | Rec. |
| `reflect-actions-string` | `scopes: ["actions"], actions: "addNote"` | `actions has invalid value` | Rec. |

### E19.4 Unit tests without transcripts (U141)

- Stand-in actions registered through `test-hooks` (one read, one mutating, one gui) exercise E3, E13 and E14 before U142 and U146 land; U142 replays E19 with the real actions.
- Activity log: 1,001 requests keep the newest 1,000; an `addNote` entry holds no field text; a `multi` of two sub-actions adds three entries, the `multi`'s last; a nested `multi` adds its entries in E16's order; an over-limit body's entry has the time its head was read; the log is empty after a restart with Keep log off and holds the newest 1,000 with it on; turning Keep log on stores the entries already in memory; each "activity appended" event carries the new entries in order.
- Agent snippet media line: with `ankiconnect.media_dir` null it says `getMediaDirPath` fails until a folder is chosen; with a folder it gives the 2 s note.
- Agent snippet: lists the endpoint with the configured port, version 6, the key requirement, the read-only state and exactly the E12.3 names; never the key.
- Fuzz (`ankiconnect_request`): any body, method and header set yields an E5 reply or 403 and never a panic.

## Open questions (not normative)

Probe results against the reference are recorded here; "settled" marks a question a probe answered. A question about what Meguri should do stays open with what the probe showed.

- R1739 says `Access-Control-Allow-Origin` is "<allowed origin or *>". E7.3 reads `*` for requests without `Origin`, which differs from the reference's `http://localhost` on every no-Origin transcript. If the Product Contract owner prefers transcript equality, the row changes to `http://localhost` and the E18 row goes. Observed: the reference sends `http://localhost` on every reply to a request without `Origin` (ping, actions, errors, the preflight and its 403), and `*` on every reply while the allow-list holds `*`, whatever the origin. Still open (policy).
- E9 rows 2 and 3 (no prompt for several `Origin` headers or for `Origin: null`) and E7.2 rows 2 and 3 (both refused even with `*`) extend R1744 to `requestPermission`, to repeated headers and to the `*` entry. Confirm. Observed: with `*` the reference serves `null`, an empty `Origin` and repeated headers alike, and grants `requestPermission` to `null`; without `*`, repeated headers count by their last value (the allow decision, the prompt and the deny-list entry all use it), `null` gets the prompt and Deny with the checkbox deny-lists `null`, and an empty `Origin` gets the prompt but nothing is appended. Still open (policy; E18 lists each case).
- E12.3 makes `sync` and `guiAddCards` mutating, so read-only mode refuses them. Confirm, or make `guiAddCards` mutating only when it carries attachments. The reference has no read-only mode, so no probe applies. Still open.
- E12.3 and `servers/ankiconnect-gui` G3 refuse a `guiBrowse` that carries `reorderCards` in read-only mode, because the sort is a collection config write. Confirm, or let read-only mode skip the sort and still navigate. No probe applies. Still open.
- E15.1: R1738 lists `note was not found: <id>` in lower case; the reference's note actions write `Note was not found: <id>`. The Product Contract owner decides which one those actions send (`servers/ankiconnect-actions` A21 Q2). `guiEditNote` keeps R1786's lower-case text either way, since the reference has no such text there (G7, G11). Observed: `Note was not found: <id>` with a capital letter (`updateNoteFields`); `guiEditNote` with an unknown id answers with Anki's database-inconsistency message that names the id. Still open (policy).
- E16: the external-folder ingest notices are a host event of their own (`servers/ankiconnect-media` X11.9), not activity-log entries. Whether the log should list them too is `servers/ankiconnect-media` X15 Q8.
- Settled: the core's permission append (E9.1) is named in `servers/host-security` H10.3 as a second writer, serialized with Swift's applies.
- Settled by probe: the 121 action names `apiReflect` lists are exactly E12.3's 73 and E12.4's 48, sorted by code point; E4's checks, E4.1's integer edges (`6.0`, `6e0`, `6.0000000000000001` and long digit strings accepted; `6.5`, `1e400`, `-1e400`, `"6"`, `true`, `null` refused), the ping only for an allowed origin, white space not a ping, key before lookup and envelope checks before key, the localhost rule of E18, the preflight headers, the `requestPermission` answers and list updates, `multi`'s slots and `apiReflect`'s table all behave as written. Meguri follows them except where E18 says otherwise.
