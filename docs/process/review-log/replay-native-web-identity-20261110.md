# Replay native web identity — implementation handoff (2026-11-10)

Role: recorded implementer. Clean start: `5149ad9e` (reviewed spec-writer
contract already committed). Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `render/display-transform#D4`, `render/display-transform#D12`,
`render/card-behaviors#B1`, `render/card-behaviors#B3`.

Review/fix log: replaced the unsupported AXIdentifier propagation assumption
with an exact public AXDOMIdentifier read on actual owned native AX links.
The host exposes live page/document/card/generation/side and trusted payload IDs
only while the bound root is committed, presented, settled and current. Native
AX traversal is confined to the launched app's windows and its exact page root;
the package test confines reads to its own process and exact fixture window.
The selected original AXUIElement is retained, revalidated against a fresh
native observation and live host binding, and pressed once with AXPress.
Existing native frame/origin/message-bridge policy is unchanged.

The export retains the complete raw XCTest tree as `xcui_root` and uses the
actual AX subtree for the owned web page in `root`. Separate raw AXIdentifier
and AXDOMIdentifier values, read status and wrong-type flags are exported.
No leaf is associated by label, position, ordinal or JavaScript. The structural
driver consumes the same trusted native-web rule for replay taps and refuses
coordinate activation of those controls. The checker accepts only exact unique
trusted native web IDs under the named D4 rule; other controls, mobile rules
and geometry findings remain active. Synthetic checker cases cover missing,
unsupported, wrong-type, duplicate, stale, untrusted, alias and mobile failures.

The native UI case retains twenty exact replay activations across all four
stages, repeated commands/leading zeroes, authored q0 collision/name,
localization and native geometry/hit checks. It adds actual hierarchy and
screenshot attachments before and after all 43 actions (20 presses, 20
checkpoints, three side/card changes), alongside four stage captures. During
a side/checkpoint transition without a ready host binding, the immediate
capture retains the actual XCTest hierarchy; it grants no native-web identity.
The MG exporter requires all 90 hierarchy/screenshot pairs and retains all
checker findings, with native-web identity and other findings counted separately.

One source consistency inspection completed; corrected actor annotations and
explicit ID coding keys from that inspection. No git, network,
build, test or UI execution is authorized here. Root owns compilation, VM
execution, independent review and provenance/privacy/similarity checks. No Unit,
visual agreement, runtime feasibility or acceptance is claimed. Failures 7686
and 93192 remain failures; no frozen image baseline is edited.

Root verification commands (not run here):

- `tools/mg mac-ui --suite replay-native`
  (`MacReplayAccessibilityTests/testGeneratedReplaysKeepNativeIdentityLocalizationAndExactCommands`).
- `tools/mg swift-test --package MeguriKit -- --filter 'ReplayAccessibilityTests/native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy'`
  (exact method selector; root checks that one case actually executes).
- `tools/mg swift-test --package MeguriKit -- --filter 'ReplayAccessibilityTests/native_generated_replay_controls_use_catalog_localization'`.
- `tools/mg image-tools-selftest -k NativeReplayIdentity` for the new checker
  cases, followed by `tools/mg image-tools-selftest` for existing checker rules.
- `tools/mg selftest --filter replay-native` for the focused MG export fixtures.

Runtime evidence remains pending: actual VM AXDOMIdentifier values, original
link presses, compilation and review. An unavailable window is a failed proof.
Only the synthetic replay route exports the host metadata. Mobile, keyboard,
ruby, contrast, hit targets, gates and final evidence remain required.

## R1 repair handoff (source only)

The supplied seed is retained in full. Original clean start remains
`5149ad9e`; this runner baseline may additionally contain independent native
Saved-removal work. This repair edits no CloudKit, Core, policy or ServiceHarness
source and uses no Git/history, private/reference data, raw logs, network,
build, test or UI execution.

P1 repair: both `root` and original `xcui_root` receive ordinary geometry and
raw ID checks. Results retain their source. The D4 identity check uses native
links only. No labels, frames or ordinals map leaves across trees. Other
controls retain missing-ID checks. Invalid D4 proof restores all raw XCTest
checks. Fixtures cover original-only overlap, bad IDs, duplicate IDs and
simultaneous findings from both trees.

P2 repair: waits observe fresh native state within one time limit. Binding
checks bracket traversal. Invalid metadata, wrong roles, duplicates and stale
bindings fail at once. Waiting performs no activation or action retry and uses
no fallback. Ordinary XCTest waits retain their prior behavior. R2 review
found that unavailable ownership could still appear as absence; repair remains
required before acceptance.

Added `NativeReplayWaitTests` use a controlled observation clock and the actual
candidate classifier to cover a present-to-absent sequence, absent-to-published
sequence inside the requested timeout, both deadline failures, invalid,
unsupported, wrong-type, wrong-role and duplicate candidates, and immediate
stale-observation propagation. Added
`MacReplayAccessibilityTests/testNativeReplayWaitObservesDisappearanceAndPublicationWithinTimeout`
uses the existing synthetic route: one fixture action schedules hide/restore
before either wait, then the production wait records actual native AX
present/absent/present observations under an unchanged binding and requires zero
received commands/actions. Its isolated-world fixture script only toggles
visibility; it performs no activation and supplies no native identity proof.
The original proof still has twenty exact activations, all four stages and all
90 hierarchy/screenshot pairs. Keyboard/mobile/geometry/ruby/hit/contrast
coverage and the original contract constraints remain required.

Root should run the original commands above, plus:

- `tools/mg ui-test --platform mac --suite NativeReplayWaitTests` for controlled
  polling/classification coverage (confirm every new test executes).
- `tools/mg ui-test --platform mac --suite MacReplayAccessibilityTests` for the
  real disappearance/publication case as well as the existing activation case.
  The dedicated `mac-ui --suite replay-native` command above still selects only
  the original activation proof and its one-case attachment exporter.
- `tools/mg ui-test --platform mac --suite DriveScriptTests` for parser coverage
  and the existing agent-driver suite for ordinary wait/driver behavior.
- `tools/mg image-tools-selftest -k NativeReplayIdentity`, then the full checker
  selftest and `tools/mg selftest --filter replay-native`, retaining every raw
  finding and its source in the native proof report.

These are requested root checks, not execution or acceptance claims. Native
compiler/API availability, actual timed publication/disappearance, the complete
package proof and independent review still need root verification. Export must
include the original seed changes together with these repairs.

R1 consistency check: one source inspection completed, including Python AST
parsing of the checker and its fixture source, Swift call/signature and binding
type inspection, and confirmation of MG's class/test selector syntax. No test,
build, native execution or acceptance result is claimed.

## Root evidence

R2 review confirms both R1 repairs. One P2 remains: an unavailable native
ownership read can satisfy a gone wait. This still needs repair.
`image-tools-selftest/20261110T111018Z-44488` passed 189 checks.
`selftest/20261110T111018Z-45067` passed all five wrapper cases.
Similarity `20261110T111035Z-47060` passed.

Package run `swift-test/20261110T111018Z-44403` failed to compile the Mac
identifier getter. Its return is a String, so integration removed the optional
binding on macOS. The next build `swift-test/20261110T111214Z-53351`
reached a missing `kAXLinkRole` import in the test. No test case ran.
Native presses, wait behavior and UI evidence remain unverified.

## R3 bounded repair (source only)

Runner baseline: `01bedc87`; original batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Replace the unavailable Swift web-role macro with the exact public `AXLink`
literal and use the public `AXDOMIdentifier` literal in both native readers,
with short AXWebConstants.h provenance. The baseline's nonoptional macOS
identifier getter and optional iOS getter are preserved; no library or control
semantics change.

Ownership traversal now requires successful, correctly typed windows,
advertised child arrays and advertised ownership identifiers. Successful public
attribute discovery can establish a leaf with no AXChildren attribute; no role
map is invented. An unavailable mandatory read fails rather than producing an
empty subtree. A successfully traversed tree without the exact owned page is
pending publication, represented separately from verified link absence. Both
live binding checks still bracket observations. Pending ownership cannot pass a
gone wait; appearance may await publication within the same requested deadline.
Zero links can prove absence only after a successful uniquely owned page capture.

New tests cover unavailable AX reads, wrong types, pending publication and
verified absence through the production helpers. Original coverage remains.

One source consistency inspection completed. No git, network, build, test or UI
execution occurred in this runner. Root owns compilation/runtime, independent
review and provenance/privacy/similarity checks. No native value, press, signed
build, Unit or gate pass is claimed.

Exact next root checks, in order (not run here):

- `tools/mg swift-test --package MeguriKit -- --filter 'ReplayAccessibilityTests/native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy'`
  to compile and execute the package proof; confirm the case actually runs.
- `tools/mg swift-test --package MeguriKit -- --filter 'ReplayAccessibilityTests/native_generated_replay_controls_use_catalog_localization'`.
- `tools/mg ui-test --platform mac --suite NativeReplayWaitTests` for the
  injected ownership-error, pending-publication and retained candidate cases.
- `tools/mg ui-test --platform mac --suite MacReplayAccessibilityTests` for
  actual hide/restore and the original twenty-command/four-stage native proof.
- `tools/mg mac-ui --suite replay-native` for the original one-case exporter,
  all 90 hierarchy/screenshot pairs and source-tagged ordinary findings.
- `tools/mg ui-test --platform mac --suite DriveScriptTests`, plus the existing
  agent-driver checks identified above. The prior 189 image-tool and five MG
  wrapper passes remain root evidence; those sources are unchanged here.

Root must retain failures and resolve compiler/native proof issues before
acceptance. All remaining original gates and final evidence remain required.

Root R3 review is clear. `swift-test/20261110T112045Z-67751` compiled.
The replay case failed at its own-window AX child read. No native press was
proved. All three Service SDK functions passed, including five cases for
real removal, newer revisions, cancellation and release. The two selected
sync variants still failed: peer-baseline observer provenance, and exchange
capture tracking error. Their evidence is retained separately. VM execution
remains required. R3 privacy and similarity passed after the note rewrite.

The first Mac build `mac-ui/20261110T112617Z-77944` rejected the generic AX
geometry pointer. Its type now requires `BitwiseCopyable`; both callers are
CGPoint/CGSize, with the same AX type checks. Independent review confirmed
this compiler correction. `mac-ui/20261110T112758Z-80949` then built and ran
one VM test, which failed at `Native replay host binding unavailable`.
It reached no native presses or hierarchy attachments. The export correctly
failed for absent evidence. Readiness publication remains under diagnosis.
