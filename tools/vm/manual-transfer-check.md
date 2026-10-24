# RF4 manual-transfer-check VM runbook

Run `manual-transfer-check --platform mac|iphone|ipad` explicitly. Ordinary
gates do not start this utility. Its Docker child, `manual-transfer-data`,
accepts `seed|verify <controller build directory>` only. Use the current
controller directory under `build/mg/manual-transfer-check/`.

The job family is `tools/mg.d/manual-transfer-check.sh`. It loads
`tools/scripts/manual-transfer-route.sh` for the leased forward and cleanup,
and calls `tools/scripts/manual-transfer-config.py` for synthetic account
validation and manifest generation. Controller doubles live in
`tools/tests/manual_transfer_fakes.sh`; their contract cases are
`tools/tests/manual_transfer_check.bats`.

The Docker data job calls the `manual_transfer_data` Rust test target's
ignored `rig_manual_transfer_data` method. It passes its owned controller
directory as `MEGURI_MANUAL_TRANSFER_DATA_RUN` and `seed` or `verify` as
`MEGURI_MANUAL_TRANSFER_DATA_MODE`. These values select generated run data;
they carry no login information. The capability attestation uses
`MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH=1`.

## Before running

Use the first configured VM and its simulator pool. Disable host UI fallback.
The controller builds Debug products on the host with
`MG_APP_TEST_SCHEME=Meguri MG_APP_TEST_PLAN=UI`. It then holds one VM lease
for copying, forwarding, manifest staging, XCTest and collection capture.
Simulator runs also take a pool lease. The Docker child needs no Apple lease.
Do not perform one-time user setup from an agent session.

Attest the selected SSH wrapper before setting
`MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH=1`. Root inspected the actual
wrapper read-only: it passes `"$@"` directly after the SSH target. It has no
argument parser, shift, argument join, inserted separator or added remote
shell. This proves structural passthrough only. Runtime forwarding remains
unproven. The filtered fixer cannot inspect the external wrapper.

Bind the attestation to that wrapper. Require all of these capabilities:

- Preserve client options, the following remote command and its standard input.
- Pass `-o BatchMode=yes -o ConnectTimeout=10 -o ExitOnForwardFailure=yes`
  and `-o ServerAliveInterval=10 -o ServerAliveCountMax=2`.
- Pass `-R 127.0.0.1:18765:127.0.0.1:<checkout-rig-port>` unchanged to SSH.
- Retain normal identity and known-host checks. Use no wildcard listener,
  shared ControlMaster, detached client or `GatewayPorts` change.
- Stop the SSH client when the wrapper stops; `exec ssh` supplies that lifetime.

Expose no keys and change no host, VM or SSH settings. Without attestation,
the controller records incomplete evidence before server startup or builds.
If the wrapper cannot meet this contract, root must resolve the controller's
transport requirement separately. The job installs no wrapper or alternate route.

## Isolated account setup

Let the controller acquire `build/manual-transfer-check.lock` and run
`sync-rig up` for this checkout's pinned compose server. It checks the single
running container's `SYNC_USER3` and `SYNC_USER4` pairs against the C/D example
entries in memory. It never prints the container environment. An override
fails before setup replaces either collection.

Reserve those two synthetic accounts for this controller. Do not steal its
checkout-local lock. After an uncatchable kill, establish that the owner has
ended before removing the lock. Leave other accounts, compose projects and
volumes outside this operation.

Setup uses `meta.ts`, stock Meguri note generation and the host transfer API.
A has one generated note and reviewable card, with zero saved answers. B has
zero cards and a nondefault account setting. Supply no real collection,
arbitrary collection directory, public service or LAN listener. Never install
a fixture collection directly into the app.

## Route and manifest

Under the VM lease, the controller opens one attached SSH reverse forward.
It waits at most ten seconds for readiness with `ExitOnForwardFailure=yes`.
An occupied port must fail startup; never adopt an existing listener. Require
both VM observations: the only listening address is `127.0.0.1:18765`, and an
HTTP request through it receives a response. Otherwise leave route proof
pending. Keep diagnostics local until redaction review.

Let the controller pipe its manifest directly into the leased VM, including
`endpoint`, A/B credentials, `deckID`, expected media hashes and the frozen
observer/native-context capabilities. The endpoint is `http://127.0.0.1:18765/`. Use only the generated example
accounts. The ignored file is
`app/Fixtures/ManualTransfer/controlled-rig.json`, written with mode 0600.
Keep credentials out of host files, process arguments, environment values
and logs. Do not stage or retain a manifest for another run.

## Native evidence

Select exactly
`ManualTransferJourneyTests/testSameLoginReviewCycleThenCrossAccountMediaRecovery`.
The fixed sequence is A Import, A same-login Export, then independent B Export.
Initial sign-in enters the exact URL and credentials and presses Log In directly,
without Save Endpoint. Login performs no collection or media transfer.

A Import retains the actual held first media request, Cancel/drain/durable
partial, passive restart/history with zero calls, explicit fresh-consent Resume
on the same operation and zero collection resend. Require server4096 download,
device6144 upload and zero-file exclusion. Save an actual Easy answer, restart,
and prove its persistence and passive restoration of A's stored login.

Without logout/relogin or endpoint, clock or policy edits, explicitly prepare
A Export at that same URL, with a fresh safety backup, preview and confirmation.
Require the same opaque account/endpoint, a distinct operation and completion
before one real hour after Import. Select media and obtain fresh consent with
zero media requests before it. This unchanged same-owner stage has exactly zero
downloads and uploads; server4096/device6144 stay exact. It has no payload to hold
or cancel. Fixed `a-export-before-consent` and `a-exported` checkpoints retain it
between the fresh Import and final B Export.

Only afterward log out and sign into distinct B. Preserve server8192 download,
device6144 upload, held request, cancellation/drain, restart, fresh-consent Resume,
exact counters/bytes/exclusions and zero collection resend. B has its own
`export-cancelled`, `export-passive-history` and `export-resume-decision` checkpoints.
The observer arms B's hold only after A Export completes and drains.

New custom-server own receipts use `next_eligible_at == completed_at`; explicit
operations need no wait. Old future receipts/maps remain valid history. Capture
the historical per-account `successCooldown` baseline at each preparation: a
partial has no operation receipt and leaves that value unchanged. Whole success
uses its own truthful receipt. Never clear history to make a nil assertion pass.
The strict reducer retains all three phases and rejects missing/wrong-account,
intervening or unfinished A Export, stale contexts and malformed history. Proofs
contain booleans/counts and safe operation facts, never identity values.

Require one passed case, zero skips and zero failures in both the xcresult
summary and case tree. Require the exact method identity. Empty receipts,
unrelated passing tests and missing hooks or manifests cannot supply proof.
Keep VM and input-loss queuing; independent failures must still fail.

After XCTest teardown stops the app, capture its activated Export with SQLite
backup, including the WAL. For Mac, use only the development sandbox's
`com.antoinefink.meguri.dev` Application Support/Collection path. For a
simulator, resolve that development bundle's data container through the VM
helper. Reject unknown layouts without searching other apps or host data.

## Peer evidence

The Docker child independently logs into A and downloads completed A through
the existing guarded rig transport into a separate owned `peer-a` directory.
Require its actual note/card/review rows to equal the independent Easy replay,
including the saved answer and scheduling. This proof is separate from native
state and from B. `peer-proof.json.independent_a` must attest the download,
saved review, one Easy, scheduling, identity/content and replay equality.

The child also independently logs into B and downloads through Meguri's
guarded rig transport. Require a current verified server copy without upgrade
transforms, the generated note/card identities and fields, one uploaded Easy
review, zero pending reviews and the persisted scheduling state. Check the
review timestamp against setup and server observations.

Replay Easy on a copy of A's seed at the saved answer instant and duration.
Keep card identity and reps as fuzz inputs and load balancing disabled.
Change only pending card and review USNs from -1 to zero. Compare complete
note, card and review rows with the peer.

Validate the native activated Export as a server copy before copying it for
comparison. Require its `mod`, `scm`, `ls` and `usn` stamps to match the
pre-download server facts. Require the download to advance server USN by one,
preserve schema time and place modification time within the observed bounds.
Apply only the declared O7.4 changes to the comparison copy: set `col.mod` to
the observed download time and advance `col.usn` by exactly one.

Compare every cell in all twelve logical tables: `col`, `config`,
`deck_config`, `decks`, `notetypes`, `fields`, `templates`, `tags`, `graves`,
`notes`, `cards`, `revlog`. Permit no masked columns, other transforms or
tolerances. A difference fails. Retain child receipts and `peer-proof.json`.
Report only safe counts and booleans. `media.typed_stages` has exact separate
Import/AExport/Export expectations; `media.cycle` proves same account/endpoint,
distinct operations, real-clock ordering, completion within an hour and immediate
eligibility. The backend field is `immediately_eligible`. B retains positive
media counts and all existing peer/media comparisons. Keep generated SQLite files in the
ignored run directory. Keep actual result bundles for independent review;
screenshots cannot replace peer comparison. Review secure-field XCTest
activities and log redaction before sharing logs or xcresults. Exclude
credentials, host keys and collection content from summaries.

## Cleanup and result

Cleanup handles EXIT, TERM and HUP by stopping native work, removing VM
inputs and closing the forward before simulator reset or VM lease release.
The observer and remote watchdogs are each bounded to 3600 seconds. Normal
input EOF and signals clean up immediately. Confirm removal of the manifest
and listener before returning the VM lease; unknown cleanup state is an
error. The ordinary VM fixture copy replaces `app/Fixtures`.

Missing attestation, VM, route, manifest, native receipt or peer proof leaves
evidence incomplete or queued. Only the exact native pass, strict peer match
and confirmed cleanup can produce pass. Preserve cancellation and queued
semantics. No result here establishes milestone or human acceptance.

## Root commands and open checks

Run these focused checks through `tools/mg`:

```sh
tools/mg selftest --filter 'manual transfer:' --jobs 1
tools/mg core-test -- --test manual_transfer_data -E 'package(meguri-host)'
tools/mg core-lint
tools/mg doctor
```

The ordinary fixture command leaves the live-rig test ignored. Only the
controller invokes `manual-transfer-data seed` and `manual-transfer-data verify`
for its own run directory. After wrapper attestation, run each platform alone
without command-line secrets:

```sh
export MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH=1
tools/mg manual-transfer-check --platform mac
tools/mg manual-transfer-check --platform iphone
tools/mg manual-transfer-check --platform ipad
```

Root owns unchanged privacy checks, P0/P1 review follow-ups (at most six
component rounds), actual compilation, fake tests, VM forwarding/native/peer
verification and the required combined gate from base `81c2075`. Tooling
round four found no new P0/P1. This source repair runs none of these checks.
Keep final milestone and human evidence open. Do not claim acceptance.
