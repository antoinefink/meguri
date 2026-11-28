# Image occlusion deselection

The task branch started at `9bbddc089e` and integrated `b6d68acb6` (PR #257).
See `editor/mask-editor` M3.1, M3.6 and M9.

The Mac journey also exercises selecting the first mask and dragging the
second while Rectangle is active.

## Clean-room review and privacy

- The implementation came from `cleanroom/20261128T064910Z-47560`.
- Independent reviewer `s-20261128T065108Z-ac9723ce` found no material issue in
  `cleanroom/20261128T065246Z-51458`. The reviewer read the complete files below
  and classified them as public Meguri source or operational metadata.

| Path | Bytes | SHA-256 |
|---|---:|---|
| `app/MeguriUITests/IOAddEditUITests.swift` | 45,217 | `d9dfc919b948ba01a476c03b2774d185656d8aba5fcc4a9413cc055b616ef532` |
| `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/MaskCanvasInteraction.swift` | 12,144 | `8e19a1ae38b62cbfe56e8a3f6fdeb10a63cd83b628a0fb9d2f6eae1825330d7a` |
| `app/Packages/MeguriKit/Tests/ImageOcclusionTests/MaskCanvasInteractionTests.swift` | 27,144 | `5f9e4630d9cd23704a74ab16c77bda6d49fb00dac5616d28f8528d26cb1e47ae` |
| `docs/process/provenance/sessions/s-20261128T064644Z-f4fb2d99.json` | 388 | `0cb787a84d5522ca61bf3e901e0274ff5fe918f5b7b782284b41dd776b3d229c` |
| `docs/process/provenance/sessions/s-20261128T065108Z-ac9723ce.json` | 390 | `d5698b607f9d0ba8c9834a54b509f422c4e0208e4ff3bbe8043fe609e14dc766` |

Hosted Linux similarity is the gate for the submitted revision.
