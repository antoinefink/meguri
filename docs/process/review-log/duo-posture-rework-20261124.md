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
