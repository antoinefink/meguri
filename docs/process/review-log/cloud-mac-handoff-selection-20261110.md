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

Parent `96515` built `97232` and delivered `98812`, then stopped before command
delivery. Audit found the same order assumption in guest preflight, smoke-plan
derivation and owner setup-plan derivation. Those readers now require the exact
two distinct names in either order. Selected scope alone retains its existing
Soak-only form. No owner CLI or SDK setup was invoked.

New regressions call the actual guest-preflight function through a generated
native-command boundary and both actual plan-derivation functions. They preserve
the original frozen bytes and existing hash/environment/configuration checks.
An audit found no remaining ordered full-suite reader comparisons. Independent
review is clear. Combined tooling check `8165` passed all 31 cases, including the
separate simulator-signing work; real app-hosted execution remains pending.
