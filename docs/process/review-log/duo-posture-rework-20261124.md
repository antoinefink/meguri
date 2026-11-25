# Remote Duo posture rework

PR #205. Independent reviewer `s-20261124T123814Z-b6a481ac` examined the remote transport correction in `cleanroom/20261124T194102Z-16122` and the final test fixture in `cleanroom/20261124T194614Z-27445`. All 46 focused Docker checks passed in `selftest/20261124T194614Z-27336`. The complete files below were classified as public.

- `docs/process/new-machine.md` — `ca64a19aae880b88be34955af62194f9f8a358f361aa20d801e789a287534b6a`
- `tools/tests/vnc_session_test.py` — `fc1d259404a54138a5fc28a7cf6022246fdb1afeee7cf4b09fa96bef20710671`
- `tools/vm/vnc-holder.py` — `b18e7fa249773c492e4bb9d2241bb8f5dacf24992bf27975362c68c36e406ac4`

After integrating master, the reviewer examined the complete updated design contract in `cleanroom/20261124T195423Z-42752` and classified it as public. Its SHA-256 is `4dad165a4448a914c773d7bfdac5fa65bc1f9b2e6f4078aac082dedd3a5f0bb6`.

## Leased open and closed commands

Independent reviewer `s-20261124T123814Z-b6a481ac` reviewed the complete integrated files in `cleanroom/20261124T202803Z-87776` and the final race corrections in `cleanroom/20261124T203724Z-97402`. The reviewer found no remaining blocker in the corrected interleavings. The whole-file admissions below match the reviewed bytes.

| File | SHA-256 | Independent review |
| --- | --- | --- |
| `docs/process/sim-posture.md` | `e7e4722c12dcb9705a1e8cd8264b70cc6c3a4f9b8ec8af3a216e5689e317e928` | `20261124T202803Z-87776` |
| `tools/tests/vnc_session_test.py` | `bdebc576c9d50cedda7a28be98c2027dedc47e5d07d17990a6e7ad4c272f416a` | `20261124T203724Z-97402` |
| `tools/vm/sim-posture-inspect.py` | `0c71d599fb3bdcb658abd5bd919afca39b907cf4e3c8c4ec4d8bdf4cdeba567e` | `20261124T202803Z-87776` |
| `tools/vm/sim-posture-visual.swift` | `56471e99fdc7acc4dec76d99248c68859a46758e50498a67cf53cdb743a87d3d` | `20261124T202803Z-87776` |
| `tools/vm/sim-posture.py` | `9fc732f45ad90e732c973bc5451ed30c4edcfc47378598681c6b7b55e700cff4` | `20261124T203724Z-97402` |
| `tools/vm/tests/test_sim_posture.py` | `56c62162ec9250725310815970941bb0287bc06a4385bcef7f55b7e3e3f7fc9b` | `20261124T203724Z-97402` |
| `tools/vm/vnc-holder.py` | `d730c4b3e646d82f0b9540a3a4d1d39d40d46d6bfafb5f890da655f8c32a5d81` | `20261124T202803Z-87776` |

Docker runs `selftest/20261124T203746Z-97957` and `image-tools-selftest/20261124T203749Z-98313` passed 84 scheduler, pool and transport cases and 22 posture cases. The transport fixture's ambiguous pause point was reproduced in isolation and corrected before the passing run. The selected host Swift compiler built the unchanged visual helper in `duo-visual-compile-check/20261124T203950Z-99664`. Read-only `sim-posture status` ran against the disconnected legacy coordinator state in `sim-posture/20261124T202344Z-82404`.

Live acceptance requires an open → closed → open run on one remote Duo lease, with inner → outer → inner display evidence. The coordinator's SSH authentication was unavailable during this verification.

## Resuming an interrupted VNC preparation

On November 25 the M3 guest restarted after the simulator pool drained. A subsequent prepare cleared the original drain records and retained the same supervisor, Tart process and VNC holder. The coordinator then acquired a Duo from the pool. The Docker recovery suite passed in `selftest/20261125T070623Z-96951`.

Independent fixer session `s-20261124T123814Z-b6a481ac` checked the interrupted cleanup and transient status cases in `cleanroom/20261125T070825Z-638`. The reviewer classified the complete files as public.

- `tools/vm/vnc-session.py` at `d08738cffa81959447e282fb90a74424da923f534cb280929cfc6562e813fc5a`
- `tools/vm/duo-fold.sh` at `e6383914de7e7f6bfcc44fdc7f31d4f7e29d644606888c0e6443248c64705b17`
- `tools/tests/vnc_session_test.py` at `9222a6d7de7567b7a4c45fbca38b33689ca4f28c83891325ecdeadbaaab42fc0`

## Verifying the signed Device Hub canvas

On the remote M3, three scheduler-held observations of the leased Duo identified the positive WindowServer owner PID and its exact signed Device Hub executable through the kernel. AppKit returned an active application with PID `-1` when queried for the same owner. The guest confirmed that the explicit Apple signature requirement verifies for the selected launcher and companion. The 3840 × 2400 VNC capture showed the exact leased title and the Closed, Partially Open and Open controls. Full pose transitions are tracked separately.

All 25 posture tests passed in `image-tools-selftest/20261125T075703Z-17098`. The 84 scheduler, transport and pool cases passed in `selftest/20261125T075603Z-13651`. Independent fixer session `s-20261124T123814Z-b6a481ac` reviewed the complete files and confirmed the secondary-window correction.

| Public file | SHA-256 | Cleanroom review |
|---|---|---|
| `docs/process/mac-ui-vm.md` | `36343bc8419f3501ec205974e6f87af6adef15e1e70b0527e575bf2cf1750ffa` | `20261125T073516Z-82423` |
| `docs/process/sim-posture.md` | `a87f540c272faf095e96b739bd8ea8aa0c4a5480339cff37618f2d0732bb77d9` | `20261125T073516Z-82423` |
| `tools/tests/vnc_session_test.py` | `a360d51acf0b86a82ef244eea57c395683707d9a12c6752c2263851aa8d26793` | `20261125T073516Z-82423` |
| `tools/vm/sim-posture-inspect.py` | `52d009284114d9eb0305b0f087d8ca0a5efe534943a7872735d58cc98b99adc5` | `20261125T073516Z-82423` |
| `tools/vm/sim-posture-visual.swift` | `4a7cfd87ed798ea236eb8d68d349f134107d7841ac26ee984f0c95c25bd4bc1a` | `20261125T073516Z-82423` |
| `tools/vm/sim-posture.py` | `096611e99ab9cebe2884b29423e68c0ba5b1e0366dc269f74f832f7b331f8d83` | `20261125T075726Z-18350` |
| `tools/vm/tests/test_sim_posture.py` | `937e1e3fea17395a3849ba1a03e039824fac6d682ce240df41457f2150f53531` | `20261125T075726Z-18350` |

## Running the VNC client on the hosting Mac

Both SSH connections to the previous VNC client timed out. The new client and Tart run on the hosting Mac. The scheduler reconnects to that client for each operation. A read-only probe on the hosting Mac survived termination of its SSH controller. The production deployment and preflight passed with the guest running.

Independent reviewer `s-20261124T123814Z-b6a481ac` reviewed the complete files below and classified them as public project source, documentation or synthetic fixtures. The fixes preserve generic SSH arguments, verify the runtime before draining, keep stop and click recovery bound to the original request, and reuse the supported legacy sessions. Docker checks covered controller loss during capture and click, receipt lookup, and migration.

| File | Complete-file SHA-256 | Independent cleanroom |
| --- | --- | --- |
| `docs/process/mac-ui-vm.md` | `c64a939877a151325b12f32cec129ff471f97c998ca926e2f6cf70cb83383c75` | `20261125T091929Z-75549` |
| `docs/process/new-machine.md` | `38bbea98b844515718436a23f535bd6eae0b63a1b6c151e84983d84a7084b3be` | `20261125T091929Z-75549` |
| `docs/process/sim-posture.md` | `655fb85d7e2941ef12c9da517b3666b575638698ce79164646351f2de5dbdee4` | `20261125T091929Z-75549` |
| `tools/mg.d/sim.sh` | `6250d4619b7bdbd835e6925abe3c7cc5fe63e4962ab6ee1db4ab93c12fb68772` | `20261125T091929Z-75549` |
| `tools/tests/remote_wrapper_test.py` | `576de9b1f51a8745b2e2fb97768bd06931910f38281970d4a8de5bb7662d83e0` | `20261125T091929Z-75549` |
| `tools/tests/vnc_session_test.py` | `97ff94100897a99e98ec03b04d6d649740663eee18d411ae28892f71c9dff29a` | `20261125T091929Z-75549` |
| `tools/vm/AGENTS.md` | `769e5c699e27fdce053f34a57fe026df91900e556566e6e9f62d8402b6fd9b94` | `20261125T091929Z-75549` |
| `tools/vm/duo-fold.sh` | `b903f936b3fbe39589e0ded3e6825706823d86ac3baac2c125921c598c8c5f56` | `20261125T091929Z-75549` |
| `tools/vm/remote-wrapper.py` | `048d5ec2df394d5f6a65bd6e6dfa519058e2e211197b686cada90094fa50fa6b` | `20261125T091929Z-75549` |
| `tools/vm/sim-posture-inspect.py` | `57c8498f891428027bc3ceb9d6d4497934fd62458ff944c915504aee5a0f013e` | `20261125T091929Z-75549` |
| `tools/vm/sim-posture.py` | `e723aee80cdc4ba02ed178aa2e8e594cc4eb91410bbff02c38b118a07bbf8b76` | `20261125T091929Z-75549` |
| `tools/vm/tests/test_hosted_posture.py` | `cdc01982383ced2c80e55e3b9f87ba016b74139e51504b1c65ffa59459f08cf4` | `20261125T093459Z-11667` |
| `tools/vm/vnc-hosted.py` | `c8916ab8cd15022303ab060b6a5d7f0b29e31466ecd1329639db8cbdcd1274ba` | `20261125T091929Z-75549` |
| `tools/vm/vnc-session.py` | `0361433453b25f9714db86a4f2d91f51d773b60d2ac32922c50b5ec6f04fe218` | `20261125T093459Z-11667` |

## Sharing the VM with M3 jobs

Folding runs on M5 and waits on the guest claim also used by M3 jobs. M3 hosts the persistent VNC connection. The production holder stayed connected across an M5 control disconnect. The first live inspection passed. Calibration stopped before input when Vision’s Accurate recognition failed on the guest.

Independent reviewer `s-20261124T123814Z-b6a481ac` classified the following complete files as public project source, documentation or synthetic fixtures.

| File | Complete-file SHA-256 | Independent cleanroom |
| --- | --- | --- |
| `docker/image-tools.Dockerfile` | `1ceac11b23309739cf879770dc5595833c4b618b2a773995262b1bda74e39bb6` | `20261125T105315Z-1142` |
| `docs/process/mac-ui-vm.md` | `4a7c28d74dc7d316d47b529a9a190bb4b6ecbd043ae356a53fa7d94660c90b4d` | `20261125T105315Z-1142` |
| `docs/process/new-machine.md` | `31cefac51a58392687e0aba28fd3d36039c14c6ff8a15d86c4d4031fc829c62a` | `20261125T105315Z-1142` |
| `docs/process/sim-posture.md` | `c61c8e4bef254147685efb4a93b32e8e2453a8953f566fd2f875c5604db7e7b7` | `20261125T105315Z-1142` |
| `tools/scripts/vm-claim.py` | `f71d76cc817da0a7e83b3232c710f58da883b8e10c96eeca484abb459d3034c8` | `20261125T105315Z-1142` |
| `tools/tests/vnc_session_test.py` | `81bebf58239c7fa8f681d4550b2e2a61a55a2933ea3e6897c64c77da69887493` | `20261125T105543Z-3873` |
| `tools/vm/AGENTS.md` | `db075d154131e99869875b404de7c6cf28eabf4eb0d432f10f82ac2242ddd71d` | `20261125T105315Z-1142` |
| `tools/vm/duo-fold.sh` | `71070ef4ff2be7b34d1543370170d52424efc3c56b49456f0f5efc16a4bc7b22` | `20261125T105543Z-3873` |
| `tools/vm/fold-claim.py` | `e959bbed9139177db65f04e922c8e0baefa59fbfa3601a9446a07e8272cd5626` | `20261125T105543Z-3873` |
| `tools/vm/tests/test_fold_claim.py` | `f2b158f4dbe641f1c67909f95313bf6cb79b055194cac3a1d65882ba60b7caa6` | `20261125T105315Z-1142` |
| `tools/vm/tests/test_hosted_posture.py` | `381a35e844fb73d642f5f72684e7b3a21ef89c8c7771dd1a751e6d71caf455ff` | `20261125T105315Z-1142` |

## Verifying the frontmost Duo canvas

The scheduler verified the running M3 holder and reused it. During a live request the M5 waited for an M3 simulator job through their shared claim. The selected Device Hub opened the leased Duo in its compact window above the management window. On the same guest frame, Accurate recognition on M5 identified the title with confidence 1.0. Fast recognition scored it at 0.5.

The final candidate passed all 307 Docker image-tool tests and 128 VM wrapper, VNC and lease tests. Native end-to-end pose verification is pending.

Independent reviewer `s-20261124T123814Z-b6a481ac` classified the following complete files as public project source, documentation or synthetic fixtures.

| File | Complete-file SHA-256 | Independent cleanroom |
| --- | --- | --- |
| `docs/process/mac-ui-vm.md` | `39961911ef6af61cebb4640841b911be6289f0631a24a5d8f5638815ab6881d2` | `20261125T120152Z-11728` |
| `docs/process/new-machine.md` | `2e72954d450e927909eb814f062570b9dcf8aae54c81529bafd07e42a527c7eb` | `20261125T110607Z-23826` |
| `docs/process/sim-posture.md` | `b6c676c9e01c985b44472685a824115c971d4786177997fd9654e55008da05ca` | `20261125T120152Z-11728` |
| `tools/mg.d/sim.sh` | `78d559e9c553d12996203754d2e64ac7a14d21d9d13a8a47b09a96786328f0c9` | `20261125T110607Z-23826` |
| `tools/tests/vnc_session_test.py` | `6531765f99542b9ac88ccc7d5cd64b8dde387d237906728973b07a614c776c31` | `20261125T111034Z-31693` |
| `tools/vm/AGENTS.md` | `01acd3234d195b3e59b52a4c63c115b144295dbd9a06134d688cdf959899e7fd` | `20261125T120152Z-11728` |
| `tools/vm/duo-fold.sh` | `36f49ed599e7b3f9e13f3082557bb91358e696867ded59b54d11153ec7165791` | `20261125T110607Z-23826` |
| `tools/vm/fold-claim.py` | `564946f07653219766b95394d127c760c4705ae0bdf02d70748b0b6c1392095c` | `20261125T110607Z-23826` |
| `tools/vm/sim-posture-inspect.py` | `77b2400f8950ab9ec31ded8dbc093a41e9b1b88c8fd1104889ec32daeca7a239` | `20261125T111034Z-31693` |
| `tools/vm/sim-posture-ocr.swift` | `e50ac5439716639e79a6f892332164440afac67be61efad7caee47867e2f50d1` | `20261125T111718Z-46626` |
| `tools/vm/sim-posture-visual.swift` | `69330e3a690576dd8a97e4072579c65c81145b48de69fe4225e1aa2ff3c789a0` | `20261125T111718Z-46626` |
| `tools/vm/sim-posture.py` | `1e67bf53a0b94d67358041776fab4ff6effbd03a02bcfb8914d181a821a4db4d` | `20261125T120956Z-22843` |
| `tools/vm/tests/test_fold_claim.py` | `86fc1ba9df429f7fd4d32a5d38e631e861019922deea7ccc6bbb8ed4220a947d` | `20261125T110607Z-23826` |
| `tools/vm/tests/test_hosted_posture.py` | `3d7868c00eba12d3eb974b214440b27959564ff2ba87039f464d13d728e844d0` | `20261125T113114Z-71613` |
| `tools/vm/tests/test_sim_posture.py` | `beda7a182485725d56eb3668cf9027be6d78aea6a8fba2a739f93a42c8762e7d` | `20261125T120956Z-22843` |

## Neutral text outside the Device Hub window

The M3 calibration selected the leased Duo and found the three matching pose controls, then refused before clicking. Offscreen inspection of a fresh parked frame located one OCR row wholly below the signed window. The revised baseline check admitted that static row. All 309 Docker image-tool tests passed, including new, moved and partly overlapping text cases. The native fold cycle is pending.

Independent complete-file review by `s-20261124T123814Z-b6a481ac`.

| Public file | SHA-256 | Cleanroom run |
| --- | --- | --- |
| `docs/process/sim-posture.md` | `be4fecbe5e3cbc522e3233317f3338735686d86673122a1ab710f7e5353ffbbd` | `20261125T122435Z-47844` |
| `tools/vm/sim-posture.py` | `ba1d4671b5908fde12b7a9d9792af2825232b5d2f80d9023d9e3544864218b04` | `20261125T122435Z-47844` |
| `tools/vm/tests/test_sim_posture.py` | `6fd677428d02d01c615591cc2dc6425f881743efe1b84d5873ad28c3b107db7b` | `20261125T122435Z-47844` |

## Wait for an older VM claim keeper

A fold on the M5 refused before guest input when it could not confirm the shared claim at handoff. Folding now waits before creating its local holder when a live matching older keeper still owns the claim. It takes the ordinary shared claim after natural release. All 128 scoped VM wrapper, lease and VNC tests passed, including handoff and cross-Mac claim cases. The PR verification section tracks native fold acceptance.

Independent complete-file review by `s-20261124T123814Z-b6a481ac`.

| Public file | SHA-256 | Cleanroom run |
| --- | --- | --- |
| `docs/process/mac-ui-vm.md` | `d92608224bcd7d2dcff83454be03856dde7bab9a9420e9546dad123b41d0b0ee` | `20261125T133956Z-45073` |
| `docs/process/sim-posture.md` | `0925ee0e393b456197a8836c79099a954bd00bc7b77ca750ca87d5b7fbf529e0` | `20261125T133956Z-45073` |
| `tools/tests/vnc_session_test.py` | `12f2aac01221f01874cc62b0559b1fba9797e01b8801870a6b13fa44dcb857e4` | `20261125T131915Z-21453` |
| `tools/vm/duo-fold.sh` | `92ee39fb80652d04bbd1fd004b1aa0af4bc7c6c02e77a3c283422e6418a739f9` | `20261125T131915Z-21453` |
