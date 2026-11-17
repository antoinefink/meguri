# Retained public HTTP worker

Original test-only action worker for `conformance/meguri-sync-local-delivery`.
It depends on the existing core, outside both shipped Cargo workspaces. It
changes no app, FFI, core, server, package or browser behavior. It neither runs
Desktop nor reads its implementation, and never reads server storage.

Original author session: `s-20261116T225101Z-bbe6c19b`. Boundary fixer session:
`s-20261117T004348Z-2115dc91`. Fixer parent source input:
`b3d271decd94a230579bc4418a85439a3b3e7358`. Seed: original eleven-path
export `6b9549a84bef337a7f02f998fda04dff686052cd1936315159d706ab4e07a270`. The filtered runner's local Git
baseline is an unrelated permitted-file snapshot, not the parent source commit.
Independent source-review input:
`222ef1559f9ebeabfbee69b7341c848414631abe0d2e7df7fb29e3f47c990de4`.
Its execution/finalization causality finding drives this fix: before-only worker
admission, immutable raw capture before actual after measurement, then offline
final attachment. The revised bytes still require parent independent review.

## Build and lock preparation

The parent runs these through existing MG Docker tooling, coordinating CPU with
app work:

```sh
tools/mg sync-server-retained prepare-lock
# Independently inspect the new peer Cargo.lock, then parent checkpoints it.
tools/mg sync-server-retained check
tools/mg sync-server-retained build
tools/mg selftest --filter 'retained peer'
```

The filtered author does not run Cargo or the peer. No separate lock is
invented here. Preparation copies `core/Cargo.lock` and lets offline Cargo
metadata add this root, then refuses any external name/version/source/checksum
outside that audited lock. Retain a failed preparation's lock for inspection.
Subsequent metadata and build use `--locked --offline`. Missing cached packages
are preparation failures; do not update dependencies or use another registry.
No new external dependency or test framework is introduced.

The build retains the actual executable, both locks, complete resolved
package/features inventory, recipe/selection hashes and source-file manifest
before/after compilation. Only `meguri-sync/test-hooks` is explicitly enabled;
it propagates hooks to engine/model/storage. The HTTP endpoint guard motivates
this difference from the application release graph. Host hooks and the
`ankiweb` switch are absent; `meguri-testkit` and its stub transports are absent.
Full resolved features, including transitive dependency defaults, are in the
build receipt, not inferred from these explicit flags. The standalone patch for
the existing core `askama_derive` override preserves that core graph choice.

## Parent integration delta

Use the existing owned package orchestration and finite proxy. Do not add the
worker to the runtime image. Deliver the retained binary to the existing
separate client container, with its network namespace joined to the exact owned
server and its current `MG_RUN_ID` environment set by that owner. The only
target is `http://127.0.0.1:8080/`. For the uncertain upload
action only, configure the existing finite proxy at job-owned
`http://127.0.0.1:18080`, forwarding solely to that target and dropping the actual
upload reply. There is no new proxy, retry or automatic recovery implementation
in this workspace. Parent must validate that fixed listener mapping before use.

Mount fixed paths:

| Path | Ownership and meaning |
| --- | --- |
| `/in` | Readonly neutral action/build/package/input manifests and content-addressed artifacts |
| `/secret/credentials.json` | Separate readonly regular0600, single link, client UID, exactly username/password; never an ordinary artifact |
| `/state` | Fresh job-owned synthetic retained-client storage, persisted between this case's worker invocations |
| `/out` | Empty owned output directory for this invocation only |

The executable takes no arguments. `/in/action.json` is exactly `case` and
`action`; `case` is one of the eight committed variant names. The worker never
accepts a URL, executable, command, source collection or container selector.
Use isolated accounts/state for each case; never let an earlier failure stop
dispatch of the other cases. The parent keeps case/action identities and the
frozen selection's actor order, not shell success chains across cases.

`/in/client-build.json` comes byte-exact from this MG build's `client/` output.
Copy its content-addressed artifacts into `/in/artifacts/`. The worker measures
its own executable and checks these artifact identities. Parent additionally
checks the canonical successful build result, clean current source, exact
source-file manifest and complete feature inventory.

Preparation actions are offline: `prepare_initial`, `prepare_snapshot`,
`prepare_forward`, `prepare_reverse_media`. Initial creation requires an absent
case directory, uses public bootstrap, opens `CoreHost`, and authors the
committed original synthetic objects via public typed operations. There is no
`Engine::open_raw`, raw stamp mode, SQL injection or fabricated review row.
Clock is a model `Clock` adapter for UTC wall time. One process-wide
`SeededRandom`, seeded once from the OS, supplies all model/job randomness;
no job restarts a seeded sequence. Public review generates actual scheduling
and history, which are frozen after the action, never predicted or fabricated.

Preparation writes `snapshot.json`, `state-inputs.json` and `action-result.json`.
Freeze all actual state files from the closed host and these exact output bytes;
the state inventory hashes every regular input file, including sidecars,
manual-transfer state and media. Independently review preparation code, rules,
baselines and initial inputs prospectively. Under those reviewed rules, capture,
retain, hash and freeze each actual authoritative offline event BEFORE its first
candidate submission. Bind case/action/actor, preparation identity, baseline and
capture/submission order; subsequent comparisons use those unchanged bytes.
`prepare_snapshot` creates a freeze boundary without authoring another event.
The review artifact may identify the prospectively accepted preparation rules;
no new external review pause is required for each event. Independent review of
all actual artifacts and outcome bindings is required before any case credit.
An incomplete preparation preserves its exact affected claim; it grants no
blanket permission to submit an unbound partial input.

Network actions require `/in/preflight.json`, strict L2
`final_package_preflight`, and the complete `/in/artifacts/<sha256>` closure.
It has the final package's exact top-level keys; its execution binding has every
L3 field EXCEPT `container_inspection_after_sha256`. A `final_package`, an
intermediate receipt, an after field or missing before evidence is refused at
admission. No future hash is predicted. The worker checks the release package,
actual process identity and before inspection before reading credentials or HTTP.
It has no Docker socket; the parent measures ownership/process/configuration and
independently validates the canonical package result.

`/in/input-manifest.json` has exactly:

```text
format: meguri-retained-input-v1
case, action
action_sha256, state_inputs_sha256, before_sha256, expected_sha256
review_sha256, client_build_sha256, preflight_peer_sha256
```

All hashes resolve actual frozen bytes in `/in/artifacts/`. The before snapshot
and complete closed-state inventory bind the local input; expected identifies
the independent authoritative snapshot. Review identifies accepted preparation
rules/baselines/code and initial inputs, with actual event artifacts reviewed
before credit. The parent establishes independence and capture/submission order.

`/in/invocation.json` has exactly:

```text
format: meguri-retained-invocation-v1
run_id, root, server_arguments, data_volume, owner_nonce
case, action, actor
client_build_sha256, input_manifest_sha256, preflight_peer_sha256
```

Actor is `client_a`. Root, run ID, exact serve argv, actual owned RW volume and
32-hex container creation nonce come from trusted MG, never fixtures. The argv
is exactly `serve`, `/data/owner`, configured HTTPS origin, `0.0.0.0:8080`,
`--trusted-peer`, configured IP. Root must equal the canonical package build
receipt. Bind before inspection to the same container/image/Created/labels,
entrypoint/argv, UID10001, readonly root and exact `/data` volume. Parent remains
responsible for independently proving these actual measurements and canonical
build/current source; neutral hashes alone cannot prove ownership.

## Execute, capture, then finalize offline

This is a retained-specific host seam in the existing `support.py`, not a
Desktop child phase or a worker mode. Both operations are offline: they use only
explicit neutral directories and SHA-256, never HTTP, credentials, environment
secrets, Docker or subprocesses. Parent calls through its existing MG tooling;
no new orchestration job is added here.

```text
python3 conformance/peers/retained-sync/support.py capture RAW INPUTS CAPTURE
python3 conformance/peers/retained-sync/support.py finalize CAPTURE FINAL_INPUTS FINAL CAPTURE_SHA256
```

Finite interface:

- RAW is the closed, readonly worker `/out`. Only `action-result.json`, `worker-error.json`,
  `state-inputs.json`, `snapshot.json` are admitted; at least result or error is
  required. Local failed outcomes and closed-vocabulary execution errors remain
  failures. Capture checks result case/action/actor/build/input/preflight and
  invocation identities and actual output-state hash, without changing outcomes.
- INPUTS is the readonly worker `/in`: exact `action.json`, `client-build.json`,
  `input-manifest.json`, `preflight.json`, `invocation.json`, `artifacts/`.
  No secret input is read/copied. Artifacts are regular content-addressed files.
- CAPTURE must not exist. Capture emits readonly `capture.json` and readonly
  `artifacts/<sha256>` containing every raw output and neutral input byte plus
  the input artifact closure. It prints ONLY capture.json's lowercase SHA-256.
  Admission format is `meguri-retained-capture-v1`, exactly `format`, `case`,
  `action`, `actor`, `invocation_sha256`, `preflight_peer_sha256`, `inputs`, `raw`,
  `case_credit`. Inputs/raw map exact filenames to actual hashes; credit is false.
- Trusted MG retains/checkpoints that printed CAPTURE_SHA256 **before** measuring
  actual after-state. A failed capture stops this action's finalization, while
  the parent still dispatches independent cases. It never fabricates a receipt.
- FINAL_INPUTS contains `peer.json` (strict complete `final_package`), the exact
  captured `invocation.json`, and content-addressed `artifacts/` for the final
  package/after evidence. Parent supplies actual before/after process/inspection
  measurements under the same trusted ownership. CAPTURE is mounted readonly.
- FINAL must not exist. Finalize checks the trusted capture digest, rehashes raw
  bytes and all artifacts, validates the complete final package and requires
  equality to preflight after removing ONLY the after-inspection hash (plus the
  package kind/binding hash change). Same server/source/image/container/Created,
  process and unchanged container StartedAt, before evidence, case/action/actor/
  build/input and preflight survive.
  It writes readonly `raw/<original-name>` with the exact original bytes,
  `artifacts/<sha256>` with the full closure and capture/final-peer bytes, and
  `final-binding.json`; it prints ONLY the binding file's SHA-256. The binding
  format `meguri-retained-final-binding-v1` has exactly `format`, `capture_sha256`,
  `case`, `action`, `actor`, `inputs`, `raw`, `final_peer_sha256`,
  `execution_binding_sha256`, `case_credit:false`. Original raw results/outcomes
  are never rewritten, retried, upgraded or relabeled.

Nonzero support exit means capture/finalization failed, no credit. Partial output
is retained as failure evidence; do not reuse it or retry it into success. Parent
integrates this attachment into its existing retained recorder/semantic validator,
never the Desktop final-full/final-normal/final-media phases. Capture/finalize
cannot grant case credit or canonical final evidence. Only the parent's
independent actual-artifact review and final semantic assembly/seal can do so.

Snapshots retain all twelve tables as ordered columns/rows with exact SQLite
cell kinds. Integers are decimal strings; text and blobs are hex; reals keep
their IEEE bits. Media is a whole filename/SHA-256 map. The comparison projection
excludes only `col.mod/scm/usn/ls`, row modification/USN fields explicitly listed
in `fixture::supported`, and no content/settings/history/card-scheduling values.
Original cells always remain in outputs. No generated-ID remapping is performed:
the independently frozen authoritative IDs must match. Parent supplies neutral
Desktop projections in this same shape, through its owner, never quarantine
internals or candidate-derived expectations. Noncanonical object blob differences
remain visible failures rather than being silently normalized away.

## Fixed execution schedule

Each row is a separate case with its own preparation and account. Initial and
baseline are preparation steps with no compatibility credit. Capture/hash/freeze
actual authoritative inputs before each subsequent candidate action under the
prospectively reviewed rules, including local reviews and Desktop events. The committed selection's actions
and expected arrays remain unchanged.

| Case | Retained actions and independent parent work |
| --- | --- |
| authentication | `prepare_initial`; capture/hash/freeze; `authenticate`: login, real authenticated meta, wrong-credential refusal |
| full-upload | `prepare_initial`; where independently prepared rich authority is available, Desktop uploads it and `import_baseline` performs public explicit Import/full-download; capture/freeze actual resident rich state; `upload`: persisted preview, explicit Export confirm; fresh Desktop receives and compares whole supported state/history/scheduling and server media independently |
| full-download | `prepare_initial` authors distinct left state; Desktop B authors/freezes right state and uploads; capture/hash/freeze; `download`: public Import prepare/confirm and activation; compare the independently chosen right collection with preserved local media, and independently preserved server media |
| normal-no-change | `prepare_initial`; when rich authority is supplied, `import_baseline`; `establish_baseline` upload; capture/hash/freeze settled state; `normal_unchanged`: actual NoChanges Outcome, exact unchanged tables/media, parent verifies required metadata exchanges only |
| normal-bidirectional | `prepare_initial`; when rich authority is supplied, `import_baseline`; `establish_baseline`; `prepare_forward`; capture/hash/freeze actual offline rows/history under reviewed rules; `normal_send`; Desktop B receives/compares, authors/freezes actual reverse events then sends; capture/hash/freeze; `normal_receive`; compare complete authoritative rows/history/scheduling/deletions and duplicate counts after both directions |
| media-bidirectional | `prepare_initial`; reviewed `media_baseline`; Desktop B receives initial map, applies/freezes forward add/replace/delete then sends; capture/hash/freeze; `media_receive`; `prepare_reverse_media`; capture/hash/freeze; `media_send`; Desktop B receives and compares exact reverse map and real totals |
| media-no-change | `prepare_initial`; reviewed `media_baseline`; capture/hash/freeze settled state; `media_unchanged`: zero returned mutations/files/bytes and exact unchanged tables/media; parent checks actual recorder transfers |
| manual-uncertain-recovery | `prepare_initial`; capture/hash/freeze; fault_driver arms existing finite proxy; `uncertain_upload`: actual Export prepare/confirm, then public offline recovery preview checks NeedsDecision/RemoteCompletionUnknown with PossiblyAccepted; no replay is sent; fresh Desktop independently verifies committed collection and media |

Manual collection operations use unmodified `TransferAccess`. Collection-only
success also asserts local media preservation. Normal calls pass transport None
and caller-owned StatusMemory and reject OneWay, skipped merges or a pending
full transfer. Media calls are separate admitted `JobKind::MediaSync` jobs with
`Job::new` and `media::reconcile`, preserve failure phase/totals, and refuse
awaiting/oversized/empty results as whole success.

The parent supplies real request observations from existing owned tooling;
never substitute stub replies or handcrafted full/normal/media requests. Only
the authenticated `meta` probe uses public `Job::request` directly. Actual HTTP
status/method counts cannot replace the returned client Outcome. Compare exact
forward/reverse recipe maps as well as authoritative frozen input snapshots.

## Receipts and incomplete claims

`action-result.json` identifies actor client_a, case/action, build, package/input
bindings, actual times, exact snapshots and public outcomes. It always says
`case_credit:false`; an action does not observe independent Desktop/fault claims.
Worker exit 0 means its local assertions completed, 1 means an observed local assertion
or preparation limitation, 2 means input/public-API execution failed. A fixed
error receipt contains no raw exception/server/credential text. MediaFailure
retains its actual partial totals; no failed action is relabeled a pass.

Parent's existing semantic recorder/validator assembles the unchanged eight
named cases as M-I01/meguri_policy/retained_peer, preserving every fixed actor,
ordered action, expected assertion and content-addressed dependency. Validate
unique selected/completed IDs and expected=selected=8. Failure is a completed
failure, not matching compatibility; unsupported required claims stay incomplete.
Require independent receiver, real wire/fault evidence, all assertions and fresh
independent review for any passing case. Do not count preparation, a build,
missing action, HTTP status, worker exit alone or a partial media result as a
passing case. This workspace does not create canonical final evidence or seals.

Inspected first-party patterns: `meguri-host/tests/manual_transfers_rig.rs`,
`meguri-sync/tests/normal_sync.rs`, `meguri-sync/tests/media_reconcile.rs` and their
public entrypoints. Existing tests are unchanged. New tooling tests are
`tools/tests/sync_retained.bats`; build-support checks audit selection/recipe and
external lock identity substitutions. Parent owns compilation, focused MG tests,
independent correctness/simplicity review, privacy/provenance/similarity and gates.

Source review identifies a lead: public plain note-type save refuses front/back
template edits. This is an unsupported public authoring claim, not an observed
compatibility/runtime failure. Forward preparation attempts it once after
supported note/review/deletion operations and records the exact partial snapshot
and `source_lead:unsupported_public_authoring`. Rich forward template authoring
remains unproved; do not delete its assertions or normalize away the difference.
Initial public note-type creation supports explicit template/CSS values in source.
Use supported explicit Import/full-download of independently prepared rich state,
then full-upload to an independent receiver, to prove resident rich retention
where available. `import_baseline` is a separate non-credit preparation action
for full-upload or either normal case, using the same public prepare/confirm
Import path and media-preservation assertion; its case/action identity remains
retained. It is never a replacement for the normal edit/review/deletion actions.
Authentication, full-download, media and independent normal attempts keep their
own setup and never depend on the unsupported forward edit or its exit status.
Actual execution determines affected claims. No compiler, tests, HTTP path,
process measurement or runtime behavior has been executed by this filtered fixer;
source characterization is not an invented red or a passing check.
