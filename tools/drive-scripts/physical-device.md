# Physical-device drive transport

Authority: U8; R1961, R1962 and R1965; explicit user authorization to test on the connected phone.
Status: Approved 2026-11-03 after independent review.

## D1. Destination and build

`tools/mg drive` accepts an explicit `--physical-device` identifier for an available, paired iPhone or iPad running iOS 27 or later. Use the existing physical-device inventory and require the selected platform to match the device. Accept only Debug development products. Never substitute a simulator, Mac or VM destination. Reject recording and resize requests before launch. This transport provides UI inspection, not CloudAgent delivery or Q7 signed-conformance evidence.

## D2. Script delivery

Use the existing `DriveScript.swift` JSON grammar documented in `AGENTS.md`. Add no actions or command language. The runner accepts either its existing script path or an inline base64 payload through `MEGURI_DRIVE_SCRIPT_BASE64`, never both. For a physical run, the host reads the selected script and supplies the inline payload; the phone must not read a build-Mac path.

The decoded JSON may contain at most 16,384 bytes. Enforce the bound before parsing or launching, on the host and runner. Reject malformed base64, oversized content, invalid JSON, unknown fields and invalid steps. An explicitly requested drive with missing input fails; it cannot pass through the test's ordinary no-script skip.

Treat the payload as private. Never print its JSON, encoded content or raw runner environment. Keep scripts, hierarchies, screenshots and exported attachments only under the run's `build/` directory. Examples contain no credentials or account settings.

## D3. Phone state and allowed actions

The first step remains `launch`, and every launch object must be empty. Refuse all launch overrides, including fixtures, imports, resets, routes and raw arguments. Refuse `resize` and motion-transition requests. Retain the existing `tap`, `type`, `swipe`, `scroll`, `wait` and `snapshot` forms and identifier checks.

The harness never uninstalls the app, clears its container, stages a collection or changes saved settings. Normal authorized UI actions act on the phone's actual app state. Begin with this inspection script, then inspect its captures before any later cloud-replacement action:

```json
{"name":"physical-inspect","steps":[{"launch":{}},{"snapshot":"initial"}]}
```

## D4. Evidence and outcome

Keep the existing capture rules: hierarchy and screenshot before and after an input action, after launch, and once for a named snapshot. Use unique accessibility identifiers and hierarchy hit points. Stop at the first failed step and retain its log and captures. Export the existing drive manifest and run hierarchy checks.

A requested physical drive passes only when its selected test runs, every step succeeds and all required artifacts and checks are present. Report unavailable device or signing prerequisites as queued or failed, never passed. A skipped test, missing result, failed step, capture failure or failed hierarchy check cannot produce a passing drive receipt. Do not log private payloads to explain a failure.
