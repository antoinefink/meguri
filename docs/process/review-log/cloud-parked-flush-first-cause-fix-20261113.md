# Native flush cancellation repair

Base: `a447a4d659359404701e02b3438fe8f3f6ea716e`. Role: fixer, implementer view.
Authority: `sync/policy` P4, P6, P11, P12, P15; `engine/jobs` J2.1.1.

Each command keeps its own cancellation evidence. The original operation must
acknowledge removal before native cancellation or lifecycle changes. A later
refresh error replaces earlier qualification. Request metadata grants no
cancellation authority. The SDK receipt checks the actual execution and instance.

Review record:

| Round | Cleanroom run | Result |
| --- | --- | --- |
| 1 | `20261113T212106Z-98817` | Required SDK receipt and independent dependent completion; repaired |
| 2 | `20261113T213916Z-6103` | Required removal guard and refresh error handling; repaired |
| 3 | `20261113T221214Z-12082` | Required guard before held configuration; repaired |
| 4 | `20261113T222255Z-17162` | No P0/P1 |
| 5 | `20261113T222858Z-21598` | Test dispatcher delta: no P0/P1 |
| 6 | `20261113T223932Z-26818` | Complete test helper and typed failure: no P0/P1 |

Focused native evidence:

| Swift-test run | Coverage | Result |
| --- | --- | --- |
| `20261113T221233Z-12598` | Original exchange six cases; absent/replaced native instance | Those cases passed; new refresh setup failed |
| `20261113T222317Z-17642` | Flush nine cases, including held configuration after failed removal | All nine passed; new refresh setup failed |
| `20261113T224019Z-27609` | Required refresh errors: direct two cases and deferred one | All three passed |
| `20261113T221341Z-14708` | App cancellation, earlier errors, shutdown and media queue | Twelve cases passed |
| `20261113T222515Z-19438` | Original app expiry and deadline after the lifecycle fix | Both passed |

The new refresh fixture executes actual Epoch/root preparation and SDK
configuration. Cleanup and zone deletion are outside this component test;
unexpected commands still fail. Original limits and assertions remain.

P2 follow-up: AppModel omits `priorError` from app diagnostics. Additional
NoOperation, repeated SDK identity and overlapping stop coverage remains open.
The separate quiet-point failure persists on clean base (`20261113T212844Z-3575`).
It remains enabled and unsuppressed.

The final helper passes all three cases. Production matches review 4.
Similarity `20261113T224041Z-29004` passes eight files with zero exceptions.
Batch base `1416a520542a8c5daa38a23bfe398da704c3d6c9` remains pending.
The complete app objective, U114 history/caps, integration checks, milestone gates
and final device evidence remain due. This repair adds no Unit trailer and makes
no full-gate or release acceptance claim.
