# Mac note editor opening focus

WebKit can focus the first field while its view is still zero-sized. With enough fields to scroll, the initial caret reveal leaves the top of the focused box outside the window. The Mac Add and Edit forms now keep the page body out of the scrollable area until WebKit has a viewport. WebKit then handles scrolling and paints the existing system-colored focus ring.

## Verification

With the original editor, the new regression failed its top-edge check in both Add and Edit. Focus and scrolling were present in both cases (`swift-test/20261126T102554Z-20751`). On `1a3ce3db0`, both cases passed after the view grew from zero to 1×1 and then to 500×240. The full field-editor suite passed all 13 tests (`swift-test/20261126T105301Z-96846`). Static checks passed all six checks (`static-checks/20261126T105302Z-97291`).

The Mac VM journey reopens a synthetic 14-field note type in a 550×350 window, types into the first field, and captures the Add window. Its receipt and screenshot attachment are under `ui-test/20261126T105211Z-90722`.

The separate collection test `preparedAddDraftFlushFeedsOneDurableU27WriteAndExistingStudy` saved the note, closed Add, and stalled in `ReviewCoordinator.start()` when entering Study. Isolated runs with the fixed and original editor reached the same async backtrace and were stopped (`swift-test/20261126T105416Z-1315` and `swift-test/20261126T105647Z-14675`).

## Independent review

The reviewer identified the zero-size mount and the intermediate window size in the original JavaScript approach. The final change uses CSS exclusively during zero-size layout. Independent reviews of the CSS and compact-window test found no outstanding source issues (`cleanroom/20261126T104431Z-59669`, `cleanroom/20261126T105235Z-94219`).

The reviewer read the complete session records, including their paths, and classified both as public operational metadata (`cleanroom/20261126T100414Z-61588`). The verified SHA-256 digests are below.

| Session | SHA-256 |
| --- | --- |
| `s-20261126T095742Z-5ac7fbf0` | `df885ac2001d76b4c1b1d4bfdf556f3871b5045356c731fae31c4bd3bdc55072` |
| `s-20261126T100354Z-0c20d258` | `a71ee3baeef8c3329e577b36524e4b42f4d5d4b04e6d3bfb84139bda3ceedf38` |
