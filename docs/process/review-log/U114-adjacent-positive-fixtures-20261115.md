# U114 adjacent positive fixtures

Starting commit: `4c006f15`. Original batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Unit acceptance remains pending.

The four SDK tests now use the qualified fixture. Their assertions and limits are unchanged. Production code is unchanged.

Author: `cleanroom/20261115T022641Z-30829`. Independent review: `cleanroom/20261115T023119Z-34082`, no P0–P2 findings. Focused verification: `swift-test/20261115T023416Z-35451`, all six methods pass in 9.993 seconds, including the Unknown preparation control and all three refusal variants.

Full Core and integration verification remain required.
