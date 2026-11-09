# Generated replay controls

Status: WIP; native proof and batch gate remain open.
Start: `f2203b79`. Fix round starts at `2881beec`.
Session: `s-20261103T133750Z-f276f58e`.
Spec: render/display-transform D4 and D12.

Export `41241` adds stable q/a IDs and names to generated replay controls.
Explicit ID lists pass through render output, FFI and CardPayload. The native
page uses the existing catalog, escaped configuration and attribute setter
before scripts and readiness. Authored controls keep their own labels.
Commands, AV lists, TTS, hidden controls and card styling stay intact.

Independent Rust and Swift review reports two P2 findings. Export `49089`
adds the missing FFI fixture fields and checks both mapping results. The
other finding requires six display snapshots to change. No further P0–P2
issue is reported. No check or assertion is removed.

Core `51287` passes five transform tests and fails its one snapshot test.
It produces six new snapshots. After removing the new generated ID and
accessibility attributes, each body matches its prior file byte for byte.
Only the assertion-line header also changes, to the current line 80. Those
six produced files are integrated. Hidden and error files do not change.
The focused snapshot check still needs to run again.

Privacy `51586` flags this new log's prose only. The log is rewritten; the
check must run again. The privacy guard is unchanged.

Generated bindings and native tests are pending. The new WebKit tests
require actual accessibility IDs, names and native activation. DOM IDs
alone do not count as proof. No hierarchy, ruby or geometry rule is waived.

## Focused results

Core `52527` passes all six transform/snapshot tests. Privacy `52853`,
similarity `54956` and core lint `54650` pass. Native core `53503` rebuilds
Debug bindings successfully. Swift `55906` passes all nine selected tests,
including both replay-ID mapping assertions. No native WebKit AX result
is claimed yet; that check and the batch gate remain pending.
