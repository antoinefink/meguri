# Native compact sheet hierarchy classification

Status: WIP; no unit acceptance.
Start:9f700afcbd63d3e68e97131e16da91d011e7c496.
Fixer:s-20261103T133750Z-f276f58e.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.

Phone61191 and67343 expose a native half-screen grabber without an identifier.
The native contract keeps that control. A spec-writer proposal and independent
review approve recognizing only the evidenced iOS structure for the missing-ID
check. Every node still receives identifier-format, duplicate and layout checks.
Unsupported or ambiguous shapes remain findings. This is a UI audit heuristic;
the exported tree contains no framework-ownership field.

Export72029 rejects the captured status hierarchy. Review records that P2.
Export77472 supports its content group and collection view, with refusal tests.
Independent code review is clear. All21 focused tests79248 pass;
the two actual retained hierarchies79721 pass with no issues.

Private79649 flags custom code/test regions. Export80971 changes only names,
comments and synthetic text with matching expected values. Independent narrow
review confirms unchanged predicates, native facts, assertions and geometry.
Private84233 and Similarity84891 pass. ImageTools84788 passes all74 hierarchy
regressions. Original failed drive receipts remain retained; no result is edited.
No additional locale, detent or platform is inferred from this evidence.
