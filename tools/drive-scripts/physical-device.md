# Physical-device drive transport

Authority: U8; R1961, R1962 and R1965; explicit user authorization to test on the connected phone.
Status: Approved 2026-11-03 after independent review.

## D1. Destination and build

`tools/mg drive` accepts an explicit `--physical-device` identifier for an available, paired iPhone or iPad running iOS 27 or later. Use the existing physical-device inventory and require the selected platform to match the device. Accept only Debug development products. Never substitute a simulator, Mac or VM destination. Reject recording and resize requests before launch. This transport provides UI inspection, not CloudAgent delivery or Q7 signed-conformance evidence.

`tools/mg run --physical-device <id> --platform <iphone|ipad> --config Debug` prepares and launches the selected development app through the existing device and development-signing guards. Normal authorized build, installation and restart are allowed here; no new human handoff is required. This path changes only that development app. Never uninstall, reset, supply fixtures, routes, raw arguments or launch-environment overrides, or substitute a destination.

Reuse the existing MG product, signing and source evidence. Collect targeted public installation metadata for the selected device's development app and the exact MG-owned UI-test runner and test bundle. Collect process metadata only for the selected development app; never collect a broad other-app inventory. Retain this metadata privately in the run's build/results artifacts. This preparation establishes the known running invocation; it does not claim survival of a later inspection. Existing authorized physical tests may prepare the matching UI-test runner before this final app launch. No daemon or session framework is added.

An explicit drive flag `--keep-app-running` opts into preservation only for that physical-device Debug development destination. Reject the flag for every other destination or product before build, installation, test or app work. Default behavior remains unchanged.

Preserve-mode drive requires matching trusted MG preparation, the matching installed development app and running invocation, and the matching installed UI-test runner and test bundle. Refuse missing, unknown or mismatching evidence before app work. Never silently rebuild, install, replace or upgrade artifacts in preserve mode. The ordinary authorized preparation path can prepare a fresh invocation; this is separate from preservation.

Run the prepared `.xctestrun` through public `test-without-building` with `UseDestinationArtifacts=true`. Omit `TestBundlePath`, `TestHostPath` and `UITargetAppPath`; supply `TestHostBundleIdentifier`, `TestBundleDestinationRelativePath` and `UITargetAppBundleIdentifier` for the matching installed artifacts. Retain required test-host metadata. This mode builds and installs nothing; no target-app launch overrides are supplied. Preservation evidence must bracket the entire MG invocation, including runner startup and completion, rather than activation alone.

## D2. Script delivery

Use the existing `DriveScript.swift` JSON grammar documented in `AGENTS.md`. Add no actions or command language. The runner accepts either its existing script path or an inline base64 payload through `MEGURI_DRIVE_SCRIPT_BASE64`, never both. For a physical run, the host reads the selected script and supplies the inline payload; the phone must not read a build-Mac path.

The decoded JSON may contain at most 16,384 bytes. Enforce the bound before parsing or launching, on the host and runner. Reject malformed base64, oversized content, invalid JSON, unknown fields and invalid steps. An explicitly requested drive with missing input fails; it cannot pass through the test's ordinary no-script skip.

Treat the payload as private. Never print its JSON, encoded content or raw runner environment. Keep scripts, hierarchies, screenshots and exported attachments only under the run's `build/` directory. Examples contain no credentials or account settings.

## D3. Phone state and allowed actions

The first step remains `launch`, and every launch object must be empty. Refuse all launch overrides, including fixtures, imports, resets, routes and raw arguments. Refuse `resize` and motion-transition requests. Retain the existing `tap`, `type`, `swipe`, `scroll`, `wait` and `snapshot` forms and identifier checks.

With `--keep-app-running`, every `launch:{}` calls public XCTest `activate()`. This includes an absent or unknown app state observed by the runner. Never choose a destructive `launch()` fallback from a state check. Supply no launch arguments, environment or other overrides. Keep the script grammar, input bounds and empty launch requirement unchanged.

For the opted-in session, do not terminate the normal app at successful or failed test completion, including failed-step and failed-capture cleanup. Keep ordinary runner cleanup. This is harness lifecycle only; it changes no runtime retry, timeout, sync, capability or ownership behavior. It grants no background execution and adds no time-limit increase, private API, runtime hook, RPC, pool, queue or timer.

The harness never uninstalls the app, clears its container, stages a collection or changes saved settings. Normal authorized UI actions act on the phone's actual app state. Begin with this inspection script, then inspect its captures before any later cloud-replacement action:

```json
{"name":"physical-inspect","steps":[{"launch":{}},{"snapshot":"initial"}]}
```

## D4. Evidence and outcome

Keep the existing capture rules: hierarchy and screenshot before and after an input action, after launch, and once for a named snapshot. Use unique accessibility identifiers and hierarchy hit points. Stop at the first failed step and retain its log and captures. Export the existing drive manifest and run hierarchy checks.

Record fixed boolean fields `kept_running` and `activated_existing` in the existing evidence. `kept_running` records requested no-termination intent. Set `activated_existing` true only when public evidence proves activation of the existing invocation. Neither flag proves survival. Record verified survival only when actual physical process evidence identifies the same invocation before all MG work and after completion. Missing or ambiguous evidence is unknown or failed, never a passing survival check; preserve-mode drive cannot pass without verified continuity. Retain selected-device app identifiers and paths only in private build/results evidence. Human-facing logs and reports contain no actual device, installation or process identifiers or paths, no other-app inventory, no script content and no raw runner environment. Normal repository artifact paths and static development bundle IDs remain allowed.

Focused physical checks must prove ordinary preparation establishes the matching signed development app and running invocation; default drive termination after success and failure; opted-in continuity after success and failure; and a subsequent inspection of the same invocation across the entire MG invocation. Check later empty launch steps and absence of preserve-mode build or installation too. Refusal checks cover unsupported destinations or products, missing or mismatching preparation or installed runner, unproved app identity or invocation, nonempty launch options, malformed or oversized input, recording and resize. Failed steps, missing artifacts, failed captures, hierarchy failures and skipped tests retain their existing outcomes. All payloads and artifacts stay private. Source review or a build alone does not establish feasibility or survival. This adds no runtime hook, RPC, daemon, session framework or time budget.

A requested physical drive passes only when its selected test runs, every step succeeds and all required artifacts and checks are present. Report unavailable device or signing prerequisites as queued or failed, never passed. A skipped test, missing result, failed step, capture failure or failed hierarchy check cannot produce a passing drive receipt. Do not log private payloads to explain a failure.
