# CloudKit acceptance and robustness

Spec: conformance/cloudkit-acceptance
Requirements: R2047, R2048, R2049, R2050, R2051, R2052, R2053, R2054, R2055, R2056, R2057, R2058, R2059, R2064, R2065, R2066; supports R88, R365, R1548, R1592, R2043, R2044, R2045, R2046; AE9, AE91, AE92, AE93, AE94, AE96
Anki: none (Meguri design)
Scenarios: none (Rust tests in `core/crates/meguri-sync/tests/` on the fake of Q3; Swift tests in `CoreBridgeTests`; signed runs of Q7)

How Meguri proves that its CloudKit sync is correct and robust: the fake CloudKit service the tests run against (Q3), the device harness and its steps (Q4), the checks every run makes (Q5), the multi-device scenario catalog (Q6), the signed-account lane (Q7), property tests and fuzzing (Q8), the soak (Q9), and the acceptance criteria per requirement (Q10). The fault points and their expected recovery are `conformance/cloudkit-faults`. The contract under test is `sync/policy` (cited as P…), `sync/cloudkit-records` (K…) and `sync/cloudkit-recovery` (V…), approved 2026-10-27 (R2066). Where this file and the contract differ, the contract wins and this file is corrected.

Units: U117 builds the fake, the harness and the catalog; U118 builds the fault runs (`conformance/cloudkit-faults`) and the properties; U248 builds the soak and the signed lane. U113's table and property tests (P21) and U114's adapter tests (P21, K22, V23) stay with their units; this file does not repeat them, and runs them as part of the lanes below.

## Q1. Lanes and evidence

| Lane | What runs | Service | Where | Tier |
|---|---|---|---|---|
| `cloud-fake` | U113's and U114's focused tests, the catalog (Q6) and the per-change property runs (Q8) | the fake (Q3) | Docker for Rust; the macOS test VM for Swift | T0 when `core/crates/meguri-sync`, `meguri-testkit/src/cloud` or the CloudKit Swift sources change; T2 nightly in full |
| `cloud-faults` | the fault table of `conformance/cloudkit-faults` | the fake, its server state outside the killed process | Docker | T0 for the named points when the code that holds them changes; T2 nightly with random kills |
| `cloud-soak-fake` | Q9.1 | the fake | Docker | T2 weekly |
| `cloud-signed` | Q7 and Q9.2 | real CloudKit, development environment of the development container | the devices in the macOS test VM; the coordinator and the `schema` leg on the host | T3 at milestones; needs user steps (Q7.2) |

- Every result carries `lane`. A fake result never fills a signed cell, and a signed result never fills a fake cell (R2064, U248). The milestone item `cloudkit-signed` accepts only `cloud-signed` results.
- Milestone items (decided 2026-10-27): `cloud-fake` fills `cloudkit-fake`, `cloud-faults` fills `cloudkit-faults` (with U250's cloud classes, `conformance/cloudkit-faults#F4`), `cloud-soak-fake` fills `cloudkit-soak-fake`, and `cloud-signed` fills `cloudkit-signed` (Q7.4). Each item needs a pass of its lane on one clean commit (Q10). M1R's gate and its first-sync validator read all four (Q13).
- No Anki oracle result, no Anki server result and no manual-transfer result counts for any CloudKit lane (R2064). The transfer lane of U248 is separate and is not specified here.
- The Rust tests of `cloud-fake` and `cloud-faults` need the oracle image for one compatibility check of `integrity` (Q5, K22.3): each final collection opens in the oracle. That check tests Meguri's output; it is not an Anki oracle result for CloudKit in the sense of R2064, and it sends nothing to any server.
- No lane contacts an Anki server or AnkiWeb. Every run counts Anki requests and manual-transfer starts, and fails when either is above 0 (P20 invariant 1, R2064).
- A run is a pass only when every check of Q5 it made passed (Q5: every run makes every check at its time). A run that could not start because of the machine's state is `queued` (`docs/process/tiers.md`); a signed run whose user step is missing is `user-step`, never a pass.

## Q2. Where the tests live

Decided 2026-10-27: CloudKit has no Anki oracle, so its scenarios are Rust and Swift tests, not files under `conformance/scenarios/`. The plan's `conformance/scenarios/soak/` and `conformance/scenarios/sync/` (U248) hold the manual-transfer lane only.

| Path | Unit | Contents |
|---|---|---|
| `core/crates/meguri-testkit/src/cloud/` | U117; U118 and U248 extend it | the fake (Q3: `fake/`), the device harness and coordinator stand-in (Q4: `devices.rs`, `coordinator.rs`), the checks (Q5: `checks.rs`), the seeded scheduler (`schedule.rs`), the program generators (Q8: `program.rs`), the soak driver (Q9: `soak.rs`) |
| `core/crates/meguri-sync/tests/cloud_policy.rs` | U113 | P21 |
| `core/crates/meguri-sync/tests/cloud_records.rs`, `cloud_recovery.rs` | U113, with U114's core side | the focused tests of K22 and V23.2 that one or two devices on the fake can run |
| `core/crates/meguri-sync/tests/cloud_acceptance.rs` | U117 | the catalog of Q6, one test per row and variant, named after the row |
| `core/crates/meguri-sync/tests/cloud_faults.rs` | U118 | the fault table of `conformance/cloudkit-faults`, through U250's child-process runner |
| `core/crates/meguri-sync/tests/cloud_property.rs` | U118 | Q8 |
| `core/crates/meguri-testkit/tests/regressions/cloud/` | U118, U248 | shrunk failing programs (Q8.4), each run in every later run |
| `app/Packages/MeguriCore/Tests/CoreBridgeTests/CloudKitSyncTests.swift` | U114 | P21's U114 list, against the Swift fake engine (Q3.8) |
| `app/Packages/MeguriCore/Tests/CoreBridgeTests/CloudKitSoakTests.swift` | U248 | the signed agent of Q7.3, and its self-test against the Swift fake |
| `app/MeguriUITests/Sync/` | U248 | the signed lane's UI smoke (Q7.4, leg `ui_status`) only; status and recovery screens are U161's and U251's |

- Each test names the spec ids it proves (in its name or doc comment), so U246's coverage report can list every P21, K22, V23 bullet and every Q6 row with its test.
- Tests read no file under `local-data/` and no real collection (R2065, U248). Their data comes from the harness's generated base (Q4.2) or U11's generated fixtures.

## Q3. The fake CloudKit

### Q3.1 Parts

- **Service**: the server side: databases, zones, records, assets, change logs, quotas. Its state lives in one SQLite file, `fake-cloud.db`, in the run directory, written in one transaction per server request, so a device process killed at any instant never changes it (`conformance/cloudkit-faults#F4`). An in-process run may hold it in memory with the same semantics.
- **Engine stand-in**: one object per engine instance, in the device's process, with the CKSyncEngine surface Meguri uses (Q3.3). It talks to the service only through requests the service logs.
- **Accounts**: Apple accounts and their per-device status (Q3.4).
- **Scheduler**: the only source of order and time (Q3.6).
- **Own-operation and read endpoint**: the CloudKit operations U114 runs outside the engine (V26.1, P4's reads), against the same service.

Decided 2026-10-27: one fake serves every Rust lane, and the service state is a file, so the kill runs of `conformance/cloudkit-faults` and the in-process catalog runs share one model.

### Q3.2 The service

It must model, per (container, environment, account), one private database:

- **Zones**: by name; creating a zone that exists succeeds and changes nothing (A20 as assumed); deleting one removes its records and adds a database change with its reason: `deleted` (by an app), `purged` (deleted in system settings, A15) or `encryptedDataReset` (A10, A18).
- **Records**: record name, record type, the field `e` (bytes), at most one asset field (a file with its size and SHA-256), and system fields: an encoded value that holds the zone, the name and a change tag. Each saved version gets a new change tag, drawn from the run's seed. The service checks K3: a record with any other field, or a field of another type, is refused with `invalidArguments`.
- **Saves**: only the save policy that fails when the server record changed (K12.2): a new-record save over an existing name, or a save whose change tag is not the server's, fails with `serverRecordChanged` and the server record. A deletion of a missing record is `unknownItem`, and so is a save with a change tag whose record does not exist (it was deleted; A5 as assumed, K17.4); under A5's other value that save stores a new record. A save into a missing zone is `zoneNotFound`; into a zone deleted by the user, `userDeletedZone`; into a zone that an encrypted data reset removed, `zoneNotFound` with the encrypted-data-reset flag set (K12.5's first row, A10). The service keeps each removed zone's cause until the zone is created again.
- **Batches**: atomic within one zone when the request asks for it (A8). Limits: 250 records per request and 2,000,000 non-asset bytes per request (A1.1), 1,000,000 non-asset bytes per record (A1), 100 MiB per asset (A2); a request over a limit fails with `limitExceeded` (for the batch, or for a one-record batch). Each limit can be set lower for a test.
- **Quota**: per account, the sum of record and asset bytes. A save that would pass it fails for that record with `quotaExceeded`; the other records of the batch save. Deletions free space. A test can raise or lower the quota at any step.
- **Change logs**: per zone, a sequence of changes; per database, zone creates and deletions. A fetch returns, for a change token, the latest version of each record changed since it and the record deletions, in pages of at most 200 records (configurable), and a new token after each page. Within a page the order is drawn from the seed: CloudKit promises no order (V5 R2). On request the service delivers a stale page: one computed before a later save of one of its records (K22.2 "a stale fetch page"). An `MGEntity` record deletion enters a log only through the fake-control step `delete_record` (Q4.3), which stands for an older or purging build: no Meguri build deletes an `MGEntity` record while purging is off (K17.4).
- **Token expiry**: a test marks a zone token expired. The engine stand-in then either surfaces `changeTokenExpired` or restarts the zone from the start without saying so (A9b as taken: false). Both variants exist.
- **Record-ID reads**: return only the fields asked for; an asset is downloaded only when its field is asked for (A4 for named-field reads). Results: the record, `unknownItem`, `zoneNotFound`, or an injected error.
- **Assets**: an asset the service delivers is written into a system staging directory of the device and deleted when the event handler returns (P11.2), so a test fails when the adapter does not stage it in time.
- **Encrypted data reset**: deletes every zone of the account that holds encrypted values, which is every Meguri zone (K3), with reason `encryptedDataReset` (A18 as assumed). Variant `survivors`: chosen zones survive and their records read back with `e` absent, or fail per record with the decryption error, one variant each (K2, A18 false).
- **Deletion from system settings**: deletes every zone of the container, with reason `purged` (A15).

### Q3.3 The engine stand-in

One stand-in object is one engine instance (V1). It is created from a saved state serialization or from none, with automatic scheduling on or off, and it delivers its events one at a time, as the scheduler picks them. It must behave as the contract assumes:

| Behavior | Contract |
|---|---|
| The state serialization is an opaque blob holding the zone and database tokens and the pending lists; Meguri never reads into it | V1, V3.1 |
| Delivers a send's per-record results before the state update that reflects them | A6 |
| Delivers fetched records and deletions before the state update that moves past them; reports a will-fetch-changes event after a `fetchChanges` call starts and a did-fetch-changes event before it returns, per scope zone, also for a zone with nothing to fetch | A7, P3 `FetchDrained` |
| A `fetchChanges` call made while a fetch runs joins it and gets only that fetch's did-fetch-changes event; a call with no signed-in account returns with no events | P3, P4 `RequestFetch` |
| An instance with no state lists every existing zone at its first fetch; a fetch from saved state with no change reports no zone event | A7, A9a |
| A no-error drain of a zone is reported only after its last record was delivered, also when the fetch is interrupted and continued | A9a |
| Asks the core for each batch through the callback object of its instance (the batch request carries the instance id), and asks for the next fetch options between server requests | V3.4, P13.2 |
| Removes a name from its pending list when its save succeeds; keeps only the names that failed with one of the seven errors it retries itself: `notAuthenticated`, `accountTemporarilyUnavailable`, `networkFailure`, `networkUnavailable`, `requestRateLimited`, `serviceUnavailable`, `zoneBusy`; drops every other failed name, `batchRequestFailed` and `limitExceeded` included. Meguri's adapter adds back the `batchRequestFailed` names, the `limitExceeded` names of a batch of more than one record and the sendable `serverRecordChanged` names itself (K12.3, K12.4, K12.6); the stand-in never does it for the adapter, so a missing add-back fails the K22.2 tests | K12.4 "Failed saves" |
| Sends a pending database change (a zone save or deletion) by itself, with no batch request | P4 `UpdatePending{full}`, P10.3 |
| Resets its tokens and pending lists at an account-change event; variant: no reset at a `signIn` with the same identity | A12 |
| With automatic scheduling on, schedules a send when names are added and a fetch at a push the scheduler delivers | P10.1 |
| `cancelOperations` makes a running call return | A22 |
| Variant `keep-alive`: after its release the object keeps fetching and asking for batches while a newer instance runs | V3.4, V23.2 "Engine instances" |

Every assumption the design rests on (A1 to A22) has a switch in the fake: its default is the assumption, and its other value is the behavior the contract's "If false" column handles (K23, V24). A Q6 row marked with an assumption runs under both values.

### Q3.4 Accounts, devices and the network

- Accounts have a user record id. Each device has an account status (`available`, `noAccount`, `restricted`, `temporarilyUnavailable`, `couldNotDetermine`), a signed-in account, reachability and its own clock (with a skew a test sets).
- The identity read needs the network (A19 as assumed); variant: it returns a cached id offline.
- A device gets a `CKAccountChanged` notification at each status or account change, and its running engine instance gets the account event (`signIn`, `signOut`, `switch`).
- A setup error (`missingEntitlement`, `badContainer`, `permissionFailure`, `managedAccountRestricted`, `badDatabase`) can be injected on the status read or on an operation (P3 `configuration`).
- Suspend: while a device's core is suspended, engine events wait in the adapter stand-in, in delivery order (P11.2).

### Q3.5 Injected errors

A test arms an injection for a scope (device, instance or own operation; zone; operation kind `send`, `fetch`, `read`, `own`; optionally a record name) and for the k-th matching request (k ≥ 1, default 1), or for every matching request while armed.

| Injection | Effect |
|---|---|
| `fail{code, retryAfter?}` | the request, or one record of it, fails with the CloudKit code; `retryAfter` in seconds where the code carries one |
| `partial{codes}` | the named records fail, the rest of the batch saves |
| `lose_result` | the service applies the request; the result never reaches the device (a network failure after the save) |
| `drop` | the service does not apply the request, and no result reaches the device (a network failure before the save) |
| `stall` | the request never returns until the device cancels it |
| `delay{steps}` | the result arrives after the given number of scheduler steps |

Codes: every code P15 and K12.3 name (`networkUnavailable`, `networkFailure`, `requestRateLimited`, `zoneBusy`, `serviceUnavailable`, `quotaExceeded`, `limitExceeded`, `batchRequestFailed`, `zoneNotFound`, `userDeletedZone`, `changeTokenExpired`, `notAuthenticated`, `accountTemporarilyUnavailable`, `invalidArguments`, `incompatibleVersion`, the asset errors, `operationCancelled`, the setup errors of Q3.4), and `internalError` for "any other error". `serverRecordChanged` and `unknownItem` are never injected: the service produces them from its state.

### Q3.6 Determinism

- A run is fixed by its seed, its program and the fake's configuration. The seed drives a ChaCha20 stream; the scheduler draws from it.
- The scheduler is the only source of order: each delivery of an engine event, push, automatically scheduled engine operation, clock advance, network change and account change is one scheduler step. Devices take steps one at a time; concurrency is modeled as interleaving at event granularity, never as threads that race. A device in a child process takes its steps by grant (Q4.5), so it follows the same order.
- Time is the fake clock. No test sleeps or reads the wall clock (R1937's seeded runs). Decided 2026-10-27: in every run the fake clock starts at 2025-01-01T00:00:00Z, and each device adds its skew (Q3.4), so ids and times do not depend on when the run happens. The oracle check of `integrity` (Q5) runs with the oracle's clock frozen at the latest instant any device's fake clock reached in the run, as `conformance/timed-operations` freezes it with `--now`.
- The run log records every step, request and result. `--seed` with the same program replays a run exactly.

### Q3.7 What the fake exposes for checks

- the service state: zones, records with their decoded envelopes (the harness decodes `e` with Meguri's envelope decoder; the fake never interprets it), change logs, quota use;
- the request log: for each request, the device, instance id or own-operation kind, zone, kind, record names, sizes and result;
- per device, the timers the device asked the fake clock for, with their owner (Q5 `no-timer`);
- per device, the engine objects alive and the facts each delivered, accepted or refused (Q5 `one-engine`, `stale-refused`).

### Q3.8 The Swift fake engine

U114's `CloudKitSyncTests` use a Swift fake behind the protocol that `CloudKitSyncAdapter` wraps around CKSyncEngine. Decided 2026-10-27: it models the rows of Q3.3 that the adapter's behavior depends on (event order, joined and short-circuited fetches, the callback object per instance, the keep-alive variant, `cancelOperations`, asset staging deadlines, the fetch options callback), not the service of Q3.2: multi-device runs are the Rust fake's. Where a row of Q3.3 has a Swift test, both fakes take it from that table, so they cannot disagree on what the engine does.

## Q4. The device harness

### Q4.1 A device

A device is a data directory (the collection, its sidecar, `meguri-cloud.db`, the media folder, the cloud staging directory), a `CoreHost` opened on it (U38), the policy (U113), a coordinator stand-in and an engine stand-in.

- The coordinator stand-in (`meguri-testkit/src/cloud/coordinator.rs`) executes the policy's actions with P4's rules: order within an action list, the tasks that later lists never wait for, the stops and cancels that start at once, the deadlines of P2, and P11's call orders. Decided 2026-10-27: U151's `SyncCoordinator` is Swift, so the Rust lanes need this stand-in. Its rules come from P4 and P11 only. U151's `SyncCoordinatorTests` check the Swift coordinator against the same rows, and the signed lane runs the real one.
- A device runs in the test process, or as a child process (`meguri-cli`, U250's runner) when the row or program can kill it (Q4.5, `conformance/cloudkit-faults#F4`).

### Q4.2 The base collection

`setup(devices, base)` makes device A's collection from a generated base, publishes it (V15, reason 1), and makes every other listed device adopt it (V14), then drains (Q4.3). Base `small` (default): one note type `QA` of the harness's own (fields `Q` and `A`, one template, its own CSS; R1959), decks `D1` and `D1::D2`, the default preset, 20 notes with one card each, 5 media files of 10 KB referenced by 5 notes, and 3 days of reviews. Base `media`: `small` plus 200 media files of 20 KB to 5 MB (40 MB in all). Every field text, tag, deck name and media file name contains a unique marker string, so `content-free` (Q5) can search for it. Bases `10k` and `150k` are U11's generated fixtures.

For `small`, `D1` and `D2` are logical roles. With the request's generated marker
`M`, the actual names are `M-D1` and `M-D1::M-D2`. Rename deck id 1 to the first
name (`decks/decks#D6`) and create its child. Retain exactly these two decks and
the unchanged default preset. Do not delete id 1: `decks/decks#D8` would reset
its name to `Default`.
Each of the five distinct media files has one reference in a different note.
The marker rules above still apply to every field value, tag and file name.

Use the request seed and a pinned `now` of `2025-01-01T00:00:00Z` (Q3.6), UTC
offset 0 and rollover hour 4. The pinned day starts at
`T = 2024-12-31T04:00:00Z`, under `fixtures/synthetic-collections#F4.3`.
For each card ordinal `i` from 0 to 19 in generation order, record Again on
each of the days starting at `T - 3 days`, `T - 2 days` and `T - 1 day`.
Its answer instant is that day's start plus 8 hours plus `i` seconds.
These are the December 28, 29 and 30 scheduler-day buckets. Require exactly
60 distinct history rows, three per card, and none on the pinned day.

Use `fixtures/synthetic-collections#F5` generator/Core calculations for the card
state, history, counters and FSRS data. Use the pinned request time throughout,
without a wall-clock input. Read back learning step 1, three learning-kind rows,
`reps = 3`, `lapses = 0` and a due instant at the last answer plus 60 seconds
for each card. Keep the computed FSRS fields. The target is learning; it must
not be replaced with a review state. All other `small` shape and marker
requirements above still apply.

### Q4.3 Steps

A program or a Q6 row is a list of steps. Device names are `A` to `D`; accounts `acct1` and `acct2`.

| Group | Steps |
|---|---|
| Life | `launch(X)`, `close(X)` (clean close), `kill(X)` (SIGKILL at a random instant), `kill(X, point[@k][:mode])` (`conformance/cloudkit-faults#F3`), `relaunch(X)`, `background(X)`, `expire(X)` (background time ends), `foreground(X)`, `quit(X)` (Mac quit) |
| Network and account | `offline(X)`, `online(X)`, `sign_out(X)`, `sign_in(X, acct)`, `status(X, status)` |
| Settings | `enable(X)`, `pause(X)`, `unpause(X)`, `auto(X, on\|off)`, `media(X, on\|off)` |
| Study and edits | `start_session(X)`, `answer(X, card, rating)`, `end_session(X)`, `undo(X)`, `redo(X)`, `edit(X, note, field, text)`, `browse_edit(X, note, field, text)` (no session step), `add_note(X, …)`, `delete_note(X, note)`, `add_deck(X, name)`, `tag(X, note, +t\|-t)`, `flag(X, card, n)`, `rename_deck(X, deck, name)`, `move_deck(X, deck, parent)`, `set_config(X, key, value)`, `add_field(X, notetype, name)`, `reorder_fields(X, notetype, order)`, `import_apkg(X, fixture)`, `restore_backup(X)`, `add_media(X, name, bytes)`, `change_media(X, name, bytes)`, `delete_unused(X)`, `remove_outside(X, name)` (deleted outside Meguri), `export(X)` (file export) |
| Cloud | `sync_now(X)`, `cancel_sync(X)`, `push(X)` (the scheduler delivers a push), `run(X, n)` (n scheduler steps of X's engine work), `drain(X…)` (Q4.4) |
| Recovery calls (V26.2) | `connect(X)`, `choose(X, choice)`, `adopt(X, source)`, `publish(X)`, `resume_binding(X)`, `resume_op(X)`, `cancel_op(X)`, `republish(X)`, `keep_local(X)`, `stop_icloud(X)`, `abandon(X, generation)`, `recover(X, copy, selection)`, `resolve(X, copy)` |
| Fake control | `inject(…)` (Q3.5), `quota(acct, bytes)`, `delete_zone(acct, zone, reason)`, `delete_record(acct, zone, name)` (the service deletes the record and logs a record deletion, as an older or purging build would; Q3.2), `reset_encrypted(acct[, survivors])`, `purge_settings(acct)`, `expire_token(X, zone, surfaced\|silent)`, `clock(X, ±duration)`, `advance(duration)` |
| Checks | `check(name…)` (Q5), `expect(X, …)` (a status, result, attention item, count or row) |

### Q4.4 Quiescence

`drain(X…)`: every listed device online, its gate open, then scheduler steps until each listed device has no pending, held or in-flight entity, no ready or waiting inbox entry that another listed device can resolve, no blob to upload or download that a listed device can serve, both fetch-scope zones drained since the last change on any listed device, and no own operation due. It fails with `no-quiescence` after 10,000 steps (configurable per row). The checks marked "at quiescence" in Q5 run after it.

### Q4.5 Child devices

Decided 2026-10-27: a device that a Q6 row, a Q8.1 or Q8.2 program, a soak or a fault run can kill runs as a child process; every other device runs in the test process. A run may have several children. Each child holds its own core, coordinator stand-in and engine stand-in, and reaches the service only through `fake-cloud.db` (Q3.1).

- **Grants.** The scheduler and every device's fake clock stay in the runner. A child takes no step by itself: it reads one grant at a time from a control channel (a Unix socket in the run directory), carries out exactly the step the grant names (a Q4.3 step, the delivery of one engine event, one automatically scheduled engine operation, a push, a network or account change), and then sends a done reply. Each grant carries the device's fake clock instant; the child's clock reads only that value.
- **Due work.** The done reply lists the engine events and scheduled operations now due in the child and the timers it asked for (Q3.7). The scheduler picks among the due work of every device, in-process or child, as Q3.6 says.
- **Check snapshot.** The done reply also carries the facts that Q5's "always" checks and Q3.7 read from that device, collected inside the child by `checks.rs`, and every fault-point hit of the step. The runner judges every check over all devices. The "at quiescence" checks read the child's files after a `drain`, as for an in-process device.
- **Kills.** A grant may name a hit: the child stops at the h-th fault-point hit of that step and reports it, and the runner sends SIGKILL; a step with fewer hits is killed after its done reply. A random kill (`kill(X)`, `conformance/cloudkit-faults#F6`) draws its step and hit from the seed, never from wall time, so a seed replays the same kill. An armed point (`conformance/cloudkit-faults#F4`) stops the child at its armed hit the same way.
- **Relaunch.** `relaunch(X)` starts a new child on the same data directory, and the grants continue from the next scheduler step.

## Q5. Checks

Each check has a name that Q6 rows, properties and soak runs cite. Every run makes every check at the time its When column gives; only "Confirmed replacements" below narrows what a check compares. "Always" checks run after every step. "At quiescence" checks run after each `drain` and compare only the devices it lists; a run that ends with a `drain` of every device (every Q6 row that names a final `drain`, every Q8.2 program) makes them there over every device.

| Check | When | Holds when | Contract |
|---|---|---|---|
| `integrity` | at quiescence, after every relaunch | each collection opens, `PRAGMA integrity_check` returns `ok`, its `sqlite_master` equals U22's expected objects, and Meguri's reference check finds no card without its note or deck and no note without its note type; at the end of every Rust test, and of every program of a property, fuzz or fault run, after every other check of the run, a copy of each device's final collection (with its `-wal`) also opens in the oracle at schema 18 with no schema upgrade, with the oracle's clock frozen as Q3.6 says, and the oracle's Check Database on that copy returns an empty problem list and leaves `col.scm` unchanged (K22.3: after every test). The device's own file is never handed to the oracle: Check Database writes the file it checks | K21 invariants 1, 2; K22.3 |
| `converged` | at quiescence | every compared device bound to the active epoch holds byte-identical canonical payloads for every entity and the same `col.crt` (fields K21 invariant 3 excludes left out); every live media name's file has the same content on every device that holds it, and the `mediaMissing` lists are equal | K21 invariant 3 |
| `history` | at quiescence | every review-log row committed on a compared device exists on every compared device bound to the active epoch | K21 invariant 7, R2052 |
| `no-loss` | at quiescence | every committed local write is explained (below) | V22 invariant 1, R2051 |
| `no-resurrection` | always | no id-keyed entity whose tombstone a device acked or applied is live on a device of the same ancestry (below); a name-keyed entity (tag, media name, config key) is live again only after a local write that came after that device applied the tombstone | K21 invariant 6, R2052 |
| `coherent` | always, after every apply | no device holds a card whose note or deck is missing, or a note whose note type is missing or has another field count, or shows part of a structural generation or epoch that is not complete | K21 invariants 2, 8, R2053 |
| `holdback` | always | while a session is open on device X, no record the service holds from X contains an effect of a step of X's held run or detached hold (P1), and none is built for an entity of X's incoming-held set; the harness reads R from `cloud_held_run` at each barrier | P20 invariants 2, 14, R2047 |
| `undo-safety` | always | no undo or redo of a step flagged `UploadPending` or `Uploaded` succeeds | P20 invariant 11, R365 |
| `cursor` | at every engine state save | every record and record deletion the stand-in delivered before the state is durably in the inbox, in `refused` or applied, or, for a deletion of a record name that no entity and no inbox entry of the binding holds, dropped in that commit (V3.1 rule 2) | K21 invariant 5, V22 invariant 2 |
| `isolation` | always | no request of a binding reaches another account, container or environment; nothing is sent while the fence is pending, while sync is paused, from an isolated binding or by a stopping instance; zone deletions only by `DeleteZones` after an `MGActive` read naming the bound epoch | V22 invariants 3, 4, 13; P20 invariants 4, 16; R2055 |
| `stale-refused` | always | every fact and batch request from an engine object that is not the current instance changed no tracking row and got no batch | P20 invariant 9, V3.4 |
| `one-engine` | always | at most one current engine instance per device and database, and no new one before the old one's `EngineStopped` (in the keep-alive variant the old object may live; `stale-refused` covers it) | P20 invariant 7, K2 |
| `confirmed-root` | always | no collection record of an instance reaches the service before that instance confirmed the bound epoch, except a publisher before its switch | K21 invariant 20, V22 invariant 14 |
| `no-timer` | always | no device holds a timer other than U151's flush timer and the deadlines of P2 | P2, V10.3, R2059 |
| `no-anki` | always | no Anki request, no manual-transfer start | P20 invariant 1, R2064 |
| `content-free` | at quiescence, and on every log the run writes | no record name, zone name or field other than `e` and the asset field of any record, and no line of the run's logs, contains a marker string of Q4.2 | K21 invariant 9, R2058 |
| `copies` | always | no unresolved conflict copy disappears; each lists after a relaunch; no copy is made twice for one conflict | V22 invariant 8, K16, R2054 |
| `media-safe` | always | a media install never replaces a valid file with bytes whose size and SHA-256 were not checked; a local media write is replaced only with the old bytes in the media trash and a copy naming them; sync deletes no media bytes except into the trash | K21 invariants 11, 18, R2057 |
| `crt` | always | nothing is sent or applied while `col.crt` differs from the binding's epoch `crt` | K21 invariant 26 |
| `bounded` | always | at most 2 asset downloads at once; staged bytes at or below 256 MiB except one larger file alone; no download starts with free space below 512 MiB after it; soak runs add Q9's memory and file bounds | P13.2, R2057 |
| `results` | after every exchange | the exchange's result follows P12.4 and P12.6 recomputed from the request log and tracking: `succeeded` only when every selected entity has an ack at or above S_c, held back and media reported apart | P20 invariant 5, R2049, R2050 |

**`no-loss`, explained writes.** Decided 2026-10-27: a committed local write is a write of a committed op that a user step of Q4.3 ran (a study, edit, media, import or recovery step). The ops sync runs by itself (`CloudIncoming`, `CloudRekey`, a structure rollback, a K15.9 resolution, the K15.6 and K15.8 fixes) are not local writes; case 8 judges the values they produce. A part is: one field of a note; one tag of a note (present or absent); one config key; a deck's leaf name and its parent; a card's scheduling, taken whole; the content of a media name; whether an entity exists; any other entity, taken whole. The harness keeps every committed local write (entity, part, value) and, per device, the set of writes it had applied when it made each write. A committed write W is explained when one of these holds at quiescence:

1. W's value is the final value of its part;
2. a later write W2 of the same part replaced it, and the device that made W2 had applied W first (W2 came after W);
3. W was concurrent with the final write, and the contract resolves the part without a copy: card scheduling taken whole from the side with the newer review, with both review rows kept (K15.3); tags and the deck tree by their merge rules (K15.6, K15.8); config by K15.8;
4. a conflict copy of W's version exists on a device that met the conflict (K16);
5. W was undone before any record of it was handed; or, in a run with a `power` fault, W's commit came after the last `flushed` line the child journaled (`conformance/cloudkit-faults#F2` item 2), and W is in no device's collection. A `kill` never explains a missing committed write (`storage/open-and-connections#C9.3`), and neither does a review session that a crash ended: after a relaunch its newest answer is sent like any change (P8.4);
6. W changed an entity that a later deletion removed, and an `edit_vs_delete` copy holds it (K17.1, K17.3);
7. a replacement the user confirmed (below) discarded W on purpose (V22 invariant 5), and W's value is in the safety backup of the replaced local collection (V22 invariant 6; the harness opens the backup), or in the previous epoch's zone that the replacement keeps (K18.5, V22 invariant 6), or still in the local collection of a device that the replacement left unbound from the active epoch (keep local only, an account switch: its binding is isolated or another account's);
8. W went into a value that a contract rule computed from explained writes, and the part's final value equals that rule's result, which the harness computes again from those writes: a K14 re-key (the moved key), the K15.6 cycle and `+` name rules, the K15.8 tag and config merges, a K15.9 hash move with its note rewrite, a K19.6 rollback.

Nothing else is a pass: a final value that is neither a committed write's value nor a case 8 result fails the check.

**Confirmed replacements and ancestries.** A confirmed replacement is a step that replaces a collection with the user's confirmation (V22 invariant 5): a restore or `.colpkg` import (V12.1), an adoption of the active or the previous epoch (V14), Replace iCloud collection (V15), the choices of V13.5, keep local only and stop iCloud (V16); and V13.3's adoption into a device with no collection. The account events of V9.1 that bind a device to another account are treated the same way. Each such step starts a new **ancestry** on the devices whose collection or epoch it replaces; a device that adopts an epoch joins that epoch's ancestry. From that step:

- `converged` and `history` compare only devices of one ancestry that are bound to the active epoch, and `history` only the review rows committed in that ancestry or carried into it by its replacement; a device that is isolated, local only or bound to another account is compared by `no-loss`, `integrity`, `coherent` and `copies` only;
- `no-resurrection` counts only tombstones acked or applied within the entity's ancestry: an entity that a confirmed replacement brings back (a restored backup that still holds a deleted note, a publication of a collection that never applied a tombstone) is not a resurrection;
- `no-loss` explains a write the replacement discarded only through case 7.

This limit applies only from a step that Q4.3 lists under "Recovery calls", a `restore_backup` or a `sign_in` with another account, and only when the contract makes that step replace or unbind something. No other step starts an ancestry, so a random program of Q8.2, which draws none of them, runs every check over every device; the soak (Q9.1) starts ancestries at its scheduled events only.

## Q6. Multi-device scenarios

Every row runs on the fake from `setup` with base `small` unless it names another, and makes every check of Q5 at the time Q5 gives, with Q5's ancestry rule for the rows that confirm a replacement or switch an account. The Checks column names the checks a row is built to exercise, which U246's coverage report reads; it turns no check on or off. Signed: the row's leg in Q7.5, where it has one.

| Name | Devices | Steps | Expected end state | Checks | Signed |
|---|---|---|---|---|---|
| `holdback_thirty_answers` (AE9) | A, B | auto on, A foreground: `start_session(A)`, 30 `answer`s 5 s apart; `run(A)` for 120 s; `background(A)`, `expire(A)`; `foreground(A)`; `undo(A)` once; `answer(A, …)`; `end_session(A)`; `drain` | during the session the service holds the first 29 answers (cards, review rows, deck) and never the newest; the flush timer asks for a send at most 60 s after a sendable change; the undo succeeds; after `end_session` every answer reaches B. Variants: `kill(A)` during the session, then `relaunch(A)`: the lost session's newest answer is sent; Mac `quit(A)` after `end_session`: the quit flush sends it | `holdback`, `undo-safety` | `holdback` |
| `incoming_change_drops_held_run` | A, B | `start_session(A)`, `answer(A, c1)`, `browse_edit(A, n, Q)`; `edit(B, n, A)`, `drain(B)`; `run(A)` until A applies B's version; `answer(A, c2)`; `run(A)`; `end_session(A)`; `drain` | after the apply, A's undo history is cut below the step that touched n; until `end_session` no record of n leaves A; after it, n holds A's field `Q` and B's field `A` on both devices | `holdback`, `undo-safety` | |
| `held_card_image` | A, B | A and B offline; `start_session(A)`; `answer(A, c, t1)`; `answer(B, c, t2)`; `answer(A, c, t3)` with t1 < t2 < t3, so R is the t3 answer and both of A's answers are in one session; `online(A)`, `run(A)` until A's barrier send of c lands; `online(B)`, `run(B)` until B has applied A's image and sent its own version; `undo(A)` (A has not fetched B's version yet); `run(A)`; `end_session(A)`; `drain` | A's held image of c carries `last_review_id` t1; B keeps t2's scheduling; every device ends with t2's scheduling and the review rows of t1 and t2 | `holdback` | |
| `sync_now_partial_failure` (AE91) | A, B | `offline(B)`; `edit(A)` 3 notes, `answer(A)` 2 cards, `add_media(A)`; `inject(fail{internalError}, record: card c1, send)`; `sync_now(A)`; then without that injection, with the media file's upload delayed past the exchange (`inject(delay{…}, send, record: blob)`, the file's `MGBlob`), `sync_now(A)`; `online(B)`, `drain`. Variant `media_off`: `media(A, off)` before the second `sync_now(A)` | first result `incomplete` with c1 outstanding, `needsAttention{exchangeIncomplete}`, c1 pending, nothing reported done for it; second result `succeeded` with media `pending{files, bytes}` naming the delayed file (in the variant, media `paused`), the item cleared, and no text that claims B, still offline, has the changes; B converges | `results` | `sync_now` |
| `kill_then_account_switch` (AE92) | A, B | `edit(A, n)`; `kill(A, cloud.send.after_commit:kill)`; `relaunch(A)`; `run(A)`; `edit(A, m)` offline; `sign_in(A, acct2)` (switch); `online(A)`; `run(A)` | the edit of n reaches the service exactly once (one saved version with A's writer); after the switch A's binding is `account_changed`, the edit of m stays pending in A's tracking and local collection, acct2's database receives no record from A, and V13 is offered for acct2 | | `account_switch` |
| `offline_same_card_and_notetype` (AE93) | A, B, C | `offline(C)`; `delete_note(A, n9)`, `drain(A, B)`; `offline(A)`, `offline(B)`; `answer(A, c1)`; `answer(B, c1)` later; `add_field(A, QA, X)`; `edit(B, n2, Q)`; `online(A)`, `online(B)`, `drain(A, B)`; `advance(10 d)`; `edit(C, n3, Q)`; `online(C)`; `drain` | both reviews of c1 exist on every device and c1's scheduling is from B's later review (K15.3); B and C show QA's notes only in the new layout, never partly; B's edit of n2 maps into the new layout; n9 and its card stay deleted on C; C's edit of n3 is kept | | `offline_reviews` |
| `quota_during_media_upload` (AE94) | A, B | base `media`; `quota(acct1, collection bytes + 10 MB)`; `add_media(A)` 20 files of 5 MB with notes; `answer(A)` 10 cards; `run(A)`; `relaunch(A)`; `export(A)`; `quota(acct1, unlimited)`; `foreground(A)`; `drain` | the collection records save, the uploads stop at `quotaExceeded` with `needsAttention{quotaFull}`, media progress separate from collection progress; after the relaunch the blobs are still pending and answers usable; export succeeds; after the trigger every blob uploads once (one `MGBlob` per content on the service) and B downloads them; no timer was armed for quota | `bounded`, `media-safe` | `quota` |
| `field_merge_and_copy` | A, B | offline both; `edit(A, n, Q, x)`; `edit(B, n, A, y)`; `edit(A, m, Q, x2)` and later `edit(B, m, Q, y2)`; online, `drain` | n holds x and y with no copy; m holds the later-stamped value, and the device whose version lost holds a `field_conflict` copy of it, which `recover` restores as a normal edit that then syncs | | |
| `delete_vs_edit` | A, B | offline both; `delete_note(A, n)`; `edit(B, n, Q)`; online, `drain`; `recover(B, copy)`; `drain` | n is deleted on both; B holds an `edit_vs_delete` copy; the recovered note has a new id and `guid` and reaches A | | `delete_vs_edit` |
| `tags_config_decks_merge` | A, B | `tag(A, n, +t1)`; `add_deck(A, X)`, `add_deck(A, Y)`, `add_deck(A, P)` in that order, so id(X) < id(Y); `drain`; offline both; `tag(A, n, -t1)`, `tag(B, n, +t2)`; `set_config(A, nextPos, 45)`, `set_config(B, nextPos, 43)`; `move_deck(A, X, Y)`, `move_deck(B, Y, X)`; `rename_deck(A, P, Q)` while `add_deck(B, P::C)`; `add_deck(A, N)` and `add_deck(B, N)`; online, `drain` | n's tags are its base tags (Q4.2) and `t2`, without `t1`; `nextPos` 45; Y, the larger id on the cycle, top-level with X under it (K15.6); the child is `Q::C`; of the two `N` decks, the one with the larger id gets the `+` suffix (K22.1 rows) | | |
| `id_collision_rekey` | A, B | offline both; the harness forces the next note id and review id equal on A and B; `add_note(A)`, `add_note(B)`; `answer` the new cards; online, `drain` | the unacked side is re-keyed; both notes and both review rows exist, the re-keyed review within milliseconds of its instant (K14) | | |
| `same_apkg_two_offline` | A, B | offline both; `import_apkg(A, f)`, `import_apkg(B, f)`; online, `drain` | no duplicate note, card, note type or review row; decided 2026-10-27: each package deck exists once per importing device, with one name and two ids (`package/apkg-import#I10.2` step 5 gives an added deck a new id, as Anki 26.9.3 does), and the one with the larger id is renamed with `+` (K15.6); every card of the package is in exactly one of them (K22.3, corrected per Q13) | | |
| `notetype_claim_race` | A, B | offline both; `add_field(A, QA, X)`, `add_field(B, QA, Y)`; `edit` notes of QA on both; online, `drain` | one claim wins; the other device rolls back (K19.6) with `structure_rollback` copies; no device ever applies a note in the losing layout; edits made in the losing layout are in copies or mapped | | `structure` |
| `abandon_three_devices` | A, B, C | `reorder_fields(A, QA)`, A claims and sends some members, `offline(A)`; `advance(8 d)` with drains on B and C; `abandon(B, g)`; `drain(B, C)`; `online(A)`, `drain` | as V23.2 "Abandon on three devices": C's waiting entries become copies, B's restoring generation is claimed with no conflict, A adopts it and ends with QA in the restored layout and its edits under the right field names | | |
| `epoch_switch_offline_device` | A, B | `offline(A)`; `edit(A)` 5 notes; `publish(B)` (Replace iCloud collection, confirmed); `online(A)`; `run(A)`; `adopt(A, active)` after its preview | A sends nothing to the old zone before its root fetch drains; A is isolated `epoch_switched` with the 5 edits pending; the preview counts them; after adoption A's collection equals B's, the 5 edits are in A's safety backup. Variant: a switch between A's root fetch and its send: the saves land in the old zone and the preview counts them as sent after the replacement | | `replace` |
| `first_uploads_race` | A, B | both non-empty, no `MGActive`; `connect(A)`, `connect(B)`, both choose upload; `run` both | one switch wins; the other device waits at V15.2 step 5 with its three choices; after `choose(adopt)` both converge on the winner's collection | | |
| `connect_cases` | A, B | the six cases of V13.3 and the four choices of V13.5, each from fresh devices against an epoch A published | each case's flow; an empty device adopts with no confirmation, its collection is usable before its media completes (R158, R1592); a non-empty device is never merged | | `connect` |
| `restore_fence` | A, B | `restore_backup(A)`; `relaunch(A)`; `sync_now(A)`; then `choose(A, replace_icloud)`; `run(B)`; `adopt(B, active)`; `drain`. Variant: `choose(A, keep_local)` | while the fence is pending nothing is sent or fetched for A's old binding, across the relaunch and Sync Now (`decisionRequired`); the publication has `MGEpoch.reason` 3; B is offered and adopts it; in the variant A is local only and B is unchanged | | |
| `zone_deleted_by_user` | A, B | `edit(A)` offline; `purge_settings(acct1)`; `online(A)`; `run` both; `publish(A)`; `run(B)`; `adopt(B, active)`; `drain` | both isolated `zone_deleted_by_user`, nothing recreated by an isolated binding, A's unsent edit intact; the publication creates the root and media zones; B adopts; media re-uploaded by a device that holds each file | | `zone_deleted` |
| `encrypted_data_reset` | A, B | `reset_encrypted(acct1)`; `run` both; `publish(A)`; `adopt(B, active)`; `drain`. Variants: `survivors` with `meguri-media` unreadable (`e` absent; then the decryption error); `survivors` with `root`'s `e` present but undecodable (it fails Meguri's decode; then it decodes to a newer `format`); with a readable `root` another device recreated | both isolated `encrypted_data_reset`; A's publication creates `meguri-root`, `meguri-media` and a new `MGMediaRoot`; an unreadable surviving `meguri-media` is deleted and recreated only on K2's sign; a present but undecodable `e` deletes nothing and the op waits at its step (V15.2 step 3, V26.1 "any other error"); a readable `root` gives its key and nothing is deleted | | |
| `media_zone_missing` | A, B | `delete_zone(acct1, meguri-media, deleted)`; `add_media(A)`; `run` both; another device publishes again | collection sync continues; media stops for good on both old bindings with `mediaZoneMissing`; neither old binding uses a recreated media zone | | |
| `sign_out_resume` | A, B | `edit(A)`; `sign_out(A)`; `sign_in(A, acct1)`; `resume_binding(A)`; `drain`. Variant: `sign_in(A, acct2)` | isolated `signed_out`, then Resume offered for the same identity; the scan runs before the first send and the edit converges; in the variant no Resume, V13 for acct2, nothing reaches acct2 | | `sign_out` |
| `late_callbacks_after_switch` | A | `inject(stall, send)`; `edit(A)`; `run(A)`; `sign_in(A, acct2)`; release the stalled results; variant `keep-alive` | every late fact and batch request of the old instance is refused, tracking unchanged by it, no record to acct2, no record sent twice | | |
| `two_containers_shared_dir` | A (two builds) | build 1 (container c1) binds and edits; build 2 (container c2) opens the same data directory, binds to its own account database and edits; each build relaunches | each build has its own active binding; neither writes the other's rows; a media removal one build has not consumed survives the other's consume (K22.3) | | |
| `media_first_and_visible` | A, B | base `media`; `connect(B)` empty; B starts a review at once; `kill(B)` during a download; `relaunch(B)`; `drain` | B's collection is usable before media completes; the first download is a blob of the card on screen (`VisibleMedia`); at most 2 downloads at once; the relaunch fetches only blobs not installed; no valid file is replaced by an interrupted install | `bounded`, `media-safe` | `media` |
| `media_content_conflict` | A, B | offline both; `change_media(A, a.png, C1)`, `change_media(B, a.png, C2)`; also both `add_media` a new name `b.png` with different content; online, `drain` | `a.png`: the later version by payload digest wins, the other bytes in the media trash with a `media_content` copy; `b.png`: one side moves to its hash name and its notes are rewritten (K15.9) | `media-safe` | |
| `stale_and_fresh_blob_deletion` | A, B | the two K22.3 runs "Stale blob state" and "Fresh deletions", with A offline 30 min and then 2 h | no device ends with `mediaMissing`; within 1 h the deletion is sent and B uploads again; after 1 h A computes no deletion | | |
| `tombstone_long_offline` | A, B, C | `offline(C)`; `delete_note(A)` 3 notes, `delete` a deck; `drain(A, B)`; `advance(200 d)` with daily drains of A and B; `edit(C)` one deleted note and one live note; `online(C)`; `drain` | purging stays off (A9b taken as false), so the tombstones stay; nothing deleted comes back; C holds an `edit_vs_delete` copy for the deleted note; the live note's edit is kept | | |
| `previous_epoch_restore` | A, B | `publish(B)` after A edits; `adopt(A, previous)` | A is local only with the previous epoch's content; nothing is sent; `MGActive` unchanged | | |
| `expired_token_and_throttle` | A, B | `expire_token(A, collection, surfaced)` then `silent`; `inject(fail{requestRateLimited, 120}, send)`; edits on A; `sync_now(A)`; `advance(120 s)`; `drain` | edits stay pending through both; the throttled Sync Now ends `waiting{throttled}` at once; no absence pass runs for the engine's restart; no app timer; convergence once the engine's schedule retries | `results` | |
| `no_state_resume_absence` | A, B | `sign_out(A)`; B adds 300 notes; `sign_in(A, acct1)`, `resume_binding(A)`; the fetch from no cursor in pages of 50, `kill(A)` after the first apply; `relaunch(A)`; `drain` | the absence pass completes with no `deletion` entry for any of the 300 notes or their cards, and they stay on both devices (K22.2) | | `no_state` |
| `adoption_over_local_media` | A, B | B holds local media files: some the epoch does not name, one it names with other content; `adopt(B, active)`; then the V23.2 "Adoption over local media" steps | withheld files are never uploaded until chosen; the superseded file's epoch bytes win with the local bytes in the trash; incoming versions follow V14.4 | `media-safe` | |
| `suspended_events` | A, B | `edit(B)`; `background(A)` and suspend with an engine fetch and a send result waiting in the adapter; `kill(A)` before `resume`; `relaunch(A)`; `drain` | the unsaved results leave the entity pending with its in-flight entry; the cursor is older and the records are fetched again; convergence with no duplicate | | |
| `optimizer_race` | A, B | pending U47: both run the optimizer after a fetch barrier, then both commit new presets | U47's rule; both converge on one preset state with no partial training result | | `optimizer` (`cloudkit-two-device-optimizer`) |
| `browse_edit_during_exchange` | A | pending U210: a Browse edit while `sync_now(A)` runs | the edit is not selected, stays pending, and the next exchange sends it | `results` | |

- A row marked "pending Unnnn" is listed now and wired when that unit lands (U248's dependencies).
- Each variant is its own test.

## Q7. The signed-account lane

### Q7.1 What only real CloudKit proves

Fake runs prove Meguri's side of the contract; they cannot prove Apple's (R2064). The signed lane proves, against the development environment of the development container:

- Apple's delivery: pushes, the engine's automatic scheduling, convergence of several real devices (R2064);
- every assumption the contract lists for the signed runs that a test account can show: A1 to A10 (A1.1 included; A9b only once it is shown, K22.3), A20, A21, A22 and A23 (K22.3), A11 to A19 (V23.2), each through the leg Q7.6 names, or recorded `unverified` there with its reason; and the engine checks P21 lists (a restricted fetch scope still reports a deleted `meguri-media`, the same-identity `signIn` and its reset, manual calls offline and throttled, `cancelOperations`, transient authentication retries, a fetch from no cursor, the joined fetch, the excluded zone with unfetched changes, the released engine object ending);
- account isolation and account events (R2055), quota recovery (R2059) and CKAsset behavior (R2057), which R2064 names;
- the schema's encryption (R2058, AE96).

### Q7.2 User steps

Agents never perform these (R2065, `docs/process/one-time-setup.md`). The lane reports `user-step` with the missing step's id until each is done.

| Step | What the user does | Needed by |
|---|---|---|
| `cloudkit-dev-container` | creates the development CloudKit container, gives the Debug and test builds its identifier and the CloudKit entitlement, and keeps the daily-driver container separate | every leg |
| `cloudkit-test-accounts` | provides two test Apple accounts (acct1, acct2) used for nothing else, and signs acct1 in on the macOS test VM and on one iPhone and one iPad simulator of the VM's pool reserved for this lane | every leg; acct2 for `account_switch` |
| `cloudkit-management-token` | creates a CloudKit management token for the development container, stored where `tools/mg` reads it | `schema` |
| `cloudkit-quota-account` | prepares acct2 with less than 100 MB of free iCloud storage | `quota` |
| `cloudkit-assisted-session` | is present for the assisted legs below (sign-out, account switch, deleting the container's data in system settings) | assisted legs; deferred to the final session with the other manual checks |

- The run refuses to start, with failure `wrong-container`, when the build's container is not the development container or its environment is not development, and when a device's signed-in account is not one of the test accounts (AE96: test builds cannot address the production container).
- Decided 2026-10-27: before a leg that needs an empty database, the agent deletes the Meguri zones of the test account's development database with a test-hooks-only call that refuses any container other than the development one. Release builds do not contain it (KTD18, U255).
- No leg uses the private collection, `local-data/` or any real study data (R2065, U248). Data comes from Q4.2's bases and U11's generated fixtures.

### Q7.3 Devices and the agent

The executable command and durable JSON-lines journal protocol is `conformance/cloudkit-agent`. Its smoke and protocol tests do not replace this lane's required signed legs or Q7.4's whole-lane evidence.

- Devices: the signed Debug Meguri app on the macOS test VM, and on the reserved iPhone and iPad simulators there. Nothing runs on the host Mac's screen (AGENTS rules); `tools/mg cloudkit-signed` coordinates from the host.
- Each device runs `CloudKitSoakTests` as an agent: an app-hosted test in the signed Debug app with test hooks (it uses the real `CloudKitSyncAdapter`, `SyncCoordinator` and CKSyncEngine). The coordinator gives each agent its steps (Q4.3's names, those a real device can do) through a command file in the run directory and reads its JSON-lines step log back.
- Offline on one device: the agent pauses sync (V16), or the coordinator terminates the app for that period and relaunches it after (as for kills, below); a leg that needs the network itself unavailable (A22) cuts the VM's network for all devices, through the VM tooling.
- Kills: `tools/mg` terminates the app process (simulator terminate, or SIGKILL of the Mac app in the VM). The agent runs inside that process and dies with it, so the host coordinator (`tools/mg cloudkit-signed`) relaunches the app-hosted test run on that device. The new agent reads the command file and its own step log, finds the last step it logged as done, and continues from the next one; a step logged as started and not done is the step the kill interrupted, and the agent checks its outcome (the leg's expected recovery) before it continues. The same holds for the kill every 45 minutes of Q9.2 and for an app terminated as an offline period (above).

### Q7.4 Evidence

Each run writes `build/mg/cloudkit-signed/<run>/result.json` using the existing `tools/mg-result.schema.json` envelope. Cloud evidence adds no top-level fields and does not change that envelope's schema version. The envelope supplies the job, clean-source commit, run times, failures, artifacts and user steps. `details.lane` is `cloud-signed`. A schema-validated JSON artifact, `signed-evidence.json`, carries the following evidence, bound to the envelope's job, run id and source commit:

- `lane: cloud-signed`, the build commit and whether the tree was clean, the container identifier and environment, each device's kind, OS build and app build, and account labels (`acct1`, `acct2`, never an address or a user record id);
- per leg: its steps with instants, each outcome, every CloudKit error code seen, and its pass or fail;
- the assumption ledger: per assumption of Q7.6, the leg that checked it, the observed behavior and `holds`, `false` or `unverified` (with Q7.6's reason). `unverified` is allowed only where Q7.6 says so, and does not fail the run. Decided 2026-10-27: a run with `--leg` fills only the entries of the legs it ran and is judged on those alone. In a run of the whole lane, a non-assisted leg that did not run fails the run, and an assisted leg not yet run leaves its entries open and makes the run `user-step` (Q7.6). An assumption found false fails the run, and the contract is corrected (spec-writer commit, its "If false" column) before the lane can pass;
- the final comparison: per device, a digest of every entity's canonical payload, the review-row count, and the media digest;
- logs, which pass `content-free` (Q5).

The runner validates this artifact against the actual signed builds, device-agent step logs and leg results before writing a passing cell. A supplied declaration of success, build/signing receipt alone, or fake-engine self-test is not signed evidence. The fake self-test has its own fake lane and cannot populate a signed cell.

`details.scenarios.cloudkit-signed` records the whole-lane outcome only after the Q7.4 aggregation below; a passing `--leg` run records that leg's evidence but never sets the whole-lane cell to pass by itself. Missing setup or assisted steps use the MG envelope's `user_steps`, `status: warn` and `details.outcome: awaiting_user`; a machine-blocked run uses MG's queued convention. Neither is a pass. An unrun required non-assisted leg, false assumption or failed evidence check is a failure. The whole-lane aggregator may reuse valid per-leg results only from the same clean source commit and verified development container, environment and test-account scope; it validates the referenced artifacts and retains their run ids. It does not accept copied outcome fields without their underlying evidence.

The milestone gate reads it as the `u248-soak` run's item `cloudkit-signed` (`docs/process/milestones.yaml`). Decided 2026-10-27: that item needs, for one clean commit, a pass of every Q7.5 leg that is not pending, from one whole-lane run or from per-leg runs of that commit, whose ledgers together fill every Q7.6 entry. A result of another commit never counts. Until the assisted legs have passed, the item is `user-step`.

### Q7.5 Legs

| Leg | Devices | What it shows | Assisted |
|---|---|---|---|
| `holdback` | Mac, iPhone | Q6 `holdback_thirty_answers` with Apple delivery: the iPhone never shows the Mac session's newest answer during 10 minutes of the open session, and shows it within 5 minutes after the session ends | no |
| `sync_now` | Mac, iPhone | an edit, then Sync Now on the Mac: `succeeded`, and the iPhone's fetch then holds the edit; media reported apart | no |
| `offline_reviews` | all three | Q6 `offline_same_card_and_notetype` with pause as offline | no |
| `delete_vs_edit` | Mac, iPad | Q6 `delete_vs_edit` | no |
| `structure` | Mac, iPhone | Q6 `notetype_claim_race`: the claim batch is atomic in its zone (A8) | no |
| `replace` | all three | Q6 `epoch_switch_offline_device`; the previous epoch's zone still exists after the switch; the adopting devices' staging fetch runs outside the engine (A14) | no |
| `connect` | iPad | an empty device adopts the active epoch (V13.3 row 2); collection before media; its staging fetch from no cursor runs outside the engine and returns every record of the zone (A14) | no |
| `media` | Mac, iPhone | base `media`: uploads, downloads, a kill during a download, a 100 MiB asset (A2), record-ID reads with named fields download no asset (A4); a kill between an asset upload and its record's save leaves an asset that no read and no other device sees (A11, visible part); each upload's staged file is an APFS clone of the media file (A16) | no |
| `no_state` | iPhone | an instance with no saved state lists every existing zone and none that is gone; an interrupted first fetch never drains early (A7, A9a) | no |
| `engine` | Mac | the P21 and K22.3 engine checks of Q7.1 that need no account change: joined fetch, scheduled sends after adding names, fetch scope and the excluded zone, manual calls with the VM offline and when throttled, `cancelOperations`, the released object ending within a bounded time (V3.4), A20, A21, A23; an edit while its older revision is being sent stays pending after that sent-event handler returns and is sent on the next eligible send, without another local edit or full update (A23, K12.4); a record and a request at K7's limits save and one over fails with `limitExceeded` (A1, A1.1); a new-record save over an existing name and a save over a deleted record, forced through test hooks, fail as K23 states (A5); per-record results arrive before the state that reflects them, from the event order in the step log (A6); a launch with the VM offline after an earlier identity read gets no cached id (A19) | no |
| `schema` | host | exports the development schema with the management token: exactly the six record types of K3, each with `e` as an encrypted bytes value and only K3's asset field, no index on a custom field, while every other leg syncs with that schema (A3); a record read back has no other field (R2058, AE96) | no |
| `ui_status` | iPhone | `app/MeguriUITests/Sync/`: after `sync_now`, the status reaches `upToDate`; with the network cut, `offline` | no |
| `quota` | iPhone on acct2 | Q6 `quota_during_media_upload` with Apple's quota error: per-record `quotaExceeded` (A13's per-record part), status, recovery after the user frees space | yes |
| `sign_out` | iPhone | Q6 `sign_out_resume` with a real sign-out and sign-in; the engine's account events and its reset (A12, A19) | yes |
| `account_switch` | iPhone | a real switch to acct2: nothing reaches acct2's database; V13 offered | yes |
| `zone_deleted` | Mac, iPad | the user deletes the container's data in system settings: the reported reason (A10, A15), Q6 `zone_deleted_by_user` | yes |
| `optimizer` | Mac, iPhone | pending U47: Q6 `optimizer_race` with Apple delivery; the milestone item `cloudkit-two-device-optimizer` | no |
| `soak` | all three | Q9.2 | no |

- Decided 2026-10-27: no signed leg triggers an encrypted data reset, which would need account security changes beyond a test step. A18 stays covered by the fake and by V10.1's fallback, which deletes nothing on any sign other than K2's; the ledger records A18 as `unverified`, which does not fail the run.

### Q7.6 The assumption ledger

Decided 2026-10-27: each assumption has one entry in the ledger (Q7.4), filled by the legs below. `unverified` is allowed only for the entries this table marks so; every other entry must be `holds` for a pass, and a `false` fails the run (Q7.4).

| Assumption | Leg | Note |
|---|---|---|
| A1, A1.1 | `engine` | |
| A2 | `media` | |
| A3 | `schema` | |
| A4 | `media` | named-field reads only; false for the engine's fetch by K23 |
| A5 | `engine` | |
| A6 | `engine`, every leg's step log | |
| A7 | `no_state`, `engine` | |
| A8 | `structure` | |
| A9a | `no_state` | |
| A9b | none | `unverified`: taken as false (K23), so purging stays off; no test account can show the engine's silent restart |
| A10 | `zone_deleted` | assisted |
| A11 | `media` | the visible part holds or fails; the removal of the orphan bytes is not observable by an app and stays `unverified`, which costs nothing (V24: staged uploads would need their own cleanup, and the sweep of V2.4 step 6 already deletes local staged files) |
| A12 | `sign_out` | assisted |
| A13 | `quota` | assisted; A13 is false by the SDK headers for quota exceeded (V24), so the entry checks the rule that replaces it: the engine drops quota-failed names and reports `quotaExceeded` per record (K12.4); `holds` means that rule holds |
| A14 | `connect`, `replace` | |
| A15 | `zone_deleted` | assisted |
| A16 | `media` | |
| A17 | none | `unverified`: a damaged collection on a signed device is U251's screen check; V19 reads the status and identity again before offering its entry, so the fallback is safe |
| A18 | none | `unverified` (Q7.5 above) |
| A19 | `engine` | |
| A20, A21, A22, A23 | `engine` | |

- An assisted leg not yet run leaves its entries open; a whole-lane run, and the milestone item, is then `user-step`, never a pass (Q1, Q7.4).

## Q8. Properties and fuzzing (U118)

### Q8.1 Hold-back property

- Programs of 20 to 80 events on device A with an open review session, and one or two other devices: answers, undo and redo, bury, flags, edits, adds, deletions, Browse edits, session begin and end, `background` and `expire`, Sync Now attempts (refused during a session, P12.1 row 2), engine runs, incoming edits from the other devices to cards and notes of A's session, rollovers, and `kill(A)` with relaunch. Weights favor answers and incoming edits to the session's cards.
- Checks: `holdback`, `undo-safety`, `no-loss`, `coherent`, and `converged` at the final drain after `end_session`.
- Volume: 200 programs when `meguri-sync`'s cloud code or the undo engine changes (T0); 10,000 nightly (T2).

### Q8.2 Multi-device convergence fuzz

- Programs of 50 to 400 steps on 2 to 4 devices: every study, edit and media step of Q4.3, network changes, pushes, delivery reordering by the scheduler, Sync Now, clock skew up to 10 minutes, injected errors (Q3.5) at a seeded rate, and kills at random fault points (the child-process lane, `conformance/cloudkit-faults#F6`). Structural changes and imports at low weight. Account events, recovery calls and `restore_backup` (a confirmed replacement, Q5) are not drawn: Q6 covers them, and Q9.1's driver adds them on its schedule.
- The program ends with every device online and `drain`.
- Checks: every check of Q5.
- Idempotence: after the final drain, a `sync_now` on each device sends no record and applies nothing, and its collection is byte-identical before and after.
- Volume: 200 programs nightly (T2); weekly 2,000 programs or 2 hours, whichever comes first.

### Q8.3 Boundary property

Random edits on A during `sync_now(A)`, at every step of P12.3: no entity committed after the capture is selected, every entity committed before it is selected, and `results` holds (R2048). 500 programs nightly.

### Q8.4 Seeds, shrinking and reports

- Each run logs its seed. A failing program is shrunk by removing steps while the same check still fails, then saved with its seed under `core/crates/meguri-testkit/tests/regressions/cloud/`, and runs in every later run.
- The report names the seed, the check, the step index, and for `converged` or `no-loss` the entity, part and differing values.
- These generators and Q8.1's are the only CloudKit generators; Q9 reuses them, and its driver adds only Q9.1's scheduled recovery and account events.

## Q9. The soak (U248)

### Q9.1 Fake soak

- **Run**: weekly (T2), `tools/mg soak --lane cloud-fake`, at most 2 hours of wall time. 4 devices (one Mac-like, three iOS-like), base `10k` with 500 media files, one seed.
- **Simulated time**: 90 days on the fake clock. Each simulated day each device is online for 1 to 4 periods and offline in between; one device is offline for a whole simulated week once per 30 days (the delayed rejoin).
- **Workload per simulated day** (these mean counts). Q8.2's generator draws the study, edit, media, network, Sync Now and kill steps. Decided 2026-10-27: Q8.2 draws no recovery call and no account event, so the soak driver (`soak.rs`) adds the Replace iCloud collection, adoption, sign-out and Resume events below itself, as Q4.3 steps at seeded instants, and the ancestry rule of Q5 applies from each of them that replaces or unbinds something: 200 answers in 1 to 3 review sessions per device; 20 note edits; 5 notes added, 2 deleted; 3 tag changes; 1 deck change; 1 config change; 5 media adds and 1 change; 2 Sync Now per device; 3 background-foreground cycles per iOS-like device. Every 10 days: a structural change; every 15 days: an `.apkg` import on one device; every 30 days: a Replace iCloud collection on one device followed by adoptions; at seeded instants: 1 kill per device per day at a random fault point, 1 token expiry, 2 throttling periods, 1 partial save failure, 1 quota exhaustion and recovery per 30 days, 1 account sign-out and Resume per 45 days, 1 media download interruption per day.
- **Sampled checks**: the "always" checks of Q5 after every step; `converged`, `history`, `no-loss`, `no-resurrection` and `copies` at a drain of all devices at the end of every simulated day and at the end.
- **Bounds**: per device peak resident memory at most 300 MB; staging as Q5 `bounded`; the tracking file at the end at most 2 times its size at the first daily drain plus 1 MB per 1,000 entities added since; no inbox entry older than one simulated day at a daily drain, except entries waiting for a structural generation of an offline device; no conflict copy beyond those the scenario's conflicts explain.
- **Pass**: no check fails; every daily drain reaches quiescence; the end comparison passes. The result records the seed, event counts per kind, every failure and recovery event, the bounds observed, and the final digests.

### Q9.2 Signed soak

- **Run**: before the M1R real multi-device trial (`docs/process/milestones.yaml`), `tools/mg cloudkit-signed --leg soak`: 8 hours of workload, then up to 1 hour for quiescence. The three signed devices on acct1, base `10k` with 200 media files (at most 50 MB).
- **Workload**: each device studies in sessions (600 answers per device in all), 100 note edits per device, a structural change every 2 hours on one device, Sync Now 10 times per device, offline periods of 20 to 60 minutes (pause, or the app terminated) twice an hour across the devices, an app kill and relaunch on a device every 45 minutes, background and foreground cycles on the simulators, a whole-VM network cut of 5 minutes every 2 hours.
- **Checks**: from the agents' step logs and a final read of each device's core: `history` (review rows equal the answers made), `converged` at the end, `no-resurrection`, `coherent` sampled after each step an agent logs, `holdback` from the service's records as the other devices fetch them, `no-anki`, `content-free` on every log; per device peak memory from the agent.
- **Pass**: every check passes, quiescence within the hour, no crash log, no attention item other than those the workload causes, and the ledger entries this leg fills (A6, from its step logs) are `holds`. The other entries are judged by the milestone item (Q7.4).

## Q10. Acceptance criteria

A requirement is accepted for CloudKit when every criterion of its row passes on one clean commit, in each lane the row names. A signed criterion waits on its user steps; a milestone never accepts it as queued.

| Requirement | Fake lane | Signed lane |
|---|---|---|
| R2047 hold-back (AE9) | P21's hold-back tests; Q6 `holdback_thirty_answers`, `incoming_change_drops_held_run`, `held_card_image`; Q8.1 at 10,000 programs with no failure | leg `holdback` |
| R2048 Sync Now boundary | P21's Sync Now tests; Q8.3; Q6 `sync_now_partial_failure`; no `sendChanges` or `fetchChanges` from event handling (P21's U114 test) | leg `sync_now` |
| R2049 completion accounting (AE91) | Q6 `sync_now_partial_failure`, `expired_token_and_throttle`; `results` in Q8.2; partial save failure after a successful API return (P21) | leg `sync_now` |
| R2050 completion meaning | `results`: media reported apart, `paused` and `pending` never shown as done; no result text claims other devices received anything (P12.6) | legs `sync_now`, `media` |
| R2051 crash windows (AE92) | every row of `conformance/cloudkit-faults` in every listed mode; Q8.2 with kills; Q6 `kill_then_account_switch`, `suspended_events` | app kills in leg `soak` |
| R2052 identities, tombstones, concurrent reviews (AE93) | K22.1's merge tables; Q6 `offline_same_card_and_notetype`, `id_collision_rekey`, `tombstone_long_offline`, `delete_vs_edit`; `history` and `no-resurrection` in Q8.2 and Q9.1 | legs `offline_reviews`, `delete_vs_edit` |
| R2053 coherent generations | Q6 `notetype_claim_race`, `abandon_three_devices`, `offline_same_card_and_notetype`, `same_apkg_two_offline`; `coherent` after every apply in Q8.2 and Q9.1 | leg `structure` |
| R2054 recoverable copies | `copies` in every run; Q6 `field_merge_and_copy`, `delete_vs_edit`, `media_content_conflict`; quota, throttling, token expiry and deleted zones never clear pending work (Q6 `quota_during_media_upload`, `expired_token_and_throttle`, `zone_deleted_by_user`) | legs `delete_vs_edit`, `zone_deleted` |
| R2055 binding and account isolation (AE92) | Q6 `kill_then_account_switch`, `sign_out_resume`, `late_callbacks_after_switch`, `two_containers_shared_dir`; `isolation` and `stale-refused` everywhere | legs `sign_out`, `account_switch` |
| R2056 replacement previews and backups | Q6 `epoch_switch_offline_device`, `restore_fence`, `previous_epoch_restore`, `first_uploads_race`, `connect_cases`; a safety backup exists before every adoption (V22 invariant 6); the previous epoch's zone exists 29 days after a switch and is deleted after 30 | leg `replace` |
| R2057 media (AE94) | Q6 `media_first_and_visible`, `media_content_conflict`, `stale_and_fresh_blob_deletion`, `adoption_over_local_media`, `quota_during_media_upload`; `bounded` and `media-safe` everywhere; the media rows of `conformance/cloudkit-faults` | leg `media` |
| R2058 encrypted values (AE96) | `content-free` in every run; the service refuses records with fields outside K3 | leg `schema`; the `wrong-container` refusal |
| R2059 quota (AE94) | Q6 `quota_during_media_upload`; P21's quota tests; `no-timer` | leg `quota` |
| R2064 distinct verification areas | Q1's lane rules: every result carries `lane`, the milestone item `cloudkit-signed` takes only signed results, `no-anki` in every run | every result carries `lane: cloud-signed` |
| R2065 setup and isolation | the fake keeps containers and environments apart (Q6 `two_containers_shared_dir`) | Q7.2's user steps recorded; the run refuses another container, environment or account |
| R2066 reviewed contract | the trio's Status lines read Approved (R2066); every item of `sync/policy#P24` and of `sched/optimizer#O19`, confirmed or open, is closed by its test (Q11) and removed from P24 or O19 in a spec-writer commit; U246's report lists a test for every bullet of P21, K22 and V23 and every Q6 row | none |

## Q11. Confirming the items of `sync/policy#P24` and `sched/optimizer#O19`

P24 asks U113 and U114, and O19 asks U47, to re-read each item, add the test that shows the fixed rule, and close it. Decided 2026-10-27: each item's test is the one below, as P24 and O19 name it; it lives where Q2 puts the section it tests. The table follows P24's Confirmed and Open lists and O19's Open list (updated 2026-10-27, after the review of the corrections of the open items and the fourth review of those corrections). Closed test obligations remain in the table with their evidence.

| Item | List | Test that shows the rule |
|---|---|---|
| K9.5 conditions, "Fresh deletions", K10.3 `blobs` "unreferenced since", P10.3 item 2 | P24 test closed (2026-11-02): core72126, 19/19, 0 ignored; [review log](../../docs/process/review-log/cloud-media-retention-q11-20261102.md) | the P24 test (U113): acked names that become `unsyncable` or fail `quotaExceeded` mark no blob deletion until 30 days after the ack that stops naming them, over 31 simulated days; a fetched name inside the 1 h window makes the builder skip the deletion; with Q6 `stale_and_fresh_blob_deletion` and K22.3 "Names waiting in the inbox" |
| K10.2 "The account check", K11.1, K11.4 | P24 confirmed | the P24 fault test (U113): a dropped dirty mark, a kill, a relaunch whose identity read fails all session and a clean close; the next online open scans and sends the lost change; with K22.2 "the account check" |
| K2 encrypted-data-reset sign, K5.2 `MediaRoot`, V10.1, V15.2 step 3, V23.2, V24 A18, V26.1 | P24 confirmed | the P24 test (U114, fake CloudKit): on the reset path a `root` read failing with each code 1 to 37, `partialFailure` included, deletes nothing and saves no `root`; outside it an `e`-absent `root` deletes nothing; with Q6 `encrypted_data_reset` and its variants |
| K13.1 asset retries, K10.3 `inbox` retry count, P4 `RetryAssetEntries` | P24 confirmed | the P24 test (U113, U114): a re-read with the same `erev` applies; an entry entering `asset` after an open's fifth drain is still read; an entry at 3 at close gets 5 reads in the next open; with K22.2 "a retryable asset failure on fetch" |
| K17.4 absence pass and A9a | P24 confirmed | the P24 test (U113, fake engine): a no-state first fetch drained after half the zone logs the absent names and makes no `deletion` entry; with Q6 `no_state_resume_absence` |
| K12.6 "Failed saves of control records", K19.3 step 3, K19.7 `claimed`, K13.1 control records | P24 confirmed | the P24 test (U113): a lost `complete` result meets its own manifest on resend and the generation becomes `complete`; with `conformance/cloudkit-faults` `cloud.claim.after_complete_send` |
| K5.2 `Active`, K18.3 step 5, K10.3 `zones`, V2.3 | P24 confirmed | the P24 test (U114 with U113): the landed `Active` holds E1, E0 and the attempt's clock; no previous epoch over an unreadable `MGActive`; a zone seen 10 days before `done` is due 30 days after `done`; with K22.2 "previous epoch retention" |
| P10.3 deletion results and `zones` row removal, V26.1 A21 listing | P24 confirmed | the P24 test (U113 `cloud_policy`, A21 false): a listing captured before `DeleteZones` and handled after its result leaves no row; unknown item on a blob deletion sets `deleted` |
| `root_read` acceptance across K2, K12.1 rule 3, K21 invariant 20, V10.2, V22 invariant 14, V26.1, P3, P10.1 | P24 confirmed | prerequisite reviewed with core/fake evidence: current-instance proof permutations, exact durable Active on confirmation and isolation, and publisher-before-switch/`done`; `confirmed-root` remains required in every run. The selected common CloudKit rerun passed 545/545 with dirty integration inputs (core-test/20261101T193641Z-63923); actual SDK evidence remains required under P21/K23 |
| P3 `FetchDrained` "Exception, for data safety", P12.3 step 3, K12.5, V26.1 no-state read | P24 open | the P24 test (U114, fake engine with per-zone events): a same-identity `signIn` during a call with `meguri-root` deleted, also after that call's root zone fetch ended, isolates `active_missing` with no `FetchDrained{root}`; a collection zone fetch from before the `signIn` completes no absence pass; the A7 failure is logged only when the latest no-state point is the instance's creation and no zone fetch of it started for the zone, also when P4's repeat after a first call joined to a scheduled fetch is the checked call, and a call that fetched the zone before a `signIn` logs only |
| K15.9 "At open", K9.3 "At open", K10.3 `apply_intents`, V2.4 step 8 | P24 open | the P24 fault tests (U113): a kill before the move with a fixed-name write before step 8 rewrites no note; a kill before the move runs the move before the op; a rerun after every key was edited leaves no old name; a kill after the rerun's COMMIT settles with no third run; with Q6 `media_content_conflict` and the media rows of `conformance/cloudkit-faults` |
| K2 reset path, V15.2 step 3, V26.1 | P24 open | the P24 test (U114, fake CloudKit): each second-read result; a submission more than 10 s after the second read is not made; a deletion held past 30 s times out and deletes nothing; two publishers in each order where each deletion lands before the other's creation; with Q6 `encrypted_data_reset`, variant `survivors` |
| K12.3 "Refused save conflicts", K12.6 claim row, K19.4 "Refused anchor", K13.1, K10.3 `refused`, K12.1 rule 8 | P24 open | the P24 test (U113): an older accepted version fetched after a refused one is stale and keeps the row; a newer one of the origin deletes it; with K22.2 "a refused save-conflict version" |
| K19.4 "Another generation fetched first", K19.2 "Joining", K19.5, K12.6, K12.3, K12.4 "Failed saves", K12.1 rules 7 and 8 | P24 open | the P24 tests (U113, two devices): a `local` generation waiting for a `claiming` one joins it when that one is `lost`; an edit on a `lost` generation's notetype joins it; a note of it waits until `rolled_back`; an `asset` entry decides when it settles; no claim anchor is added back alone; a claim is still built whole after the record bound is halved, and repeated `limitExceeded` reports `claimStuck`; with K22.2 "control records" |
| K12.4 "Between full updates", K12.3 "Saved", A23, P4 `UpdatePending{changes}`, P12.3 step 4 | P24 confirmed | the P24 adapter tests (U114): an edit made while a send is in flight is handed after Saved; a deferred add never overtakes a later removal or a deletion add; with auto off 400 selected records whose first 250-record batch fails `limitExceeded` are all sent within 8 calls; with K22.2 "a save of revision 3 in flight while the user edits to revision 4"; fake-test evidence reviewed; signed A23 remains required under P21/K23 |
| V17.6, V2.4 step 8, V14.2 steps 6 and 9, V15.2 steps 1, 2 and 4, V13.4, P10.3 item 4 | P24 open | the P24 `cloud_recovery` fault tests (U113): the binding before's intents are settled before the new binding's scan; the preview's backup notes; a salvage binding's row goes with its last copy; with Q6 `epoch_switch_offline_device` and V23.2 "A waiting generation at Replace iCloud" |
| V10.2 "Which value the mark records", "Which mark counts", V2.3 "replaced through", K5.2 `Active`, K18.3 step 5, K10.3 `bindings`, P3 "The start of a fetch call" | P24 open | the P24 test (U114 with U113, two devices): after a restore or a `file_mismatch` the publisher writes no `replaced_through_ms` and every acked entity counts; a no-state `fetched: false` drain leaves the last-drained instant; with V23.2 "A switch during a long fetch call" |
| V2.3 "Rows of another build", V2.4 step 3, V21 `Busy`, V15.2 steps 2 and 5, V15.3, `engine/collection-replace#R6.7` | P24 open | the P24 test (U113, two build configurations): another build's published decision survives a Cancel; the three restart cases with that decision; with V23.2 "Rows of another build" |
| K19.8, K19.6, K12.6, K19.4 "Claim landed", K19.5, V18, V2.3 | P24 open | the P24 tests (U113, fake server, four devices): an abandon covers the owner's later generation of the same anchor and restores from its layout; a complete later generation ends the chain, and the restoring generation leaves out its anchors (none published when it covers them all) and its claim succeeds; one rebuild through every rolled-back inverse; the hold ends per anchor; with K22.2 "an abandoned own generation" and Q6 `abandon_three_devices` |
| P6 gate row, V15.2 at `confirmed`, V2.3, V2.4 step 3, V15.3 | P24 open | the P24 `cloud_recovery` tests (U113): a `confirmed` row settled or removed; a Cancel after V8.2 or a restore returns the binding before isolated, with no scan; with P21 "Publication (P6)" |
| O12 rules 1, 2, 3 and 8, State, Inputs (`FetchCallEnded`) | O19 open | the O19 tests (U47 `cloud_policy.rs`, and a U151 or U114 adapter test): an arming call that ends with no drain ends the task, also when it ends while a pre-launch job is held (at that job's `Ended`); only the `Ended` of a job started with the processing-task flag set ends it; no second end after an expiry while the decision waits |
| O12 rules 1 and 8, `EndProcessingTask` | O19 open | the O19 tests (U47 with U154 and U151): the launch step runs after a running `suspend`; an expiration during rule 6's send ends the task once before the send returns |
| O12 State and rules 1 and 4, O7 | O19 open | the O19 tests (U47 `tests/optimizer.rs`, `cloud_policy.rs`): `Training` always precedes the job's other reports; the start-id property test with fast jobs |
| O13 "One after the other", P3 `FetchDrained` | O19 open | the O19 adapter tests (U114, fake engine) on the call's own will and did events, a missing did-fetch-changes event closed at the next call's return or will event, and a replaced operation group, and the signed run that records whether `operationGroupID` comes back (P21) |
| O12 `CancelOptimizer`, O9, O10, O15, O17, `sched/review-session#C2` step 0 | O19 open | the O19 tests (U47 `optimizer.rs`): suspend's cancel at S1's job-cancel step (O18 item 9), before its participant flush and its drain; the mark against `CancelOptimizer`; the hook on a queued commit |

## Q12. Decisions

- Decided 2026-10-27: CloudKit tests are Rust and Swift tests (Q2), never conformance scenarios, since no Anki oracle exists for CloudKit (R2064).
- Decided 2026-10-27: the fake's service state is a file outside every device process (Q3.1), so a kill never changes what the server holds, and one fake serves in-process and child-process runs.
- Decided 2026-10-27: each assumption has a switch in the fake, and a row that depends on one runs under both values (Q3.3).
- Decided 2026-10-27: `no-loss` is judged by explained writes (Q5), so random programs need no hand-written expected state and still fail on an invented or silently dropped value.
- Decided 2026-10-27: the Rust lanes use a coordinator stand-in built from P4 and P11; U151's tests and the signed lane cover the Swift coordinator (Q4.1).
- Decided 2026-10-27: signed legs that change an account or delete iCloud data are assisted and run in the final session with the other manual checks; the other legs are automated once the user steps are done (Q7.2, Q7.5).
- Decided 2026-10-27: no signed leg triggers an encrypted data reset; A18 is recorded `unverified` (Q7.5).
- Decided 2026-10-27: the signed lane may empty its own development database with a test-hooks-only call that refuses any other container (Q7.2).
- Decided 2026-10-27: offline on one signed device is a pause or a terminated app; only the VM-wide cut is a real network loss (Q7.3).
- Decided 2026-10-27: the fake soak is 4 devices over 90 simulated days in at most 2 hours; the signed soak is 3 devices for 8 hours plus up to 1 hour of quiescence (Q9).
- Decided 2026-10-27: a replacement the user confirmed starts an ancestry, and `converged`, `history`, `no-resurrection` and `no-loss` judge the writes it discarded through the safety backup or the kept previous epoch (Q5), so the recovery rows test V22 invariants 5 and 6 instead of failing by design; no row turns a check off.
- Decided 2026-10-27: the oracle open check of `integrity` runs at the end of every Rust test and program, as K22.3 asks, not only nightly (Q5).
- Decided 2026-10-27: the host coordinator relaunches a signed device's app-hosted agent after each kill, and the agent resumes from its command file and step log (Q7.3).
- Decided 2026-10-27: every assumption has a ledger entry and a leg; only A9b, A17, A18 and the removal part of A11 may stay `unverified` (Q7.6).

## Q13. Changes other files need

This file does not change them; their owners do.

- `docs/process/one-time-setup.md`: the user steps of Q7.2 (`cloudkit-dev-container`, `cloudkit-test-accounts`, `cloudkit-management-token`, `cloudkit-quota-account`, `cloudkit-assisted-session`), and `tools/mg doctor` listing them.
- `tools/gate/tiers.yaml`: the `soak` row needs the sync server for the transfer lane only; no cloud lane needs the sync server or an Anki server. The `cloud-fake`, `cloud-faults` and `cloud-soak-fake` rows declare the oracle image, for `integrity`'s compatibility check alone (Q1, Q5); the `cloudkit-signed` row needs neither. Split `soak` into a cloud row (`soak --lane cloud-fake`, Docker, weekly) and a transfer row, add `cloud-faults` (T0 named points, T2 nightly) and `cloudkit-signed` (T3), and register the T0 cloud tests.
- The plan: U117's and U118's **Files** should name the CloudKit paths of Q2 beside their manual-transfer files; U248's `conformance/scenarios/soak/` is the transfer lane only.
- The VM tooling (`docs/process/mac-ui-vm.md`): a way to cut and restore the VM's network for the signed legs (Q7.3).
- `docs/process/milestones.yaml` and `tools/gate/tiers.yaml`: use the mapping below. Register each tier run before removing its pending marker. The suite ids `u246-sync-area` and `u250-sync-classes` are explicit scoped aliases of the aggregate runs named below, not invented result labels. Those aggregate runs remain pending until their jobs and coverage exist. M1R's first-sync validator requires all six suite ids below; later milestones inherit them. Each referenced first-sync result must resolve to an actual passing MG receipt and its validated evidence, on the first-sync build's clean commit and finished before that sync began. Validate the expected job, lane and scoped items from this mapping; copied `outcome: pass` fields alone cannot satisfy it. A fake or manual-transfer receipt cannot satisfy `u248-soak`.

| M1R suite / first-sync result id | Tier run id | MG job / lane | Required scope in `details` |
|---|---|---|---|
| `u246-sync-area` | `u246-conformance` | `conformance-aggregate` / sync compatibility area | `areas.sync` |
| `cloudkit-fake` | `cloud-fake` | `cloud-fake` / `cloud-fake` | `scenarios.cloudkit-fake` |
| `cloudkit-faults` | `cloud-faults` | `cloud-faults` / `cloud-faults` | `scenarios.cloudkit-faults` |
| `cloudkit-soak-fake` | `cloud-soak-fake` | `soak --lane cloud-fake` / `cloud-soak-fake` | `scenarios.cloudkit-soak-fake` |
| `u248-soak` | `cloudkit-signed` | `cloudkit-signed` / `cloud-signed` | `scenarios.cloudkit-signed` |
| `u250-sync-classes` | `u250-fault` | `fault` / cloud fault classes | every class listed below |

`u250-fault` adds these F4 cloud classes, each individually required by `u250-sync-classes` under `details.classes`: `cloud-send`, `cloud-fetch`, `cloud-apply`, `cloud-media`, `cloud-rekey`, `cloud-claim`, `cloud-tracking`, `cloud-start`, `cloud-adopt`, `cloud-publish`, `cloud-own`, `cloud-abandon`. Its existing required non-cloud classes remain required. A shared validated execution may supply more than one aggregate without rerunning unchanged inputs, but every resulting receipt keeps its lane and exact coverage; one lane never fills another's cell.
- The plan: U117's **Dependencies** gain U113, U114, U250 (its kill rows and variants run through the child-process runner, Q4.5) and U21 (`faultpoint!`); U118's gain U114.
- `sync/cloudkit-records#K22.3`: its bullet that the same `.apkg` imported on two offline devices converges with no duplicate decks cannot hold, since each import adds its decks with new ids (`package/apkg-import#I10.2` step 5). A spec-writer commit changes it to Q6 `same_apkg_two_offline`'s end state.

### Q13.1 First-sync receipt references

The `first-sync.json` evidence keeps its existing `build`, `sync_started_at` and `backup` fields. Each of the six `results` entries above identifies its actual MG receipt with `{job, run_id}`. These are the MG job name and the envelope's `run_id`, not the tier run id or a file path. For example, `results.cloudkit-fake` names job `cloud-fake` and that run's id. Old copied `commit`, `finished_at` and `outcome` fields may remain for display, but must agree with the receipt when present; they never supply missing receipt facts.

- Resolve the exact file `<results-directory>/<job>/<run_id>/result.json`, using the milestone command's existing results directory (normally `build/mg`). Before constructing the path, require the mapped job name and the MG envelope's run-id format. Never use `latest`, search for another passing run, accept a supplied absolute path, or replace the referenced run with a later result. A later gate commit or later run does not change the evidence the first sync relied on.
- Read a regular receipt from that results directory, with no symlink or parent-path escape. Validate the complete `tools/mg-result.schema.json` envelope and require its job and run id to equal the reference. Its `root` must identify the main checkout whose results are being read. The receipt must have `dry_run: false`, `status: pass`, `exit_code: 0`, `summary.failed: 0`, no recorded failures, and `commit: {sha: <build.commit>, dirty: false}`. A queued result, an awaiting-user result, a missing or disabled job, or an unimplemented evidence validator is not a pass.
- Match the receipt's arguments to the mapped tier run for the first-sync build. Expand commit placeholders with `build.commit`, not the gate commit; use that build's run definition if its arguments changed during the trial. Apply the mapped suite's pass rules and require every mapped scope item to be `pass`, using the existing scope-cell convention (a string or an object's `status`). Do not allow a different lane, a filtered leg or an approved deferral to stand in for one of these six required results.
- Both receipt times must be valid MG times, its start must not follow its finish, and its finish must precede `sync_started_at`. The clean build may be older than the gate commit: this check remains about the build used for the first sync. The existing backup file and before-sync time checks remain required.
- Validate the evidence required by the selected lane, not just its scope cell. Read only artifacts declared by that receipt, within the checkout's build output, with no symlink escape; check them by that lane's evidence schema and source/run binding rules. Missing, unreadable, mismatched or unvalidated evidence fails this result. Artifact contents and account values never enter failure messages.
- For `u248-soak`, require the declared `signed-evidence.json` of Q7.4 and its whole-lane validation. If it aggregates per-leg results, validate the referenced receipts and artifacts under Q7.4's same-commit, development-container, environment and test-account rules. A passing per-leg receipt, fake-lane evidence, signing/build receipt alone, or copied whole-lane status cannot satisfy this entry. The signed evidence schema and lane validator must exist before this result can pass; this reference format does not implement or waive them.

The first-sync validator reports the selected job, run id and source commit for each result. It retains the exact receipt/artifact references it checked so the original evidence can be inspected later. It never manufactures a passing aggregate or marks pending lane work complete.
