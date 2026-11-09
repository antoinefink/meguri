# Selected control observation: pending native integration

U113/U248 start: `e6b93870`. Batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261109T094715Z-c7c7c45e`.
Fixer: `s-20261103T133750Z-f276f58e`.
Exports: 92617, 95751, 98117. Contract: conformance/cloudkit-agent J3.2.

The test-only reader joins current ownership with actual publication or
adoption controls. Adoption requires the exact operation-owned backup,
activation and current file. Publication retains the actual request, attempt
and Saved/Conflict result after ACK commit, then joins successful done Active.
It changes no schema, write authority or product decision. Restart without
the original publication observation stays explicitly missing.

Initial review P2 lost request/result provenance. R1 P2 found allocation before
the audit bound. Both are fixed; R2 is clear. Correlation now uses a capped
streaming hash before audit clones. Over-bound input leaves product acceptance
unchanged and supplies no proof. Root applied one equivalent nested-if lint
rewrite after export; its predicates and order are unchanged.

Focused core-test 99266 passes 14 cases, including real matching conflicts,
rolled-back ACK commit, cap boundaries and successful over-bound publication
without audit proof. Final shared core-test 5031 passes all 21 cases;
core-lint 6379 passes four checks. No assertions or limits were weakened.
The reader has no native control bridge yet. Signed execution, full integration
and unit acceptance remain pending; no Unit trailer is justified.
