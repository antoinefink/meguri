# Root-confirmation prerequisite review, 2026-11-01

Scope: the single P24 root_read acceptance item and its matching Q11 row.
No normative rule changes, other prerequisite closure, Unit acceptance or signed
CloudKit claim are proposed. Main-checkout files were not edited.

The corrected rule is consistent: a usable current owner may confirm only from
its own qualified root drain, a confirming Active read, or the publisher's
accepted Active at done. Epoch-only reads and lost-zone Active reads do not
confirm. Every accepted Active is retained as the last-applied exact record;
isolation keeps that record without granting send authority.

## Clause-to-test map

- cloud_recovery.rs:1458,
  root_confirmation_safety_finite_permutations_require_current_instance_proof:
  six permutations of epoch-only, stale read and stale drain, each followed by
  either current-instance confirming proof. All negative batch attempts have no
  mutations or Service requests; only accepted proof permits the real Save.
  Its lost-zone path checks the newer Service envelope and system fields,
  isolation, pending edit, unchanged ACK, durable reopen and refusal of late proof.
- cloud_recovery.rs:1290,
  normal_root_confirmation_persists_exact_service_active_across_reopen:
  both record-ID read and completed root fetch replace an older stored Active,
  persist exact returned data, retain the local edit and ACK, and require fresh
  proof after reopening before a real conditional Save.
- cloud_recovery.rs:1370,
  ordinary_epoch_switch_persists_exact_service_active_across_reopen:
  both read and completed fetch atomically retain exact Active with isolation;
  neither emits RootConfirmed or builds a collection mutation, before or after
  reopening and late-proof delivery.
- cloud_publication.rs:393,
  confirmed_initial_publication_scans_unacked_rows_saves_real_batches_and_switches_last:
  actual SDK collection Saves precede Complete and the final own Active Save;
  the accepted switch finishes the op and survives reopening.
- cloud_publication.rs:576,
  p24_active_lost_reply_preserves_attempt_fields_and_done_restarts_thirty_day_retention:
  a real landed Active whose reply was lost is recovered after reopen, retains
  its exact accepted fields, and reaches done without another switch.

## Evidence and limits

Production repair is committed at 9d88dddf. The input receipts
core-test/20261101T184946Z-96902 (3/3) and
core-test/20261101T185129Z-97904 (29/29, complete recovery target)
record dirty aeb9e555 inputs, subsequently included in that repair commit.
They are not clean-head or common-suite receipts. Earlier expected assertion
failures, the R1 finding and the helper compiler correction remain recorded in
docs/process/review-log/cloud-root-active-persistence-20261101.md.

The two publication tests passed in clean selected CloudKit receipt
core-test/20261101T134745Z-91787 at
4fa09dad1b6eca37355ead52bf75a260b5d3aec9 (420/420, not the whole workspace).
Their test source is unchanged through the repair. Production integration since
that receipt is not represented as freshly tested by the older receipt.
The fresh selected common CloudKit run,
core-test/20261101T193641Z-63923, passes 545/545: Host 112, Package 106,
Sync 327; 387 tests were unselected. This includes the relevant root and
publication cases, media workflows, and crash/reopen cases. Its inputs are dirty
9d88dddf integration inputs, not a clean-head receipt or whole-workspace proof.
The following filtered cleanup changes comments and test names only; production
code paths and checks have the same logic. Root will record the matching source.

workflow_runtime.rs:854 retains current-instance, verified ownership, scan and
publication guards. Its accepted normal path stores Active with RootAck; its
isolation path uses runtime_store.rs:206 to commit Active with isolation before
runtime cache changes. publication_runtime.rs:1051 records the accepted Active
and confirmation through publication_done. The reviewed fake/core prerequisite
has no remaining rule or implementation finding in this bounded audit.

The patch removes one P24 item and updates only its Q11 evidence row.
confirmed-root remains an invariant for every acceptance run; this packet does
not claim all acceptance runs were executed. Actual SDK and signed P21/K23
requirements, U113/U114, batch and release acceptance
remain open. Root owns integration, independent review and required checks.

Independent review found no P0-P2 issues in the closure patch and its evidence limits.
