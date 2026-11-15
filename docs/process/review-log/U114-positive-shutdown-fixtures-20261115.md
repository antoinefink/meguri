# U114 positive shutdown fixtures

Starting commit: `646cd64a`. Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`. This is repair evidence; unit acceptance remains pending.

The full Core run `swift-test/20261115T021136Z-27308` reached 479 completed methods, then all seven old-callback variants refused their initial batch. The issued-stop case reached its one-minute limit and stalled. That incomplete run was cancelled and retained.

Both scenarios require eligible SDK work. Their default fixture had Unknown epoch authority. The repair uses the existing canonical epoch intake and actual policy full update before capturing guards or installing gates. It extracts the same strict setup for use with the existing policy and executor. Negative Unknown cases, all body assertions, seven transitions, cancellation and finalization ordering, and timing limits remain unchanged.

Author: `cleanroom/20261115T022028Z-28704`. Independent read-only review: `cleanroom/20261115T022600Z-30340`, no P0–P2 findings. Focused check: `swift-test/20261115T022846Z-31882`, all seven methods pass, including seven old-callback variants. Issued-stop completes in 1.224 seconds. Five prior pending-state regression controls also pass.

This focused receipt does not replace the full Core package or integration gate.

Privacy scan: `private/20261115T023029Z-33633`, no findings. Safe implementer similarity report: `similarity/20261115T023015Z-33292`, one file compared and zero flags.
