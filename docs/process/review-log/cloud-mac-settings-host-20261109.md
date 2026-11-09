# Mac settings test host

Start: `da321315`; original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261103T133750Z-f276f58e`, filtered export `28003`.
Only the test fixture changes. Both mounts retain standard NSHostingControllers;
size, style, view/state and every native assertion remain unchanged. The geometry
requirement and diagnostic now use one immutable rectangle.

Team-signed run `17190` cannot launch and executes zero tests. Ad-hoc run `23284`
executes the selected case and fails its geometry requirement. This does not prove
a profile defect. Controller-owned run `20261109T192254Z-68432` passes first-mount
geometry and reaches the native button assertion, which fails with zero matches.
The second mount is not reached. No full selected-test pass is claimed.

Independent hosting review: `build/cloud-mac-settings-hosting-review-20261109.md`.
The native button measurement remains under diagnosis. No signature, consent,
product layout, assertion, limit or baseline was changed by this fixture repair.
No unit or integration acceptance is claimed.
