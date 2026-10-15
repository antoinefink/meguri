# The `meta` status check and the sync status

Spec: sync/meta-status
Requirements: R1513, R1514, R1515, R1516 (core: the message is passed through), R1517, R1518, R1559 (computation); AE63
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/meta/

Every collection sync starts with the status check: a `meta` request whose answer decides whether the sync stops, runs normally or needs a one-way transfer. The same check, with an offline shortcut and a short memory, answers "what is the sync status?" for the app's indicator (R1558, U113). This file says what the check sends, what it refuses, how it compares the two sides, how the status is computed, and the meta probe the optimizer gate uses (KTD8). The `meta` body is `wire-format.md` W3; the request, its redirect and its errors are `transport.md`; error kinds are `errors.md`.

## M1. The local record

The check compares the server's `meta` answer (`wire-format.md` W3) with these local values, read from the open collection when the job builds the `meta` request:

| Value | Source |
|---|---|
| `mod` | `col.mod` (epoch milliseconds) |
| `scm` | `col.scm` (epoch milliseconds) |
| `usn` | `col.usn`: the server's update sequence number as of the last sync |
| `empty` | true when the `cards` table has no row |
| `ts` | the core's clock (the clock seam that stamps `mod`), in whole epoch seconds, truncated |

The check never writes to the collection.

## M2. The check (R1513, R1514, R1515)

1. Send `meta` (`wire-format.md` W3) with the job's base, following at most one redirect as `transport.md` T7 says. Any transport failure ends the check with that failure (`errors.md`).
2. If the answer's `cont` is false, stop with the kind `server_message`, whose text is the answer's `msg` exactly as received, possibly empty (R1514). Nothing else is checked.
3. Let the offset be the answer's `ts` minus the local `ts` (M1). If its absolute value is greater than 300, stop with the kind `clock_incorrect`, carrying the offset in seconds (R1515). Exactly 300 s, ahead or behind, passes.
4. Compare the two records (M3).

- A check that stops at step 2 or 3 sends no other request in its job and writes nothing, locally or remotely (AE63). The job's other requests (`start` and the rest) come only after a result of M3 that calls for them.
- Before the check, a normal sync job may refuse for a reason of its own (the AnkiWeb size limit of R1563, U109); the check itself has no such rule.
- The official server answers `cont` false only to clients older than protocol 10, so `server_message` from `meta` is tested against the scripted stub (`transport.md` T13), as are non-empty `msg` values.

| Server `ts` minus device `ts` | Result |
|---|---|
| -360 | `clock_incorrect`, offset -360 (AE63) |
| -301, 301 | `clock_incorrect` |
| -300, 0, 300 | the check goes on |

## M3. Comparing the records (R1517, R1518)

The first row that matches decides:

| Condition | Result |
|---|---|
| server `mod` equals local `mod` | no changes: nothing to sync |
| server `scm` differs from local `scm` | one-way sync required, with the directions below |
| otherwise | normal sync required |

- Equal `mod` wins over differing `scm`: a collection whose schema time moved locally (without its `mod` moving) still gets "no changes" (oracle, `sync/meta/outcomes`).
- A one-way sync offers upload when the local collection has cards or the server's is `empty`, and download when the server's collection is not `empty` or the local one has no cards:

  | Local has cards | Server `empty` | Upload | Download | Outcome |
  |---|---|---|---|---|
  | yes | no | yes | yes | the user chooses (R1536, U109) |
  | no | no | no | yes | download only (R1518) |
  | yes | yes | yes | no | upload only (R1518) |
  | no | yes | yes | yes | the user chooses |

- A normal sync records whether the local side is newer: local `mod` greater than server `mod`. That is `lnewer` in `start` (`wire-format.md` W4).

Two facts of Anki's stamps that make this table behave as it does (oracle, `sync/meta/outcomes`):

- A new, never-changed collection has `mod` 0, and so has the server's collection of a new account, so their first check finds no changes.
- A downloaded collection carries the `mod` the server gave its collection when it served the download, and the download sets the local `ls` to it, so the next check finds no changes until either side changes something.

## M4. What a check returns

| Field | Value |
|---|---|
| result | no changes, normal sync (with `local_is_newer`) or one-way (with upload and download allowed) |
| `server_message` | the answer's `msg` when not empty, exactly as received; the core never sanitizes, trims or shows it (R1516: the app shows it after the sync as sanitized rich text, R168, U151) |
| server `mod`, `scm`, `usn`, `media_usn`, `ts` | from the answer; `usn` is what the normal sync sends back as each sent object's `usn` (`wire-format.md` W6.2), `media_usn` starts the media sync (R1534, R1575) |
| local `usn` | sent as `minUsn` in `start` |
| `newEndpoint` | the new base when the check followed a redirect (`transport.md` T7) |

## M5. Offline status (R1559)

Computed from the local `col` row alone, with no network:

| Condition (first that matches) | Offline status |
|---|---|
| `ls` is 0 | unknown: the collection never synced, so only the server can tell |
| `scm` greater than `ls` | one-way sync needed |
| `mod` greater than `ls` | local changes pending |
| otherwise | nothing pending locally |

- `ls` is checked first: a collection that never synced would otherwise always read "one-way sync needed", even when the server's account is new too (where M3 finds no changes).
- Anki answers "no changes" in the first and last rows alike and asks the server in both; Meguri names the first row "unknown" so the indicator can say so before the answer arrives. Both then ask (M6).

## M6. The sync status

The status the app asks for (U113 calls it for the indicator, R1558) combines M5, a remembered answer and M2:

1. Compute the offline status (M5). "One-way sync needed" and "local changes pending" are the status. No request is made, and the remembered answer is neither read nor changed.
2. Otherwise ("unknown" or "nothing pending locally"), when a remembered answer exists for the same base and host key and is less than 300 seconds old, it is the status. No request is made.
3. Otherwise run the check (M2, M3) and give its result: no changes, normal sync needed (the server has changes), or one-way sync needed. Remember it with the time the check began. A check that fails (any error kind) returns the error and leaves the remembered answer as it was.

The status result also carries `newEndpoint` when step 3 followed a redirect, which the app stores as for a sync (`login-endpoints.md` L4). It carries no server message: messages are shown only after a sync (R1516).

### M6.1 The remembered answer

- One answer at most, held in memory with the open collection: closing the collection forgets it, and so does replacing it by anything but a one-way sync (an import of a whole collection, a backup restore). A one-way sync stores its own outcome (below).
- It is valid only for the base and host key it was obtained with. A status with another base or key ignores it and replaces it when its own check completes. So a change of server or account never shows the old server's answer; a new login to the same account (the official server issues the same key) keeps it.
- Age is measured on the core's clock (M1), in whole seconds. An answer stored at second T serves a status request at T + 299 and not at T + 300 (oracle, `sync/meta/status`). An answer stored in the future (the clock moved back) counts as expired; Anki would keep serving it.
- Every completed sync job replaces it with the job's own outcome, stored at the moment the job ends:

  | Job | Remembered |
  |---|---|
  | normal sync job that found no changes or ran to the end | no changes |
  | normal sync job that stopped because a one-way sync is required | one-way sync needed |
  | one-way upload or download that succeeded | no changes |

  A job that fails or is cancelled leaves it as it was. So right after a download the status is "no changes" without a request (oracle, `sync/meta/status`), although the server's `mod` moved when it served the download (M3).
- Local changes need no rule of their own: they move `mod` past `ls`, and step 1 answers first.

| Situation | Status | Request made |
|---|---|---|
| never synced, new account | no changes (both `mod` 0, M3) | yes |
| 60 s after a normal sync that found nothing, another device uploaded meanwhile | no changes (remembered) | no |
| 300 s after that sync | the server's answer | yes |
| just after a one-way download | no changes (remembered) | no |
| a note added since the last sync | normal sync needed (offline) | no |
| `scm` moved past `ls` | one-way sync needed (offline) | no |

## M7. Meta probe (KTD8)

A job that only asks the server for its current record, for the optimizer's commit gate (U47, U51): it runs M2 steps 1 to 3 and returns the answer's `mod`, `scm`, `usn`, `media_usn` and `ts`, plus `newEndpoint` when it followed a redirect. It compares nothing, writes nothing, and neither reads nor changes the remembered answer (M6.1). Its failures are the check's (`server_message`, `clock_incorrect`, every transport kind); the gate treats any failure as "server state not confirmed".

## M8. Errors

| Where | Kind (`errors.md`) |
|---|---|
| `cont` false | `server_message`, text = `msg` |
| clock offset beyond 300 s | `clock_incorrect`, with the offset |
| `meta` answer that does not decode (`wire-format.md` W3) | `bad_response` |
| transport failures, statuses, redirects | per `errors.md` E4, E5 |

## M9. Scenarios

Actor scenarios under `conformance/scenarios/sync/meta/` (`specs/conformance/sync-ops.md`). They are live and stay `pending` until Meguri's executor syncs (sync-ops.md S10.2).

| Scenario | Shows | Golden |
|---|---|---|
| `outcomes` | upload only, download only, no changes after a download, a normal sync after a local change, equal `mod` over differing `scm`, a choice when both sides have cards (M3) | recorded |
| `clock-behind` | AE63: the server 360 s behind, `clock_incorrect` for both actors, the collection unchanged and the server's content unchanged (M2) | recorded |
| `clock-edge` | exactly 300 s passes (M2) | recorded |
| `status` | the sync status: offline answers, the remembered answer for 299 s and not 300 s, the answer of a sync and of a download remembered, an online answer, no request where none is needed (M5, M6) | waits for the driver's `status` kind (sync-ops.md S4.1) |

`smoke/sync/server-clock-ahead` shows 301 s refused, and `sync/transport/identity-accepted` a download-only outcome. What the official server cannot show (`cont` false, a non-empty `msg`, an answer that does not decode, the remembered answer's key rule, a clock moved back) is tested in `meguri-sync` against the scripted stub (`transport.md` T13).
