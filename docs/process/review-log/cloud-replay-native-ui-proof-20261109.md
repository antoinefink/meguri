# Native replay UI proof in progress

Start: `da321315`; batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261109T094715Z-c7c7c45e`, export `70518`.
Fixer: `s-20261103T133750Z-f276f58e`, exports `79077`, `83403`.

The Debug Mac fixture uses production rendering and bridge admission. The native
case requires twenty exact activations across four renders, native identifiers,
localized names and four hierarchy captures. The existing checker is unchanged.
The suite is part of canonical Mac UI execution. Normal app UI is unchanged.

Build `77055` stopped at two Swift capture errors. Their source repair is reviewed;
actual VM execution remains pending. Selftest `85013` passes six cases and fails
one stale usage-list assertion in `agent_loop.bats`; its repair remains pending.
The new replay-specific selftests also remain to be run.

Independent reports cover the initial proof and both repair deltas. Native
identifier mapping is unverified. The old package failure stays enabled until
equivalent native proof passes. No unit, integration or release acceptance is claimed.

Selftest `89436`: twelve checks passed. Reviewed export `88808` adds
`import AppModel`. Build `90816` stopped at test line 93; native execution
remains pending. Run `90227` could not reach Docker. Failures are retained.
