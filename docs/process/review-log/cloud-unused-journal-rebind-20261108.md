# Unused review journal binding

U151 and related U113/U114 work remain unaccepted. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Component start: `3299984cb480715b1f4aef4e64d0f38e40790638`.

The normal phone open reports the fixed category `pending_answer_identity_mismatch` during the review-session open step. Its journal eligibility is unknown. No actual journal contents were read by the orchestrator and no phone data repair has been performed.

## Contract and proof

Spec commit `3299984c` adds only the independently reviewed managed-opening exception for an exact unused version-2 Header. Raw strict opening and all history-bearing, compacted-history, corrupt, retired, legacy and wrong-platform rules remain. The exception requires a durable exact archive, a distinct fresh incarnation/current binding and zero replay. Spec writer session: `s-20261108T190858Z-d79453ef`.

Filtered test export `cleanroom/20261108T203152Z-79290` uses fixer session `s-20261103T133750Z-f276f58e`. Independent review clears the real managed Engine/ReviewService setup, strict raw refusal, pre-Ready archive observation and teardown. Existing production code and tests are unchanged.

`core-test/20261108T203725Z-81786` compiled and ran the exact regression with test-hooks. It failed at the intended pre-Ready assertion with `review journal: pending_answer_identity_mismatch`. Workers and engine close completed before the assertion. This is semantic RED evidence, not a passing test. Implementation, fault cases, negative coverage and phone qualification remain pending.
