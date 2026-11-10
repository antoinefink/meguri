# Schema 1: optional macOS D4 native-web observation

`HierarchyExport` retains schema 1 and its existing fields. When a live trusted
replay fixture binding is available, `root` contains the actual public AX
subtree in place of the exact owned XCTest web view. `xcui_root` retains the
entire original XCTest snapshot, including every identifier verbatim. This is
not a mapping between XCTest leaves and AX leaves.

`native_web_rule` is exactly `render/display-transform#D4`. `native_web` and
`native_web_binding_at_end` are independently observed matching host bindings:
`page`, `document`, `card`, `generation`, `side`, `root`, `generated_ids`.
Generated IDs come from the current trusted payload, never a DOM query.

Actual AX nodes carry `native_ax: true`, `ax_identifier` and
`ax_dom_identifier` (string or null), plus separate `ax_identifier_error` and
`ax_dom_identifier_error` numeric AXError values and separate
`ax_identifier_wrong_type` and `ax_dom_identifier_wrong_type` booleans. Missing,
unsupported and wrong-type DOM identities never locate a generated replay.
`identifier` preserves the existing string field convention; the raw optional
AX fields retain missing-value/error distinctions. No field is filled from
another identity field. Labels, frames and hit points retain their ordinary
meaning and cannot identify native replay controls.

Only exact unique trusted generated link identities use this rule. Raw
AXIdentifier checks, other controls and geometry checks continue. Mobile
exports retain their existing requirements. Synthetic fixtures in
`NativeReplayIdentity` test the rule; actual VM exports remain required proof.

When `xcui_root` is present, checker findings name their observation in `source`:
`root` for ordinary checks on the export containing the actual AX replacement,
`native_ax` for the D4 binding/identity check, and `xcui_root` for the independent
original XCTest diagnostics. Raw format, duplicate-ID and geometry checks run
on both trees without combining their identifier namespaces or matching leaves.
Original XCTest links inside the exact replaced page are comparison evidence
for raw identifiers and geometry; their empty raw identifier cannot assert a
missing AXDOMIdentifier. Only actual AX links can establish D4 identity, and
every other actual AX control retains ordinary missing-ID checks. Original
XCTest controls of other types and links outside that page also retain
missing-ID checks. Without validated native replay identities, XCTest receives
its full ordinary checks. No raw identifier is populated or aliased.

Replay waits retain one live binding and one time limit for traversal and AX
messages. Publication can occur within that limit. Invalid link metadata,
wrong roles, duplicates and stale bindings fail at once. A gone result needs
verified native absence. A wait performs no press or action retry. It selects
no leaf through its name or frame.
