# Multi-party sync scenarios

Spec: conformance/sync-ops
Requirements: R1876, R1883, R1884, R1926, R1927, R1932, R1933; R1499 (`auth_failed`), R1559 (`status`), R1530 and R1533 (`sanity_check_failed`), R88 and R726 (`review_session`)
Anki: 26.09.3
Scenarios: conformance/scenarios/smoke/sync/, conformance/scenarios/sync/

This file defines the part of the scenario language in which several clients sync with one Anki sync server: actors, the scenario's sync server, the ops `actor`, `sync`, `net_fault`, `reopen` and `converge`, the fault proxy and the recorder in front of the server, and how the oracle and the runner run such scenarios. The rest of the language is `dsl.md`. Observation values, `db_dump` among them, are in `observations.md`. The machine-checkable forms are `conformance/schema/scenario.schema.json`, `observation.schema.json` and `protocol.json`; when this text and a schema disagree, fix both in one change.

What normal, one-way and media sync do to data (R1926–R1933) is the sync area's (`specs/sync/`). This file states only what the harness needs.

## S1. Actor scenarios

- A scenario that declares `actors` is an actor scenario. Each actor is one client with its own collection. All actors sync with one Anki sync server that exists for one run of the scenario (S3).
- An actor scenario needs the oracle for its server, so it is live (dsl.md D9): T0 never runs it (S10.2).
- An actor scenario has no `handoff` step: single-collection live chains (D9.2) and actors do not mix.
- The top-level key `server` and the ops `actor`, `sync`, `net_fault` and `converge` exist only in actor scenarios. `reopen` and the observation `db_dump` work in every scenario.

## S2. Actors

### S2.1 Declaring actors

```yaml
actors: [client_a, meguri, client_c]
```

- `actors` is a non-empty list of distinct names of two kinds:
  - Meguri actors: `meguri`, `meguri_2`, `meguri_3` and so on (`meguri`, or `meguri_<n>` with n ≥ 2 and no leading zero). In T1 Meguri's executor plays them. When the oracle records a golden, an oracle client plays them like any other actor.
  - Oracle actors: `client_a` to `client_z`. The oracle always plays them. They stand in for Anki desktop and AnkiMobile, which run the same sync client as the oracle; only the client version string differs.
- The order of `actors` matters: the first actor runs the setup (S2.2), and `converge` lists actors in this order (S7).

### S2.2 Collections and setup

- Each actor has its own collection, created as a new collection by its executor at `clock.start`, before the setup runs.
- The setup (dsl.md D6) runs on the first actor's collection only. The other actors start with an untouched new collection. (A new Anki collection already holds a default deck, a default preset and Anki's stock note types.)
- Op arguments that name note types, decks and presets are looked up by name in the current actor's collection. So an actor that has not received the setup's note type by sync cannot add a note of it: the step is a run error.

### S2.3 The current actor

- `actor: <name>` makes the named actor, which `actors` must declare, the current actor. Its output is `{}`. Steps before the first `actor` step run on the first actor. Naming the current actor again changes nothing.
- The current actor runs `observe` and every op that works on a collection: `add_note`, `answer`, the other collection ops of D7.2, `sync` and `reopen`.
- These steps are global and run whatever the current actor:
  - `advance` and `set_clock` move the scenario clock, which every actor and the server share (S3.2);
  - `net_fault` changes the network path of every actor (S5);
  - `converge` reads every actor (S7);
  - `actor` itself.

### S2.4 Logical ids

- `#nN` and `#cN` number notes and cards across the whole scenario, in creation order, whichever actor's step created them (dsl.md D7.1). Sync keeps a note's or card's real id, so a note that reaches another actor keeps its logical id there.
- An observation on an actor whose collection lacks a referenced note or card reports it as each kind defines absence (`card`: `null`).
- The `ids` observation reports the whole scenario's table, whichever actor runs it.

### S2.5 Randomness

Each Meguri actor has its own random source (dsl.md D5). Its seed is the first eight bytes, read as a big-endian unsigned integer, of the SHA-256 of the UTF-8 text `<scenario_sha256>/<actor name>`. So two Meguri actors never share a sequence. Scenarios without actors are seeded as D5 says.

## S3. The scenario's sync server

### S3.1 Process

- The server is Anki's own sync server from the pinned release, run from the oracle image's `anki` package. The oracle worker starts it as its child process when an actor scenario's session starts, and stops it when the session ends: after the last step, on `end`, or when the run fails. It is never a compose service: the compose `syncserver` serves end-to-end runs only, on the real clock.
- Every run gets a new server with:
  - a new, empty base directory under the worker's scratch tmpfs (`SYNC_BASE`);
  - a port the worker chooses for the run (`SYNC_PORT`);
  - one account (`SYNC_USER1=<username>:<password>`). Its credentials are test values fixed in the driver, the same for every scenario, so recordings do not change from one run to the next (S9).
- The server creates the account's collection the first time a request reads it (the first status check), at the server's clock. Until an upload, that collection is a new, empty one.
- The server listens on one IP address, `SYNC_HOST` (an address, never a host name):
  - `127.0.0.1` when the run has no fault proxy and no recorder (S8.1): every client then connects over loopback;
  - `0.0.0.0`, all of the worker's addresses, when the fault proxy or the recorder must reach it. The proxies connect to the worker's `oracle-internal` address, while the converge client still connects over loopback (S7.1), so the server must accept both. Listening on the worker's `oracle-internal` address alone would refuse the converge client. The worker has no network but `oracle-internal` (S8.2), so this exposes the server to nothing else.
- The server gets no proxy variables.
- Before the first step, the worker waits until the server accepts connections. If the server exits, or does not accept connections within 10 s, the session fails with `sync_unavailable` (S10.5). The server's log goes to the worker's stderr, which `tools/mg` keeps in its logs.

### S3.2 Clock

- The server runs under the worker's faked clock (KTD23), so it shares the scenario clock (R1926). Every `advance` and `set_clock` moves the server's clock with the actors'.
- `server.clock_offset_secs` (an integer from −86400 to 86400, default 0) puts the server's clock that many seconds ahead of the scenario clock, or behind it when negative, at every instant of the run. The worker keeps a second clock file for the server at the scenario clock plus the offset.

```yaml
server: {clock_offset_secs: 301}
```

- 301 is the skew of R1932: an Anki client refuses a sync when the server's clock and its own differ by more than 300 s (S4.4).

## S4. The `sync` op

### S4.1 Kinds and results

`sync: {kind: <kind>}`, `kind` required; `login` also takes `credentials` (S4.5). Each kind does what an Anki client does at that point, on the current actor:

| `kind` | What the actor does | Output on success |
|---|---|---|
| `login` | Asks the server for a host key for the scenario's account, and keeps it for its later syncs. A later `login` replaces it. | `{sync: {result: ok}}` |
| `normal` | A collection sync as a client starts it: the status check (the `meta` request), then the whole incremental sync when the status check allows one. Media are never synced. | `{sync: {result: <outcome>}}`, below |
| `full_upload` | A one-way upload: its collection replaces the server's, as after the user chose upload. | `{sync: {result: ok}}` |
| `full_download` | A one-way download: the server's collection replaces its own. | `{sync: {result: ok}}` |
| `status` | The sync status as the client's indicator asks for it (`specs/sync/meta-status.md` M6): answered from the actor's own stamps when they decide, from the answer it remembers for 300 s, or from the server's `meta`. It syncs nothing and changes nothing. | `{sync: {result: <status>}}`, below |

Outcomes of `normal`:

| `result` | Meaning |
|---|---|
| `no_changes` | No one-way sync is needed: the status check found the two collections unchanged since they last synced, or the incremental sync ran to the end. |
| `full_sync` | A one-way sync is needed, and both directions are possible: the client asks the user. |
| `full_upload` | A one-way sync is needed, and only an upload is possible: the server's collection has no cards. |
| `full_download` | A one-way sync is needed, and only a download is possible: the actor's collection has no cards. |

Results of `status`:

| `result` | Meaning |
|---|---|
| `no_changes` | Nothing to sync, as far as the actor knows: nothing pending locally, and the server's record (asked now or remembered) shows no change. |
| `normal_sync` | A normal sync is needed: the actor has local changes (answered without a request), or the server's record shows changes. |
| `full_sync` | A one-way sync is needed, whichever directions are possible: the actor's schema time is past its last sync (answered without a request), or the server's record has another schema time. |

- `status` makes a request only when its answer needs the server; a scenario shows that it made none by running it while a `net_fault` resets every connection (`sync/meta/status`).
- The answer an actor remembers is forgotten when its collection is opened again. The oracle opens the collection again for `reopen` and also before each `legacy_json`, `legacy_update` and `blob_decode` (`model-ops.md` L5), and Meguri's executor does not for those three, so a scenario that relies on the remembered answer runs none of these ops between the steps concerned. A one-way sync keeps it: it remembers its own outcome (`specs/sync/meta-status.md` M6.1).
- `full_upload` and `full_download` make no status check and do not ask whether a one-way sync is needed: like the client after the user's choice, they transfer. A scenario that wants the outcome on record runs `normal` first, as the smoke scenarios do.
- `full_upload` and `full_download` close the actor's collection and open it again. Logical ids stay valid.

### S4.2 Preconditions

- An actor's first `sync` step, in step order, has kind `login`. This is checked when the scenario loads.
- A `normal`, `status`, `full_upload` or `full_download` of an actor that holds no host key, because no `login` of it succeeded or an `auth_failed` cleared its key (S4.4), is a run error: `invalid_scenario`.

### S4.3 Equal instants

The status check compares the two collections' modification and schema times, in milliseconds. Under a frozen clock, two changes made in the same instant have equal times, and the status check then finds no change where there is one. For example, the server creates the account's collection at its first status check; a first `normal` at `clock.start` finds the server's schema time equal to the actor's, so it runs an incremental sync instead of reporting `full_upload`. So scenarios advance the clock (`advance: 1m`) before each actor's block of changes and syncs, as the smoke scenarios do.

### S4.4 Rejections

A sync that fails records `{sync: {error: <kind>}}` (dsl.md D7.3). The list of kinds is closed:

| `error` | When |
|---|---|
| `auth_failed` | The server refused the credentials of a `login`, or the host key of any later request, with HTTP 403 (R1499). After it the actor holds no host key, whatever an earlier `login` gave it, so its next `sync` step must be a `login` (S4.2). The actor's collection does not change. |
| `clock_incorrect` | The status check found the server's clock more than 300 s from the actor's, ahead or behind. Only `normal` and `status` make the status check, so only they get this kind (`status` only when it asks the server). The actor's collection does not change. |
| `network` | The request did not complete at the network level: the connection was refused, reset or closed before the response's headers arrived, or no data moved for the client's stall timeout. |
| `sanity_check_failed` | A `normal` whose incremental sync got `bad` from the server's sanity check (`specs/sync/normal-sync.md` N8). The round's changes are undone on the actor and on the server; the actor's schema time becomes the current instant, its modification time, last-sync time and update sequence number stay, so its next `normal` reports a one-way sync. |
| `db_check_required` | A `normal` whose incremental sync got HTTP 400 from the server on one of its requests: the server could not apply it, for example a note whose note type it lacks (`specs/sync/errors.md` E4). The round's changes are undone on the actor and on the server; the actor's schema time stays, so its next `normal` runs the same round and fails the same way until the data is repaired. |

- Any other failure is a run error (`anki_error` on the oracle) until the sync area adds its kind with the driver (U111, U118). Among these: a server error, a conflict, and a connection reset after the response's headers arrived (Anki reports that as a sync error, not a network error).
- Anki's client reports `sanity_check_failed` and `db_check_required` as the same sync error with the same message. They differ in what they leave: only a failed sanity check moves the actor's schema time. An executor tells them apart by that, or by its own error kinds, never by the message (R1887).
- A failed sync leaves the actor's collection as Anki's client leaves it on that failure. The scenario observes it, and the golden records it.
- Kinds come from the error's class and kind, never from its message text (R1887).

### S4.5 Login credentials

```yaml
- sync: {kind: login, credentials: wrong_password}
```

- `credentials` is allowed only with `kind: login`. Its values:
  - `account` (the default): the scenario's account, as the worker reports it (S10.4).
  - `wrong_password`: the account's user name with the account's password followed by `-wrong`. Every executor derives it from the account the same way, so no other credential exists. The server refuses it: `{sync: {error: auth_failed}}`.
- A scenario still never names a credential (S8.1): `credentials` only says how the executor derives one from the account.

## S5. Network faults: `net_fault`

```yaml
- net_fault: {kind: reset, direction: to_client}
- net_fault: {kind: clear}
```

- A fault applies to every connection between an actor and the server, existing and new, from its step until `clear` or the end of the run. It never applies to the converge client (S7).
- `reset`, with `direction` required:
  - `to_client`: the next time data flows from the server to a client on a connection, the connection is reset (TCP RST) and that data is not delivered. A request therefore reaches the server, the server handles it, and the client gets a reset instead of the response's headers.
  - `to_server`: the same for data from a client to the server. The request never reaches the server.
- `clear` removes every fault.
- Setting a fault that is already set (same kind and direction) changes nothing.
- The scenario's fault proxy (S8) carries out the faults: it is a toxiproxy 2.12 proxy, `reset` is the toxic `reset_peer` with `timeout` 0 on the stream `downstream` (`to_client`) or `upstream` (`to_server`), and `clear` deletes every toxic of the proxy.
- The output is `{}`.
- U118 adds the other faults of R1932 (latency, data limits, stalls) when its suites need them.

## S6. `reopen`

`reopen: {}` closes the current actor's collection (the collection, in a scenario without actors) and opens it again from its file. An open that fails is a run error. The output is `{}`. A scenario uses it to show that a collection still opens after a failed sync.

## S7. Convergence: `converge`

R1927 asks that collections that synced end with matching normalized dumps, with every excluded field documented. `converge: {}` checks this for every actor at once.

### S7.1 Procedure

1. A fresh oracle client, which is not an actor, creates a new collection at the current clock, logs in to the scenario's account and makes a full download over the direct path (S8.1). Faults and the recorder never apply to it, and it never appears in a recording. A failure of its login or download is a run error.
2. It takes the raw dump (S7.2) of every actor's collection and of its own.
3. It removes the excluded fields (S7.3) from every raw dump.
4. For each actor, in `actors` order, it lists where the actor's dump differs from its own (S7.4).
5. It deletes its collection.

`converge` is an op, not an observation, because it changes the server: the server raises its update sequence number and sets its modification time when it serves a full download. A scenario usually ends with it.

### S7.2 Raw dump

The raw dump of a collection is the value of the `db_dump` observation (observations.md O8) for all twelve tables, without O8's normalization of random values: `notes.guid` is the stored text, and every BLOB digest covers the stored bytes. So two actors that hold different random values for the same object differ in their raw dumps, while the `db_dump` observation, which must be equal in every run of the scenario, hides those values.

### S7.3 Excluded fields

The fields below record a party's sync history, not its collection's content. They differ between parties whose content is equal. `<key>` stands for every row of the `config` table (its `KEY`, O8).

| Pointer | Column | Why it is excluded |
|---|---|---|
| `/col/1/mod` | modification time of the collection | Every incremental sync ends with the server setting it to the instant the sync finished and the client copying that value. The server also sets it when it serves a full download, and an uploader sets it when it prepares the upload. So each party holds the time of its own last sync. Each changed row carries its own modification time, which stays compared, except in `config` (below). |
| `/col/1/usn` | update sequence number of the collection | It counts sync rounds. The server raises it by one at the end of each incremental sync, which the client then copies, and when it serves a full download; an uploader raises its own before it uploads. Each party holds the count of its own last round. Each row keeps its own `usn`, which stays compared, except in `config` (below). |
| `/col/1/ls` | last sync time | An incremental sync sets it to the instant the server finished, a full upload to the uploader's new schema time, a full download to the downloaded collection's modification time. It records when and how the party last synced. |
| `/config/<key>/mtime_secs` | modification time of a config entry, in seconds | Config travels whole, and the receiver of an incremental round rewrites every entry with its own current time, whether or not the value changed (below). So the time records when the party last received the config, not when the entry last changed. |
| `/config/<key>/usn` | update sequence number of a config entry | In the same round, the receiver writes every entry with the round's usn and the sender sets every one of its entries to 0 (below). So the value records the party's role in its last round, not a pending change. |

How config moves in an incremental sync (the sync area specifies it in full):

- Every incremental round moves the whole config in one direction. The client sends all of its config entries when its collection's modification time (as the status check compared them) is later than the server's; otherwise, equal times included, the server sends all of its own. Config entries are never chosen by their `usn`.
- The receiver deletes its config and writes each received key and value with `usn` set to the round's usn (the server collection's `usn` when the round began) and `mtime_secs` set to the receiver's current time, in seconds.
- The sender sets `usn` to 0 on every one of its config entries and leaves their `mtime_secs` as they were.
- A full upload or download copies the config entries as stored; the uploader does not change them first.

The key set and each entry's `val` stay compared, so a config value that did not reach a party is still a difference.

Example, `smoke/sync/three-party` (times are scenario clock times on 2030-01-15): client_c sends its config in its round at 10:05, so the server holds every entry with that round's usn and 10:05, and client_c holds every entry with usn 0 and 10:03: the server stamped 10:03 when it received client_b's config, and client_c's download at 10:04 copied that. In the rounds of client_a at 10:06 and client_b at 10:07 the server sends its config: its entries go to usn 0 and keep 10:05, client_a's get that round's usn and 10:06, and client_b's that round's usn and 10:07. The converge client downloads entries with usn 0 and 10:05. Without the two config exclusions, client_a and client_b would differ in both columns of every config entry, and client_c in `mtime_secs` only.

No other field is excluded. If oracle actors that last synced with nothing pending differ anywhere else, that is a finding for the spec writer: never exclude a field to hide it.

### S7.4 Differences

- A difference is an RFC 6901 pointer into the dump, at the first level where the two dumps disagree:
  - `/<table>/<key>` when a row exists on one side only;
  - `/<table>/<key>/<column>` when a column's value differs;
  - `/<table>` when a table exists on one side only (never the case when both are raw dumps of all tables).
- Values compare exactly, as JSON values: an INTEGER 1 and a REAL 1.0 differ.
- Each actor's list is in pointer order: pointers compare segment by segment, segments in the order JCS sorts member names (UTF-16 code units), and a pointer comes before the pointers it is a prefix of.

### S7.5 Output

```json
{"converge": {
  "differences": {"client_a": [], "client_c": []},
  "dump": {"cards": {...}, "col": {...}, ...}
}}
```

- `differences` has one member per actor. `[]` means the actor converged.
- `dump` is the fresh client's collection as the `db_dump` observation of all twelve tables (normalized as O8 says), without the excluded columns. It puts the converged content in the golden, so T1 compares Meguri's run with the oracle's for both convergence and content.

## S8. Endpoints, the fault proxy and the recorder

### S8.1 Paths to the server

An endpoint is the base URL a client syncs with. The worker derives every endpoint of a run from its own environment and from the server it started. No scenario names a host, a port or a credential (R1876).

| Path | Used by | Route |
|---|---|---|
| direct | the converge client; oracle actors when the run has no fault proxy and no recorder | `http://127.0.0.1:<server port>/`, inside the worker |
| fault proxy | every actor, when the run has a fault proxy | `http://<fault proxy host>:<listen port>/`, whose upstream is the server's `oracle-internal` address |
| recorder | oracle actors in a recording run | an HTTP forward proxy in front of the actor's path (S9) |
| Meguri edge | Meguri actors in T1 | an endpoint on 127.0.0.1 of the runner's host that reaches the fault proxy |

- A run has a fault proxy when the scenario has a `net_fault` step or the caller plays an actor (S10.4). Every actor, oracle or Meguri, then reaches the server through it, so a fault applies to them all.
- The fault proxy is a toxiproxy proxy that the worker creates for the run, in the toxiproxy that `ORACLE_SYNC_FAULTS` names (`<host>:<API port>` on `oracle-internal`). Its name is unique to the run, toxiproxy assigns its listen port, and its upstream is `<worker address>:<server port>` on `oracle-internal`. The fault proxy host in the actors' endpoint is the host of `ORACLE_SYNC_FAULTS`. The worker deletes the proxy when the session ends. A run that needs a fault proxy fails with `sync_unavailable` when `ORACLE_SYNC_FAULTS` is not set or the API refuses.
- The recorder is an HTTP forward proxy (mitmdump in regular mode with `docker/mitm/record_sync.py`) that `ORACLE_SYNC_RECORDER` names (`<host>:<port>` on `oracle-internal`). When it is set, the run is a recording run (S9).
- When it starts, the worker removes `http_proxy`, `https_proxy`, `all_proxy` and `no_proxy`, in any letter case, from its environment, so a stray variable cannot reroute a client. A recording run then sets its own (S9).
- The Meguri edge is needed only once a Meguri adapter syncs (U105). Its compose wiring lands with that adapter and publishes only on 127.0.0.1, as the rig's `sync-edge` does.

### S8.2 Network and services

- The worker, the fault proxy and the recorder are only on `oracle-internal` (`internal: true`), so nothing in them has a route out, whatever upstream a proxy is given (R1876, container side).
- `docker/compose.yml` holds two services for this, on `oracle-internal` only, with `init: true` and no published port:
  - `sync-faults`: toxiproxy 2.12.0, pinned by the same digest as the rig's `toxiproxy`, with its API on 8474 and no proxy at start;
  - `sync-recorder`: mitmdump 12.2.1, pinned like the rig's `mitmdump`, in regular (forward proxy) mode with `record_sync.py`, for recording runs only.
- `tools/mg` starts `sync-faults` for oracle runs that select actor scenarios (`oracle record`, `oracle compare`, `oracle test`, `conformance --live`) and passes `ORACLE_SYNC_FAULTS=sync-faults:8474` to the workers. It starts `sync-recorder` only for recording runs (S9).
- The real-clock rig (`tools/mg sync-rig`) does not serve conformance runs, and these services do not replace it.

### S8.3 Endpoint checks (R1876)

- When an interactive session of an actor scenario begins, the worker reports the run's endpoints (S10.4): the actors' endpoint, the converge client's endpoint, the fault proxy's upstream when the run has a fault proxy, and the recorder in a recording run.
- Before it sends the first step, the parties executor passes every reported endpoint, and the Meguri edge endpoint when Meguri plays an actor, through `exec::sync_guard::check_endpoint` (the rule of `meguri_sync::endpoint_guard`). A refused endpoint ends the session with no step run; the scenario's status is `error`, with kind `endpoint_refused`.
- No client connects, and no data crosses the fault proxy to its upstream, before the first step. So in T1 every connection to the server, through the fault proxy or not, comes after the check.
- The runner also checks the toxiproxy API endpoint it gives the workers (`ORACLE_SYNC_FAULTS`) before it starts any worker, since a worker connects to that API when a session begins.
- Golden recording (`tools/mg oracle record`), `tools/mg oracle compare` and recording runs run whole scenarios in the oracle harness, with no Rust on the path. There, the endpoints come only from the worker's environment, which `tools/mg` sets, and the network isolation of S8.2 keeps every connection inside `oracle-internal`.

### S8.4 Per-session Meguri edge

This section specifies Meguri's test transport. Each mixed session takes its
target from `sync.endpoints.actors` and its ownership tokens from `sync.route`
in the same `begin` response. Replay allocates no edge. The route ownership
extension below corrects the earlier assumption that parent-side worker
teardown ordering alone could protect a published edge.

**Handoff.** `tools/mg` supplies `conformance run --live` with
`--meguri-edge-cmd`: a JSON array of command arguments, as for the existing
worker command. The runner passes this allocation capability through LiveRun
to Parties; a single endpoint shared by concurrent sessions is insufficient.
The command runs directly, without shell expansion. Each invocation reads one
JSON request from stdin and writes one JSON response to stdout:

| Request | Successful response |
|---|---|
| `{"cmd":"acquire","target":<actors endpoint>,"route":{"proxy":<token>,"worker":<token>},"expected_worker":<trusted checkout token>}` | `{"endpoint":<host endpoint>,"lease":<opaque token>}` |
| `{"cmd":"release","lease":<opaque token>}` | `{"released":true}` |

This handoff runs in the existing native T1 lane: MG launches the CLI on the
Mac host, and that CLI invokes the host helper. Docker publishes the returned
loopback port on that same host. The Docker T0 lane does not allocate an edge;
MG continues to refuse a Docker-only live lane. A host loopback endpoint must
never be installed in a client running in another network namespace. This
adds neither a Docker socket nor oracle-network membership to dev or filtered
implementation containers. Only the MG host helper manages Docker resources
within the current checkout's project and run.

Requests and responses admit only those members, with nonempty strings where
shown. A malformed response, unsuccessful process exit or allocation failure
is a run error (`sync_unavailable`), never `pending` or a missing capability.
A refused endpoint remains `endpoint_refused`. A missing allocation command
for an otherwise supported mixed run is `sync_unavailable`. No synthetic
successful sync result, alternate server or database copy replaces the edge.

**Allocation.** Before invoking acquire, Parties checks every endpoint from
`begin` under S8.3. The helper accepts only an HTTP root URL, with an explicit
port, whose host exactly matches the fault-service host in the run's trusted
`ORACLE_SYNC_FAULTS` configuration. It refuses credentials, query, fragment,
an absent/invalid port and the fault-service API port. The target is the
reported actors endpoint, never the reported upstream or converge endpoint.
No scenario, actor step or account value can set a helper command or target.
The trusted launch factory assigns the expected worker identity before
starting the driver and retains it with Worker. Exclusive checkout gives
Parties that identity independently of the worker's response. Parties and
the helper require `route.worker` to equal this `expected_worker`, then check
that the tokens belong to the checkout/run, the named proxy still has the
reported listen endpoint and its upstream belongs to that specific worker's
retained namespace. Do not obtain expected identity from `begin`, including
by copying its token into another field. Reject missing trusted context or
any mismatch before edge publication or proxy deletion. A consistent tuple
for a different worker in the same run is still refused.

The helper starts one forwarder with an immutable target for this lease. It
uses the existing edge pattern: one attachment to `oracle-internal` and one
to a publishing network, with Docker selecting a fresh host port published
only on `127.0.0.1`. The returned endpoint is an HTTP root URL with that
literal address and explicit port. Parties validates both its exact shape
and S8.3 before giving it and the reported account to the existing Seat.
The helper exposes no host control listener, proxy API or caller-selected
route. It does not change the shared fault service's networks or publish its
ports. Its only data route ends at this session's fault proxy; external
destinations and the real-clock rig are excluded.

Allocation readiness checks inspect the listener and published mapping;
they send no client request through the proxy before the first step. The
forwarder preserves TCP resets and orderly half-closes as the existing rig
edge does. It adds no HTTP decoding, caching or request rewriting. Independent
sessions retain independent leases even for identical scenarios, actor names
or worker processes; a forwarder is never retargeted for another session.

**Route ownership from creation.** MG supplies opaque current-run and unique
worker tokens to the trusted worker launch, outside scenario data. The
container receives its ownership labels before it starts; every mixed
session's proxy receives its run-owned unique name before it is created.
`ORACLE_SYNC_EDGE_OWNER` is a fresh 32-digit lowercase hexadecimal run token;
`ORACLE_SYNC_WORKER` is a distinct 32-digit lowercase hexadecimal worker token,
also stored in the container label `meguri.mg.worker`. A mixed proxy's exact
name is `meguri-edge-<owner>-<32-digit lowercase hexadecimal session token>`.
`route.proxy` is that name and `route.worker` is the worker token. The helper
gets the same owner through trusted MG configuration and checks the existing
checkout/project/run/purpose labels as well as this token. Container cleanup
requires a match on every ownership field, not just the proxy-name prefix.
The worker reports those exact proxy/worker tokens in `sync.route`. Missing
trusted ownership configuration prevents mixed `begin` from creating a route.

For mixed sessions, deletion belongs to the host owner from proxy creation,
not only after acquire succeeds. The worker's session close, EOF, error and
independent process exit must not delete that proxy. This is the bounded
exception to S8.1's worker-deletes rule. The existing named fault proxy keeps
its listener allocated until the host explicitly deletes it; no new public
fault-service control endpoint or general lease service is required.

Retain the upstream namespace too. The backend server uses a per-session
port inside a pooled worker container. Retaining only the proxy listener
would still allow an old edge to reach a later server at that old upstream.
A container-local lifetime holder must therefore own the worker container
and its network attachment independently of the driver process, starting
before mixed `begin`. Driver EOF or self-exit leaves that namespace held.
Each pooled worker has a separate container/namespace; checking a worker out
gives one interactive session exclusive use of it. Keep `--jobs 4` concurrent:
each active mixed session uses its worker's own namespace. A shared container
or global session lock is insufficient. Do not let driver exit trigger
automatic container removal; the holder, not the driver, controls its lifetime.
While any published edge may refer to it, the namespace is quarantined:
no replacement driver, new session, new exec, backend listener or reassigned container
address may use it. Its original backend may remain alive or stop; stopping
is safe only because nothing can bind a replacement endpoint there. Host
stdio retention or delaying child reap does not establish this guarantee.
Use the existing worker launch/lifetime wrapper for this bounded ownership;
it needs no general command server or arbitrary process-management API.

**Release and failure.** Parties stops actor work and releases the edge before
`end`. Release verifies removal of every publication and forwarding connection
for that lease before deleting its exact proxy. Only after this succeeds may
the worker be reused or its container/namespace retired. A proxy-delete error
is reported; it may leave a harmless reserved proxy, never a published route
to a replacement backend. An owned release is idempotent; foreign tokens fail.

After successful `begin`, validation or allocation failure still attempts
`end`. That closes session data/server resources but does not free a retained
mixed proxy or permit worker reuse while edge removal is unresolved. On later
errors, malformed responses or cancellation, stop steps, attempt the same
edge-first cleanup and end or close/reap the driver. Independent driver exit
is a run failure; its lifetime holder and proxy remain owned until cleanup.
Do not keep a failed session eligible for the worker pool.

The helper labels every allocated container with the existing MG run label
and its edge purpose, including containers created before an interrupted
acquire can return a token. Normal cleanup and the cancellation trap first
remove this run's edges, then its named mixed proxies, then its retained
worker containers. Run-owned proxy names and worker labels support this sweep
even when no begin/acquire response reached the caller. If edge removal cannot
be proved, retain the proxy and worker namespace; report cleanup failure and
keep the run failed. Do not delete them merely because a deadline expired or
the CLI exited. MG must not restart/recreate the shared fault service while
any edge publication remains unresolved. Cleanup leaves other runs, projects
and the shared service intact. Helper invocations remain bounded and cancelled
with the run. Credentials never enter helper requests,
arguments, labels or diagnostics; transport details stay in ephemeral run
state, outside scenarios, goldens and committed reports.

**Verification.** Focused tests cover target rejection before allocation,
returned-edge rejection before any actor step, exact Seat routing, acquisition
failure, each early exit after acquisition, idempotent owned release, foreign
release refusal and interrupted allocation cleanup. Concurrent mixed runs
exercise distinct proxies and retain isolation when either run ends.
Inject driver self-exit and EOF before/after begin response, lost acquire
responses, helper timeout, edge-removal failure and proxy-delete failure.
With two valid same-run workers, substitute B's complete target/proxy/worker
tuple into A's response. A's trusted checkout identity must cause refusal
before publication or deletion, while B stays usable. Cover missing trusted
identity, mismatches and the correctly bound path, including when both
workers use the same numeric backend port in distinct namespaces.
Prove that no old proxy port or upstream address/port can be reused behind
a surviving edge; attempt a later session and require refusal/quarantine until
removal is confirmed. Cover pre-token owned sweeps, whole-run cancellation,
normal reuse after cleanup and foreign-resource preservation. A service
restart must be refused while an edge remains unresolved.
Include two concurrently active worker containers using the same numeric
backend port in distinct namespaces. After one driver exits with edge removal
blocked, its old edge must never reach the other worker. The unaffected
session must still make progress under `--jobs 4`.
Transport tests retain reset and half-close behavior and prove no
upstream request during readiness checks. Published T1 scenarios then run
through production sync, first serially and then concurrently, preserving
the replay-first and real capability checks of S10.3. Stub dispatch tests
alone do not establish transport parity. The neutral interactive protocol
and its implementation must add the route member together. Scenario inputs,
golden observations and oracle pins are not changed by this specification;
any resulting driver freshness update follows the normal validation process.

## S9. Recordings of oracle-client exchanges (R1933)

- A recording run records the exchanges between the oracle actors and the server, with request and response bodies zstd-decoded, so that the sync area can describe the protocol from them and compare Meguri's requests with them semantically (U105, U117).
- In a recording run, the worker routes every oracle actor's sync traffic through the recorder: it sets `http_proxy` to the recorder and `no_proxy` to `127.0.0.1,localhost`, and the actors use the server's `oracle-internal` address, or the fault proxy, never loopback. The converge client uses loopback, so it bypasses the recorder.
- In a recording run, each sync step's endpoint has a prefix: `http://<host>:<port>/<actor>/<step index>/`. The recorder removes `/<actor>/<step index>` from the request path before it forwards the request, and records the exchange with two more members: `actor` (a string) and `step` (an integer, the scenario's step index). The rest of each line is `record_sync.py`'s format. Outside recording runs there is no prefix.
- A recording is `conformance/recordings/scenarios/<scenario id>/exchanges.jsonl` with `recording.json`: `{oracle: {anki_version, driver_sha256, lock_sha256}, scenario_id, scenario_sha256}`, keys sorted, two-space indents, a final newline.
- Only `tools/mg oracle recordings <filter>...` writes recordings. It records one scenario at a time, each with a recorder of its own whose output directory is that scenario's, and keeps no recording whose recorder reported an error. Never edit a recording by hand.
- A recording run must not change any observation: a scenario's recording run gives the observations of its golden.
- U13 records `smoke/sync/two-party` and `smoke/sync/three-party`.

## S10. Running actor scenarios

### S10.1 Goldens

`tools/mg oracle record` runs an actor scenario as one `run` request on one worker. The oracle plays every actor, Meguri actors included, starts the server and, when the scenario has a `net_fault` step, the fault proxy, and returns every step's output. The golden has the usual form (dsl.md D11). A `run` request with `steps`, `resume` or `handoff` for an actor scenario is `invalid_scenario`.

### S10.2 Capabilities and T0

When an actor scenario has at least one Meguri actor, Meguri needs (dsl.md D8):

- the op `actor`: Meguri's executor can play an actor, with a collection of its own, driven by the parties executor;
- the new collection (dsl.md D8), from which every actor starts (S2.2);
- the setup sections, when the first actor is a Meguri actor;
- every op and observation kind of the steps that Meguri actors run;
- `advance` and `set_clock`, when the scenario uses them;
- the observation kind `db_dump`, when the scenario has a `converge` step: Meguri gives the raw dumps of its actors.

`net_fault` and `converge` run on the worker and add nothing else. A scenario without Meguri actors needs nothing from Meguri.

In T0, an actor scenario is `live` when Meguri has what it needs (always, when it has no Meguri actor) and `pending` otherwise. T0 never runs it. Meguri declares `sync` only when an adapter syncs. Until then, every scenario in which a Meguri actor syncs is `pending`.

### S10.3 T1

The parties executor (`core/crates/meguri-testkit/src/exec/parties.rs`) is the only executor of actor scenarios. `tools/mg conformance --live` runs each selected actor scenario in two parts:

1. The replay: the oracle plays every actor, in an interactive session (S10.4). The observations are compared with the golden with no deviation applied. A difference is `fail` (oracle drift), whatever Meguri can do.
2. The mixed run, when the scenario has Meguri actors and Meguri has what it needs: Meguri's executor plays the Meguri actors and the oracle the others. The observations are compared with the golden, deviations applied (dsl.md D13).

When the replay passes and Meguri lacks what the scenario needs, the status is `pending`. A `pending` scenario that is not active does not fail T1.

### S10.4 Interactive sessions

The parties executor drives one worker per actor scenario through `begin`, `step` and `end` (`conformance/schema/protocol.json`):

- `begin` takes `caller_actors`: the Meguri actors the caller plays (none in the replay). When the caller plays the first actor, it runs the setup on Meguri first and passes the real note and card ids the setup created, in creation order, as `ids`. The worker creates collections only for the actors it plays, runs the setup when it plays the first actor, starts the server, and creates the fault proxy when the run needs one. It answers with:
  - `ids`: the real ids its setup created, when it ran the setup;
  - `sync`: `{endpoints: {actors, converge, upstream, recorder}, account: {username, password}, route: null | {proxy, worker}}`. `upstream` and `recorder` are `null` when the run has no fault proxy or no recorder. `route` is required and contains the two opaque ownership tokens of S8.4 for a mixed session, otherwise `null`. The caller's Meguri actors log in with `account` (and S4.5's derived password for `credentials: wrong_password`).
- The caller checks every endpoint (S8.3), then sends one `step` request for each scenario step, in order:
  - A step the worker runs (a global step, or any step while an oracle actor is current): `{"cmd": "step"}`. The response's `ids` lists the real ids the step created; the caller adds them to the id table of its Meguri actors.
  - A step of a caller-played actor: the caller runs it on Meguri's executor, then sends `{"cmd": "step", "done": {"output": <the step's output>, "notes": [<real ids>], "cards": [<real ids>]}}` with the real ids the step created, in creation order. The worker records the output as that step's and adds the ids to the scenario's table.
  - `converge`, when the caller plays actors: `{"cmd": "step", "dumps": {<actor>: <raw dump>, ...}}`, with exactly the caller-played actors.
  - `advance` and `set_clock` run on the worker as global steps; the caller also applies them to each Meguri actor's clock.
- The worker answers `protocol_error` when a step it runs comes with `done`, when a caller-played step comes without `done`, or when `dumps` does not match the caller-played actors.
- `end` reports the complete step observations and stops the session server. Worker-owned proxies are deleted here. Caller-owned mixed routes instead follow S8.4's edge-first release, including after EOF or error.

### S10.5 Errors

- `sync_unavailable` (protocol error kind): the worker cannot start the scenario's server (it exits, or does not accept connections within 10 s), cannot reach the toxiproxy API, or cannot create the run's fault proxy. The scenario's status is `error`.
- `endpoint_refused` (runner error kind): S8.3.

## S11. Smoke scenarios

`conformance/scenarios/smoke/sync/` holds U13's sync smoke scenarios:

| Scenario | What it shows |
|---|---|
| `two-party` | client_a uploads a collection with one note; client_c downloads it; both converge. |
| `three-party` | Three oracle clients: an upload, two downloads, and a note and a review that travel by incremental syncs in both directions; all converge. |
| `meguri-downloads` | `two-party` with a Meguri actor as the downloader: `pending` until a Meguri adapter syncs, never `fail`. |
| `server-clock-ahead` | The server's clock is 301 s ahead: `normal` gets `clock_incorrect` and the collection is unchanged. |
| `reset-during-download` | The download's response is reset: `network`; the collection reopens unchanged; after `clear` the download succeeds and all converge. |

## S12. Left to the sync area

- Stock note types: Meguri's new collections hold Meguri's own stock note types (R1959). When a Meguri actor runs the setup and uploads, the converged dump holds Meguri's stock note types where the golden holds Anki's. Scenarios in which a Meguri actor's collection reaches the server need a decision of the sync area (a deviation, or a `db_dump` option) before they can pass. `meguri-downloads` has the Meguri actor download, so its golden applies as it is.
- BLOB digests compare bytes: an actor's dump matches only when Meguri encodes each record's protobuf bytes as Anki does.
- FSRS memory states inside `cards.data` compare as text, exactly; O7's tolerances do not apply to dumps.
- `converge` does not check the history fields of S7.3, so it cannot show that a Meguri actor sets the `col` row's `mod`, `usn` and `ls`, or the `usn` and `mtime_secs` of config entries, as Anki does. The sync area pins those per party with `db_dump` observations (`{db_dump: {tables: [col, config]}}`) after each kind of round, in its own scenarios.
- Media sync, server errors and the other faults of R1932: their kinds and arguments come with U111 and U118. Wrong credentials came with U105 (S4.5, `auth_failed`); the `status` kind with U106; `sanity_check_failed` and `db_check_required` with U107 (S4.4). `specs/sync/errors.md` E10 maps Meguri's error kinds to this list.
- Graves and tags a client receives are stamped with the update sequence number of its own round, while the sender and the server hold the earlier round's (oracle, `sync/wire/records-down`; `specs/sync/normal-sync.md` N4.3, N5.5). `converge` reports those `usn` columns of a receiver as differences, so scenarios in which deletions or new tags cross a sync observe each party with `db_dump` instead of `converge` (`sync/normal/graves`, `sync/conflicts/deletion-vs-edit`). S7.3 does not exclude them (decided with U107): each party's value is fixed by the rules, and a `db_dump` of the Meguri actor compared with the golden pins it, which an exclusion would hide.

## S13. `review_session`

Meguri holds the newest answers of a review session back from a normal sync (R88, R726; `specs/sync/hold-back.md`). Anki's collection library has no review session, so the scenario language marks one with an op of its own.

```yaml
- review_session: begin
- answer: {card: "#c1", rating: good}
- review_session: end
```

- **Argument** `begin` or `end`. **Output** none (`{}`). It works on the current actor (on the collection, in a scenario without actors), and changes nothing in the collection.
- **Meguri.** `begin` opens a review session as U43's active-session registry knows it, without building a queue, selecting a deck or writing anything. While it is open, every undoable op the actor's steps run belongs to it (`engine/undo` M1, the step's session), and `answer` still answers the card it names, with no check of the queue's head. A normal sync of the actor then holds back as `hold-back.md` says. `end` ends the session as dismissing its summary does (KTD6).
- **Oracle.** Both forms do nothing and record `{}`. The oracle playing a Meguri actor therefore syncs as Anki does: everything pending goes up and the undo history is emptied. Each scenario that shows hold-back lists Meguri's values where they differ in `conformance/deviations.yaml` (`hold-back.md` H17).
- **Checked when the scenario loads**, per actor in step order: `begin` and `end` alternate, starting with `begin`. A session may stay open at the end. An actor with an open session runs no `reopen`, `full_upload` or `full_download` (they close the collection).
- **Capability.** Meguri declares `review_session` once its executor has U43's session registry and U107's hold-back (S10.2).
