# AnkiConnect envelope, policy and registry (U141)

Spec-writer notes for `specs/servers/ankiconnect-envelope.md` E1-E19
(session s-20261110T085429Z-165d83d3, wave 2, package `ankiconnect-envelope`).
No code was copied; behavior is described in own words, and only interface
facts are reproduced (action names, JSON keys, header names, status codes,
port, version numbers and the short error strings listed below). No
transcript exists yet (U136): every "Rec." expectation in E19 comes from
reading the sources below, and the goldens `tools/mg oracle record` writes
once the U136 recorder exists are the authority.

## The reference build

- Target: the AnkiConnect add-on build of 2025-11-09 (121 actions), as
  recorded in the private research dossier 09 (local-data, not quoted here)
  and in the plan (R1799, U136). The exact pin is the oracle maintainer's
  `ADDON_PIN` step.
- Source read: the public upstream repository
  `https://git.sr.ht/~foosoft/anki-connect` (GPL-3.0-or-later, read-only,
  checked out in the session scratchpad, never copied into the repo) at
  commit `532a3c8` (2025-11-06, "Added guiPlayAudio"). It is the last commit
  before 2025-11-09 and defines exactly 121 actions, matching the installed
  build's count; the next commit (`de6e6e1`, 2025-12-03) adds a 122nd. The
  match is likely, not verified byte for byte: U136 must confirm it against
  the installed build before recording (the plan's `ADDON_PIN` step).
- Upstream HEAD (2026-10) has moved on to a new "API v1" and is not the
  target.

## Sections

| Spec section | Source (version) | Read for |
|---|---|---|
| E2 | add-on `plugin/web.py` (532a3c8); research dossier 09 §1.2; plan U135, R1677, R1678 | any method, request target ignored, body length taken from `Content-Length` only (no chunked decoding), one reply per connection then close; Meguri's limit and keep-alive from the plan |
| E3 | `plugin/web.py` request wrapper; `plugin/__init__.py` request handler (532a3c8) | the order: preflight before parsing, the ping only for an allowed origin with a zero-length body, a parse failure answered as an error only for an allowed origin, the 403 for any other disallowed request except `requestPermission`, the key check before the action lookup |
| E4, E4.1 | `plugin/web.py` request schema (532a3c8); python-jsonschema integer semantics (floats with no fraction count as integers, booleans do not) | members and their types, `params` must be an object, `key` unchecked, `version` default 4 |
| E5 | `plugin/web.py` header builder and reply formatters (532a3c8); Python `json.dumps` defaults | 200 headers (`Content-Type`, `Access-Control-Allow-Origin`, `Access-Control-Allow-Headers: *`, `Content-Length`), bare result at version 4 or less, errors always wrapped, the ping banner, ASCII-escaped output with `", "` and `": "` separators; the reference 403 headers |
| E6 | `plugin/util.py` defaults (`apiVersion` 6), `plugin/__init__.py` `version` action and versioned-name lookup (532a3c8) | version constants; every action registered without per-version names |
| E7 | `plugin/web.py` origin check and preflight; `plugin/util.py` and `plugin/config.json` defaults (532a3c8); plan R1742-R1746, R1679 | exact-match list, `*`, the localhost rule as the reference implements it (127.0.0.1 forms and extension prefixes only), the fallback `http://localhost` value, the private-network preflight header, no Allow-Methods; Meguri's wider rule and `null` handling from R1743, R1744 |
| E8 | `plugin/__init__.py` request handler (532a3c8); plan R1747, U135 | the key comparison and its `requestPermission` bypass; the reference refusing a supplied key when none is set |
| E9 | `plugin/__init__.py` `requestPermission`; `plugin/web.py` origin injection and reply origin (532a3c8); README `requestPermission` section; plan R1749, R1750 | the three answers, `requireApikey` spelled with lower-case k in the code (the README writes `requireApiKey`), the deny-list check, list updates by outcome, the echoed origin on a disallowed request; timeout and coalescing from the plan |
| E10 | `plugin/__init__.py` `apiReflect` (532a3c8) | the two validation messages, `{"scopes": []}` without the scope, request order and repeats kept, alphabetical listing when `actions` is null |
| E11 | `plugin/__init__.py` `multi` and request handler (532a3c8); plan R1752, R1753 | each sub-request through the handler with its own key and version, no origin check per sub-action, a non-object element failing the whole call, `requestPermission` failing inside `multi` |
| E12 | `plugin/__init__.py` (532a3c8, the 121 registered actions); plan R1754-R1775, U141-U143, U146 | the reference action list for E12.4; Meguri's set and flags from the Product Contract |
| E13, E16, E17 | plan R1779-R1781, U141, U144 | no reference counterpart |
| E14 | plan R1659, R1790, R1795-R1797; Meguri specs `engine/lifecycle`, `engine/op-model`, `engine/undo` | Meguri-designed |
| E15 | the files above; plan R1738 | which strings are the reference's interface texts |
| E9.1 | plan U141 ("outcomes update the config lists"); Meguri spec `servers/host-security` H10.3, H11.2 | Meguri-designed: the core appends during the resolve call, Swift mirrors the append |
| E10 (first-match table) | `plugin/__init__.py` `apiReflect` (532a3c8) | item types checked only while listing actions; `scopes` absent or null rejected |
| E4.1 (number edges) | python-jsonschema integer semantics and Python's JSON decoder (finite whole floats only; digit-only integers of any size) | `1e400` not an integer, long digit strings integers |
| E16 | plan R1780, U141, U144; Meguri spec `servers/host-security` H10.1, H11.1 | Meguri-designed entry shape, ordering and Keep log rules |
| E18 rows on origins and decoding | `plugin/web.py` origin check (532a3c8: `*` allows every origin, the empty one included; headers kept in a map, the last value wins); `plugin/__init__.py` `requestPermission` (prompt for any not-allowed, not-ignored origin; repeated appends) and request handler (key, lookup, call); Python's JSON decoder (`NaN`, `Infinity`, deep nesting accepted) | the differences listed |
| E2 (client content types) | Yomitan `ext/js/comm/anki-connect.js` and asbplayer `common/anki/anki.ts`, public upstream sources read in the session scratchpad (revision not recorded) | Yomitan posts `Content-Type: application/json` and calls `sync` with `{version}`; asbplayer sends no content type |
| E18, E19 | derived from the rows above | |

## Interface strings reproduced from the reference

`unsupported action`, `valid api key must be provided`,
`collection is not available`, `scopes has invalid value`,
`actions has invalid value`, the ping banner `AnkiConnect v.6`, and the JSON
keys `apiVersion`, `permission`, `granted`, `denied`, `requireApikey`,
`scopes`, `actions`. Every other error string in E15.2 is Meguri's.

## Notes for U136 (oracle maintainer)

- E19 rows marked "Rec." assume each scenario starts from a fresh profile
  whose add-on config is the scenario's (`webCorsOriginList`,
  `ignoreOriginList`, `apiKey`); the recorder must write that config before
  the first request.
- Rows that need a permission prompt, a busy collection, read-only mode or
  chunked bodies are Meguri-only: the reference has no read-only mode, its
  prompt is modal, and it reads chunked bodies as empty.
- `version-overflow`, `version-big-digits`, `reflect-no-scope-bad-item` and
  `json-content-type` are recordable; `nan-token`, `origin-twice` and the
  `list-star-*` rows that the reference allows are Meguri-only.
- The reference sends a raw `HTTP/1.1 403 Forbidden` line with
  `Access-Control-Allow-Origin` and `Access-Control-Allow-Headers` but no
  `Content-Length`, then closes; the recorder should record that as a 403
  with an empty body.

`tools/mg similarity` was not run from the cleanroom worktree; run it from the
main checkout (with the AnkiConnect sources in the local similarity corpus,
U136) before merging.

## Probes against the running add-on (2026-11-10)

Spec-writer session s-20261110T085429Z-165d83d3, item `ac-envelope`.
Add-on: AnkiConnect release 25.11.9.0 (upstream commit `532a3c8`, the
archive and file hashes of `oracle/ankiconnect/ADDON_PIN`), inside Anki
26.9.3 offscreen, image `meguri-oracle-ankiconnect:spike`, frozen clock
`ORACLE_CLOCK_START=1894701600`, `TZ=Europe/Paris`, on an `--internal`
Docker network with no published port. Requests were raw HTTP/1.1 written
from inside the container to `127.0.0.1:8765` (so repeated and malformed
headers could be sent); add-on config was changed between requests by
editing the profile's add-on `meta.json`, which the add-on reads on every
request; permission dialogs were answered by the recorder's driver add-on.

| Spec text | Probe and result |
|---|---|
| intro ("checked against that build") | about 150 requests over E2-E12; every claim in those sections held except the rows below |
| E2 `HEAD` | `HEAD` with a body gets the full reply body (new E18 row) |
| E2 connection | two pipelined requests: one reply, then close |
| E5.3 | 403 for every refused origin: `HTTP/1.1 403 Forbidden`, ACAO `http://localhost`, ACAH `*`, no `Content-Type`, no `Content-Length`, empty body, close |
| E7.4 | `Access-Control-Request-Private-Network: TRUE` or `false` gets no allow header; only `true` does |
| E7.2, E18 localhost rule | allowed: `http://127.0.0.1`, `http://127.0.0.1:3000`, `http://127.0.0.1:x`, `https://127.0.0.1`, the three extension prefixes; refused: `https://127.0.0.1:8443`, `http://127.0.0.1x`, `http://127.0.0.10`, `http://localhost:8080`, `https://localhost`, `http://[::1]:3000`, `CHROME-EXTENSION://abc`, `http://LOCALHOST`; matching is case-sensitive |
| E9 params, E18 | `requestPermission` with `params: {"x": 1}` fails with the unexpected-keyword text before any dialog (no queued answer consumed) |
| E9.1, E18 (removed row "appended again") | two concurrent `requestPermission` from one origin with two queued `allow`: one dialog, the second request granted from the updated list, one list entry, one answer left unused |
| E11, E18 | `[createDeck B, 1]` fails with a runtime text but deck B exists afterwards; a sub-action with `"version": "6"` ran (its deck exists) and its slot holds a runtime text |
| E18 `key` row | no key configured: `"key": null` accepted; `""`, `false`, `5`, `"whatever"` refused |
| E18 empty `Origin` | `Origin:` empty and no `*`: dialog shown; Deny with the checkbox appended nothing; ACAO empty |
| E18 `null` with `*` | `requestPermission` from `Origin: null` with `*`: granted |
| E18 modal | while a dialog waits, `version` and the ping got no reply within 4 s |
| E15.1 | `updateNoteFields` unknown id: `Note was not found: <id>`; `modelNameFromId` unknown id: `model was not found: <id>` |
| E12 | `apiReflect` with `actions: null` lists 121 names, sorted, equal to E12.3 plus E12.4 |
| E10 examples | `scopes ["other"]` with `actions "x"`: `actions has invalid value`; `scopes ["actions","actions",5]`: `"scopes": ["actions"]` |
| Open questions | observations added to each; none of the policy questions is answered by a probe |

## Review fixes (2026-11-10, second pass)

Same session and item. Probe container `acenv-r02-81080` on its own
`--internal` network, same image and clock, removed afterwards. Add-on
`plugin/web.py` and `plugin/__init__.py` of the pinned release were read
from the image (`/opt/ankiconnect/addons21/2055492159/`) for the header
parser and the permission branches.

| Spec text | Source and result |
|---|---|
| E18 empty `Origin`, `null` (R0.2) | with one queued `allow`: `requestPermission` from `Origin: ` (one space) granted, ACAO empty; then `deckNames` from it 200 `["Default"]` with ACAO empty (403 before). Same for `Origin: null`: granted, then served with ACAO `null`. The add-on's Yes branch appends the origin with no emptiness check; its ignore branch requires a non-empty origin |
| E18 header line without a space (R0.1) | `Origin:https://evil.example` with `deckNames`: 200 `["Default"]`, ACAO `http://localhost`. The parser splits header lines on colon-space only, so such a line is not read as `Origin` (reviewer's probe also: `Origin:` alone allowed and granted; `Origin:  http://localhost` 403) |
| E2, E19 `head-request` (R1.2) | Meguri rule: HTTP framing for `HEAD`, `Content-Length` absent or the `GET` length, not compared; same rule as `servers/migaku-protocol` P4. No reference source |
| E19 permission kinds (R1.4) | `permission-allow`, `permission-deny-ignore`, `permission-deny` reclassified as Rec., since the driver add-on answers the dialog (`servers-transcripts.md`, T19.1); list contents moved to new declared rows whose ids end in `-lists` |
