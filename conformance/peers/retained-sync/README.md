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
target is `http://127.0.0.1:8080/`. Every actual HTTP Job uses the existing
fixed proxy at job-owned `http://127.0.0.1:18080`, forwarding solely to that
target. It passes ordinary actions through and drops the actual successful
upload acknowledgement only for the fixed uncertain-upload admission. There is no new proxy, retry or automatic recovery implementation
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

## Finite retained host runtime

Runtime author: `s-20261116T225100Z-296ac8bc`; unit start
`81c99a427b025497bffc06acec0fb496f97231b6`; coherent batch base
`480df4526514e294f5a3a16dd4ab83a1fbcc7f47`. Parent-confirmed prepared clean
source: `ae8caefa0165b7811f8ff911f269322a3faecdef`. Frozen agreement:
`8e69a6337554ac477530ef09544422729c8a1d2b70b399a8e9a563f2e71cfdbb`.
These are supplied canonical identities, not this filtered Git baseline.

Author source review corrected executable closure, immutable companion
expectation binding, separation of failed operations from cleanup failure,
full offline state admission and isolation of malformed case inventories.
Only static syntax/whitespace/source inspection was performed here. Parent's
independent source review, canonical application, actual checks and artifact
review remain outstanding; this log grants no runtime or compatibility pass.

The existing host entry point now also accepts exactly:

```sh
tools/mg sync-server-interop --lane retained --phase final \
  --build-receipt build/mg/sync-server-package/<fresh-run>/result.json \
  --client-build-receipt build/mg/sync-server-retained/<fresh-run>/result.json \
  --author-session s-20261116T225100Z-296ac8bc
```

It requires an integration role, clean current source, a canonical current
release package and a canonical current retained `build` receipt. The latter
rechecks executable, both locks, complete current source inventory, recipe,
selection and compiled feature graph. The source inventory now includes both
runtime shell files. Rebuild after the fixed-proxy adjustment; earlier compiled
capability receipts cannot admit this runtime. Build/lock/check helpers remain
`docker:dev`; no retained worker is added to the production image.

The host builds the existing pinned tooling image through its existing owned
build seam and checks the unchanged source/lock digest around that build and the
run. It calls only the agreed finite entry points `retained-companion --input
/in --credentials /secret/credentials.json` and `retained-observer --input /in`.
Their implementation remains separately owned. No SDK internals are imported.
The observer receives the worker's exact RO input, no expectations and no secret
mount. Offline companion preparation has network `none` and no secret mount.
Online companion and worker receive the same case-owned protected pair in a
separate RO file mount. Only account setup receives the owned server volume;
clients and observer never receive server data. Browser origin stays
`https://candidate.example.test`.

All eight actual worker `prepare_initial` attempts run before the first account
or package factory. Each has fresh input/output/state and retains a complete
closed state/output inventory, including refusal/error results. Factories then
run in selection order with distinct volume suffixes and creation nonces, using
the existing package init/start/account/process/stop/remove helpers. They retain
the same outer run identity. Created/StartedAt equality is enforced within each
lifetime; timestamps need not be globally unique. Every factory is attempted
even if another preparation or case failed. Cleanup checks exact creation labels
and observed absence; cancellation uses a pre-creation owned-child ledger.

The collection schedules use the agreed companion preparations and actions:

- Upload: offline rich authority, companion baseline upload/media preparation,
  retained import setup, retained collection upload, empty fresh witness receive.
- Download: offline distinct right authority and server-media map, companion
  baseline upload, retained collection download, then a noncredit empty fresh
  download witness. The witness uses the original separately frozen server map.
- Normal: rich preparation/upload, retained import and establishment, companion
  ordinary settlement. No-change then uses a frozen settled state checked against
  the original baseline. Bidirectional continues with retained offline forward
  edit/review/deletion, retained ordinary send, companion ordinary receive,
  companion offline reverse capture, companion ordinary send and retained receive.
- Media: retained initial reconciliation; companion initial-map receive with a
  fresh cursor, offline forward-map capture and media send; retained receive,
  offline reverse-map capture and media send; companion reverse receive.
- Authentication and unchanged-media use their existing public worker actions.
  Uncertain upload runs once, followed by an empty fresh witness against the
  original collection and independently empty server-media map. A collection-only
  writer never seeds or repairs server media for that witness.

Preparation receipts and outputs remain authoritative original bytes. The
retained adapter checks whole snapshot shape/column order/cell kinds and applies
only `fixture::supported` protocol-stamp masks. Forward/reverse authorities retain
the original baseline outside declared edits/deletions, an actual single review
and its actual scheduler rows. Settings blob changes are confined to their
original declared object and column, kept byte-exact and subject to independent
actual-content review. Template-authoring refusals and partial outputs remain
incomplete claims; import setup cannot replace normal edits/reviews/deletions.
If initial authoring fails but storage exists, a later public offline snapshot
can allow independent actions to proceed; it never repairs that initial claim.
Unavailable authority refuses that send locally and does not shorten other case
schedules. A malformed case inventory also remains unavailable while the other
seven cases are assembled. No automatic retry, fallback transfer or success-driven case reduction
is performed.

For each worker network action, all real Jobs use fixed18080, forwarding only to
owned8080 through the separately authored observer. The observer is started
foreground in its container; its exact ready identity is checked before the
worker runs once. After the worker joins, the host sends TERM, joins the observer
and requires its closed ready/wire files. Normal actions pass through. Only
`manual-uncertain-recovery/uncertain_upload` arms actual successful acknowledgement
loss. Wire validation accepts HTTP `POST` and the closed public `/sync/*` and
`/msync/*` method paths, and checks byte/completion sums, actor/invocation and
fault identity. Fresh-witness wires refuse collection/media content-send methods,
so a receive cannot repair missing server content and then claim equality.
It keeps upstream and delivered completion separate. No body,
header, query, credential or raw child log is retained by the host recorder.

Worker capture/finalize keep their existing exact schemas. A separate runtime
checkpoint freezes their capture digest, actual raw/state inventory and actual
observer ready/wire hashes **before** the real after-process/container measurement.
Companion capture freezes the original observations and full CAS, including its
actual wire, at the same boundary. The adapter uses the existing strict public
package validator without Desktop child-phase fields, checks the unchanged
process and lifetime, completes the package and finalizes entirely offline.
It rechecks original raw/wire bytes and never changes a captured false outcome.
There is no extra companion record-stage HTTP call. All helper attachments say
`case_credit:false`.

Outputs under the outer MG run:

- `preparations/<case>/`: all eight actual offline outputs, complete state,
  immutable checkpoint and content-addressed closure.
- `instances/<case>/`: factory ownership, actual command/container receipts,
  numbered action input/raw/capture/checkpoint/final-input/final directories,
  per-case events and observations, and checked cleanup result. Actual partial
  artifacts remain when a later check fails.
- `output/cases.json`, `invocation.json`, `artifacts/<sha256>`: ordinary existing
  evidence schema, exact eight selected IDs/actions/expected arrays, real peer
  identities, all actor/fixture/wire/capture/final dependencies, and explicit
  unavailable/different observations. Completed count excludes unavailable
  cases; expected and selected always remain8. `review-required.json` preserves
  the independent actual-artifact review requirement. No catalog expansion or
  final seal is produced.

The five added focused tooling checks cover independent failed preparation
while dispatching all8, immutable ordered checkpoints, observer readiness/secret
mount/graceful cleanup and single execution, a complete all8 unavailable-evidence
assembly/closure, and companion expectation mismatch rejection. They are policy
checks, not SDK/HTTP mocks or runtime evidence. **UNEXECUTED in the filtered author
copy**: parent runs the existing retained suite through MG after independent
review and canonical application, then fresh builds and real owned HTTP/SDK
integration. Local static inspection provides no source-only compatibility pass.

Bounded follow-up fixer `s-20261117T004348Z-2115dc91`, parent start
`baec15c3e861216fd6b28d071e77562c9db4e05a`: original six-file seed
`6eaa629bf9b631558cf1bf8d4853d4a722da4adef1848c6aeb75bda7304ac3c6`
was already applied by MG. Parent supplied accepted complete seed review
`14f1317ed4e89e705bf52a0d226bb04f7a67b13b90aed3bade89105b90614ead`;
these follow-up bytes require a new independent complete-file closure review.
Only support, retained Bats tests and this log changed. Preparation reuse now
reads the saved checkpoint CAS, checks its digest and every inventory entry,
and restores a separate working state; evolving state never replaces the
original inventory or authority. Companion admission pins Desktop 26.09.4 build
c06370ff with the same four-field client/runtime closure. Wire checks preserve
independently measured upstream completion within the bounds expressible by the
unchanged schema; incomplete delivery remains unsuccessful, including truthful
upstream/delivered totals of 1/0. Body lengths never infer completion.

Before runtime, the parent must stage the **exact complete successful independent
source/preparation closure review report**, after reviewing the final source, at
`build/final-client-unit/retained-accepted-source-review-20261117.txt` under the
checkout root. Missing, nonregular, non-UTF-8 or empty/whitespace-only files refuse
runtime. The adapter retains the original bytes unchanged in CAS and binds their
hash as `review_sha256`; it does not parse roles, outcomes, sessions or approval
text. The owning MG and final independent review still determine applicability
and actual-artifact acceptance. A generated identity, summary or hash is not a
substitute for the complete report.

Four appended policy checks cover original-CAS reuse after consistent raw/result
mutation and working-state separation; a successful offline companion
capture/checkpoint/package/finalization lifecycle using the existing synthetic
package fixture, with pin and captured-byte negatives; partial upstream/downstream
and invalid wire totals; and fixed review-file failures/exact UTF-8 retention.
Review text is explicitly synthetic and claims no actual acceptance. All tests,
builds and runtime checks remain **UNEXECUTED here**. Parent owns expected-red
testing, fresh MG/checkpoint/build/runtime checks and complete independent review;
no source-only pass or compatibility credit is claimed. All8 dispatch, original
outcomes, `case_credit:false`, separate server-media authority, frozen agreement
and public metadata remain unchanged. The worker fixed proxy is untouched;
the separate public-API note-type audit remains outside this repair.
