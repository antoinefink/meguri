# Native replay identifier observations — 2026-11-14

Normal phone drives reported Play audio links with empty XCTest identifiers.
The renderer already emits deterministic HTML IDs. That source fact alone
does not establish native iOS identifier publication or XCTest export.

Read-only diagnosis 20261114T220816Z-78588 identified an existing iOS test.
Unit-sim 20261114T221358Z-81506 executed that exact method and failed: its
in-process traversal saw zero links where three were expected. It therefore
cannot establish whether an observed native link has an identifier. The
failure remains enabled and retained.

The existing synthetic identifier probe wrote a value before checking its
readback. Filtered change 20261114T221901Z-86181 adds three booleans recording
the original optional value before that setter. Independent review
20261114T222102Z-86678 found no P0, P1 or P2 issue. Existing setter behavior
is unchanged. No raw identifier or card data is exported. Geometry remains
diagnostic only and supplies no control identity or action mapping.

VM drive 20261114T222215Z-87246 compiled the candidate and executed all three
steps. Its final report had leafFound=false and originalReadObserved=false.
No original value was observed; those false values do not establish absence.
The whole job failed with four missing-identifier findings on Play audio.
No checker exemption or passing accessibility result was introduced.

Privacy 20261114T222221Z-88304 and similarity 20261114T222225Z-88647 passed.
The change remains a diagnostic observation, not a rendering repair, native
iOS D4 mapping, audio-output proof, fresh media-transfer proof or acceptance.
The unresolved observation boundary stays open. Continue useful app work
rather than repeating the same unavailable observation unchanged.
