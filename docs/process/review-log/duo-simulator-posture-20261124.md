# Duo simulator posture

Base `a15f6ccc2031e1331d1dda06447f9d116bb1cf29`. Two sidebar verification runs on the remote test VM received the folded display. An explicit posture job gives a caller control of the leased Duo before driving its UI.

The cleanroom implementation used session `s-20261124T122334Z-5d83997e`. Fixes in `s-20261124T124450Z-c7837ff7` use the native XCTest attachment transport following the first guest run. Independent reviewer `s-20261124T123814Z-b6a481ac` reviewed all ten complete candidate files and the three session records in `cleanroom/20261124T130519Z-9056` and classified them as public. The checked SHA-256 of `tools/tests/sim_vm.bats` was `1e46b7099d01003308cbf9757796e31c619fc3f6afe63101489ccb74ad8e207d`.

The Mac Debug runner compiled. All 12 focused harness checks passed in `selftest/20261124T130515Z-7825`. The live guest attempt `sim-posture/20261124T130517Z-8662` timed out while activating Device Hub. A transition remains unverified. Review requested stronger lease protection and narrower simulator row selection.

The same independent review classified the complete `tools/tests/sim_posture.bats` as public at SHA-256 `0b35b5754388fe39fe2f5c487dbff4f1e3c50caba1379ad0bee8e8305b421529`.
