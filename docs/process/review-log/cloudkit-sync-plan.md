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
