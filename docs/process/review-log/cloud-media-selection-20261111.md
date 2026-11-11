# Media download selection — 2026-11-11

Role: fixer. Starting commit: `e524c19a`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The normal signed Mac preview downloads its collection, but media makes slow
progress. A bounded host stack sample finds the selection job inside
`media_download_blocked_by_unsyncable`, loading complete MediaName records.
This is local debug evidence, not a signed acceptance receipt.

The selector checks every remaining download candidate even after it has
selected a download. Each check reads the complete media ledger, entities and
inbox. The repair checks whether a download is still needed before that read.
It preserves candidate order, upload selection, and the separate validation
of retained work before submission. It changes no stored state or interface.

The regression uses real incoming MediaName records and metadata replies, then
measures complete entity-body reads during one Runtime selection. It verifies
that a download is selected and that work is bounded by four times the entity
count. It does not measure SDK latency or prove live media delivery.

- Red `core-test/20261111T074443Z-99980`: 624 body reads for 24 entities.
- Focused `core-test/20261111T074508Z-670`: all four checks pass, including
  visible priority, collection drains and unsyncable-file admission.
- Wider `core-test/20261111T074539Z-2604`: all 118 FFI media tests pass.
- Format `core-format/20261111T074721Z-5494`: passes; no further source changes.
- Lint `core-lint/20261111T074736Z-6181` and privacy
  `private/20261111T074755Z-6511` pass.

Direct diff review confirms that later upload candidates can still replace the
selected download as before. Selected-download validation remains in selection
and admission. No independent reviewer was used, following the request to work
without another agent. Independent review, the integration gate and live
verification remain open. No Unit or milestone completion is claimed.
