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
