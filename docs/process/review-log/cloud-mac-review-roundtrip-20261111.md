# Native Mac review round trip — 2026-11-11

Role: fixer. Built source: `29d8f246`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/cloudkit-recovery#V14`, `sync/policy#P12`.

The normal signed Mac app completed this practical workflow against development
iCloud. Only this Mac was used. A second device and the required acceptance jobs
remain untested.

## Observed workflow

1. Verified standalone build `build-app/20261111T100044Z-31227`: clean source,
   matching executable hash, valid signature and development cloud configuration.
2. Saved a private collection package, including available media, through the
   native export and Save dialogs. See `export-search-authority-20261111.md`.
3. Started a review, revealed the answer, chose Good and left the session without
   Undo. Study counts changed from 20 new, 1 learning and 19 to review to 20 new,
   0 learning and 19 to review. The session reported one recorded answer.
4. Chose Sync Now. The three pending changes cleared before replacement.
5. Confirmed the normal Replace Local Collection from iCloud preview once.
   Before replacement, the local summary showed 43 reviews. The download advanced
   through at least 154 completed pages and 4,928 records, then replaced the
   collection and dismissed the sheet. No restart or repeat of that download was
   used to obtain this result.
6. The downloaded collection retained the changed study counts. Quit normally,
   verified that the exact preview process exited, then opened the same product.
   The changed counts and the one-answer session remained.
7. Opened a read-only replacement preview after reopening: the local summary still
   showed 43 reviews, 1,001 notes and 1,001 cards. Cancel closed that sheet with no
   collection change. Local media counts: 549 before this trial, 565 after it.

The app is left open on the deck list. No account or connection values, collection
package, or private card contents are included in this log or committed.

## Open issues and limits

- Media status reports 2,389 files absent both locally and in iCloud. Available
  media grew during this run, but this does not prove complete media recovery or
  playback of every file.
- During collection download the sheet showed a generic error carrying a media
  download admission refusal, while page counts continued to increase. It should
  distinguish those failures and show useful collection progress.
- After activation, status showed unknown/account unavailable. A later summary
  read stayed busy until Quit. The process exited after a delay, with no force
  signal. During Quit, callbacks also reported a closed transport. The reopened
  app could read the summary. This trial does not establish the cause of that
  activation error.
- After reopening, status still carried a cancellation diagnostic alongside the
  missing-media warning. Error ownership and clearing need focused diagnosis.
- Physical-device round trip, required integration gates and final acceptance
  remain open. The retained partial gate failures are listed in
  `cloud-three-fixes-review-20261111.md`; none is waived by this native trial.

This is practical review-data evidence only. No Unit trailer, milestone pass or
release acceptance is claimed.
