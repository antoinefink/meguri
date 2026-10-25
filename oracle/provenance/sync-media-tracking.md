# Media sync state and change tracking (U110)

Spec-writer notes for `specs/sync/media-tracking.md` and
`specs/conformance/media-sync-ops.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Black-box probes ran in the oracle
image (`meguri-oracle:26.9.3-107922635412d808`) through the `anki` Python API
with `--network none`, and the reset probes against Anki's own sync server
(`python -m anki.syncserver`) started inside the same container on loopback.

| Spec section | Anki files read | Read for |
|---|---|---|
| media-tracking T2, T3 | `rslib/src/sync/media/database/client/schema.sql`, `rslib/src/sync/media/database/client/mod.rs` | one row per name, null checksum and zero mtime for a deletion, the partial index on the pending flag, the folder marker and last media USN, the sanity count (rows with a checksum) |
| media-tracking T4 | `rslib/src/sync/media/database/client/changetracker.rs`, `rslib/src/media/files.rs` (time helpers), `rslib/io/src/lib.rs` (directory iteration keeps regular files only) | folder time read before the listing and stored at the end; whole-second file times compared, never sizes; the keep-the-flag rule for an unchanged hash; deletions for rows not seen |
| media-tracking T4.1 | `rslib/src/sync/media/syncer.rs`, `rslib/src/media/mod.rs` | registration is the first step of a media sync; the store's own writes keep the marker in step only when it was in step before |
| media-tracking T5 | `rslib/src/media/files.rs` (the normalized-name check with its Apple branch), `rslib/src/sync/media/upload.rs` (Apple drops non-NFC rows at upload) | NFD names on Apple and elsewhere |
| media-tracking T8 | `rslib/src/sync/media/syncer.rs`, `rslib/src/sync/media/upload.rs` | Anki marks uploaded rows clean unconditionally (no stamp), reads file bytes at zip time |
| media-tracking T10 | `rslib/src/sync/media/database/client/mod.rs`, `rslib/src/sync/media/changes.rs` | deletion rows stay clean after upload and are removed only when a server deletion is applied |
| media-tracking T11 | `pylib/anki/media.py` (reset deletes the `.db2` file), `qt/aqt/preferences.py` (reset at logout), `rslib/src/sync/media/database/client/mod.rs` and `syncer.rs` (reset after a sanity mismatch) | what reset forgets, when it runs |
| media-sync-ops V3 | `rslib/src/sync/media/progress.rs`, `rslib/src/progress.rs`, `proto/anki/sync.proto`, `rslib/src/backend/sync.rs`, `rslib/src/sync/media/changes.rs` | what the client counts as files and deletions up and down, and how the counts reach Python |

Black-box results the spec relies on (oracle image, Linux):

- The media database stores the folder marker in milliseconds (`1894701600000` after a stamp of 1894701600 s) and file times in whole seconds.
- A same-size rewrite in place stamped with the same instant as the folder's last check is not registered; a new file a second later makes the next registration list the folder, and the rewritten file keeps its old checksum.
- A file grown to 104,857,601 bytes and a file emptied both become rows with a null checksum, mtime 0 and the pending flag; refilled, they become pending rows with new checksums. `.DS_Store` never appears.
- A file named `cafe` + U+0301 + `.mp3` is never registered on Linux (no row, no deletion), and stays on disk under that name.
- Two clients and one server: client A uploads three files (Added 3 up); client B downloads them (Added 3 down). B's reset (`col.media.force_resync()`, which deletes `collection.media.db2`) then a media sync: Added 0/0, Removed 0/0, every row clean, last USN back to 3. With `d.gif` added and `b.jpg` deleted in B's folder before the reset: Added 1 up and 1 down (`b.jpg` downloaded again, `d.gif` uploaded), USN 4; A's next media sync downloads `d.gif`.
- Media sync needs only the login's host key and endpoint: no collection sync before it.

Black-box results on the native macOS oracle (`tools/mg oracle compare --platform apple`, 2026-10-16, after U14's merge brought the lane):

- `quick-check`, `outside-add`, `skip-transitions`, `reset-identical` and `reset-pending` equal the Linux goldens exactly (the actor scenarios ran against the sync server, client string `26.09.3,29bb700b,macos`).
- `nfd-name` differs only in both `media_quick_check` values: they hold the NFC name `caf` + U+00E9 + `.mp3` with SHA-1 `91c489cb245f6627d97c2c8a047398017897eaae`; `media_dir` still lists `cafe` + U+0301 + `.mp3`. This is T5's Apple statement (register under the NFC form, leave the name on disk), now observed rather than read.

Driver hints for the oracle maintainer (U110 role work):

- `media_quick_check`: the same registration as `media_pending` in `ops/media.py` (a media sync to an unreachable endpoint, ended by `NetworkError`), without zeroing `meta.dirMod` first; then read the rows with `dirty = 1`.
- `media_reset`: `col.media.force_resync()` (the logout path). It only unlinks `collection.media.db2`; Anki opens the media database per call, so no handle is held.
- `media_sync`: `col.sync_media(auth)` with the actor's login auth (endpoint as the actor's path, S8.1), poll `col.media_sync_status()` until inactive; `NetworkError` → `{"error": "network"}`, any other exception → `anki_error`. Counts: the final `progress.added` and `progress.removed` strings, in the worker's English locale, read `Added: ⁨U⁩↑ ⁨D⁩↓` and `Removed: ⁨U⁩↑ ⁨D⁩↓` with U+2068/U+2069 isolation marks around each number (uploaded files and downloaded files; uploaded deletions and downloaded deletions). The syncer updates these counts without throttling, so the last progress is final; parse the numbers only, and make a missing progress or a string that does not match a run error rather than guessing. The handler publishes zero counts when the sync starts, so a media sync with nothing to do still reports `Added: ⁨0⁩↑ ⁨0⁩↓`.
- `media_pending` and `media_quick_check` in actor scenarios must read the current actor's collection.

## Driver (`driver/ops/media.py`, `driver/ops/media_sync.py`, U110 oracle step)

Oracle-maintainer notes, read at tag 26.09.3; no code was copied.

| Driver part | Anki files read | Read for |
|---|---|---|
| `media_quick_check` | `rslib/src/sync/media/database/client/changetracker.rs` | the folder time compared with the marker before any listing; per file, the whole-second time compared before any hash |
| `media_reset` | `pylib/anki/media.py`, `rslib/src/sync/media/database/client/mod.rs` | `force_resync` unlinks `collection.media.db2`; the database is in WAL mode and opened per call |
| `media_sync` | `pylib/anki/collection.py`, `rslib/src/backend/sync.rs`, `rslib/src/sync/media/syncer.rs`, `rslib/src/progress.rs`, `rslib/src/sync/media/progress.rs`, `proto/anki/sync.proto`, `ftl/core/sync.ftl` | the background task and its status call (active until the thread ends; its error raised once); the progress handler stores zero counts when created and every count change unthrottled; the `added` and `removed` messages and their `up` and `down` arguments |

- libfaketime fakes file times by default: Anki's `statx` (Rust) then reads every file's and the folder's time as the frozen clock at the moment of the call, while Python's `os.stat` reads the stamped times. Under it, after any `advance` every file looks changed and a same-size rewrite that kept its time is hashed again (`quick-check` recorded `f two` at step 5). Workers now run with `NO_FAKE_STAT=1` (`entrypoint.sh`, `macos/run-native.sh`, checked at worker start); with it Anki reads the stamped times and the rewrite keeps its first hash, as the black-box probe above found. All 281 goldens compared unchanged after the switch.
- `col.tr.sync_media_added_count(up=, down=)` renders `Added: ⁨1⁩↑ ⁨2⁩↓` in the worker's locale (no digit grouping at 1000); the driver reads the counts by rendering the messages with sentinel counts and checks that the counts it read render the message again.
- Three recordings (`tools/mg oracle record sync/media/tracking`, then `compare` twice) agree exactly for all six scenarios.

## Prepared intake on 2026-10-25

The neutral U110 corpus and V1-V4 vocabulary were recovered from
0387ff4dfb51563bf8bdcc486cd25119a095aff0, with original scenario and golden
bytes retained. The earlier source sessions are preserved in the source
packet; this intake uses s-20261025T132312Z-c047b13c.

Shared grammar/schema/deviation/catalog entries were merged additively.
Current media-tracking and selected-transfer policy remain authoritative.
V3/V4 prose now distinguishes explicit synthetic actions from app authority
and historical recordings from current support or freshness. No new Anki
research, recording, pin change or implementation was performed.
