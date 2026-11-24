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
