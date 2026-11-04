# U113 adoption failure stage

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: d8c27d149d6ad582c330a1984bf12d00c049713e.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Physical run88849 and inspections90670,96705,1829,29764 pass their own checks.
The app's download does not complete: its error value is connection.tracking.
That class also includes invalid data; it does not prove a database failure.
The last entered stage was lost when the Debug probe recorded the failure.

Export40359 preserves that stage for the same attempt and adds it to the
existing Debug error accessibility value. It names an entered boundary, not
the exact failure cause. New attempts reset it and old attempts cannot replace
it. Error classification, transport behavior and Release output are unchanged.

Independent review is clear. Core44522: 16 pass. Kit46302: 16 pass.
Private45686 and similarity45934 pass. A new signed phone run must capture the
failure stage before any repair is selected. The full batch gate remains open.

Signed phone run57377 uses source17998998. Its first seven steps pass, but the
300-second wait for download completion fails. The XCTest harness exits
successfully; the drive receipt correctly records the failed step. This is
not a successful import.

The retained hierarchy ends at adoptionPageReply. Inspection65612 later sees
adoptionPageResult without restarting the app. This proves a page reply reached
Core; it does not prove collection replacement or media convergence. The app
remains running for the import to finish. No unchanged import was restarted.

A source review finds that adoption pages request at most two records to keep
retained assets within the staging budget. The request includes every field.
Raising that limit alone would break the worst-case asset bound. Neither this
limit nor the SDK's discretionary default proves the cause of the long wait.

## User-requested download priority

Component start: 594a6797. Filtered export71955 adds one native configuration
assignment and one assertion to an existing test. Adoption pages now request
user-initiated priority. Other operations retain their existing priority.
Page sizes, asset budgets, timeouts and cancellation remain unchanged.

The installed SDK defines the default as discretionary. The user explicitly
requests this download and cannot review the collection until it finishes.
This justifies the priority choice; it does not establish the delay's cause.
Reference: https://developer.apple.com/documentation/cloudkit/ckoperation/configuration-swift.class/qualityofservice

Test-only Swift73381 fails at the new priority assertion in both cancellation
variants. After the production assignment, Swift74744 passes all 22 native
transport tests, including both variants and the original asset-retention
checks. Private76004 and similarity76251 pass. Independent review reports no
P0–P2 findings. A signed device run and the full batch gate remain outstanding.

## Retained phone outcome

Run79217 installs the priority change; inspection81037 shows no collection.
The new import84366 again exceeds its 300-second observation window.
Inspection90968 still shows adoptionPageReply. Observation93745 reaches its
600-second wait failure, then the test runner exits unexpectedly. Its selected
test and completion evidence are invalid; it proves neither an app crash nor
a completed import.

Inspection23627 observes the same installed source2a7044ae without reinstalling.
Only that source's app/core files were selected for this read-only check; master
was restored cleanly afterward. The app reports connection.tracking, with
last-entered stage adoptionPageFailureResult. Tracking includes validation and
storage failures; the exact cause remains unknown. No current-head or complete
sync acceptance is claimed from that inspection.

The next component starts at f30ada47. Native21297 records two semantic RED
tests for metadata-only pages and refusal of unexpected assets before copying.
The implementation will retain resource limits and complete-candidate validation.

## Metadata adoption review

Export22877 adds metadata pages, serial payload reads and a provisional cursor.
It is not accepted. Independent review found six issues: mixed-form record
order, untagged native test records, missing payload request priority, separate
staging budgets, cached initialization failure and a removed release assertion.
The next repair retains input order and all original resource and cleanup checks.

Core28501 fails compilation at a new test's AdoptStep path; no tests ran.
Core28181 could not reach Docker from the restricted process; it ran no tests.
Neither result is semantic RED. Repair29327 includes the compilation correction
and all six review findings. The full batch gate and signed import remain open.
