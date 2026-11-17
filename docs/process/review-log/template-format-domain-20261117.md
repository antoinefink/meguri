# Checked review-template format operation

Starting commit: `3f40fdd528d9c204de775c62833e2c49753a802d`.
Prepared for the required retained-peer normal-sync case assigned to Core.
Existing app gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Author `060714-15040` adds a public checked operation for one existing
template front/back and optional CSS. The original aggregate digest and
ordinal identify its base; the optional ID is a crosscheck. Plain-save
guards remain, including callers that skip template checks. This operation
always validates V3 and uses the existing executor and card generator.

Plan and writer both check the original digest. Only changed fronts
recompute requirements and generate missing cards. Retained cards, metadata
and template row stamps stay. Aggregate and generated rows share one undo
step and transaction. No-op, cancellation and rollback have focused cases.
No schema, SCM, import or full-transfer shortcut is introduced.

Review `062037-25560` found one P2: a concurrent ID change returned the wrong
refusal. Fixer `062609-28263` moves the digest before the ID check and extends
the existing test for both plan and writer. Final review `063118-36239`
finds no P0–P2 findings. Root verifies complete author/reviewer source equality.

Eight new tests are present without skip annotations. Actual Docker checks,
existing regression execution and the retained peer round trip remain pending.
The real precommit fault checks rollback after aggregate, cards and counters
were written. No new fault hook is needed.

This is WIP. No frozen API, Unit completion, passing full gate or sync
acceptance is claimed. The separate app Resume proof failure remains retained.

Scoped privacy `063903-42298`, static `063854-41479` and similarity
`063857-41848` pass. The filtered three-file report has zero flags or
exemptions. Actual Docker execution remains pending.
