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

The independent privacy reviewer read and admitted all 28,599 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionNoteFlowTests.swift` as public. SHA-256 `1a0b5313b59afc56cadc94f774fd69fc92c0fb2b035d5efac25c397481e5d439`.

The independent privacy reviewer read and admitted all 50,645 bytes of `app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift` as public. SHA-256 `729caa794863db4b841159f756e30df63d06102dbae7e5ddb3d52e7b2f113aba`.

The independent privacy reviewer read and admitted all 16,978 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Model/MaskEditorSession.swift` as public. SHA-256 `c4b6ee938977f446a3de0d4c3ab1dbe070364d8b9ffa0d3a5542abeec659d4b9`.

The independent privacy reviewer read and admitted all 29,461 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionNoteFlowTests.swift` as public. SHA-256 `b63193382305b61cd310c7c10edd451777f2b54f345e851ddabe4f5865029a9c`.

The independent privacy reviewer read and admitted all 4,221 bytes of `app/Packages/MeguriKit/Tests/ImageOcclusionTests/MaskEditorCommandTests.swift` as public. SHA-256 `45a381178815c391e3c2de3c62086fc3fdb1504d7d2f398216671137cb27cd45`.

The independent privacy reviewer read and admitted all 51,346 bytes of `app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift` as public. SHA-256 `8592cb04d99039254ece7e389906bca1c34de50cc5b2cba22abff076da8d30d1`.

The independent privacy reviewer read and admitted all 34,506 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionNoteFlowTests.swift` as public. SHA-256 `31fab3ca100375579d03536fe0e6877dd3c0115ee3bee4e7aadcb4e350dac84a`.

The independent privacy reviewer read and admitted all 12,593 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionImageSourceTests.swift` as public. SHA-256 `cbabc674d9256e6fcc32718e346dff885f858d44a4480d5d2221d5e7e7abebe6`.

The independent privacy reviewer read and admitted all 21,328 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `661998d1d56b00418c9071fba59ffcd803a4c5f9a61fb88aa696a2295d6b45d6`.

The independent privacy reviewer read and admitted all 2,193 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/AGENTS.md` as public. SHA-256 `0a18dd5cafe468b6c92183a5c3dc4a4ee2a130071b3fbec1f203b2687dcf56c6`.

The independent privacy reviewer read and admitted all 21,339 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `9836fc85e93999c846f3e13c8c71ceb574569fa97febb673277ed4bb01b69323`.

The independent privacy reviewer read and admitted all 21,341 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `14e66ae8fed3dbad944f129994ba14cc04cf7dfeb19bb183596782ca9879a2a0`.

The independent privacy reviewer read and admitted all 36,005 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionNoteFlowTests.swift` as public. SHA-256 `a6b059634e32f22104b24e5b119c71094121fa5f40bed9c05451a0d1e977a6a7`.

The independent privacy reviewer read and admitted all 24,725 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `69127fd8bdf09a512b015da47994b79e32f22cc40bf2aa19bb94a54f3c63ca42`.

The independent privacy reviewer read and admitted all 37,029 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionNoteFlowTests.swift` as public. SHA-256 `38632ad733fcbc9ce782b76974d4796c02ad55ddb8a6f657a2e44b590428071b`.

Source review covered F13, O4.1, and M5.

The focused Rust media suite passed all 13 tests in `core-test/20261126T101106Z-82994`. Nine Swift bridge tests passed in `swift-test/20261126T100308Z-54840`. The iOS Simulator Dev build passed in `build-app/20261126T100307Z-54389`. The native Add screenshot in `ui-test/20261126T095815Z-32198` exposed the initial zoom and macOS accessibility frame issues corrected in the follow-up.

Verification receipts: `core-test/20261126T104859Z-81496` (15 passed) and `ui-test/20261126T104509Z-64172` (Add, two cards, Edit Note).

The independent privacy reviewer reviewed all 291,418 bytes of the native interface contract and admitted the public document at SHA-256 `222c5bc30ccdf96f5f7c165f8796d2ed93db55d7cc776d662cb14b982acfb285`.

The independent privacy reviewer read and admitted all 26,664 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `d9808ec46e45b931185ca72203231e024baeea1fb91c8888f5e92e359a21f8d5`.

The independent privacy reviewer read and admitted all 71,606 bytes of `docs/process/verification-status.md` as public. SHA-256 `8ec51195b37d6f85427ee90efd068937676be625afa88f4980506c305519cf56`.

The independent privacy reviewer read and admitted all 52,244 bytes of `app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift` as public. SHA-256 `f8766122d5dc2e9820b842ba02ccc12d4b4a608321aa5841fc910e45cda5d803`.

The independent privacy reviewer read and admitted all 17,598 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Model/MaskEditorSession.swift` as public. SHA-256 `8ffb2f13b36100a2f03c3a86f882a894df1a625a55bab5a164d2aec3937b2c87`.

The independent privacy reviewer read and admitted all 26,683 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `4c00b8c028d978c54bc3f1548842d1bbd7d56674fc2daa36faae99a20a8a9bb1`.

The independent privacy reviewer read and admitted all 26,818 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `f0fc68a61843adf2187f018bea9274ac9536e860b70410eee71618be3455e157`.

The independent privacy reviewer read and admitted all 35,711 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `7c0fc12cf3d0cea0bea8597b7d5892797ad686854ed9f033c441c3d865513167`.

The independent privacy reviewer read and admitted all 7,387 bytes of `app/Packages/MeguriKit/Sources/Features/ImageOcclusion/Canvas/CanvasNavigationInput.swift` as public. SHA-256 `8b96c6fd445207b692c3b15a8803a479d071dc113d74b93a10d122e943a9bf9a`.

The independent privacy reviewer read and admitted all 57,431 bytes of `app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift` as public. SHA-256 `b52b30a89692d6efbb9385cb161e1e9bf719add476980a4ff3cafe9ef53684ee`.

The independent privacy reviewer read and admitted all 43,752 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionNoteFlowTests.swift` as public. SHA-256 `4e655055a0c88de7410c1666f6bfab3f95da1432e843d66903095be21814e9f7`.

The independent privacy reviewer read and admitted all 57,816 bytes of `app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift` as public. SHA-256 `31540954e8eceb04a368a5c27c1e6406d9f1043e234710997f103cb18782dcf2`.

The independent privacy reviewer read and admitted all 47,744 bytes of `app/Packages/MeguriKit/Tests/AppModelTests/OcclusionNoteFlowTests.swift` as public. SHA-256 `935861e65791369115fb3f0f901a97d0eed69cb673d50b210f24121af8c3179d`.

The independent privacy reviewer read and admitted all 42,300 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `1a9ff7b55732a536ba2565caf974cef6cf9a11444cffa19aab3606248e28a594`.

The independent privacy reviewer read and admitted all 45,049 bytes of `app/MeguriUITests/IOAddEditUITests.swift` as public. SHA-256 `c46e6dca3af898ba074b68ad0fa8026d93c3a6c03f0ee1599a3317083ecea338`.

The independent privacy reviewer read and admitted the complete 72,455-byte merged verification register. SHA-256 `f6d11e917a2d9e74eba72039aa36e53c6b0e656901fc83425b33b7cbf587c21f`.
