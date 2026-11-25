# Bluetooth output between review words

Base `e7feaf0e93172dd8cc1a01c2d82a0512f55710ad`. Unit `review-bluetooth-continuity`, `render/audio-playback` A4, A6, A7 and A9.

## Device observation

On iPhone with AirPods, a short word lost its opening syllable after a quiet interval. Immediate replay sounded complete. With iOS Background Sounds enabled it sounded complete after the interval as well. Prior work in #183 prepared the native player before scheduling the word. This unit keeps a silent output stream between eligible clips during a foreground review. The physical check for this change uses Background Sounds off, the same word after a long pause and on immediate replay, and another app playing to check that ducking ends between words. Simulator results cover software behavior only.

## Verification and review

- Baseline `swift-test/20261125T105333Z-21907` passed 24 existing card audio tests. The first filtered implementation `20261125T105314Z-21225` and independent review `20261125T110346Z-35305` covered review admission, idle mixing, playback and teardown. `swift-test/20261125T110931Z-43827` passed 37 tests. The first iPhone build rejected two deprecated APIs. The SDK correction `20261125T110923Z-43480` uses throwing graph connection and playback. `unit-sim/20261125T111143Z-46815` passed 44 iPhone tests with no skips.
- Filtered fix `20261125T110611Z-38397` fenced pending card presentations and native scheduling, kept the current word resumable and handled a stopped output engine. Independent review `20261125T111541Z-51653` covered all eleven changed files. Compiler correction `20261125T111724Z-55089` extracts notification values before actor isolation. `unit-sim/20261125T111833Z-55730` reported the resume assertion before waiting for a play event. `unit-sim/20261125T112503Z-62806` reproduced it alone. Both jobs were stopped after the failure and released their simulator.
- Filtered fix `20261125T112119Z-59741` reviewed the queue handoff during physical stop, configuration changes and HTML suspension. Filtered fix `20261125T112451Z-62419` covers the resume ordering, audible lease lifetime, restart after failure and speculative preparation test.

- The isolated resume failure was the handoff between the native play attempt and the transport task recording it. The fix preserves a submitted word for explicit resume while revoking a pending attempt before it can play on a replacement route. Filtered integration `review-output-integration` reconciles the audible lease and the silent-output lifetime across both fixes. The original resume assertion stayed enabled.

- Independent filtered review `20261125T113218Z-69357` examined the final seven files in full. It confirmed the six PR findings were addressed. The reviewer checked interruption and stopping order, configuration notifications, HTML pause on both pages, and the first-card preload assertion.

- A subsequent native backend check stopped inside the real player's pre-scheduling hook. The controller retained an unsubmitted clip. The full Mac playback run `20261125T113758Z-75319` reported both failing assertions and `20261125T114111Z-80691` reproduced them on their own. Filtered fix `20261125T114125Z-83116` gives each play attempt a permit shared by revocation and the actual native scheduling call. Both original disconnect regression tests were retained. The Mac transport run `20261125T114407Z-88267` passed 67 tests.
- Filtered runs `20261125T113941Z-78494` and `20261125T114318Z-85599` covered partial session activation followed by cleanup failure, entering waiting, finished or error, and returning to an eligible card. The iPhone package was rebuilt after these changes. See PR #219 for the combined iPhone and Mac results and the physical listening follow-up.

- Review `20261125T114929Z-98698` also found that automatic refresh after an error could restore controls for an invalidated card. Fix `20261125T115239Z-4631` clears the previously presented key. Review `20261125T115401Z-6625` verified it. Mac run `20261125T115423Z-7211` passed all 151 selected tests, including same-card refresh from the question and answer sides and explicit continuation from waiting and finished.
- External review found an overtaken hardware activation and a decoder failure during pause. Filtered fixes `20261125T115654Z-12573` and `20261125T115717Z-13108` require confirmed audible activation, retire a clip after genuine backend failure, and verify the visible WebKit player is active before revocation. Filtered review `20261125T120024Z-16229` covers those files. The PR records final combined test results.

## Privacy review

The independent filtered reviews above cover the final files. The independent filtered reviewers approved these complete Swift files as public implementation and synthetic tests. The independent spec and session reviewer approved the complete policy and all listed session records as public operational material. Only files flagged by the private scan have pins. Repository paths and SHA-256 follow.

```text
11eda11c77a802d0951393f3a2c5b3f96a5fd1187284a2157c8b54539f579311  specs/render/audio-playback.md
2609f9fde767c011219bba509519fd667f167a6f3937ce865095b64795acd2a3  app/Packages/MeguriKit/Sources/CardRendering/Audio/AudioSessionCoordinator.swift
836c8128dd2bd184b1a41329cf2920e59ef5cf000736d2ed561fbd3fd84bc1b7  app/Packages/MeguriKit/Tests/CardRenderingTests/AudioContinuityTests.swift
5c88bf3b94b11c8a7dcea45903bb551d5db4826f9f6f8e59c5fbe6f00a8edcd9  app/Packages/MeguriKit/Tests/CardRenderingTests/CardAudioControllerTests.swift
b6a10e67d9cd661046765f7cb53346e0fb9bbb9b391e2376d6c325d9c1bd96cd  docs/process/provenance/sessions/s-20261125T105050Z-ab1a331e.json
40ae09ceff113d80ef9286b5913c26f4234922206633d4339aff3c58dd010609  docs/process/provenance/sessions/s-20261125T105240Z-45547158.json
d89f7d4e33e88979b87775ca86ff3e57e55c862e7c00a930a2c697ef6e6a9066  docs/process/provenance/sessions/s-20261125T105356Z-a7c0cd1d.json
4773aa8c5e9eded1c6699804e2737e1b5e69b6e2176a27d8bfa4c8344a15a5f5  docs/process/provenance/sessions/s-20261125T110507Z-10fda75b.json
cc1f043712aed695b638e87d45f3e29f9ce8f8219010cb0d6f3cd8e1c49fa490  docs/process/provenance/sessions/s-20261125T110905Z-779126bb.json
95f4cf9bf395ba64134f148b4fc10a5a1127aa5c4262a8a463b577046f8d1937  docs/process/provenance/sessions/s-20261125T111242Z-942c0020.json
4c44c6928895b2ec6b4824d9d55377961c3c6aada928dc646efc2c284055ff2f  docs/process/provenance/sessions/s-20261125T111709Z-5b44cce4.json
fb1692023e5bfd08e1bccacb3bbfdf8e7c36d1d5efa07f641081c9c1a2485e6b  docs/process/provenance/sessions/s-20261125T112054Z-81fc51b2.json
30f32b807548010cebd76d2c3bd739baac83f28180b3d7234e26c0f39530bfbc  docs/process/provenance/sessions/s-20261125T112420Z-dd623be5.json
a8a75963100d6681df093d3c6d0d83d4d95a608eb5a415cb37bc0754617842d8  docs/process/provenance/sessions/s-20261125T112619Z-80fb4934.json
d76f9a3b31981ac048ee88af3ca12e1961db3463d39dce443567d34bc590d92e  docs/process/provenance/sessions/s-20261125T112937Z-ebfefe1d.json
ca06a4e4c9cfe1a633ca5a331e34ffe77dfab17cffa738ca14cedda075e8b776  docs/process/provenance/sessions/s-20261125T113924Z-4f79929c.json
8743a92700f1b60fee91539447c5389e5ce0d6c0d95ad7ef9ac5c3a9a0313354  docs/process/provenance/sessions/s-20261125T114111Z-554edce8.json
```

The inherited R554 missing-middle timing exception remains tracked in `docs/process/review-log/U69.md`.
