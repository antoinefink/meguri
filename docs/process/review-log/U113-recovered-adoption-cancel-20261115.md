# U113: cancel an adoption after recovery

Status: reviewed repair with focused checks passed. Native render and integration acceptance remain open. No Unit trailer or release acceptance.

Starting commit: `a1810015de9943c21d408af16ce3edf11851ece9`.
Pending batch gate base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: recovery V14.3, policy P17 and P12.1, collection replacement R6.6, status strings T9 and T13.5.

Cancel now qualifies the exact recovered operation. It preserves the previous isolated binding, conflict copies, collection, stage and R6 evidence. It removes candidate work atomically, then offers the V13 collection choice. Generation, file identity, slot and control-lane checks remain.

The policy distinguishes this recovery from another build scope, ordinary adoption completion and an accepted replacement publication. The qualified candidate takes precedence over the binding ID tie-break. Pause and account errors retain their specified order. FFI supplies the exact recovery class; Swift hides Resume for that class and uses the approved explanation and confirmation.

## Review and repair

| Review | Required result |
|---|---|
| `20261115T122709Z-65303` | Correct full file identity, scope and Sync Now precedence. |
| `20261115T125139Z-87067` | Preserve resolution after a later successful publication. |
| `20261115T131010Z-95793` | Admit the actual publication engine at Zones before entity upload. |
| `20261115T132711Z-5552` | P0/P1 clear. Correct the missing recovery explanation; the catalog and resolved-text test now cover it. |

## Retained verification

- `core-test/20261115T130736Z-94710`: corrected created-time regression fails on the original pair-only observation. The earlier provider-only fixture stopped before recovery and supplies no proof.
- `core-test/20261115T130655Z-94351`: both publication sequences fail; the previous binding wins the ID tie-break.
- `core-test/20261115T132424Z-3969`: the actual policy refuses engine admission at Zones before the repair.
- `core-test/20261115T132519Z-4644`: all 137 selected tests pass. The filter excludes 327 other cases. Coverage includes preserved conflicts, concurrent and stale Cancel, initial absence, native identity, publication admission, cancellation and cold reopen.
- `swift-test/20261115T132732Z-6442`: the selected CoreBridge test passes with regenerated bindings.
- `similarity/20261115T132807Z-7661`: 20 files, zero findings; only the filtered implementer view was read.
- `swift-test/20261115T133559Z-17431`: the native presentation test fails on the raw catalog key before the copy fix. `swift-test/20261115T133822Z-20969`: the same test passes with the approved text.
- `core-lint/20261115T133426Z-16040`: correctness and layering checks pass.
- `private/20261115T134021Z-24171`: all 21 final source files pass. Earlier comment and test-name matches were reworded; the guard was unchanged.
- Final similarity scan: 21 files, zero findings; the filtered implementer view was read.
- `core-test/20261115T134021Z-24084`: the renamed created-time regression passes. The wording pass changes only comments, test names and a fixture directory label; other reviewed source bytes match.

The signed Mac build `build-app/20261115T134338Z-29073` passes from clean `add0078656130a0ef024c5f61e76c4dffa6745be`, with zero errors and warnings. Signature and provisioning checks pass. The older preview quit normally; the exact Personal Preview product opened, with one new process and a matching executable hash. It remains running after startup.

Native computer use failed to start its connection, so no visual inspection is claimed. Recovered-state render and the full batch gate remain pending. The selected Swift tests prove status mapping, copy and cancellation behavior; they do not prove rendered controls.

The physical-phone drive `20261115T124340Z-82037` stops before its script: XCTest cannot enable automation. It proves no interface behavior. No recovered-error snapshot fixture was found. Native render and full batch gate evidence remain required. Global attention completeness, generation integration and signed acceptance remain open.
