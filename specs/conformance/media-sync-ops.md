# Media sync ops and observations for scenarios

Spec: conformance/media-sync-ops
Requirements: R1883, R1502, R1573, R1583, R1855
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/media/

The scenario vocabulary for media sync state (U110), which media sync scenarios (U111) and Check Media scenarios (U130) reuse: the observation `media_quick_check` and the ops `media_reset` and `media_sync`. They build on the media folder vocabulary of `media-ops.md` (the media folder F1, outside changes F4, file times F12) and on actor scenarios (`sync-ops.md`). The behavior they observe is `specs/sync/media-tracking.md`; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes.

These are explicit scenario actions. They grant no app launch, background,
timer or review-end permission to contact a server. Meguri's selected media
stage still follows `sync/anki-transfers` M5/M6 consent and identity rules.
Local checks and resets start no requests. Routine app sync remains CloudKit.

## V1. Observation `media_quick_check`

`media_quick_check` (no arguments) runs the change check a media sync begins with, then reports what the next media sync would send:

1. The executor runs the change check as it is (`sync/media-tracking#T4.2`): when the folder's modification time equals the change marker recorded by the last check, nothing is listed or hashed. On the oracle this is Anki's own registration of folder changes, the first step of a media sync, with its stored marker left as it is.
2. The value is `{"media_quick_check": {<name>: <sha1> | null, ...}}`: every entry pending for the next media sync, as in `media_pending` (`media-ops.md` F11): the lower-case hex SHA-1 of a file to upload, `null` for a deletion.

`media_pending` forces a full comparison; `media_quick_check` shows what the check skips while the folder's time stands still. Under a frozen clock, an outside change in the same instant as the last check (no `advance` between them) stamps the folder with the time it already had (`media-ops.md` F12), so the check does not list the folder.

It works in every scenario, and in an actor scenario on the current actor (`sync-ops.md` S2.3).

## V2. Op `media_reset`

Resets the media sync state of the current collection as logging out does (`sync/media-tracking#T11`, R1502): the next media sync compares every file with the server from USN 0.

- **Argument** `{}`. **Output**: none (`{}`).
- On the oracle: Anki's own reset of media sync state, the one its desktop client runs at logout. On Meguri: the reset of T11.
- The media folder and the trash do not change.
- It works in every scenario, and in an actor scenario on the current actor.

## V3. Op `media_sync`

The current actor explicitly runs one media reconciliation to completion
(U111). This step does not start a collection transfer.

- Actor scenarios only (`sync-ops.md` S1). The actor uses the host key of its `login` (S4.1) and syncs with the scenario's server over the actor's path (S8.1), so `net_fault` faults and the recorder apply to its requests too. It makes no collection sync and needs none before it; the `sync` op never syncs media.
- An actor's first `media_sync` step must come after its first `sync` step, which is a `login` (S4.2); this is checked when the scenario loads. A `media_sync` of an actor that holds no host key, because its `login` was rejected, is a run error: `invalid_scenario`.
- **Argument** `{}`.
- **Output** `{"media_sync": {uploaded_files, uploaded_deletions, downloaded_files, downloaded_deletions}}`, integers counting what this media sync did:

  | Member | Counts |
  |---|---|
  | `uploaded_files` | files sent in upload batches that the server accepted |
  | `uploaded_deletions` | deletions sent in upload batches that the server accepted |
  | `downloaded_files` | files received from the server and written into the folder |
  | `downloaded_deletions` | server deletions the actor applied: names the server reported deleted for which the actor held a file or a deletion record, except a file with a change pending, which it sends instead |

  These are the counts a client shows the user as files added and removed in each direction (R1583). How many files the change check looked at is not part of the output.
- **Rejections** (`dsl.md` D7.3): `network`, as for `sync` (`sync-ops.md` S4.4): a request did not complete at the network level. Other failures use the executor's run-error path; they are not successful counters or an implicit resume. Any additional U111 rejection kind must land with its driver and schema contract.

## V4. Driver and executor status

- The existing prepared U110 work supplies the stable vocabulary and six tracking scenario/golden pairs. Restoring these assets does not prove current driver support, freshness or execution. The parser, adapters and matching oracle support remain required before activation.
- The prepared goldens came from the Linux oracle. The historical native comparison described in `sync/media-tracking` T5 had the intentional NFD difference: Apple registers the NFC form while Linux skips it. Keep `media-nfd-renamed-check` and `media-nfd-renamed-dir`; these scenarios get no Apple overlays and are not recorded with `--platform apple` (R1889, `media-ops.md` F13).
- Meguri's executor needs the testkit model to read them (`core/crates/meguri-testkit/src/scenario/`) and adapters: `media_quick_check` and `media_reset` with U110's tracker, `media_sync` with U111's media sync. Until the model reads them, T0 reports the tracking scenarios as `invalid`; a scenario that uses one before its adapter lands is `pending`.
