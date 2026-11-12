# Stopped-binding Resume account reaffirmation

Status: bounded fixer export; checks unrun; no acceptance claim.
Upstream start base supplied by integrator: `741f4211` (clean master).
Filtered checkout starting commit: `13f4d42749c3aef9b8887bb088d3d5ab4ee35e3b`.
Role: fixer, following `docs/process/roles/implementer.md`.

The independently confirmed R11/V9.2/S2.2 regression occurs when a matching
account read completes after a stopped binding's ready ConfirmResume preview.
The binding remains unusable and unverified with no SDK, so the existing
`adapter_account_locked` preservation predicate advances the adapter token.
The unchanged exact comparison in `validate_resume_context_locked` then
refuses the original retained context before explicit Resume reaches its own
proof yield. Read-only analysis 10186 and independent review 20820 were
reported in the task; neither was rerun here.

Authority: `docs/process/review-log/U114-r11-account-proof-contract-20261103.md`
(R11), `specs/engine/suspend.md#S2.2`, and
`specs/sync/cloudkit-recovery.md#V9.1`, `#V9.2`, `#V26.2`.
R11 permits reuse of matching proof accepted in this process and names the
same-backend ready ConfirmResume preview as the stopped binding's authority
witness. No new contract decision or waiver is introduced.

The repair constructs the existing internal AcceptedIdentityRead locally at
the already guarded completion boundary. Only that validated completion can
supply it to account handling; direct account events supply none. Its request,
original guard, incarnation, collection ownership and available matching
identity must agree with current State. The new internal Resume helper also
requires no SDK, an unusable/unverified binding, the ready retained preview,
matching preview account/binding/file/decision and actual active/epoch reads.
It delegates to the unchanged original-context validator, including binding,
scope, opening, generation, incarnation, file/invalidation, exact adapter guard,
tracking binding and publication/adoption operation comparisons. An ordinary
NotOffered result declines preservation; other validation errors propagate.

No retained context is refreshed, reconstructed or rebased. The accepted
read's resulting guard is recorded as before, after account handling succeeds.
Reaffirmation leaves isolation, usability and verification unchanged. The
existing active-SDK preservation expression and SDK-event behavior remain in
place. Provider, fence, operation, file and final SDK validation are unchanged.
The helper reads only current State and existing tracking data, with State
before tracking access; it calls no provider or callback and takes no compact
authority lock. No Swift, FFI, shared schema or public API changes.

Added Host regressions in `cloud_publication_phase.rs` use the existing actual
publication/preview fixture and identity begin/completion APIs:

- `stopped_resume_retains_original_preview_across_accepted_matching_account_read`:
  an accepted matching process read, ready stopped-binding preview and another
  matching completion retain the original guard and offer. The binding stays
  isolated/unusable/unverified with no SDK before the explicit Host Resume,
  which succeeds using the original preview without more Service requests.
- `stopped_resume_reaffirmation_requires_accepted_read_and_current_original_authority`:
  an uncompleted lookup plus direct account event, stale context after pause,
  changed publication operation and changed account cannot retain authority
  or resume the original preview.

The existing
`resume_proof_refuses_changed_account_file_fence_operation_and_replaced_preview`
and active SDK/native authority negative tests are unchanged. No assertions,
timeouts, failure flags or retries were weakened or added to bypass failures.

One bounded source consistency pass inspected the final source diff and both
account-handling call sites; the source diff whitespace check passed.
No formatter, builds, Rust/Swift tests, VM work, privacy scan, similarity
scan, independent review or full gate ran in this filtered session. The
integrator owns formatting, focused Rust tests (including existing refusal and
active SDK/native authority negatives), the actual retained Swift row
`serviceResumeOwnProofFinishesWithoutWaitingForItsOwnTask` with priorAccount
true and its retained selection, independent review, provenance/privacy/
similarity checks and the full coherent batch gate. This export establishes
source changes only and supplies no acceptance or runtime verification.

## Integration evidence

- Actual starting commit: `741f4211`. Approved-contract reading `cleanroom/20261112T103459Z-20820` resolves the unchanged-account case under R11 and S2.2; no new rule or approval is needed.
- Independent source review `cleanroom/20261112T104418Z-40405`: no P0-P2 findings. Exact context comparisons and final validation are unchanged. The tracking-error branch has not been exercised.
- Initial `core-test/20261112T104455Z-41896` stopped at compilation: one new test arm returned an unused operation id. Filtered correction `48788` discards that return with a semicolon. Exact comparison confirms no other test behavior or production change.
- `core-test/20261112T104804Z-52186`: 135 passed, 335 skipped by the explicit selector. This covers the retained Core/FFI selection and all selected publication/Resume tests, including both new cases and the unchanged refusal cases.
- `swift-test/20261112T105039Z-56300`: both `serviceResumeOwnProofFinishesWithoutWaitingForItsOwnTask` rows passed. The full four-group run remains failed with 21 issues in the two-fetch and mixed/flush groups. The publisher group passed. Those failures remain open.
- Similarity `20261112T105043Z-57453` passed. Full integration and signed-device verification remain pending. No Unit trailer or release acceptance.
