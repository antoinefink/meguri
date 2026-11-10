# Settings rows, review settings keys and their observation (U160)

Spec-writer notes for `specs/review/review-preferences.md`,
`specs/storage/settings-keys.md` and `specs/conformance/review-config-ops.md`,
and the scenarios in `conformance/scenarios/storage/settings-keys/`. Read at
tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. Written 2026-11-10
(session s-20261110T085429Z-165d83d3, item shell-settings: U153, U154, U159,
U160, U161), revised the same day after review. The oracle was not run; both
scenarios are drafts without goldens.

Meguri's own design, with no Anki source: which rows Settings shows and the
section each sits on (settings-keys S2, including the hidden Anki Server
section of N12.4), writing only the changed key and only when the shown value
changes (review-preferences P3, P4, P6.1; settings-keys S3), the single-field
Settings form for the hour and learn-ahead rows (P6.1), the one skip-undo class
for every Settings write (R115, settings-keys S3 item 5), committing a field
instead of writing per keystroke and refusing values outside the range
(P3.1), the timebox change applying mid-session without resetting the
timebox (P3), the card on screen keeping its page when the play-buttons key
changes (P6), the R131 branch's accessibility label and button size without
an interval (P4.1), and the device-local rows (R132, R134–R137, R143, R148,
R416, R852).

| Spec section | Anki files read | Read for |
|---|---|---|
| review-preferences P1, P2 | `rslib/src/config/bool.rs` (key names and which booleans default to true), `rslib/src/config/mod.rs` (learn-ahead seconds default 1200, timebox seconds default 0, the hour capped at 23), `rslib/src/scheduler/mod.rs` (hour for the current scheduler, default 4), `rslib/src/config/schema11.rs` (`estTimes`, `dueCounts`, `timeLim` in a new collection) | each key's name, type and default; the readers falling back to the default on a decode failure |
| review-preferences P3 | `qt/aqt/preferences.py` (minutes shown as seconds / 60 rounded down, saved as minutes × 60), `qt/aqt/forms/preferences.ui` (timebox spin box 0–9999, learn ahead 0–999, hour 0–23), `pylib/anki/collection.py` (the timebox start and check: time since the start compared with the current limit, read at each check), `qt/aqt/overview.py`, `qt/aqt/toolbar.py`, `qt/aqt/main.py` (the timebox started when study starts, whatever the limit), `qt/aqt/reviewer.py` (restarted after Continue) | the conversion and ranges; when the timebox clock starts; Meguri's no-write when the shown minutes are committed again is its own |
| review-preferences P4, P4.1 | `qt/aqt/preferences.py` (the play-buttons box shown as the inverse of `hideAudioPlayButtons`), `qt/aqt/reviewer.py` (interval text above the answer buttons only when `estTimes` is set), `qt/aqt/main.py` (the audio keys read through the typed boolean read) | the inversion; what `estTimes` hides in Anki |
| review-preferences P5 | `rslib/src/preferences.rs` (the scheduling, review and editing groups and their keys), `qt/aqt/preferences.py` and `qt/aqt/forms/preferences.ui` (which keys have a control: of P5's list only the LaTeX box), `qt/aqt/reviewer.py` (remaining counts hidden when `dueCounts` is off), `proto/anki/config.proto` (the preferences groups) | which keys Anki's preferences save writes that Meguri does not show |
| review-preferences P7 | `rslib/src/preferences.rs` (one transaction named for the preferences op; the hour written only when it differs from the effective hour; every other key of the groups set from the typed read or the control on each save; the creation offset removed when it does not read as an integer), `rslib/src/config/mod.rs` (the review-mix read mapping other values to the first mode; the creation offset read and removal), `qt/aqt/preferences.py` (preferences rebuilt from the controls and saved on close when they differ from those read on opening), `qt/aqt/forms/preferences.ui` (spin box ranges, backups included: minutes between backups 5–9999, counts up to 9999), `rslib/src/storage/config/mod.rs` (a write skipped when the compact bytes are equal), `qt/aqt/reviewer.py` and `pylib/anki/collection.py` (the reviewer's and the timebox check's raw reads of `estTimes`, `dueCounts` and `timeLim`; a missing key fails the raw read), `pylib/anki/config.py` (the raw read) | the one-save behaviour, the rewrite of other-shaped values, the save on close without a change, the clamped ranges, the two kinds of reads |
| settings-keys S1, S5 | `qt/aqt/profiles.py` (sync options, network timeout, answer keys, allowed URL schemes kept in the profile), `qt/aqt/preferences.py` (which settings are profile settings and which collection settings; the save on close) | the device-local versus collection split; when Anki writes on closing |
| settings-keys S2, S3, S4, S6 | none beyond the specs cited there | Meguri's row map and rules, built on `storage/config-table`, `editor/config-keys`, `browser/config-keys`, `stats/heatmap-config`, `package/backups`, `engine/undo`, `sched/deck-options` and the design contract |
| review-config-ops R2 | `rslib/src/preferences.rs` (the preferences read: hour, learn-ahead seconds, the review booleans, timebox seconds; no write), `rslib/src/config/bool.rs`, `rslib/src/config/mod.rs` (the hour read: an 8-bit integer capped at 23, otherwise 4) | the members and their defaults as Anki's read gives them |
| scenario `prefs-keep-seconds` | `rslib/src/preferences.rs` (learn-ahead and timebox seconds saved as read; the hour compared with the effective hour before writing) | why both executors leave the 90-second rows and the stored 30 untouched |
| scenario `review-key-writes` | `rslib/src/storage/config/mod.rs` (compact-byte comparison before writing) | expected stored side, as `storage/config-table` C4 |

Follow-up for the oracle maintainer: `review_config` (review-config-ops R2)
needs a driver op reading Anki's preferences (scheduling and review groups),
its schema shape and the testkit model before `review-reads` is written.
