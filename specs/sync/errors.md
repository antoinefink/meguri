# Sync error kinds

Spec: sync/errors
Requirements: R1561 (classification), R1499 (kind), R1562 (core: no panic or crash path), R1514, R1515, R1516; supports R165, R1555, R1556
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/meta/, conformance/scenarios/sync/transport/

Every way a sync job can fail, sorted into one list of kinds that the app presents (U151, U160) and the sync policy acts on (U113). The kinds are Meguri's own classification; E9 compares them with Anki's. Transport failures arrive as the categories of `transport.md` T11; this file maps them, the HTTP statuses, decode failures and local failures to kinds.

A *sync job* is any of: login, sync status check, meta probe, normal sync, one-way upload or download, media sync (`transport.md`).

## E1. The error value

A failed job returns one error value with:

| Field | Content |
|---|---|
| `kind` | one id of E3, lower case with `_` |
| `class` | E2 |
| `retry` | E2 |
| parameters | the kind's parameters (E3), typed |
| `method` | the sync method whose request or answer failed (`meta`, `applyChanges`, `msync/begin` and so on), absent when the failure was not tied to one request |
| `detail` | English text for logs and diagnostics only, never the message shown to the user |

- The kind ids, the classes, the retry values and the parameter names are an interface: Swift maps each kind to its own String Catalog entry (U151, U160), U113 reads `class` and `retry`, and they cross the FFI by these names (U37, U114). Renaming one is a breaking change.
- `detail`, `method` and the parameters never hold the host key, the password, a request or response body (other than the server's own message text, which is a parameter by design), or a URL with credentials (`transport.md` T11).
- Every failure inside a sync job becomes one of these values. No failure crosses the FFI as a panic, an abort or an untyped error (R1562).

## E2. Classes and retry

| `class` | Meaning for the app (R165) |
|---|---|
| `transient` | a quiet banner or status glyph with Retry; the same sync can succeed later without the user changing anything |
| `actionable` | an alert that names the next step (log in again, fix the date, check the server address, run Check Database, free space) |
| `silent` | not shown as an error: the user cancelled, the system took the time away, or the sync waits for something to end |

R165's third class, `blocking` (the collection cannot be opened), is never a sync error: no sync failure stops study or leaves the collection unopenable (R1562, R1544).

| `retry` | Meaning for U113 |
|---|---|
| `auto` | retryable category; a safe bounded retry may occur only inside the existing explicitly authorized manual operation, never as permission to start a new Anki request on a lifecycle/timer trigger |
| `after_trigger` | retry is eligible after the cause ends, but requires explicit Start/Resume; the end of review or foreground never starts Anki requests |
| `no` | retrying without the user changing something fails the same way |

A manual transfer shows every `transient` and `actionable` error (R1555). `sync/anki-transfers` M1–M6 owns admission, partial outcomes and recovery. An error or retry hint cannot release an unknown-upload guard, prove rollback, repeat a possibly accepted upload or resolve a cloud fence. These constraints apply to every kind below.

## E3. Kinds

| `kind` | When | `class` | `retry` | Parameters | What the message conveys (wording: U160) |
|---|---|---|---|---|---|
| `auth_failed` | HTTP 403 on any request (R1499) | actionable | no | | the email or password was incorrect; log in again. The app clears the stored host key and redirected endpoint (U151) |
| `conflict` | HTTP 409 | transient | auto | | another device is syncing; try again in a moment |
| `client_too_old` | HTTP 501 | actionable | no | | the server needs a newer version of Meguri |
| `server_error` | HTTP 500, 502, 503, 504, 429, or another 5xx | transient | auto | `code` | the server has a problem; try again later |
| `db_check_required` | HTTP 400 (origin `server`), or local data that cannot be sent (origin `local`, E7) | actionable | no | `origin`: `server` or `local` | run Check Database, then sync again, or force a one-way sync |
| `proxy_auth` | HTTP 407, from the proxy or the server | actionable | no | | the proxy needs authentication |
| `unexpected_status` | any other 4xx status | actionable | no | `code` | the server answered with an error; check the server address |
| `offline` | no route to the server: the name could not be resolved, or the network or host is unreachable | transient | auto | | the device is offline |
| `network` | any other network failure: refused, reset or closed connection, connecting took over 30 s, the connection broke while the answer was read | transient | auto | `head_received`: whether the answer's status line had arrived | a network problem interrupted the sync |
| `timeout` | the stall watchdog fired (`transport.md` T8), or HTTP 408 | transient | auto | | the server stopped responding |
| `tls` | the TLS handshake failed | actionable | no | `self_hosted`: true when the job's server kind is `self_hosted` (`login-endpoints.md` L3) | a secure connection could not be made; when `self_hosted`, a hint that the server must offer HTTPS with TLS 1.2 or later and a certificate the device trusts (R1561) |
| `redirect_refused` | a redirect that is not followed (`transport.md` T7) | actionable | no | `code` | the server sent the sync elsewhere in a way Meguri does not follow; check the server address |
| `bad_response` | a 2xx answer that breaks the transport's checks (`transport.md` T6.2) or does not decode (E6) | transient | auto | | the server's answer could not be read |
| `invalid_endpoint` | the job's base is not a valid endpoint (`login-endpoints.md` L1) | actionable | no | `rule`: which L1 rule | the server address is not valid |
| `invalid_input` | a caller value out of range: an empty user name or password at login, a zero stall timeout | actionable | no | `field` | (shown only for login fields; otherwise a bug) |
| `proxy_unusable` | the job's proxy cannot be used (`transport.md` T10) | actionable | no | | the system proxy setting cannot be used |
| `endpoint_refused` | builds with `test-hooks` only: the endpoint guard refused a URL (`transport.md` T12) | actionable | no | | (tests only) |
| `server_message` | `meta` answered `cont` false (R1514); a one-way upload answered with a text other than `OK` (`one-way.md` O6.4); a media answer holds only `err` (`wire-format.md` W15) | actionable | no | `text`: exactly as the server sent it, possibly empty (an `upload` answer: its first 16,384 bytes, `one-way.md` O6.4) | the server's own text, shown sanitized (R168, U151); a generic line when empty |
| `clock_incorrect` | the server's clock and the device's differ by more than 300 s (`meta-status.md` M2) | actionable | no | `offset_secs`: server minus device | fix the device's date, time and time zone (R1515) |
| `upload_too_large` | a sync over the AnkiWeb limits (R1563; builds with feature `ankiweb` only): the precheck before a normal or one-way upload job, or the prepared upload (`one-way.md` O9) | actionable | no | `size_bytes`, `limit_bytes`, `measure`: `uncompressed` (the collection file) or `compressed` (the upload body) | the collection is too large for AnkiWeb |
| `sanity_check_failed` | `sanityCheck2` answered `bad` (R1533, U107) | actionable | no | `client`, `server`: the counts tuples of the answer when present (`wire-format.md` W12) | the collections did not match; the next sync is one-way |
| `notetype_schema_changed` | an incoming note type that would replace a local one has a different number of fields or templates (R1525, U107) | actionable | no | `notetype_id` | a note type changed incompatibly; a one-way sync is needed |
| `missing_notetype` | an incoming note uses a note type the collection does not have (R1528, U107) | actionable | no | `notetype_id`, `note_id` | run Check Database, then sync again, or force a one-way sync |
| `download_invalid` | a one-way download that the open gate refuses or whose integrity check fails (R1543, `one-way.md` O7.2) | actionable | no | `reason`: `corrupt`, `too_old`, `too_new`, `v1_scheduler`, `not_a_collection` or `upgrade_failed`; `version`: the `col.ver` found, with `too_old`, `too_new` and `upgrade_failed` | the server's collection is damaged, or made by an Anki version Meguri cannot open (which, by `reason`); the local collection is unchanged |
| `media_resync_failed` | the media sanity check still disagrees after the one automatic re-sync (R1581, U111) | actionable | no | | media could not be brought in sync; sync again or run Check Media |
| `disk_full` | not enough free space: the free-space check (R2015) or a write that failed for lack of space | actionable | no | `needed_bytes`, `available_bytes`, each when known | free up space on the device |
| `local_storage` | any other local file or database failure | actionable | no | | the collection could not be written |
| `session_active` | a one-way sync is needed while a review session is open (R458); the job made no change | silent | after_trigger | | finish review, then explicitly retry the transfer |
| `collection_corrupt` | the collection is damaged (lifecycle `Failed{Corrupt}`, U132): a normal sync or a one-way upload is refused before any request (R2012, `one-way.md` O4); a one-way download is still allowed | actionable | no | | the collection is damaged and is never uploaded; restore a backup or download the server's collection |
| `safety_backup_failed` | the safety backup before a one-way sync failed for a reason other than a lack of space (R1539, `one-way.md` O5); the job made no change | actionable | no | | the backup before the one-way sync failed; nothing was changed |
| `exclusive_busy` | another exclusive job holds the collection (an import, Check Database, another sync); a media sync started while a one-way sync runs (`one-way.md` O4) | transient | auto | `running_op`, when known: `one_way_sync`, `restore`, `colpkg_import`, `apkg_import` or `check_database` (R171) | another operation is running, named when known |
| `cancelled` | the job's cancel token fired (`transport.md` T9) | silent | no | | |
| `interrupted` | the system ended the job: background time ran out or the app is suspending (R1554, C8) | silent | after_trigger | | |
| `internal` | a broken invariant inside Meguri (a bug), for example rows still pending in the outgoing view at the sanity check (R1530) | actionable | no | | sync failed unexpectedly |

## E4. HTTP statuses

A `status` failure from the transport (`transport.md` T11: any 4xx or 5xx, and a 407 from a proxy) becomes a kind by its code alone, never by its body or reason text:

| Code | Kind |
|---|---|
| 400 | `db_check_required`, origin `server` |
| 403 | `auth_failed` |
| 407 | `proxy_auth` |
| 408 | `timeout` |
| 409 | `conflict` |
| 429 | `server_error` |
| 500, 502, 503, 504 | `server_error` |
| 501 | `client_too_old` |
| any other 5xx | `server_error` |
| any other 4xx | `unexpected_status` |

- The order of checks of `transport.md` T6.1 means a 403 or 407 without `anki-original-size` is classified by its code, never as `bad_response`.
- On which requests the official server sends each code (`transport.md` T14): 400 for a malformed request and for a request its collection could not apply (a note whose note type it does not have, for example); 403 for an unknown host key or a wrong login; 409 for a request whose session key is not the one that started the sync, and for `downloadFiles` naming a file missing from its disk; 501 for a `meta` body version it does not support; 500 for internal failures.

## E5. Transport categories

| `transport.md` T11 category | Kind |
|---|---|
| `invalid_endpoint` | `invalid_endpoint` |
| `invalid_input` | `invalid_input` |
| `invalid_proxy` | `proxy_unusable` |
| `network`, cause: name lookup failed, network unreachable, host unreachable, network down | `offline` |
| `network`, any other cause | `network` |
| `tls` | `tls` |
| `timeout` | `timeout` |
| `status` | E4 |
| `redirect` | `redirect_refused` |
| `protocol` | `bad_response` |
| `cancelled` | `cancelled` |
| `endpoint_refused` | `endpoint_refused` |

- A connection refused by the server's host is `network`, not `offline`: the device has a route, the server is not listening.
- A connection that breaks after the answer's status line arrived is `network` with `head_received` true. Anki reports that case as a sync error rather than a network error; Meguri keeps it a network problem, since retrying is the cure.

## E6. Decode failures

- Every answer a job decodes (`login-endpoints.md` L5, `wire-format.md` W3 to W17) is checked against its rules. Any break (invalid JSON, nesting beyond 127 levels, a missing required member, a wrong type, a refused schema-11 object, a tuple of the wrong length, an unexpected `status` string, a malformed zip) is `bad_response`, with `method` set and a `detail` naming the member or element (`decks[0][3].conf`), never the body.
- A failed decode applies nothing from the answer: decoding completes before any of it is used (U107 stages incoming changes, KTD6).
- Decoders run on untrusted input. They allocate only in proportion to the bytes received, recurse at most to the nesting limit, and never panic, abort or overflow the stack. `core/fuzz/fuzz_targets/sync_json.rs` feeds arbitrary bytes to every response decoder; it runs 10 minutes nightly with no panic (U19).

## E7. Local failures

| Failure | Kind |
|---|---|
| SQLite reports a full disk, a file write fails for lack of space, or the free-space check refuses (R2015) | `disk_full` |
| any other SQLite or file failure on the device | `local_storage` |
| a row that must be sent cannot be read (`model/row-records` R10) or encoded (a `config` value that is not JSON, `wire-format.md` W6.4) | `db_check_required`, origin `local` |
| an incoming note type, note or sanity answer that U107 refuses | `notetype_schema_changed`, `missing_notetype`, `sanity_check_failed` |
| a one-way download that the open gate refuses or whose integrity check fails | `download_invalid` |
| a normal sync or one-way upload of a collection in `Failed{Corrupt}` | `collection_corrupt` |
| a forced safety backup fails, other than lack of space (`disk_full`) | `safety_backup_failed` |
| an invariant Meguri relies on does not hold | `internal` |

- A failed normal sync rolls back and sends `abort` (R1532, U107); the kind is the original failure's, never the outcome of `abort`.
- A cancel that races a failure is `cancelled` (`transport.md` T9).

## E8. What the kinds lead to (informative)

- `auth_failed`: the app clears the host key and the redirected endpoint and asks the user to log in again, with wording that the email or password was incorrect (R1499, U151).
- `conflict` may be retried safely inside the already authorized manual operation; otherwise offer explicit Retry. Never start an automatic Anki job or retry an upload whose completion is unknown.
- `sanity_check_failed`: U107 has already set `scm` so the next sync is one-way (R1533); the status shows "one-way sync needed".
- `clock_incorrect`: nothing changed anywhere (AE63); the message asks the user to fix the date, time and time zone.
- `server_message`: the text is shown after the sync as sanitized rich text (R1516, R168, U151).
- `tls` with `self_hosted`: the message adds the HTTPS and TLS 1.2 hint (R1561).

## E9. Anki's classification (informative)

Anki's client sorts HTTP statuses the same way for 400 (database check required), 403 (authentication), 409 (conflict), 501 (client too old), 500, 502, 503 and 504 (server error) and 407 (proxy authentication). It reports every other status as a generic network error, and its network errors are offline (name resolution or unreachable), timeout, proxy authentication or other. Meguri differs only where Anki has no specific kind: 408 is a timeout, 429 and every other 5xx a server error, every other 4xx `unexpected_status`, a redirect not followed `redirect_refused`, an unreadable answer `bad_response`. Anki shows the same message for a 400 and for a failed sanity check; Meguri keeps them apart because only the second has already made the next sync one-way. Anki has no kinds for the local and policy failures of E7 and E3 (`session_active`, `exclusive_busy`, `interrupted`, `disk_full`); it reports them as generic errors.

## E10. Scenario rejection kinds

In actor scenarios (`specs/conformance/sync-ops.md` S4.4) a failed `sync` step records a rejection kind. Meguri's executor maps its kinds to the closed list there:

| Meguri kind | Scenario `error` |
|---|---|
| `auth_failed` | `auth_failed` |
| `clock_incorrect` | `clock_incorrect` |
| `offline`; `network` with `head_received` false; `timeout` | `network` |
| any other | a run error until sync-ops.md lists a kind for it (U107, U111, U118 add theirs with the driver) |

## E11. Tests

- A table test drives `meguri-sync` through the scripted stub (`transport.md` T13), which answers each status of E4 (and 401, 404, 413, 505) without `anki-original-size`: each gives the kind, class and retry of E3 and E4.
- Each transport category of E5, each decode failure class of E6 (one per `wire-format.md` section), and `server_message` from `cont` false and from a media `err` envelope, through the stub.
- Against the official server (actor scenarios): `auth_failed` (`sync/transport/login-wrong-password`), `clock_incorrect` (`sync/meta/clock-behind`, `smoke/sync/server-clock-ahead`), `network` (`sync/transport/login-reset`, `sync/meta/status`).
- No sync error crashes the app or blocks study (R1562): the fuzz target of E6, and U114's FFI tests that every kind crosses as a typed value.
