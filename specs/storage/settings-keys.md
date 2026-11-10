# Settings rows and where they are stored

Spec: storage/settings-keys
Requirements: R112, R113, R114, R115, R116, R323, R324, AE8; supports R109, R128–R141, R143, R146–R150, R153, R416, R852, R1498, R1504
Anki: 26.09.3 (the collection keys, and what Anki's Preferences screen and its save write, S5); none for the row map and Meguri's own write rules
Scenarios: conformance/scenarios/storage/settings-keys/

Every setting Meguri's Settings shows (U160 and the pages mounted in it: U161, U163, U163a, U164, U229) lives in exactly one place (R112). This file says which place for each row, names the spec that owns each collection key, and gives the rules every collection-config row shares, so that opening and closing Settings never changes the collection (R114, AE8). The table and its byte rules are `storage/config-table`.

## S1. Storage locations (R112)

| Location | Where | Synced | The row shows |
|---|---|---|---|
| collection config | a row of the collection's `config` table (`storage/config-table#C1`) | yes: CloudKit sends it as a config entity (`sync/cloudkit-records`; concurrent changes merge by `#K15.8`), and a manual Anki transfer replaces the whole table (`storage/config-table#C9`) | the "Synced" marker and footnote (R113, `docs/design/native-interface-contract.md` N11.4) |
| device-local preferences | Swift settings of this installation (KTD12) | no | no marker |
| Keychain | this device's Keychain, not synchronizable (R1498) | no | no marker |
| deck or preset record | a `decks` or `deck_config` row (owned by the deck options area, U232) | yes, as that record | not on a Settings page |

- A collection-config row's key is Anki's key, spelled as Anki spells it, or a `meguri` key (`storage/config-table#C8`). A device-local, Keychain or deck/preset row never writes the `config` table (R324, `storage/config-table#C8`).
- No row is stored in two places, and no row moves between places with a platform or a build.

## S2. The row map

Sections in the order of `docs/design/native-interface-contract.md` N11.4 (R109's pages plus Collection and Anki Server). A section whose unit has not landed is absent; its rows keep their location when it lands.

| Section | Row | Location | Key or owner |
|---|---|---|---|
| Account & Sync | iCloud connection, Enable/Pause | device-local (the CloudKit binding, `sync/cloudkit-records#K10.2`) | `sync/cloudkit-recovery#V16`, `sync/status-strings` T14 |
| Account & Sync | Sync audio and images (R122), Sync automatically (R123) | device-local | `sync/policy` |
| Review | Next day starts at, Learn ahead limit, Timebox time limit, Show play buttons on cards with audio, Interrupt current audio when answering (R128–R130, R133); Show next review time above answer buttons (R131) only if `review/review-preferences#P4.1`'s R131 branch holds | collection config: `rollover`, `collapseTime`, `timeLim`, `hideAudioPlayButtons`, `interruptAudioWhenAnswering`; `estTimes` | `review/review-preferences#P1` |
| Review | Show progress bar during review (R132), Space and Return answer Good (R134), Answer keys (R135), Play card audio in Silent Mode and Lower other audio (R136), Open card links in and Allowed link schemes (R137), Double Tap Prevention (R416) | device-local | U175, U178, U180, `render/card-behaviors` |
| Editing | Paste clipboard images as PNG (R138), Paste strips formatting (R139), Default deck when adding (R140) | collection config: `pasteImagesAsPng`, `pasteStripsFormatting`, `addToCur` | `editor/config-keys#K2`, `#K3`, `#K4` |
| Editing | Image max size (R852) | device-local | `editor/paste#P6` |
| Browsing | Default search text, Ignore accents in search (R141) | collection config: `defaultSearchText`, `ignoreAccentsInSearch` | `browser/config-keys#K6.4`, `#K6.5` |
| Browsing | column widths, density and the other Browse display state | device-local | `browser/config-keys#K11` |
| Appearance | Card zoom (R143) | device-local | U67 |
| Heatmap | colour theme, calendar mode, limits, exclusions, excluded decks (R146) | collection config: members of `heatmap` | `stats/heatmap-config` |
| Heatmap | First day of week (R149) | collection config: `firstDayOfWeek` | `editor/config-keys#K5` |
| Heatmap | Show heatmap on Home, on deck page, streak stats (R148) | device-local | U229 |
| Anki Server, hidden (N12.4) | server endpoint (R1500) | device-local | `sync/login-endpoints#L4` |
| Anki Server, hidden (N12.4) | username and host key (R1498) | Keychain | `sync/login-endpoints#L5`, `#L7` |
| Anki Server, hidden (N12.4) | network stall timeout (R1504) | device-local | `sync/transport#T8` |
| Backups | minutes between backups, daily, weekly and monthly backups to keep (R150) | collection config: `backups` | `package/backups` |
| Integrations (Mac) | Migaku and AnkiConnect-compatible server toggles (R153) | device-local | `servers/` |

- **Hidden Anki Server section.** Anki server sync is hidden from the interface (N12.4; AGENTS.md, 2026-11-01): Settings shows no Anki Server section, and U160 and U161 add no UI for it. Its three rows are listed only so the kept code and its tests store them in one place. They appear only in UI suites launched with N12.4's hidden-interface argument.
- **Not in Settings.** Settings shows no row for any other collection key. "Load balancer" (`loadBalancerEnabled`, R665) is in deck options' Advanced section (R751, U232). The keys Anki's preferences save writes and Meguri does not show are listed in `review/review-preferences#P5`; with every other unused key they survive unchanged (`storage/config-table#C7`, R116).
- The Collection, Maintenance and About sections hold actions and facts, no stored setting. The Note Types row is navigation.

## S3. Rules for every collection-config row (R114, R115, R116)

1. **Reading.** A row shows its key's typed read (`storage/config-table#C3`) with the default of the owning spec. A read never writes: opening a section, searching Settings (R111), switching sections and closing Settings write nothing.
2. **When to write.** A row writes only when the user commits a value that differs from the one the row shows (for converted values, the shown value: `review/review-preferences#P3`, `#P6.1`). Committing the value already shown writes nothing, even when the stored bytes are another shape that reads the same, or a number of seconds that is not a whole number of minutes.
3. **What is written.** One whole-value write of that one key (`storage/config-table#C4`, compact form `#C5`); a member of an object key is changed by `storage/config-table#C6` (`heatmap`, `backups`). No other key is written with it. This holds for the `rollover` and `collapseTime` rows too: they submit O10's single-field Settings form (`review/review-preferences#P6.1`), not O10's whole save. A write whose result is the same value as the stored one writes nothing (C4).
4. **Missing keys.** A missing key stays missing until its own row changes. Going back to the default writes the default value. Settings never removes a key.
5. **Undo.** A Settings write is applied at once, with no Save button, and records no undo step (R115). It is an op of source `UserNonUndoable` marked skip-undo (`engine/undo` M2): the earlier undo steps stay undoable and the redo history is emptied. This is the one class for every collection-config row: `editor/config-keys#K1`, `stats/heatmap-config`, `review/review-preferences#P6` and `#P6.1`, and, by this item's edit requests, `browser/config-keys#K10.2`'s two Settings keys and `package/backups`' Settings write, which now say the write clears both histories. A Settings write can still clear the history through its effects: a `rollover` change that changes the day number runs the rollover op (`sched/day-clock#D9`).
6. **Changes.** A write that changes a key reports a config change naming it (`engine/change-stream`), and only then does CloudKit tracking mark the config entity pending (`sync/cloudkit-records#K11.1`). A write that changes nothing reports nothing and leaves nothing pending.
7. **Unknown data.** Keys Meguri does not show, add-on keys and unknown members of a key it edits keep their values (`storage/config-table#C6`, `#C7`; R116, R323).
8. **Committing.** A value typed into a field is written when the field commits, never per keystroke; a toggle, picker or stepper commits at each change (`review/review-preferences#P3.1`, N11.12). A refused value writes nothing and the row shows its read again.

## S4. Opening and closing Settings (AE8)

- With any collection, opening Settings, opening each of its sections and closing it leaves every `config` row's bytes, `usn` and `mtime_secs`, `col.mod` and the CloudKit pending state as they were, records no undo step and empties neither history.
- This includes keys whose stored value reads as the default (`estTimes` `0`, `heatmap` with an unknown member), whose effective value differs from the stored one (`rollover` `30`, shown as 23), and whose seconds are not a whole number of minutes (`timeLim` `90`, shown as 1). Settings shows their reads and leaves their bytes. Anki's Preferences screen, closed without a change, saves in the last case (S5).
- When the user changes one heatmap setting, only that member of `heatmap` changes; its unknown members and `version` stay (`stats/heatmap-config`, `storage/config-table#C6`), and they are still present after a sync (AE8).

## S5. Differences from Anki

- **Where settings live.** Anki keeps its sync options, network timeout, answer keys, allowed link schemes and display options in its per-device profile and its collection options in the collection config. Meguri keeps the same split: the profile's settings are device-local (S1), the collection keys are the same keys with the same values.
- **One save on close.** Anki's Preferences screen saves when it closes, when the preferences rebuilt from its controls differ from those it read on opening. It then writes all of its collection groups (scheduling, review, editing and backups) together, as one undoable step named "Preferences". That save adds missing keys, replaces values of another shape with their typed reads and can remove `creationOffset` (`review/review-preferences#P7`). Meguri writes each row's key at once, alone, and only when it changes (S3).
- **Closing without a change.** Meguri never writes. Anki writes nothing either, unless a stored value does not survive its controls: a learn-ahead or timebox limit that is not a whole number of minutes or is above its spin box, or backup values outside theirs (`review/review-preferences#P7`). Then closing performs the whole save above.
- **Changing one setting.** Both apps leave that setting with the same value. Anki's save can also change other keys of the screen, as above; Meguri's does not.

## S6. Tests and scenarios

- AE8 is a Meguri integration test (U160): S4's walk through every section against a fixture with an unknown `heatmap` member, `estTimes` `0`, `timeLim` `90` and `rollover` `30`. Then one heatmap member is changed and a sync round trip runs against the local Anki sync server (U13), as U160's verification asks: its code and tests stay while its interface is hidden (N12.4). The CloudKit leg is `sync/cloudkit-records`' own: the config entity carries the whole value, unknown members included.
- `SettingsStoreTests` (U160) check S2's location for every row (each row declares one, R112) and S3's rules for each collection row.
- Oracle scenarios cover the stored side of the writes Anki's library can make: `conformance/scenarios/storage/settings-keys/` (`review/review-preferences#P8`).
