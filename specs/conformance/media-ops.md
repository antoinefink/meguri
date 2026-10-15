# Media ops and observations for scenarios

Spec: conformance/media-ops
Requirements: R1883, R980, R981, R982, R984, R985, R986, R2018
Anki: 26.09.3
Scenarios: conformance/scenarios/media/

The scenario vocabulary for the media folder (U31, reused by Check Media in U130 and media sync in U111): the setup section `media_files`, the ops `media_outside`, `add_media`, `trash_media`, `restore_media_trash` and `empty_media_trash`, and the observations `media_dir`, `media_trash` and `media_pending`. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The behavior they observe is `specs/media/media-files.md`.

## F1. The scenario's media folder

- Every scenario collection has a media folder named `collection.media` next to `collection.anki2` in the session directory, empty when the collection is created. On the oracle it is Anki's media folder for that collection; on Meguri the store's folder (`media/media-files#M1`).
- Each executor keeps its trash and its media sync state its own way. Only the observations below look at them.
- Handoffs carry the media folder (`dsl.md` D9.2) but not the trash or the pending state. No live scenario uses these ops yet; the area that first needs one defines what crosses.

## F2. File contents

`media_files` entries, `media_outside` entries and `add_media` give a file's bytes in exactly one of these forms:

- `text: <string>`: the string's UTF-8 bytes (`""` is an empty file);
- `fill: {size, byte?}`: `size` bytes (an integer from 0 to 209,715,200) all equal to `byte` (an integer from 0 to 255, default 0). For large files without large scenario text.

## F3. Setup section `media_files`

`setup.media_files` is a list of entries, written in list order after every other setup section (`dsl.md` D6.8). An entry is either a file, `{name, text}` or `{name, fill}`, or an empty subdirectory, `{name, dir: true}`.

- `name` is the entry's exact name in the folder: a non-empty string of at most 255 UTF-8 bytes, without `/` and U+0000, and neither `.` nor `..`. Any other character may appear, including characters the name transform removes, upper case and decomposed forms.
- Two names of one list must differ after NFC under the `unicase` comparison (`text/unicode-primitives#U2`), or the scenario is `invalid_scenario`: so `Cat.jpg` and `cat.jpg`, `é` and `e` + U+0301, or `thumbs.db` and `thumbſ.db` never appear together, and every scenario runs on case-insensitive volumes too.
- **Effect.** Each entry is created directly in the media folder through the file system, never through the media store: the store learns of it the way it learns of any outside change (`media/media-files#M7.3`). After writing, the executor sets the modification time of every file it wrote, and then of the folder itself, to the scenario clock's current instant in whole seconds (F12).

## F4. Op `media_outside`

An outside program changes the folder.

- **Argument**: a non-empty list of entries, applied in order: the F3 forms, which create the entry or replace a file's bytes in place, and `{name, remove: true}`, which deletes that file or empty subdirectory. Names follow F3. A name that is a case or normalization variant of another entry present in the folder, or a `remove` of an entry that does not exist, is `invalid_scenario` when the step runs.
- **Output**: none (`{}`).
- **Effect.** As F3: through the file system, bypassing the store; then every written file's modification time and the folder's are set to the clock's current instant (F12). The store does not see the change until it next checks the folder.

## F5. Op `add_media`

Adds a file through the media store's add path (`media/media-files#M3`): on Meguri the store's add, on the oracle Anki's add-media-file call with the desired name and bytes.

- **Argument** `{name, text}` or `{name, fill}`: `name` is the desired name, any string; the bytes as F2.
- **Output** `{add_media: {name: <stored name>}}`: the name the store chose, which a field would reference.
- **Rejections** (`dsl.md` D7.3): `invalid_name` and `ignored_name`, recorded by Meguri only (`media/media-files#M14`). Anki fails on a name the transform empties and stores an ignored name, so scenarios never use such names: a golden cannot hold the outcome.

## F6. Op `trash_media`

Moves files to the media trash through the store (`media/media-files#M10.1`; on the oracle Anki's trash-media-files call).

- **Argument** `{names: [<name>, ...]}`, at least one.
- **Output**: none (`{}`).
- **Scenarios** list only names of files that exist under exactly that name: Anki stops at a name without a file and ignores case variants on a case-sensitive volume, while Meguri resolves each name on its own (`media/media-files#M14`). Trash a file that came from `media_files` or `media_outside` only after a `media_pending` observation has registered it: Anki records a deletion only for files it has registered.

## F7. Op `restore_media_trash`

Restores every trash entry through the store (`media/media-files#M10.2`; on the oracle Anki's restore-trash call). Argument `{}`; output none (`{}`).

## F8. Op `empty_media_trash`

Deletes every trash entry permanently (`media/media-files#M10.3`; on the oracle Anki's empty-trash call). Argument `{}`; output none (`{}`).

## F9. Observation `media_dir`

`media_dir` (no arguments) reports the media folder's entries as the file system lists them: `{"media_dir": {<name>: <entry>, ...}}` with

- a regular file: `{sha1, size}`, the lower-case hex SHA-1 of its bytes and its size in bytes;
- a subdirectory: `{dir: true}` (its contents are not listed).

Names are exactly the listed names (UTF-8), in any case and form. Ignored, empty and large files appear like any other file. Scenarios create no other kind of entry.

## F10. Observation `media_trash`

`media_trash` (no arguments) reports the trash's entries: `{"media_trash": [{name, sha1, size}, ...]}`, one item per entry with the name it was trashed under, sorted by `name` (UTF-8 bytes), then `sha1`. An empty trash, or one never created, is `[]`.

## F11. Observation `media_pending`

`media_pending` (no arguments) reports what the next media sync would send:

1. The executor first registers the folder with a full comparison (`media/media-files#M7.2`, M7.3): every tracked file whose size or modification time differs from the store's record is hashed, whatever the folder's change marker says. On the oracle this is Anki's own registration of folder changes, the step a media sync begins with, run so that it cannot skip the scan.
2. The value is `{"media_pending": {<name>: <sha1> | null, ...}}`: every entry pending for the next media sync, the lower-case hex SHA-1 for a file to upload and `null` for a deletion.

The registration changes only the store's records, exactly as the next media sync's first step would.

## F12. Modification times and the clock

Registration tells a changed file by its modification time (Anki compares whole seconds), so executors never leave times to chance:

- F3 and F4 set the times of the files they write, and of the folder, to the scenario clock's current instant. Under a frozen clock, two `media_outside` steps in the same instant give the same times, and a rewrite of a file to bytes of the same size can go unseen. A scenario that wants an outside change to be seen advances the clock before the step (`advance: 1s`), as actor scenarios do before a sync (`sync-ops.md` S4.3).
- Files the stores write (`add_media`, restores) carry whatever time the file system gives them; registration compares them with what the store recorded, so their exact values never reach an observation.

## F13. Driver and executor status

- The oracle driver needs the setup section, the five ops and the three observations; until it has them, `tools/mg oracle record media` cannot write goldens. The oracle maintainer adds them (U31's oracle step) and then marks them stable in `dsl.md` D6.8 and D7.2 and `observations.md` O4.
- Meguri's executor needs the testkit model (`core/crates/meguri-testkit/src/scenario/`) and a media adapter (U31's implementer step). Until the model reads them, T0 reports the media scenarios as `invalid`; until the adapter runs them, `pending`.
