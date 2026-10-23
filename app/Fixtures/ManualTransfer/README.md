# Controlled native transfer rig

The integrator stages an ignored `controlled-rig.json` in this directory in the
test VM. `ManualTransferJourneyTests` requires these exact fields:

- `endpoint`: the controlled server URL, accessible on VM loopback.
- `username`: the synthetic Import account A username.
- `password`: account A's password.
- `deckID`: the ID of a generated, reviewable deck in A's server collection.
- `exportUsername`: the username of a separate synthetic Export account B.
- `exportPassword`: account B's password.

Provision A and B on the same controlled server. Their usernames must be
nonempty and unequal after trimming whitespace; the test compares the resulting
strings exactly. B must be a separate account, rather than an alias, a new key
for A or a different endpoint identity. Neither account may use real credentials.
There is no fallback from B to A. Keep this manifest within the isolated run;
it is not a report attachment. Never include its passwords in logs or evidence.
The test enters credentials in the app's normal login controls. The test login
key stays in the device-only Keychain, and the app does not retain the password.

Use the pinned server from `tools/mg sync-rig`. A controlled `tools/mg` fixture
and forwarding job must expose only that server's workstation loopback port on
VM loopback. Mac apps and simulators in the VM both use the forwarded address;
the workstation's `127.0.0.1` does not reach the VM. Keep listeners off the LAN.
Retain endpoint validation and certificate checks, and never use a public server.
The native test accepts only loopback hosts and rejects URL credentials, queries
and fragments. Provisioning and forwarding are outside this patch. The
integrator must supply tooling and receipts for generated content in A and for
comparison of B's exported content, saved answer included, on the server and peer.

## Execution and evidence

Run the suite on each of Mac, iPhone and iPad using
`tools/mg ui-test --platform … --suite ManualTransferJourneyTests`. Prepare an
isolated development installation with a generated server collection, an
admitted deck that can be reviewed, and both synthetic accounts. Resolve earlier
unfinished transfers first and retain the required safety copies. Every launch
uses system time so the app and server enforce the real deadline. UTC and
disabled animations affect presentation only. Leave clocks, receipts, cooldown
durations, deadlines and protocol validation intact.

First the test signs in as A in Settings, imports the server collection after
confirming the replacement preview, and saves a real review answer. It restarts
with the persisted collection and no collection fixture. Once A's stored login
has loaded, preparing another Import must produce the existing cooldown and its
recorded deadline, without a confirmation preview. This state is captured before
the test logs out. It then signs in as B through the same login controls, chooses
Export, prepares the safety-copy preview, confirms replacement, and waits for
completion. Login itself must perform no preparation or transfer. Switching
credentials must retain A's history and deadline; core checks, retained copies
and candidate generation still apply throughout.

Report an Import from A and Export to B, with A's cooldown still active.
This test does not show A transferring after its deadline.
Rust and Swift deadline tests remain required.
Skip the suite if its manifest is absent.
Record a missing manifest, VM route, or peer proof as queued work.
Skipped and queued runs do not pass acceptance.

Retain screenshots of A's completed Import, saved review, durable review after
restart and cooldown rejection, plus B's Export preview and completion. The UI
receipt must also include a read-only rig comparison of B's logical content on
the server and peer against the reviewed collection, including the saved answer.
Images cannot prove that content comparison. Required evidence still includes
compilation, execution in the VM, independent visual inspection, fault cases,
renders and full gates. Cover interruption, recovery, saving copies, decision
facts and keyboard focus. Static Settings images document appearance only.
