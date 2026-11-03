# U151 media cancellation evidence

Status: WIP; no unit acceptance.
Component start:d2c5af6f.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T133750Z-f276f58e.

Intentional media cancellation can reach generic failure presentation. Exact
run ownership and first-error ordering must distinguish it from other errors.
No runtime qualification or presentation change is implemented yet.

Export234 is partial. Export5186 adds the missing tests and dormant defaults.
Review finds two new-test P2 issues. Export6672 corrects the unsupported Pause
ordering and places queue arrival under the same watchdog. Existing assertions
and limits remain. Independent review clears the corrected test structure.

Native7393: two expected qualification failures; two controls pass. One new SDK
assertion also fails. Review confirms it exceeds the typed transport contract.
Export10877 removes that new assertion and corrects the Swift8942 compilation
error. Original assertions remain. Failed receipts stay failed.

Swift11504 stops at another test compilation error: a conditional NSError cast
of a nonoptional Error. No app-level test executes. A bridge syntax correction
is required; assertions and error identity remain required. Runtime producer
logic and app-facing RED evidence remain pending.

Export13867 corrects only the bridge syntax; identity assertion stays.
Native14358: SDK control passes; Pause and Stop fail only qualification.
The corrected AppModel run executes both selected tests: mapping and all
negative controls pass. Only qualified cancellation fails presentation and
revision assertions. Outputs, original ACKs and cleanup checks pass. This is
the app-facing RED witness. Producer and presentation repair remain pending.
