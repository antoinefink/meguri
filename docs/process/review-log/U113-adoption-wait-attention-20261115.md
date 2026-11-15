# U113 adoption wait attention

Starting commit: `cf0f8de5967de0f97236ff5a39bedd756cea5423`. Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

P3/P14.4 and V14.3 require actual recovery waits. Projection now reads the scoped adoption row before the bound-collection branch. Accepted transport failures remain visible without a collection. Active staging is not a wait. Ready work contributes attention when a review blocks it or its recovered candidate is isolated for a file mismatch. Publication behavior and global completeness remain unchanged.

Author: `cleanroom/20261115T112428Z-67805`. Complete source reviews: `113345Z-98994` and `114436Z-32556`. The first found a test compile error, corrected in `113523Z-10428`, and an existing recovered-Cancel gap. Later fixture repairs are `114040Z-31188`, `115037Z-39787` and `115724Z-41935`. Root read every delta.

Final fixture review: `120038Z-42952`, CLEAR, no new P0–P2. The required recovered-Cancel P2 remains open.

Evidence:

- Tests alone: `core-test/20261115T113635Z-16312`, four expected behavior failures in six checks. Waiting imports were omitted; runnable staging was counted.
- Initial integration: `113717Z-21835`, 53/55 passed. `114508Z-33520`, 54/56 passed. Both retained two new fixture failures.
- Diagnosis: `115349Z-41248` passed the foreign-scope method after its missing clock was attached. Fixed tags located the Ready refusal in candidate preparation, before abort. The empty-page fixture lacked scheduler configuration. The final fixture transports a valid scheduler record through ordinary intake. No gate or assertion was removed. All temporary source traces were restored before final checks.
- Final: `120111Z-43661`, 56 passed. This includes connection jobs, adoption guards and the real Rust review-save-sync-reopen test. Other tests were outside the selector.
- Rust lint, layering and release-content checks: `core-lint/20261115T120205Z-44772`, passed.
- Privacy: `private/20261115T120115Z-43979`, three files passed.
- Similarity: `similarity/20261115T120119Z-44277`, three files, zero flags; only the implementer view was read.

Required follow-up: recovered Ready/FileMismatch adoption has no live owner, so existing availability hides Cancel and execution cannot perform it. Ordinary abort would restore the old binding incorrectly. This P2 needs a dedicated guarded cleanup preserving isolation, files and the R6 journal, followed by Choose Collection. This commit is partial U113 work. Unit and release acceptance remain pending.
