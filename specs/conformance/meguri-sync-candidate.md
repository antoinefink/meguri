# Meguri Sync candidate test boundary

Spec: conformance/meguri-sync-candidate
Requirements: Meguri Sync U3, U5, U6, U11; R4, R6, R29, R32
Anki: 26.09.3
Scenarios: conformance/recordings/scenarios/meguri-sync-server/

## K1. Original test policy

These rules are original Meguri test policy, not reference-server observations.
They extend `conformance/meguri-sync-server#C11` for the candidate invocation.
They establish no interoperability result by themselves. Reference code and
runtime dependencies remain quarantined outside the production workspace.

## K2. Owned peer and endpoint

The MG job creates the candidate process, isolated network namespace and
temporary synthetic data root. The candidate listens at
`http://127.0.0.1:8080/` in that namespace. The reference client joins the same
namespace. Test proxies may use additional job-owned loopback listeners there;
their upstream remains that fixed candidate endpoint.

Public fixtures cannot select a URL, executable, shell command, container or
host network. No public port is needed. A missing or unconfigured candidate
fails; there is no fallback to another server. The orchestrator verifies the
candidate process/container belongs to its current MG invocation.

## K3. Synthetic account and protected inputs

Candidate orchestration owns initialization and account setup through the
original account contract. Use a fresh synthetic account and temporary data,
never an existing profile. The quarantined client adapter does not need access
to server source or its storage root.

Pass username and password in a separate, read-only JSON input owned by the job,
with host permissions 0600. Mount it only for the client invocation. Do not put
credentials in public fixtures, ordinary logs or command arguments. Retained
observations describe authentication shape and outcome without exposing bearer
secrets or passwords. Refuse missing, malformed or unreadable inputs.

## K4. Build identity and evidence

Pass a separate MG build receipt using the candidate peer identity fields from
the existing schema: version, build, source commit, source SHA-256, lock SHA-256
and executable SHA-256. Bind its profile, enabled features, dirty-source state
and actual build invocation as additional build evidence. A source commit alone
does not identify uncommitted source or the executable being tested.

The orchestrator checks that receipt against the actual running owned binary.
The client adapter validates the neutral receipt and retained hashes; stale or
wrong inputs fail. A debug build can establish only its
own candidate result, not release-package acceptance. Final release evidence
must bind the exact final artifact.

Case selection, synthetic inputs, actions, outcomes, execution counts and
independent review remain bound under the existing conformance contract. A
correct invocation format, empty run or schema-valid receipt cannot establish
Desktop, Mobile or retained-Meguri compatibility.

## K5. Separate build and normal-completion evidence

The same committed executable build may serve the full-transfer and incremental
selections. The existing full-only build receipt describes how that executable
was produced; each client invocation separately binds its actual selected phase,
cases and completion counts. Both invocations use the build receipt's recorded
producer identity. Reuse does not turn one phase's result into another's credit.

The selected Desktop incremental cases cover reviews, deletions and collection
objects in both directions, no-change sync and the declared chunk boundaries.
Every credited client normal call must report completion without a pending full
transfer. Reject a pending full-transfer decision even when observed traffic
contains only metadata and no collection bytes. Retain actual client decisions
with their actor identities alongside the wire and final-state observations.
A request list or successful HTTP status cannot substitute for that completion.
