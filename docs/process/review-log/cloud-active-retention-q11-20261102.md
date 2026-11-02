# P24 Active and zone retention prerequisite

Ledger-only closure of K5.2 Active, K18.3 step 5, K10.3 zones and V2.3.
No normative rule changes. The matching Q11 test requirement stays in place.

- Review start: `0a5806f844ad4ef5e3d86d72e6c57e7ace1151e4`.
- U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
- Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Receipt: `core-test/20261102T224349Z-83881`, clean review start, selected 5/5 passed; zero failures or skipped tests.

Two selected `meguri-sync::cloud_publication` tests prove this prerequisite:

- `p24_active_lost_reply_preserves_attempt_fields_and_done_restarts_thirty_day_retention` checks actual Service Active fields, readable and unreadable previous epochs, and the attempt clock. A real save lands before process death; reopening preserves that exact earlier record. The `done` commit advances the earlier zone observation. Cleanup stays ineligible until exactly 30 days after `done`, then deletes the zone through an own operation.
- `confirmed_initial_publication_scans_unacked_rows_saves_real_batches_and_switches_last` checks the actual initial Active has no previous epoch.

The first test observes the zone 10 days before the attempted save, then reopens one day later. This covers an observation earlier than `done` and the exact 30-day boundary. Independent source review found no P0-P2 gap in this prerequisite. The receipt contains individual passing results for both tests; its other three tests do not expand this closure.

The patch removes only the matching P24 bullet and records this evidence in its Q11 status cell. It retains Q11's required test text and K22.2 previous-epoch retention coverage. Other P24 items, U113/U114, the original batch gate, signed SDK checks and milestone acceptance remain open.
