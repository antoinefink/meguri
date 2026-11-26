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
