# Image occlusion authoring

U205 and U206 start from `d98894622`. The Add and Edit Note flows use the
existing Rust image occlusion format and collection operations. The native
canvas owns the image and mask interactions. The fields view owns the other
note fields and tags. Browse keeps the selected note inside its inspector.

## Session record privacy review

The independent privacy reviewer read each complete record and admitted the
following bytes as public operational metadata. The coordinator verified the
SHA-256 digests against the files in this checkout.

| Session | SHA-256 |
|---|---|
| s-20261126T090109Z-df59ef0b | c5009abeca25bfe68b68651d8656202a528d4c241f9d08f32af6eb3564b2c35b |
| s-20261126T090125Z-beeb9067 | 2711ca4c1c62ffe65ac08dcb3259e2f7ebab5429b742eda307d45497bb447eab |
| s-20261126T090405Z-f8c3bebf | c4c402eb6873e1d8eb4766f621efa0664290566749b08c310f783409a73f0e6c |
| s-20261126T090823Z-6f631e80 | 45f14c06f0cd07e4da6dd19dca416cfd23391a2380f71b5e8b7a78758f4c79b8 |
| s-20261126T090833Z-cd0beb38 | f24582de4fc675577fda8a32714183b55cc832f529c1cadeb179c4abf3660e77 |

The same reviewer admitted the entire 14,749-byte `app/MeguriUITests/IOAddEditUITests.swift`, including its synthetic geometry diagram, as public. Its reviewed SHA-256 is `963ca87f70935f16f0d6795230d020433cec1567fdb569536f1bfe8af55e223f`.

The independent privacy reviewer read and admitted all 6,510 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/CanvasNavigationInput.swift` as public. SHA-256 `5429e236529d0e8f8f87396c3d1c85b72da5dea8158046c78cd6b9921546c8ae`.

The independent privacy reviewer read and admitted all 38,089 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/BrowseInspectorTests.swift` as public. SHA-256 `8dbb714b027d1e138953bd32032092c2ae32f07449dcc4e752e4243c22d42fec`.

The independent privacy reviewer read and admitted all 6,560 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/CanvasNavigationInput.swift` as public. SHA-256 `7b61bc8c195060972701e5c3206d49a103d9b11e20ca7ff6ec4955dfd6f9e1c0`.

The independent privacy reviewer read and admitted all 1,794 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/AGENTS.md` as public. SHA-256 `5bfb128d099517da9cf40767e606dc6d3fad01313ec18d3b9340e1866d919f18`.

The independent privacy reviewer read and admitted all 8,395 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/MaskAppearance.swift` as public. SHA-256 `305387de7825b84b44734164254ff42482f184a2a2ba3b5bcc54b35ff6639988`.

The independent privacy reviewer read and admitted all 15,229 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Model/MaskEditorSession.swift` as public. SHA-256 `a0fd291247903570ddd0d0a134433f90ae9cc7fc59009c45ecb825cae6b58ec9`.

The independent privacy reviewer read and admitted all 3,629 bytes of `app/Packages/MeguriKit/Tests/ImageOcclusionTests/MaskEditorCommandTests.swift` as public. SHA-256 `cd2978f27435edc7c985f4ec8515b454a9c41a3783a439ce4e24971defec8b5e`.

The independent privacy reviewer read and admitted all 16,559 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Model/MaskEditorSession.swift` as public. SHA-256 `1eca80f995b8201795c6faa14dfb3597b200a8961ba716411af7f1ba4c63db96`.

The independent privacy reviewer read and admitted all 1,097 bytes of `app/Packages/MeguriCore/Tests/CoreBridgeTests/ImageSourceRequestTests.swift` as public. SHA-256 `96c21a3630fef1265be0330880d5f0bb7f4a9de93c4e77c3b7b330279ffff8da`.

The independent privacy reviewer read and admitted all 15,904 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `71557f0bc50dfc2d76c2940a5f3634b10fec564486c6f77304711c418436ad30`.

The independent privacy reviewer read and admitted all 7,118 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/CanvasNavigationInput.swift` as public. SHA-256 `c94356e618294a9266b4a71d4522ca8fe1228b065567d0f568c31c1901a122cc`.

The independent privacy reviewer read and admitted all 16,823 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Model/MaskEditorSession.swift` as public. SHA-256 `7dee01b2d53ea51fb5c2e44bd6b8eebbdd57abf23440ae1f1ac5dd58c80759a4`.
