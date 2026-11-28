# Image occlusion pointer feedback

The task branch started at `9bbddc089e5da30dd5b2415d4fdb97536ad5738f`.
The editor selected through a SwiftUI drag gesture that shared the canvas
with a double-click recognizer. Box previews carried ordinal 0 until the
writer assigned the real ordinal on release.

The Mac canvas now selects on mouse-down. A selection click leaves the document intact.
Rectangle and ellipse previews use the next available ordinal. Selection
uses a direct group lookup and retains the topmost-mask hit test. The
screen retains its existing arrangement and system pointer styles.

## Clean-room and review

- `cleanroom/20261127T192216Z-78424` exported the initial patch.
- Review and focused fixes covered pointer ownership after a lost mouse-up,
  invisible windows, label double-click versus drag, simultaneous touch
  navigation, polygon guidance, and release after an image, tool or field change.
  The verified source came from `cleanroom/20261127T193907Z-95591`.
- Independent filtered source review `cleanroom/20261127T194112Z-96193`
  found no remaining material issue. Hosted Linux similarity is the gate
  for the submitted revision.

## Routine privacy admissions

The independent filtered reviewer read both complete session records and
accepted them as public operational metadata. The orchestrator verified
these digests against the record commits.

| Session | SHA-256 |
|---|---|
| s-20261127T192047Z-ba9fb5bd | `4119616bfe3764c31d9a9d4cb36fe89534e986c9ff28d8d9a94bfd3532d26400` |
| s-20261127T193118Z-38ee5757 | `3ffc506cbe0695921f89ff1537d4ddf0fc89eead5a24f2fbe523eb0d281915b7` |

The independent Swift reviewer (`review_swift`) read the complete staged
files, including comments, literals and paths, and classified them as
public Meguri code. The orchestrator confirmed their exact staged and
working bytes match the reviewed paths and hashes.

| Path | Bytes | SHA-256 |
|---|---:|---|
| `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/CanvasNavigationInput.swift` | 13,340 | `7aac7f43d09803cfadfbaae3cb2ef0188379db9f370d658c38a586fd69e06f71` |
| `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/MaskCanvas.swift` | 26,185 | `cbf1759e81c4c3ed80e3483342852c01c2707c2de2539a8a47f0a029de301387` |
| `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/MaskCanvasInteraction.swift` | 11,999 | `e1eb135b1cd43336c8c589151fa668ae34756529784a12ef9b4be01f3af78574` |
| `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Model/MaskEditorSession.swift` | 18,159 | `1fda641d4eca09839579a83a0504b8a92ca36b6ee43cc0cb4f6113c13ae441ca` |

## Focused verification

The baseline passed 43 existing package tests (`swift-test/20261127T192216Z-78681`).
On that code with the new draft test alone, all four rectangle and ellipse
cases exposed the incorrect preview (`swift-test/20261127T193349Z-88097`).
The final Swift source passed 79 tests (`swift-test/20261127T194200Z-97581`).
The receiver tests check selection before mouse-up, active move feedback,
hover and leave delivery, and cancellation. The model tests check preview
and saved ordinals, gaps, and legacy masks.

The Mac VM journey passed on the first native candidate
(`ui-test/20261127T193542Z-90307`). It drew two masks, moved one, saved,
and reopened both at their recorded positions. The rendered Add Note form
showed the expected two numbered masks. Final native and hosted receipts
are tracked on the pull request. The inherited full-gate failures remain
in [the verification register](../verification-status.md).
