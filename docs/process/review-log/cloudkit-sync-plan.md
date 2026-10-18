# CloudKit sync plan review

- Unit start: `9c8eb32370a7a2f750da25a1d521123f4b8851cf`.
- Session: `s-20261018T145745Z-64b52004`, spec-writer.
- Scope: current plan and milestone manifest; no app/core implementation.

## Review and fixes

1. Verified Apple API behavior against primary documentation: manual send/fetch completion, application-owned durable state, account isolation, private database quota and encryption boundaries.
2. Independent claim/consistency review found obsolete schema-upload warnings, Anki optimizer gates, restore pause handling, release entitlement checks and onboarding/provider instructions. Replaced active instructions and their tests with the CloudKit contract; retained protocol details only for manual Settings transfers.
3. Added explicit CloudKit evidence scopes to the milestone manifest so old Anki coexistence results cannot satisfy the new signed cloud lane. Reconciled exact milestone text bindings and preserved existing requirement/unit identities.
4. Checked that the reviews-first milestone has a minimal collection-only file import before the full package UI lands. Separated test containers and disabled sync before scripted private-fixture performance work.

## Verification

- Required `tools/mg gate --base 9c8eb32370a7a2f750da25a1d521123f4b8851cf`: static checks, milestone bindings and performance catalog passed in runs `20261018T151309Z-86457` and `20261018T151508Z-88950`. The final committed-tree gate receipt is reported with delivery.
- CloudKit integration is planned, not implemented or exercised. The reviewed record/conflict/durable-tracking specs and signed development-container multi-device evidence remain prerequisites (R2066).

## ce-doc-review (2026-10-18)

Review start: `4435bb0599d42e8742f40ca2393663435f92988d`. All seven independently dispatched Codex reviewers completed: coherence, feasibility, security, adversarial, design, scope and product. No Compound Packs were configured. Different-provider review was skipped under the plan's Codex-only execution instruction.

Six retained findings were applied under the user's instruction to apply findings with best judgment:

1. Coherence: remove the old scope exclusion that forbade the selected private CloudKit contract.
2. Coherence/design: align remaining welcome/empty-Home requirements and backend-specific initial-download tests with iCloud onboarding and Settings-only Anki login.
3. Coherence: align older background-task assumptions with CKSyncEngine scheduling and the maintenance/optimizer registration owner.
4. Coherence: make replacement failure/cancellation tests restore the prior generation's cloud publication decision, including a pre-existing pause.
5. Coherence: trigger existing structural safety backups before mutation after destructive-preview confirmation, independently of the removed Anki warning dialog.
6. Design: make preserved conflict edits accessible through a per-object Review Conflicts flow, with durable relaunch access and normal-edit recovery tests.

The other five lenses returned no retained findings. No product choices, approvals or deferred review findings remain. Changes are confined to the plan and this required unit log. CloudKit contract work and signed integration evidence remain implementation prerequisites; this review does not mark them implemented.

Fix verification: coherence and design independently rechecked their applied findings and returned no remaining findings. The pre-commit project gate passed static checks, milestone bindings and the performance catalog in run `20261018T153653Z-6840`; the final committed-tree gate is reported with delivery.
