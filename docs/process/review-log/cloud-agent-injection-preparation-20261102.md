# CloudAgent host-local injection preparation

Status: WIP with focused checks; no signed runtime acceptance.
Component base / clean source: `d8c8be67cc20049791c5c1ffb85d530c218e4713`.
Implementer session: `s-20261101T235351Z-264d9cd0`; no seed.

Root supplied build86998: compilation succeeded, but the exact verifier refused
agent-environment because the required app-local injection library was absent.
Physical harness remains paused at empty88269.

Generated regressions were authored before production changes. They cover
exclusive bounded copies, nofollow source/destination binding, substitution,
ordinary/stale exclusion, platform/config opt-in, preparation/join ordering,
and final nested/outer signature-byte inventory. Command doubles and generated
signature markers supply no actual signature or native evidence.

The app uses a native Frameworks CopyFiles phase with CodeSignOnCopy and the
existing COPY_PHASE_STRIP=NO. Source exclusions select only explicit MG
CloudAgent Mac Debug builds. A cleanup-only native phase removes stale copies
before ordinary builds' final signing, with an exact conditional sandbox output
to avoid two producers for the enabled CopyFiles output. MG preparation removes
the enabled build's previous generated copy before starting native signing.

Selected source authority is factored from the existing vm-xcode-stage source
check without changing its signing requirement or InstalledSource policy.
The helper holds the selected directories, nofollow source and exclusive staged
file through the joined native build and rechecks their bindings and bytes.
The existing verifier and final freeze remain unchanged.

Independent authority supplied by root:
- https://developer.apple.com/library/archive/technotes/tn2206/_index.html
- https://developer.apple.com/documentation/xcode/customizing-the-build-phases-of-a-target
- https://developer.apple.com/documentation/xcode/build-settings-reference
- https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac

One source consistency pass corrected generated command isolation, preserved
the existing dispatcher tests with an explicit preparation double, and retained
normal writable copy modes for native CodeSignOnCopy. No execution performed.

Root must verify native CopyFiles source filtering, sandbox cleanup, and final
nested-then-outer signing on the selected Mac Xcode, including enabled to
ordinary incremental transitions. If native conditional copying cannot safely
meet this contract, stop; no custom signing or after-build repair is provided.
The smallest fallback prerequisite is a separately selected MG-generated
project copy with the native CopyFiles membership omitted when disabled,
retaining the same app target and native signing pipeline; it is not implemented.
Actual build/launch and frozen-host unchanged proof remain outstanding.

Independent review finds a P2 in the original wrapper: native child completion
was not owned through cancellation. Fixer4626 exports91772 with the original
Popen handle, deferred launch, bounded group termination and joining before
source/staging descriptors are released. Delayed-termination cases verify that
order and preserve unrelated process groups. Re-review requires no repair.

Shared selftest96270 passes21/21 wrappers, no skips. Filtered3533 rewrites only
authored test regions; independent comparison preserves assertions and behavior.
The integrator uses the standard positional subTest case label in two contexts;
review confirms that only failure-display metadata changes. Final focused5433
passes the runtime-preparation wrapper, and privacy passes for every changed file.
The final complete seed is build/cloud-agent-injection-reviewed-seed-20261102.patch.

During Antoine's direct Mac pass, Xcode MCP builds and launches the ordinary
Debug product with this preparation disabled. That result does not prove the
enabled copy, stale-copy removal, actual nested signature or frozen SDK launch.
Those explicit native checks remain pending. No unit or gate is accepted here.
