# Native Settings category links — 2026-11-11

The phone Settings root lost its driver targets after a normal app update. Its native category buttons had no accessibility identifiers. The existing Mac rows already used `settings.root.<page>`.

Source commit: `ba58e895`; starting commit: `9852d66d`. The filtered author run `cleanroom/20261111T165146Z-60217` adds the same identifier to the mobile NavigationLink. Labels, roles, destinations, appearance and interaction stay native.

Independent review: `cleanroom/20261111T165430Z-62946`, no P0–P2 findings. Its metadata-propagation concern was then checked on the physical phone. In `drive/20261111T165554Z-65067`, each category has one button identifier. The Account & Sync link opens its native destination. The exact updated root render was inspected. All 14 interactions completed, including the read-only collection summary and Cancel. The saved review count remained 48 after the update.

Focused checks: `lint-ui/20261111T165304Z-61108`, `private/20261111T165330Z-62134`, `similarity/20261111T165558Z-65332` and phone preparation `run/20261111T165437Z-63294` pass.

The drive receipt remains failed: its hierarchy checker reports 225 issues across captures, including unnamed native cells and overlapping combined labels. Successful navigation does not waive these findings. iPad coverage and the required integration gate remain open. No unit or milestone acceptance is claimed.
