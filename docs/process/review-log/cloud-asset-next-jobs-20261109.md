# Owned asset-selection jobs

Original source start: `bf1aa6a2`. Integration start: `15a0c9c3`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer session: `s-20261109T094715Z-c7c7c45e`.
Fixer session: `s-20261103T133750Z-f276f58e`. Anki: none.
Filtered exports: `22252`, comment-only `66590`, fixture-only `72076`.
The filtered synthetic Git hash is not the main-checkout source base.

Ordinary next work uses the original registered pool, no long-operation slot.
Checks after waits and scan precede cursor effects, preserving original Host, generation, SDK, pass and retry command.
No Driver or Host mutex is acquired beneath Runtime/tracking.
Typed results distinguish pending, cancellation, true exhaustion and issued work.
After effects, original results and handles survive cancellation or listener failure.
Swift retains ordered drain/result delivery and original cancel/finish ownership.
Synchronous compatibility APIs remain. Retry and first-grant accounting are unchanged.
Independent source review and fixture review report no P0–P2 findings.

Core lint `20261109T170627Z-65196` passes all four checks.
Focused Core `20261109T171013Z-68625` passes 28 of 30 checks.
Both failures are retained. One expected early Driver validation instead of the actual later Runtime stale-incarnation refusal.
The other required a sixth issued retry command after the budget was exhausted.
The corrected fixtures assert exact Runtime error conversion and the stale original SDK.
They preserve five actual charged retries, then prove fresh-pass exhaustion using the fifth issued execution.
Core `20261109T171744Z-74922` passes those two corrected checks; the other 28 bodies remain unchanged.
Final lint `20261109T171700Z-74126` passes all four checks.
All timing limits remain; the new tests reserve the existing idle runner as core/AGENTS.md requires.

Privacy `20261109T171840Z-75673` passes all sixteen source/evidence files.
Similarity `20261109T171855Z-76043`: 14 passed, one excluded, zero exceptions.
Native CoreBridge checks are grouped with the live-resume repair to reuse Apple products.
Whole submission, required resource consumers, the original batch gate and real signed round trip remain pending.
No Unit trailer, P24 closure, milestone pass or release acceptance is claimed.
