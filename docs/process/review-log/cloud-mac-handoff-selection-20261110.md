# Mac handoff test selection

Starting commit: `e2917485`. Pending iCloud batch; no unit acceptance.

Actual parent `92131` built signed Mac product `92859` and passed VM delivery
`94523`. It then stopped before app-hosted preflight with `implementation_missing`.
The frozen native plist contains both required suites in lexical order, while
the handoff compared the array to source-plan order. Existing build admission
already requires exactly those suite names as a set.

The handoff now accepts either order of the same two distinct string names.
It still refuses missing, duplicate, extra and malformed selections. Legacy
Soak-only products still report missing preflight code before setup. No generated
plist, test plan, build/delivery guard or signed evidence is changed.

Independent review found no P0–P2 issue. Focused tooling check `95934` passed,
including both-order preflight/handoff cases and refusal regressions. The next
real app-hosted run remains pending; VM delivery alone proves no cloud operation.
