# Current Desktop sync observations

Spec: sync/server-desktop-current
Requirements: R4, R6–R14, R18
Anki: 26.09.4, Desktop build c06370ff
Scenarios: conformance/scenarios/sync/server/ (planned); current selections below

## D1. Authority and limits

This supplement records independently reviewed, executed reference observations.
It supplements the 26.09.3 baseline contracts without changing their global pin.
Implementations must preserve these selected outcomes for the same synthetic inputs.
An observation does not establish a rule for inputs that were not tested.

The client and reference server used the same Desktop backend runtime, SHA-256
`bf0ccafe0f15bcba59a652db68c4cfdfaa8191756f8c547839c9d134dee12784`.
The runtime dependency lock SHA-256 was
`56e1bef86def9f5f230d12317e3510f2176eb079928f3a75f86afb0dcc408ec8`.
This is Desktop backend coverage, not a Desktop GUI or AnkiMobile result.

The neutral archives are:

- `conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-foundation.json`,
  SHA-256 `b302e7c77890d1c55dfdddf5e8630b79e85aa289b40a2fac496f299ae02850b9`.
- `conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-edges.json`,
  SHA-256 `1bac52dfdf20519bfe9abdef8700e3e87a34318da1edde7bccb7a2fef9570c45`.
- `conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-normal.json`,
  SHA-256 `33e4f576a85f19bf877baefc382e91121e4e0307fe73b4b06fc8e24807e4d4a7`.

The source-bound review and seal receipts are recorded in
`docs/process/review-log/meguri-sync-U3.md`. They cover 21 smoke cases and eight
edge cases. They do not close every pending probe in the baseline contracts.
Candidate-server interoperability remains a separate verification obligation.
The additional twelve-case incremental observations in D7 and their committed
rerun are recorded in `docs/process/review-log/meguri-sync-normal-reference.md`.

Executed selections: `conformance/recordings/scenarios/meguri-sync-server/desktop-selection.json`
and `conformance/recordings/scenarios/meguri-sync-server/desktop-edge-selection.json`.

## D2. Observed transport

The selected client exchanges used POST, protocol version 11, HTTP chunked
request framing and raw zstd. Their JSON header members were `c`, `k`, `s`, `v`.
Response `anki-original-size` values matched decoded response lengths.

Observed collection endpoints were `/sync/hostKey`, `/sync/meta`, `/sync/start`,
`/sync/applyChanges`, `/sync/applyChunk`, `/sync/chunk`, `/sync/sanityCheck2`,
`/sync/finish`, `/sync/upload` and `/sync/download`.
Observed media endpoints were `/msync/begin`, `/msync/mediaChanges`,
`/msync/downloadFiles`, `/msync/uploadChanges` and `/msync/mediaSanity`.

The no-argument bodies observed for `chunk`, `download` and `finish` were
`{"_pad":null}`. Accept these bodies. This selection does not establish all
valid endpoints, framing alternatives, compression choices or request methods.
In particular, it does not justify rejecting valid Content-Length requests.

## D3. Initial state and full transfer

The fresh account reported `mod:0`, `usn:0`, `media_usn:0`, `empty:true`,
`cont:true` and `hostNum:0`. Its `scm` used milliseconds and `ts` used seconds.
The first served blank collection had schema version 18, `usn:1` and `ls:0`.
Its creation and schema stamps remained consistent with the recorded fixture.
This does not specify every required schema object or default metadata value.

The original one-card collection required a full upload. The blank receiver
had zero notes and cards and passed the integrity check. Neither result makes
an empty-card account safe to overwrite without an explicit sync direction.

Full upload returned HTTP 200 with the exact success text `OK`.
The upload and two later served downloads had these stamps:

| Snapshot | mod | scm | ls | usn |
|---|---:|---:|---:|---:|
| Uploaded file | 1794757513048 | 1794757513048 | 1794757513048 | 1 |
| First served download | 1794757513052 | 1794757513048 | 1794757513048 | 2 |
| Second served download | 1794757513055 | 1794757513048 | 1794757513048 | 3 |

All three retained `crt:1794715200` and `ver:18`.
These clock values describe this execution; they are not implementation constants.
The downloads advanced `mod` and `usn` while retaining the uploaded `scm` and `ls`.
These are served-file stamps. No receiver-local `ls` value was retained.

All three files had the same logical user-table SHA-256,
`ef0b926a1e5a9a8b101a084946efa2dddb6cf355dc107533f71dcd942616212d`.
That comparison covers every present user table except the separately stamped
`col` row, with duplicate rows and BLOB types preserved. It establishes logical
content preservation for this fixture, not byte-identical SQLite files or
preservation of every possible unknown schema extension.

## D4. Media and a settled no-change sync

The selected successful media replies contained `data` and `err`; the error
projection was empty. The projection does not independently establish the full
wire type of `err`. The baseline media envelope contract still applies.
Ordinary `begin` replies supplied a nonempty string `sk` and USNs 0, 1 and 2.

Single-file uploads returned `[1,1]`, then `[1,2]`.
Media listings returned `(filename, USN, SHA-1)` tuples in ascending USN order
for the two synthetic additions, followed by an empty listing.
Fresh-receiver file SHA-256 values matched both original files.
`mediaSanity` returned `OK`.

A controlled response replacement supplied an empty string `sk`, and the
Desktop backend continued successfully. This was an injection, not a natural
reference-server reply. It does not establish optional request authentication
or AnkiMobile behavior.

Once both sides were settled, the selected collection sync requested only
`/sync/meta`; the media sync requested only `/msync/begin`.

## D5. Selected replay and pagination outcomes

Replaying the exact SDK media addition request preserved result `[1,1]`.
Replaying its deletion request preserved `[1,2]`. Both replays returned HTTP 200.
The receiver retained the expected bytes after addition and no file after
deletion. These observations cover identical request replay for this fixture.

The receiver obtained these listing page lengths:

| Files available | Page lengths |
|---:|---|
| 0 | No listing request |
| 1 | 1, 0 |
| 999 | 999, 0 |
| 1000 | 1000, 0 |
| 1001 | 1000, 1, 0 |

Every expected file USN and hash matched. The zero-file case does not establish
the server's response to a listing request, because the client sent none.
These cases leave other pagination, replay and concurrent-mutation probes open.

## D6. Idle delay and explicit abort

An actual SDK chunk request succeeded with HTTP 200 after a measured 121,000 ms
delay. This gives a bounded success observation, not an automatic expiry time.

In a separate round, an explicit abort after 121,001 ms returned HTTP 200.
The forwarded chunk then received HTTP 409 and the Desktop backend failed the
sync. The local edit survived and the pending server edit rolled back.
This tests explicit abort handling, not natural reference-server expiry.

## D7. Selected incremental exchanges

These observations use Desktop 26.09.4, build c06370ff. The committed reference
rerun and retained validation passed all twelve cases. They do not establish
candidate-server behavior or close the entire pending normal-sync family.

The twelve selected observations distinguish ordinary Desktop exchanges from
auxiliary protocol fixtures. Actual Desktop receivers inspect the resulting
collections in both kinds of observation. Auxiliary request bytes are not
attributed to Desktop.

| Selected observation | Result |
|---|---|
| Settled collection | One metadata request; no full collection transfer. |
| One scheduler review | One card tuple and one review tuple; no note tuple or full transfer. The receiver retained the review. |
| Notes and note-type CSS | Authored edits arrived in both directions. A forward note deletion also removed its card. |
| Other collection objects | One authored deck, preset, tag and configuration value arrived at the second client. |
| Exactly 250 new tuples | Batches contained 250 then zero tuples, with `done` false then true. |
| 502 new tuples | Batches contained 250, 250 and two tuples, with `done` false, false and true. Receiver IDs were unique. |
| 251 card graves | Batches contained 250 then one grave. The receiver had no cards and retained its 251 orphan notes. |
| Inclusive cursor | Rows at and above the cursor were selected; the row below it was excluded. |
| One existing deck | An older incoming modification lost. Equal and newer modifications replaced it. The reply contained the prior deck version. |
| Pending note | Equal and older incoming modifications lost; a newer modification replaced it. A nonpending note accepted an older incoming modification. |
| Repeated note IDs | For nonpending tuples, the later occurrence survived even when modification decreased from 30 to 10. A newly pending first occurrence blocked an older repeat. The same order rule held across chunks. |
| Repeated review ID | One row remained, retaining the first tuple's ease value of two. |

The selected successful sanity exchange ignored supplied due and grave counts.
Immediately after finish, metadata showed USN advanced by one and modification
equal to the returned finish time. A later raw served file retained last-sync
equal to that time. That full download separately advanced modification and USN;
its creation and schema stamps stayed unchanged. The received note retained the
round's USN. Receiver-local stamps do not substitute for this served-file check.

These observations cover note conflicts and one deck conflict matrix only.
Card, note-type and preset conflicts, case-folded names, configuration replacement
and null values remain unobserved here. They also leave same-clock finish, failed
sanity, lifecycle faults, AnkiMobile and candidate incremental sync unobserved.
Reviews and deletions were exercised forward only. Media was disabled.
