# Exact local image and client proof contract

Spec: conformance/meguri-sync-local-delivery
Requirements: local server interoperability, integrity and usable Docker delivery
Anki: none; original acceptance policy, no new reference observation
Scenarios: conformance/recordings/scenarios/meguri-sync-server/

## L1. Scope and separate accounting

Implement conformance/meguri-sync-server#C12 and meguri-sync-candidate#K6 with
final selections Desktop31, actual Mobile10 and retained peer8. Preserve existing
full11/normal10/media4 assertions and intermediate receipts. Do not alter the
1,294 research catalog or relabel historical complete21. Missing/failed/skipped/
unavailable required outcomes leave affected acceptance incomplete; never reduce
expected counts to passing cases or claim broad P-family research completion.

Generator authoring precedes generated hashes. Freeze recipes, exact case/action order, independent expected-value rules and normalization prospectively. Independent review accepts those rules, preparation code and concrete generated initial inputs before final execution. An actual client-generated event is output of its declared action and input to the following action. Using the independently reviewed preparation code and rules, capture, retain, hash and freeze its authoritative offline snapshot BEFORE that event's first HTTP submission to the candidate. Bind the snapshot and capture/submission order to the case, action, actual actor, reviewed preparation identity and frozen baseline. Compare subsequent candidate and receiver state to those unchanged authoritative rows and the independent recipe values, using only declared normalization. Independently review all actual event artifacts and outcome bindings before final credit; no external review pause is required after each event. Cover every action input, never derive expectations from candidate output, and never change case/assertion/count scope from runtime results. Preparation supplies no final candidate credit. Existing identity/artifact/review/sealing rules remain; no fabricated timestamps, builds or rows.

## L2. Disjoint final package receipt

Keep existing intermediate_build variant of meguri-sync-package-v1 unchanged.
Add final_package with exactly these keys:

| Key | Required value |
| --- | --- |
| format | meguri-sync-package-v1 |
| kind | final_package |
| server | Existing meguri_sync server-peer identity |
| source_dirty | false |
| profile | release |
| features | [] |
| executable_artifact_sha256 | Actual retained executable; equals server executable identity |
| lock_artifact_sha256 | Actual frozen server/Cargo.lock; equals server lock identity |
| mg_build_receipt_sha256 | Canonical successful non-dry sync-server-package result bytes |
| build_invocation | Exactly job, run_id and arguments matching package result; arguments [] |
| execution_binding_sha256 | Actual owned execution binding under L3 |
| package | Exactly image_id, inputs_sha256, runtime_inventory_sha256, image_inspection_sha256, assets_sha256 |

Use existing canonical package receipt boundary: same checkout/root/result location,
clean current source, fixed producer/args, pass/non-dry, release/features[]. Rehash
actual immutable image inspection, package-input/inventory and assets bytes and
verify relationships. server.source_sha256 is actual retained package source archive
SHA-256 from package-inputs.source_archive_sha256. server.source_commit equals clean
current/package source. server.lock_sha256 equals frozen server/Cargo.lock entry.
server.executable_sha256 equals inventory usr/local/bin/meguri-sync-server, retained
binary and measured running executable. Existing version/build identify that peer.
Package manifests transitively bind source/locks/features, assets/notices/covered
source. Debug/package archive definitions are distinct. Historical byte equivalence
does not create a final build. Recorder and semantic validator retain/rehash every
dependency together in existing content-addressed artifacts; missing/stale fails.
Independent seal binds this same final image/source/fixture/runtime/observation.

## L3. Actual owned execution

The final execution binding retains all existing keys and adds exactly package,
server_process, container_inspection_before_sha256 and
container_inspection_after_sha256. package is the exact five-key L2 object, every
value equal to the peer receipt. server_process has exactly pid and
executable_sha256: pid is integer 1 through 2147483647 in the owned container PID
namespace; executable_sha256 is lowercase 64-hex SHA-256 of that actual process
executable, equal to observed_executable_sha256, server.executable_sha256 and the peer
receipt executable_artifact_sha256. The two inspection fields are lowercase 64-hex hashes of retained
actual before/after container-inspection bytes. Rehash artifacts and verify their
ownership/configuration relations.
Retain before/after container inspections and actual measured process evidence.
Verify immutable image, exact executable entrypoint/serve args, UID10001, readonly
root and exact owned private data mount. Docker init PID1 is not assumed server.
Use smallest existing separate owned-tooling measurement; no general monitoring,
privileged fallback, shell/Python/runtime installation or broad process access.
Fail if actual process binding cannot be proved. Hook binaries remain separate.

Automated candidate endpoint stays http://127.0.0.1:8080/ in owned isolated namespace,
no public port/alternate fallback. Fixtures cannot choose arbitrary URL/executable/
command/container. Migration source has distinct owned runtime and fixed private
origin http://127.0.0.1:8081/. Freeze pre-switch actual synchronization, source
identity and collection/media authority. Credentials remain separate readonly
regular0600 input with exact owner/link/shape checks, absent from ordinary logs/
artifacts/args. Require empty owned output. Cleanup verifies run/name/creation nonce
ownership for containers/secret/volume; failure fails job. No unrelated prune.
Actual Mobile private routing is separate operator setup, no fixture loophole.

The final-only trusted MG invocation adds exactly root, server_arguments,
data_volume and owner_nonce to the existing invocation fields. root is a nonempty
absolute canonical MG root identity string, not a filesystem selector. data_volume
is the nonempty actual owned RW volume name mounted at /data, not fixture input.
owner_nonce is the existing candidate-container meguri.sync.owner creation nonce,
exactly 32 lowercase hex characters. Volume ownership uses the existing volume
helper; its separate nonce may differ. server_arguments is the producer-configured exact string array
[serve,/data/owner,<configured HTTPS origin>,0.0.0.0:8080,--trusted-peer,<configured
exact IP>]. All four come from the trusted owning job, never public fixtures or
credentials. Match them to canonical build root and before/after actual inspection,
including exact mount/name/labels/serve argv and equal Created identity. Existing
run_id/container_id still bind that owned container. These are invocation
verification inputs; L3 execution binding adds only its four already defined keys.

Use one fixed execute-then-record handoff. Before client actions,
/in/preflight.json has exactly L2's top-level keys, but kind is
final_package_preflight. Its execution_binding_sha256 resolves exact L3 binding
with ONLY container_inspection_after_sha256 omitted. All other package/process/
identity checks remain strict. Preflight and execute grant zero final credit.

Each final segment invocation declares child_phase, one of final-full,
final-normal, final-media, final-migration, final-replacement-upload,
final-replacement-download, final-metadata-only, final-fresh-fresh or final-rich,
and child_stage, exactly execute or record. Outer MG arguments remain outer
arguments; actual child CLI identity is retained separately. Execute produces
immutable candidate-observations.json and execute-admission.json. Admission has
exactly format, child_phase, invocation_sha256, preflight_peer_sha256 and
observations_sha256. format is meguri-sync-final-execute-v1; child_phase matches
segment; hashes are lowercase 64-hex and resolve actual retained raw/preflight/
execute-invocation bytes in the content-addressed store.

Trusted MG captures admission SHA-256 BEFORE measuring actual after-state, then
creates complete final_package/binding. Record invocation additionally contains
execute_admission_sha256, the fifth conditional trusted comparison input.
Execute/record common invocation fields equal, except child_stage and this
record-only admission hash. Fixed --stage record is offline, no client requests,
retry or outcome changes. Final output starts empty and raw input is readonly.
Record revalidates complete final identity and SAME immutable observations.
Final binding equals preflight after removing ONLY after-inspection hash: same
image/source/container/Created/process/before. No case/action/actor/fixture/expected/
observed/outcome changes; only actual final artifacts may be attached.

Final assembly reads exactly /in/segments/<child_phase> for the nine fixed
segments, each retaining invocation.json, selection.json, evidence.json,
summary.json, artifacts/ and candidate-observations.json. Each segment has fresh
owned storage/account and one protected credential pair. Exact unchanged full11+
normal10+media4+six single additions must equal 31 unique required IDs, same image/
source/runtime and outer MG identity, with no missing/skipped/failed credit.
Assembler/seal accept strict final_package evidence only, never preflight.

## L4. Exact client requirements

Desktop roles: client_a is Desktop A, client_b Desktop B, fresh_client independent
fresh Desktop. Mobile/retained roles: client_a is the selected actual Mobile or
retained peer; client_b its actual Desktop companion; fresh_client independent
fresh Desktop. service is owned image, operator synthetic manual action and
fault_driver controlled fault peer. Each actor change has a separate ordered
action/receipt identity. Companion Desktop cannot become selected-client witness.

Desktop31 preserves existing25 and adds genuine other-server migration, populated
authoritative full upload/download, reachable authored no-card metadata, fresh/fresh
decision and rich final fresh receiver. Rich proof compares independent expected
supported object/config/template values after full and EACH normal direction,
then final notes/cards/history/scheduling/deletions/media maps. Zero cards never
implies absent objects/config/media. Include note only if actual-client-valid and
reachable, never invalid raw injection. Full collection replacement and media have
separate exact expected maps. Public normal completion cannot leave pending full.

Mobile10 is identity/auth, initial download, bidirectional collection edits/deletes,
bidirectional media, rich fresh receiver, populated authoritative upload/download,
no-change and one review each direction. Witness rich state resident on Mobile by
supported authoritative full upload or supported synthetic export before independent
Desktop comparison; fresh Desktop re-reading unchanged server alone is insufficient.
Record installed 26.10 app build/OS and hashed real-device execution bound to fixture
and owned image. Use new isolated synthetic profile/account, never owner's data.
Desktop/service results cannot replace device proof. Ground manual steps in actual
available UI; missing device/build/route/actions remain explicitly incomplete.

Retained8 uses M-I01/meguri_policy/retained_peer: actual authentication, full upload,
full download, normal no-change, normal bidirectional edits/reviews/deletes, media
bidirectional mutations, media no-change and conservative manual uncertainty
recovery. Real public HTTP APIs, no substituted transport. Separate test-only
workspace remains outside shipped server graph. No app/core behavior edit; unsupported
or client-origin failures block affected claims. No P-family/application acceptance.

## L5. Safety evidence and narrow additions

Keep meaningful existing safety/browser tests and identify exact source/lock/features,
including hooks versus hook-free release. Historical total alone is not evidence;
never reduce inventory to that total. Reuse interruption/lost-finish ordinary retry,
media lost-ack replay, isolation/generation, bounded decode/path/archive/schema,
physical tmpfs ENOSPC, root/startup, setup/invitations/auth/reset/recovery/proxy proofs.

Trace normal/full collection_operation and media publish_at through shared account
barrier, generation check and short commit. Reset/disable use that barrier, retire
round and advance generation. Existing both-order proofs plus exact source trace
can establish staging ordering; do not claim every phase directly executed. Add
only missing assertion. Strengthen existing full lost-ack process-loss test with
applicable ordinary retry/conservative recovery and exact committed collection/media
preservation. No duplicate all-phase campaign or new fault framework.

## L6. Interface, local workflow and ordinary sanity

Reuse package-test's four useful fixtures, same-volume restart, private ownership,
assets/notices/covered source, exclusions and actual executable evidence. Rehearse
documented init/start/stop/test/opaque-ID recovery against exact final image.
Retain nine frontend proofs and one actual zero-retry full browser journey with
nine screenshot/a11y states against exact image through existing fixture. Narrow
package smoke alone cannot establish full member/settings/reset usability. Fix
observed singular account count, then visually verify; no mirror copy test.

Retain exact HTTPS origin/trusted socket/Secure cookies over HTTP, missing/null/
wrong Origin and forwarded-spoof rejection. No TLS/trust installation or browser
security weakening. Ordinary logs capture elapsed/methods/bytes/tuples/media and
available resources for initial/no-change/review/media with input/workload/settings/
hardware/runtime/image identity. No benchmark/capacity program, backup/restore or
deployment. Required independent review, dependency/provenance/privacy/similarity
and original-base coherent gates remain mandatory.

## L7. Concrete values and normalization

Final Desktop expansion is fixed: unchanged candidate-full-selection.json eleven,
candidate-normal-selection.json ten, candidate-media-selection.json four, then
candidate-final-additions.json six. This is not configurable inclusion or a new
framework. Separate Mobile/retained templates retain counts ten/eight. Source
templates bind final-fixture-recipes.json SHA-256; generated input manifests bind
initial inputs before final execution and dynamic event inputs before their
applicable candidate submission under L1. Completion names/actions/expected arrays
are frozen; unsupported client paths do not delete cases or manufacture success.

Original rich_initial and sequential forward/reverse changes in the recipe define
independent expected authored values. Boolean match observations require exact
comparison to those values AND complete supported authoritative pre-candidate
snapshots. Retain observed projected values and hashes, not only true booleans.
Compare after full, each normal direction and final fresh receipt. Defaults and
generated IDs come from frozen actual authoritative fixture, never candidate output.
Map generated IDs only through frozen logical keys; preserve ordered field/template
arrays, exact integer/JSON tokens and review/card scheduling rows. Exclude only
neutral-contract protocol-owned modification/sync/USN stamps; identify every
normalization explicitly. Do not normalize away object settings, history or media.

Replacement recipes define exact distinct authoritative fields and media maps.
Full collection-only transfer preserves the independent server media map. Ordinary
bidirectional reconciliation with disjoint local/server names yields their exact
declared union; do not treat collection replacement as media replacement. Freeze
actual client decisions and chosen collection/media authority separately. Migration
must synchronize both collection/media with fixed owned source before switching.
Existing original SVG/WAV/binary media coverage remains in unchanged media4.
