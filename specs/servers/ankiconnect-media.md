# AnkiConnect media actions and the external media folder

Spec: servers/ankiconnect-media
Requirements: R1761, R1762, R1769, R1770, R1771, R1772, R1773; AE79 (core part); supports R1779, R1780, R1789, R1790
Anki: 26.09.3
Scenarios: none yet. The transcript format is `servers/transcripts`, but its harness (U136) is not built; X14 lists the planned scenarios of `conformance/scenarios/servers/ankiconnect/media/` as request and reply tables, and the external-folder tests, which are Meguri's own (no AnkiConnect counterpart).

The media actions of Meguri's AnkiConnect-compatible server (`storeMediaFile`, `retrieveMediaFile`, `getMediaFilesNames`, `deleteMediaFile`, `getMediaDirPath`), the `audio`, `video` and `picture` attachments that note actions accept, the rules for the `path` and `url` content sources, and the external media folder that `getMediaDirPath` returns and whose files become collection media. The interface targeted is AnkiConnect API version 6 as served by the AnkiConnect add-on build of 2025-11-09 in Anki 26.09.3 (the build U136 pins). The external folder, the readable folders and the source restrictions are Meguri's (R1770–R1773): AnkiConnect returns Anki's own media folder and reads any path.

Media storage itself (names, dedupe, conflict renames, trash, staging, registration, sync state) is `media/media-files` and `media/media-journal` (U31); this file says which of their paths each action takes. The envelope, read-only gate, wait and activity log are `servers/ankiconnect-envelope`; the note actions that carry attachments are `servers/ankiconnect-actions`; the shared remote fetcher is `servers/host-security` (U135); the Settings UI, folder grants and the file-system watcher are U144's.

## X1. Terms

- **Collection media folder**: `collection.media/` (`media/media-files#M1`).
- **External folder**: the folder the user chose in Meguri's AnkiConnect settings for `getMediaDirPath` (R1770): the `path` of the config member `ankiconnect.media_dir` (`servers/host-security#H10.2`), set by U144 from the resolved security-scoped bookmark, which U144 keeps open. There is at most one. None is set until the user picks one (`ankiconnect.media_dir` null). Rust never reads the `bookmark` member.
- **Readable folders**: the `path` of each entry of `ankiconnect.readable_folders` (R1772).
- **Allowed roots**: the external folder, the collection media folder and each readable folder, each **canonicalized** (made absolute with every symbolic link resolved, `.` and `..` removed, each component in the spelling the file system reports for the directory entry) when the config is applied. A folder that cannot be canonicalized, or cannot be opened as a directory at that time (missing, or no access because U144 could not resolve its bookmark), is not an allowed root until the next apply. U144 applies the config again whenever it resolves or loses a bookmark, so this is how a stale bookmark reaches Rust (X11.7).
- **Inside a root**: a canonical path whose components start with all the components of the canonical root and have at least one more. The root itself is not inside it. Components compare byte for byte; both sides are in the file system's own spelling, so a case-insensitive volume compares the stored spellings.
- **MD5 hex** of bytes: their MD5 digest as 32 lower-case hexadecimal digits.
- **Content source**: one of the members `data`, `path`, `url` of a `storeMediaFile` request or an attachment object (X3).
- **Flat name**: a text containing no `/`, no `\` and no NUL, and not equal to `.` or `..`. (A name such as `a..b.mp3` is flat.)

## X2. Actions

| Action | Parameters | Result | Kind | `affected` (R1780, `servers/ankiconnect-envelope#E16`) |
|---|---|---|---|---|
| `storeMediaFile` | `filename`, `data`, `path`, `url`, `skipHash`, `deleteExisting` | stored name, or null | write | files written (0 or 1) |
| `retrieveMediaFile` | `filename` | base64 text, or false | read | 0 |
| `getMediaFilesNames` | `pattern` (default `*`) | list of names | read | 0 |
| `deleteMediaFile` | `filename` | null | write | files trashed (0 or 1) |
| `getMediaDirPath` | none | absolute path | read | 0 |

Write actions are refused in read-only mode (R1779) by the envelope. Each write action is one `LocalApi` op of the media store (`media/media-journal#J5`), so media sync and Check Media see it like an editor change (R1789).

## X3. Content sources

### X3.1 Choosing the source

Each of `data`, `path`, `url` is a string or `null`; another type is `invalid parameter: <name>` (`servers/ankiconnect-envelope#E12.2`). The first of the three that is a non-empty string is used: a missing member, `null` and the empty string count as absent. When all three are absent: error `You must provide a "data", "path", or "url" field.` A later member is never tried when an earlier one fails.

| Source | Bytes |
|---|---|
| `data` | the base64 decoding of the text (X3.2) |
| `path` | the file at that path (X3.3) |
| `url` | the body of the download (X3.4) |

### X3.2 `data`

Standard base64 (RFC 4648 §4 alphabet, `=` padding), decoded in two steps:

1. Every character that is not `A`–`Z`, `a`–`z`, `0`–`9`, `+`, `/` or `=` is removed (line breaks, spaces, and also the URL-safe `-` and `_`).
2. What remains must be a run of alphabet characters of length L followed by padding, with L mod 4 = 0 and no `=`, L mod 4 = 2 and exactly `==`, or L mod 4 = 3 and exactly `=`. It decodes to floor(L × 3 / 4) bytes. Anything else (no padding where it is needed, `=` inside the text, too many `=`, more text after the padding, L mod 4 = 1): error `invalid base64 data` (Meguri wording, X12).

| `data` | Bytes |
|---|---|
| `YQo=` | `61 0a` |
| `YQ==` | `61` |
| `YWJj` | `61 62 63` |
| `Y Q o =`, `YQo=` + LF | `61 0a` |
| `YQo`, `YQ` (unpadded) | error |
| `YQ=o`, `=YQo` | error |
| `YQo==`, `YQ==YQ==` | error (AnkiConnect decodes both; X13 `ankiconnect-media-base64`) |
| `-_8=` | error (after step 1, `8=` is not valid) |

The empty string never reaches decoding: X3.1 treats it as absent. A `data` made only of removed characters (a single space) decodes to no bytes, which is valid.

### X3.3 `path` (R1772)

The checks run in this order; the first that fails gives the error. `<path>` is the `path` member as given.

1. The path must be absolute; otherwise `cannot read file: <path> (not permitted)`.
2. **Root check.** Split the path into components. Resolve the longest prefix that exists, following symbolic links, to its canonical form (X1), and append the remaining components. When a remaining component is `.` or `..`, or the result is not inside an allowed root (X1): `cannot read file: <path> (not permitted)`. So a path outside every root is refused the same way whether or not it exists (`/etc/hosts`, `/etc/does-not-exist`), and nothing tells a caller whether a file outside the roots exists.
3. When the path does not exist (some components remained in step 2): `cannot read file: <path> (not found)`. Only paths inside a root reach this.
4. **Open and verify.** Open the canonical path for reading, not following a symbolic link in its last component. Ask the system for the opened file's own path (the descriptor's path, in the file system's spelling) and check it again against the allowed roots, as in step 2; a component replaced by a link between step 2 and the open is caught here. A failure: `cannot read file: <path> (not permitted)`.
5. When the opened file is not a regular file: `cannot read file: <path> (not a file)`.
6. When its size is above 268,435,456 bytes (the AnkiConnect body limit, `servers/host-security#H8`): `cannot read file: <path> (too large)`.
7. Read the bytes from the opened descriptor. A read failure: `cannot read file: <path> (<reason>)`, `<reason>` the system's description.

So a symbolic link inside a readable folder that points outside every root is refused, and a root configured through a symbolic link (`/tmp` for `/private/tmp`) accepts its children, because both sides are canonical. This holds in every build, sandboxed or not.

### X3.4 `url` (R1773)

Fetched with the shared fetcher of `servers/host-security#H13`, with AnkiConnect's limits (30 s, 5 redirects, the 256 MiB body limit of H8). The fetcher reports its failure kinds; this file gives their texts:

| Failure | Error |
|---|---|
| a scheme other than `http` or `https`, for the URL or for a redirect target | `cannot download <url> (only http and https are allowed)` |
| a 6th redirect would be needed | `cannot download <url> (too many redirects)` |
| the download, from the start of the fetch (name resolution included) to its last body byte, takes more than 30 s (`servers/host-security#H13`) | `cannot download <url> (timed out)` |
| the body passes 268,435,456 bytes | `cannot download <url> (too large)` |
| a final status that is not 2xx | `<url> download failed with return code <status>` (AnkiConnect's text; `<status>` in decimal) |
| any other failure (name resolution, connection, TLS) | `cannot download <url> (<reason>)` |

`<url>` is the member as given. The body is taken as is; no content type is checked. A 2xx status other than 200 is accepted (H13); AnkiConnect refuses it (X13 `ankiconnect-media-url-limits`).

### X3.5 When the bytes are obtained

Decoding `data`, reading a `path` and downloading a `url` happen before the action's op opens, on the request's worker and never on the collection writer (`engine/op-model#O2`), so a 30 s download never holds up collection writes, the UI's included. Each source yields either its bytes or its error text (X3.2 to X3.4). The op then only names and writes files (X4 steps 3 to 5, X10.2) and builds fields; it reads no file outside the collection media folder and makes no network request.

## X4. `storeMediaFile` (R1769)

Parameters: `filename` (string, required), the content sources, `skipHash` (string or null, default null), `deleteExisting` (boolean or null, default `true` when missing; `null` counts as false, as in AnkiConnect).

1. Get the bytes (X3), before the op (X3.5).
2. When `skipHash` is a string equal to the MD5 hex of the bytes (exact comparison, so upper-case hex never matches): the result is `null` and nothing is written or trashed. No op runs.
3. Take N = transform(`filename`) and its lowercase candidate L, as `media/media-files#M3` steps 1 and 3 define them. M3's refusals (N empty, N ignored) are errors with X12's texts, before anything is looked up. N is a flat name (`text/media-filenames#F1`), so nothing below ever looks outside the collection media folder.
4. When `deleteExisting` is true: resolve N (`media/media-files#M5`), and when it resolves to nothing, resolve L. When one resolves to a file: if that file's content equals the bytes, the result is that file's name and nothing is written (R1571); otherwise the file goes to the trash (`media/media-files#M10.1`, called with the resolved name), and step 5 follows in the same op.
5. Add the bytes under the desired name `filename` (`media/media-files#M3`): the name is transformed and lowercased, identical content already present is reused, and other content under a taken name gets the hash name (M4). The result is the stored name.

No lookup, trash call or write ever receives the untransformed `filename`. The result is the name to put in fields (R983); it can differ from `filename`.

| Present before | Request | Result | After |
|---|---|---|---|
| none | `{filename: "note.txt", data: "YQo="}` | `"note.txt"` | `note.txt` = A |
| none | `{filename: "Clip.MP3", data: <A>}` | `"clip.mp3"` | `clip.mp3` = A |
| `clip.mp3` = A | `{filename: "clip.mp3", data: <B>}` | `"clip.mp3"` | `clip.mp3` = B; A in the trash |
| `clip.mp3` = A | `{filename: "clip.mp3", data: <B>, deleteExisting: false}` | `"clip-<sha1 of B>.mp3"` | both files |
| `clip.mp3` = A | `{filename: "clip.mp3", data: <A>}` | `"clip.mp3"` | unchanged, nothing trashed |
| none | `{filename: "x.mp3", data: <A>, skipHash: <MD5 hex of A>}` | `null` | nothing |
| none | `{filename: "x.mp3", data: ""}` | error `You must provide a "data", "path", or "url" field.` | nothing |

## X5. `retrieveMediaFile` (R1769)

`filename`: a string. When it is not a flat name (X1): error `invalid media file name: <filename>` (X12), before any file is looked at (`../collection.anki2`, `a/b.mp3`). Otherwise resolve it (`media/media-files#M5`): a file gives its bytes in standard base64 with padding; nothing gives `false`.

## X6. `getMediaFilesNames` (R1769)

`pattern`: a string, default `*`. When it is not a flat name: error `invalid media file name: <pattern>`. The empty pattern is flat and matches no name, so its result is `[]`. The result is the names of the regular files directly in the collection media folder whose name the pattern matches, sorted by UTF-8 bytes ascending (X13 `ankiconnect-media-names-order`). Subdirectories, the trash and staging are never listed.

Matching is a shell-style glob over the whole name, case-sensitive:

- `*` matches any run of characters, the empty run included; `?` matches one character;
- `[` … `]` matches one character of the set it encloses; a set starting with `!` matches one character not in it; `a-z` inside a set is a range of code points; a `]` right after `[` or `[!` belongs to the set; a `[` with no closing `]` is a literal `[`;
- every other character matches itself;
- a name starting with `.` matches only a pattern starting with `.`.

| Folder | `pattern` | Result |
|---|---|---|
| `a.mp3`, `b.mp3`, `c.jpg`, `.hidden.mp3` | `*.mp3` | `["a.mp3", "b.mp3"]` |
| the same | `[ab].*` | `["a.mp3", "b.mp3"]` |
| the same | (missing) | `["a.mp3", "b.mp3", "c.jpg"]` |
| the same | `A.MP3` | `[]` |

## X7. `deleteMediaFile` (R1769)

`filename`: a string. Not a flat name: error `invalid media file name: <filename>`. Otherwise one op trashes it (`media/media-files#M10.1` `trash([filename])`): a file it resolves to moves to the trash and its deletion becomes pending for media sync; a name that resolves to nothing changes nothing. The result is `null` either way.

## X8. `getMediaDirPath` (R1770)

- With an external folder configured (`ankiconnect.media_dir` not null) that is an allowed root (X1): its `path` as configured, without a trailing `/`. This holds also while its scans are suspended (X11.7): files written there still reach collection media through the note hook (X11.6).
- Otherwise (none configured, or a folder that is not an allowed root, such as one whose bookmark U144 could not resolve): error `no media folder is set up: choose one in Meguri Settings › AnkiConnect › Media Folder` (X12; the label is confirmed against U144's screen, X15 Q6).

The collection media folder is never returned: it lives in the app container, and tools writing there would meet macOS privacy prompts.

## X9. Name-only actions and other names

Only `retrieveMediaFile`, `getMediaFilesNames` and `deleteMediaFile` apply the flat-name check (R1769). `storeMediaFile` and attachments never use `filename` as a path: X4 step 3 transforms it into a flat name (`text/media-filenames#F1` removes `/` and `\`) before any lookup, trash call or write, so `a/b.mp3` is stored as `ab.mp3` and `../collection.anki2` can neither be read, replaced nor trashed through it.

## X10. Note attachments (R1761, R1762)

`addNote`, `addNotes`, `updateNoteFields` and `updateNote` (with `fields`) accept, in the note object, the members `audio`, `video` and `picture`, each `null`, one attachment object or a list of them. (`guiAddCards` takes the same members; `servers/ankiconnect-gui`.) The `canAdd*` actions ignore them (`servers/ankiconnect-actions` A7.7).

### X10.1 The attachment object

| Member | Meaning |
|---|---|
| `filename` | the desired name, as in X4 (required) |
| `data`, `path`, `url` | the content sources (X3) |
| `skipHash` | as in X4 |
| `deleteExisting` | boolean or null, as in X4, but **default false**: a missing or `null` member does not trash an existing file |
| `fields` | optional list of field names to append the reference to |

### X10.2 Processing

The attachments are processed after the fields are set and before the note is checked (`servers/ankiconnect-actions` A7.2 step 8, A8.1 step 3): first every `audio` entry, then every `video` entry, then every `picture` entry, each list in its order. A `null` entry is skipped; an entry that is neither `null` nor an object fails the note with `invalid parameter: audio` (`video`, `picture`). Before the op opens, every entry's source is read (X3.5), giving bytes or an error text per entry; an entry whose own members fail (below) reads nothing. Then, inside the op, for each entry:

1. Store the file as `storeMediaFile` does (X4 steps 2 to 5 with the entry's members and its pre-read bytes), staged (`media/media-files#M13`).
2. On success with a stored name `N` (not `null` from `skipHash`) and a `fields` member that is a list of strings: for each listed name that is exactly the name of one of the note's fields (case-sensitive, also for `addNote`, whose `fields` member matches case-insensitively), append to that field's current value `[sound:N]` for audio and video, or `<img src="N">` for a picture. No separator is added. Names that match no field are skipped.
3. On failure (the entry's pre-read error of X3, an error of X4, or a member error of the entry itself, such as a missing `filename`: `missing parameter: filename`, `servers/ankiconnect-actions` A1), the error text with `&`, `<`, `>` written as `&amp;`, `&lt;`, `&gt;` (in that order) is appended to each listed field, as in step 2. The note does not fail and the other attachments go on (R1762). When this entry has no `fields` member, the note fails instead with `missing parameter: fields`, and with `invalid parameter: fields` when `fields` is not a list of strings (AnkiConnect fails here too, with its runtime's text; X15 Q7). For `addNote` and `updateNote*` that is the action's error; for `addNotes` it is that note's entry in the error list (`servers/ankiconnect-actions` A7.6).

On success, a `fields` member that is absent or not a list of strings appends nothing and is not an error.

A `skipHash` match appends nothing. The X3.5 reads of an action that fails before step 1 are discarded. When the action later fails (a duplicate, a missing deck, any note of an `addNotes` batch), its op rolls back and none of its staged files is left (`servers/ankiconnect-actions` A18 `ankiconnect-failed-add-media`).

| Note `fields` | Attachments | Field values after |
|---|---|---|
| `Term`: `word` | `audio: [{filename: "a.mp3", data: <A>, fields: ["Term"]}, {filename: "b.mp3", data: <B>, fields: ["Term", "Gloss"]}]` | Term `word[sound:a.mp3][sound:b.mp3]`, Gloss `[sound:b.mp3]` |
| `Term`: `word` | `picture: {filename: "Cat.JPG", data: <C>, fields: ["Gloss"]}` | Gloss `<img src="cat.jpg">` |
| `Term`: `word` | `audio: {filename: "x.mp3", url: "ftp://h/x", fields: ["Gloss"]}` | Gloss `cannot download ftp://h/x (only http and https are allowed)`; the note is added |
| `Term`: `word` | `audio: {filename: "x.mp3", url: "ftp://h/x"}` | error `missing parameter: fields`, nothing added |
| `Term`: `word` | `audio: {filename: "x.mp3", data: <A>, fields: ["term"]}` | unchanged (case-sensitive) |

### X10.3 The external folder before the commit

Every note add or update of this file and of `servers/ankiconnect-actions` (`addNote`, `addNotes`, `updateNoteFields`, `updateNote`) runs X11.6's hook on its final field values inside its op, before the commit.

## X11. The external media folder (R1770, R1771, AE79)

Files that tools write into the external folder become collection media under their own names: copied, never moved, and never synced from the external folder itself.

### X11.1 The index

The integrations store (`servers/host-security#H10.1`, tables added by U143's migration) holds one index for the external folder: its **index path**, and one entry per external name. The index path is the `path` given to the last `media_folder_granted` call (X11.2); with no grant yet it is null.

| Column | Meaning |
|---|---|
| `name` | the directory entry's name, exactly as listed (primary key) |
| `state` | `baseline`, `pending`, `ingested` or `conflict` |
| `size` | the file's size in bytes at the entry's last observation |
| `mtime_ns` | the file's modification time at that observation, in nanoseconds since the Unix epoch |
| `stored_name` | the collection media name the file was last ingested under (X11.4); null when it was never ingested |

States:

- `baseline`: the file was in the folder when it was granted; it is copied only when a note references it (X11.6).
- `pending`: observed once at (`size`, `mtime_ns`), not yet confirmed by a second observation.
- `ingested`: its bytes at (`size`, `mtime_ns`) are in collection media under `stored_name`.
- `conflict`: a scan found unrelated content under its name in collection media and copied nothing (X11.4).

**Suspension.** Scans run only while the configured external folder (X1) is not null, is an allowed root, and its `path` equals the index path byte for byte. Otherwise a scan is **suspended**: it reads nothing, writes nothing and reports `suspended` (X11.8). A configuration change never clears or rewrites the index; only a grant replaces it.

**When the index is written.** Index changes are written in one integrations-store transaction, one writer at a time (scans and hook updates are serialized). A change that goes with a media write is written only after the media op that made the write has committed, and is dropped when that op rolls back. A crash between the commit and the index write leaves the entry as it was; the next scan repeats the ingest, which M6 step 2 makes a no-op, and then writes the index.

### X11.2 Granting a folder (baseline)

`media_folder_granted(path)` is called by U144 only right after the user picks the folder in an open panel, never when a bookmark is resolved at launch. It lists the folder and replaces the whole index: the index path becomes `path`, and every regular file then directly in the folder gets an entry `baseline` with its size and modification time. Nothing is copied. So a folder that already holds documents never sends them to collection media or to sync. When the folder cannot be listed, the call fails and the index is unchanged.

A `baseline` entry stays `baseline`, also when its file changes, until a note references it (X11.6); it is then ingested and becomes `ingested`, and later changes re-ingest it.

The grant and the config apply that sets `ankiconnect.media_dir` to the same folder may come in either order; U144 makes both after a pick. Until both have happened, the configured path and the index path differ and scans are suspended, so no file of the new folder is ingested before its baseline exists.

| Sequence | Between the two calls | After both |
|---|---|---|
| apply (`media_dir` = P2), then grant(P2) | config P2, index P1 (or null): suspended | scans of P2 against P2's baseline |
| grant(P2), then apply (`media_dir` = P2) | config P1, index P2: suspended | the same |
| launch: the bookmark of P2 resolves to P3 (folder moved or renamed), apply P3, no grant | config P3, index P2: suspended | stays suspended until the user picks the folder again; the note hook still works (X15 Q9) |
| the user clears the folder (`media_dir` null) | | no scans; the index is kept, and a later grant replaces it |

### X11.3 Scan

`ingest_external_media()` (X11.8) brings collection media in line with the external folder. Running it again right after a scan that left no `pending` entry, with no change in the folder, writes nothing: no media write, no journal change, no index write.

It considers each regular file directly in the folder; subdirectories and their contents, symbolic links, names starting with `.` and names the media store ignores (`media/media-files#M8`) are skipped. "Same" below means the same size and modification time as the entry's. The first row that matches gives the action:

| Entry | File now | Action | Entry after |
|---|---|---|---|
| none | any | nothing copied | `pending` with the file's values |
| `pending` | same | **ingest** (X11.4) | `ingested` (or `conflict`, X11.4) with those values and `stored_name` |
| `pending` | other | nothing copied | `pending` with the new values |
| `ingested` | same | nothing | unchanged |
| `ingested` | other | nothing copied | `pending` with the new values, `stored_name` kept (re-ingested once stable) |
| `baseline` | same | nothing | unchanged |
| `baseline` | other | nothing copied | `baseline` with the new values |
| `conflict` | same | nothing | unchanged |
| `conflict` | other | nothing copied | `pending` with the new values (another try once stable) |

A file is therefore ingested only after two scans saw the same size and modification time, so a file still being written is never copied half-done. An entry whose file is gone is removed from the index; the copy in collection media stays (removing a file from the external folder never removes collection media). A rename in the folder is a removal and a new file.

### X11.4 Ingest

Ingest writes the file's bytes into collection media in a media-store op of source `LocalApi`, under the **target name**:

- an entry with a `stored_name` (re-ingest of a file ingested before): that name;
- otherwise the external name, when it is normalized (`text/media-filenames#F1`); otherwise its transformed name. An empty or ignored transformed name (M8) is not ingested: the entry stays as it was and a `skipped` notice is given (X11.9).

The write is `media/media-files#M6` (exact-name write: no lowercasing and no hash name, unlike X4), with this guard on M6 step 3:

| Collection media holds, under a case variant of the target name | Scan | Note hook (X11.6) |
|---|---|---|
| nothing | written under the target name | the same |
| the same content | nothing written (M6 step 2); the present file's name is the stored name | the same |
| other content, and the target name is the entry's `stored_name` | replaced: the old bytes go to the trash (M6 step 3, M10.1), the entry becomes pending for sync | the same |
| other content, and the entry has no `stored_name` (an unrelated collection file) | nothing written; the entry becomes `conflict`; a `conflict` notice | the bytes are added by `media/media-files#M3` with the target name as the desired name (so under M3's hash name, M4); the note's references are rewritten to the stored name (X11.6 step 3); a `renamed` notice |

So an external file never replaces a collection file that did not come from it: an unrelated `clip.mp3` already in collection media keeps its bytes for every note that uses it. A written file counts as media like an editor add: it is pending for the next media sync and Check Media sees it as used once a note references it (R1771). A file written under a name other than its external name gives a `renamed` notice.

### X11.5 Triggers

- **File-system events.** U144's watcher on the external folder calls the scan off the main thread with an event latency of 0.3 s and a debounce of 0.5 s, waiting at most 1 s while events keep coming, so a file written once is copied within 2 s (R1771): the first scan records it, the confirming scan copies it.
- **Confirming scan.** A scan whose result has `rescan` true (X11.8) is followed by one more scan 0.5 s later, run by its caller (U144's watcher in the app), whatever triggered the first one.
- **Launch.** Once the collection is open and the folder's access is resolved, one scan runs; it compares with the persisted index, so files written while Meguri was closed are picked up (by the confirming scan). Launch never records a new baseline.
- **Notes.** X11.6.
- Linux and Docker tests call the scan directly.

### X11.6 The hook in note writes

Before a note add or update of X10.3 commits, inside its op:

1. Extract the media names its final field values reference (`text/media-references#M4`).
2. For each name that does not resolve in collection media (`media/media-files#M5`, staged files of the op included) and that is a regular file directly in the external folder (looked up by that exact name, on the volume's rules), ingest it at once (X11.4 in its "note hook" column, staged in the op), whatever its index state (`baseline`, `pending`, `conflict` or none). No second observation is needed here: the tool referenced it, so it is complete. The hook runs while scans are suspended too, as long as an external folder is configured and is an allowed root.
3. When step 2 stored the file under a name other than the referenced one (a transformed name, or M3's name after a conflict), every reference in the note's fields that used the referenced name is rewritten to the stored name.
4. The index changes (entry `ingested` with the file's current size and modification time and its `stored_name`) and the notices are kept with the op: written and posted only after the op commits (X11.1), dropped when it rolls back.

The file's bytes are read for this step inside the op; the external folder is local and the hook reads only files that notes reference. Names that resolve already, and names not in the external folder, are left alone. `addNotes` runs the hook for every note of the batch; a rolled-back batch leaves no ingested file and no index change, so later scans still copy a non-baseline file.

| External folder | Request | Effect |
|---|---|---|
| `clip.mp3` written just now (not yet scanned) | `addNote` with Term `[sound:clip.mp3]` | `clip.mp3` copied into collection media in the add's op; pending for sync; Check Media reports no missing file (AE79) |
| `Clip.mp3` | `addNote` with `[sound:Clip.mp3]` | stored as `Clip.mp3` (case kept) |
| `doc.pdf` (baseline) | `addNote` with `<img src="doc.pdf">` | `doc.pdf` ingested; entry `ingested` |
| `a?b.png` (a name F1 changes) | `addNote` with `<img src="a?b.png">` | stored as `ab.png`; the field holds `<img src="ab.png">`; a `renamed` notice |
| `pic.png`, while collection media holds an unrelated `pic.png` | `addNote` with `<img src="pic.png">` | the name resolves already, so nothing is ingested and the note uses the collection file |
| `a?b.png`, while collection media holds an unrelated `ab.png` | `addNote` with `<img src="a?b.png">` | stored under M3's hash name of `ab.png`; the field holds that name; a `renamed` notice |

### X11.7 Configuration changes

- Choosing another external folder takes a grant (X11.2), which replaces the index; until the grant and the apply have both happened, scans are suspended.
- Clearing the setting stops the scans and the hook; collection media keeps every file already copied; the index is kept until the next grant.
- When U144 cannot resolve the bookmark, it applies the config again and the folder is no allowed root (X1): scans and the hook stop and `getMediaDirPath` fails (X8) until a later apply finds the folder accessible.

### X11.8 Interface

Two core operations, exported to Swift and called directly by Linux tests. Neither waits for the collection.

| Operation | Input | Result | Failures (nothing changes) |
|---|---|---|---|
| `media_folder_granted` | `path`: the absolute path of the picked folder | the number of `baseline` entries recorded | `not_a_directory`, `unreadable` (the folder cannot be listed), `store_error` (the integrations store cannot be written) |
| `ingest_external_media` | none | `status`, `rescan`, `ingested`, `notices` (below) | `store_error` |

`ingest_external_media` result members:

| Member | Value |
|---|---|
| `status` | `done`; `no_folder` (no external folder configured, or it is not an allowed root); `suspended` (X11.1); `folder_unavailable` (listing the folder failed); `collection_unavailable` (the collection is not available, `engine/lifecycle` L6, or the media op could not run) |
| `rescan` | true when an entry is `pending` after the scan, or the status is `collection_unavailable`; the caller then scans again 0.5 s later (X11.5) |
| `ingested` | the number of files this scan wrote into collection media |
| `notices` | the X11.9 notices of this scan, in name order |

With `no_folder`, `suspended` or `folder_unavailable`, nothing was written. With `collection_unavailable`, no file was copied and no entry was confirmed; only index changes that need no media write (new or updated `pending` and `baseline` values, removed entries) were written.

### X11.9 Ingest notices

A notice reports an external file that did not land in collection media under its own name: `{time, external name, stored name or null, kind}`, `kind` one of `renamed` (written under another name), `skipped` (its transformed name is empty or ignored) and `conflict` (a scan found unrelated content under its name). Notices go out as a host event to the app (`servers/host-security#H11.1` "media ingest notices", `MediaIngestNotices` of H20, with the notices as a JSON list of `{"time": <Unix ms>, "external_name": <string>, "stored_name": <string or null>, "kind": <kind>}`) after the write they describe has committed, from scans and from the note hook alike, and are returned by the scan (X11.8). They are not entries of the request activity log (`servers/ankiconnect-envelope#E16`), which holds one entry per request and no names; how Settings shows them is U144's, and whether the activity log should list them is X15 Q8.

## X12. Meguri error texts

Texts without an AnkiConnect counterpart (proposed wording, X15 Q1). Implementers take them only from here.

| Where | Text |
|---|---|
| X3.2 | `invalid base64 data` |
| X3.3 | `cannot read file: <path> (not permitted)` (R1772), `cannot read file: <path> (not found)`, `cannot read file: <path> (not a file)`, `cannot read file: <path> (too large)`, `cannot read file: <path> (<reason>)` |
| X3.4 | `cannot download <url> (only http and https are allowed)`, `cannot download <url> (too many redirects)`, `cannot download <url> (timed out)`, `cannot download <url> (too large)`, `cannot download <url> (<reason>)` |
| X5, X6, X7 | `invalid media file name: <filename>` |
| X4: M3 `invalid_name` | `invalid media file name: <filename>` |
| X4: M3 `ignored_name` | `media file name is reserved: <filename>` |
| X8 | `no media folder is set up: choose one in Meguri Settings › AnkiConnect › Media Folder` |

X3.4's status failure keeps AnkiConnect's text `<url> download failed with return code <status>`, and X3.1's keeps `You must provide a "data", "path", or "url" field.`

## X13. Deviations from AnkiConnect

Proposed `conformance/deviations.yaml` entries for the U136 replay. "Scenario" names the X14 row whose golden shows the difference and whose Meguri expectation asserts Meguri's behavior.

| Proposed id | AnkiConnect | Meguri | Scenario |
|---|---|---|---|
| `ankiconnect-media-dir` | `getMediaDirPath` returns Anki's media folder | the external folder, or an error (X8) | `media/dir` |
| `ankiconnect-media-flat-names` | `retrieveMediaFile` takes the last path component; `deleteMediaFile` and `getMediaFilesNames` follow separators and `..` | error (X5–X7) | `media/read` |
| `ankiconnect-media-path-roots` | `path` reads any file | allowed roots only (X3.3) | `media/path` |
| `ankiconnect-media-url-limits` | 10 s timeout, the client library's redirect rules, only status 200 accepted | 30 s, 5 redirects, `http`/`https` only, any 2xx accepted, 256 MiB cap (X3.4) | `media/attach-fail`; the stub tests below |
| `ankiconnect-media-names-order` | `getMediaFilesNames` in directory order, also matching directories; the empty pattern gives `[""]`; on a case-insensitive volume a pattern without wildcards matches any case | regular files only, sorted; the empty pattern gives `[]`; matching is case-sensitive everywhere (X6) | `media/names` |
| `ankiconnect-media-delete-existing` | with `deleteExisting`, the existing file is trashed by the exact given name and the same content is written again | the transformed name resolved by M5; identical content is kept, nothing trashed (X4 step 4) | `media/store-conflict` |
| `ankiconnect-media-base64` | the runtime's lenient decoder: decoding stops at the first complete padding, extra `=` are accepted | X3.2's rule: such texts are errors | `media/store-data` |
| `ankiconnect-media-error-text` | the runtime's error texts for decoding, file and download errors, also where they are appended to fields (X10.2 step 3) | X12's | `media/attach-fail`, `media/path` |

The case rules of `media/media-files#M14` apply to every stored name and need their own entries where a golden shows them. The failed-add and `canAdd*` differences are `servers/ankiconnect-actions` A18's (`ankiconnect-failed-add-media`: `media/attach-rollback`).

## X14. Scenarios

Requests are `version` 6 without key or Origin; replies shown unwrapped. Byte contents: A = `61 0a` (base64 `YQo=`), B = `62 0a` (`Ygo=`). `Pair` and deck `D` are `servers/ankiconnect-actions` A20's. Scenarios that add a note end with `observe: [db_dump(notes, cards), media]` (R1948, `servers/transcripts` T14). A row marked "(Meguri)" asserts Meguri's side of the X13 entry it names.

| Scenario | Setup | Request | Reply and media after |
|---|---|---|---|
| `media/store-data` | empty media | X4's rows 1, 2, 6, 7; then `storeMediaFile {filename: "y.txt", data: "YQo=="}` | as X4; the last: error `invalid base64 data` (Meguri, `ankiconnect-media-base64`) |
| `media/store-conflict` | `clip.mp3` = A | X4's rows 3, 4, 5 | as X4 (trash contents checked by a `media_pending`-style observation) |
| `media/store-traversal` | `a.mp3` = A | `storeMediaFile {filename: "../collection.anki2", data: "Ygo="}` | `"..collection.anki2"` (F1 removes the `/`); stored in collection media; the collection file untouched, nothing trashed |
| `media/read` | `a.mp3` = A | `retrieveMediaFile {filename: "a.mp3"}`; `{filename: "missing.mp3"}`; `{filename: "../collection.anki2"}`; `{filename: "a/b.mp3"}` | `"YQo="`; `false`; error `invalid media file name: ../collection.anki2` (Meguri, `ankiconnect-media-flat-names`); error `invalid media file name: a/b.mp3` (the same) |
| `media/names` | X6's folder | X6's rows; `getMediaFilesNames {pattern: ""}` | as X6 (order, Meguri `ankiconnect-media-names-order`); `[]` |
| `media/delete` | `a.mp3` = A | `deleteMediaFile {filename: "a.mp3"}`; again | `null`, file in the trash, deletion pending; `null`, nothing |
| `media/dir` | no external folder | `getMediaDirPath` | error X8 (Meguri, `ankiconnect-media-dir`) |
| `media/attach` | `Pair`, deck `D` | `addNote` with X10.2's rows 1 and 2 | fields as X10.2; files stored |
| `media/attach-fail` | the same | X10.2's rows 3 and 4 | as X10.2 (row 3's appended text: Meguri, `ankiconnect-media-error-text`) |
| `media/attach-rollback` | the same, with a note `x` already in `D` | `addNote` with Term `x` (a duplicate) and `audio: {filename: "n.mp3", data: "Ygo=", fields: ["Gloss"]}` | error `cannot create note because it is a duplicate`; no `n.mp3` in media (Meguri, `ankiconnect-failed-add-media`) |
| `media/attach-notes` | the same | `addNotes` with two notes, the second's `audio` lacking `fields` and failing (`url: "ftp://h/x"`) | error `['missing parameter: fields']`; nothing added |
| `media/skip-hash` | the same | `addNote` with `audio: {filename: "x.mp3", data: <A>, skipHash: "60b725f10c9c85c70d97880dfe8191b3", fields: ["Term"]}` | note added, Term without a reference, no file (the MD5 of A) |
| `media/path` | readable folder `/tmp/r` holding `f.mp3`, a link `out` → `/etc/hosts` | `storeMediaFile {filename: "f.mp3", path: "/tmp/r/f.mp3"}`; `{…, path: "/tmp/r/out"}`; `{…, path: "/etc/hosts"}`; `{…, path: "/etc/does-not-exist"}`; `{…, path: "/tmp/r/missing.mp3"}`; `{…, path: "r/f.mp3"}` | `"f.mp3"`; error `cannot read file: /tmp/r/out (not permitted)`; error `cannot read file: /etc/hosts (not permitted)`; error `cannot read file: /etc/does-not-exist (not permitted)`; error `cannot read file: /tmp/r/missing.mp3 (not found)`; error `cannot read file: r/f.mp3 (not permitted)` (Meguri, `ankiconnect-media-path-roots`) |

The `url` source is tested against a local HTTP stub in Meguri's own tests (`core/crates/meguri-servers/tests/ankiconnect_media.rs`): 5 redirects succeed, 6 fail; `file://` and `ftp://` fail; a 404 gives `<url> download failed with return code 404`; a 204 succeeds with no bytes; a body above the cap fails with `too large`; a stalled body fails once the 30 s limit passes (real time: fetch limits do not follow the host's test clock, `servers/host-security#H21`); while a download stalls, a UI write and another API write commit without waiting for it (X3.5).

External-folder tests (Meguri only, `ankiconnect_extmedia.rs`, a temporary folder in Docker):

| Test | Steps | Expected |
|---|---|---|
| AE79 core | write `clip.mp3`, then `addNote` with `[sound:clip.mp3]` before any scan | `clip.mp3` in collection media before the commit, pending for sync; Check Media: used, nothing missing |
| idempotent scan | scan until no entry is `pending`; scan again with no change | the last scan writes nothing (media, journal, index) |
| re-ingest | modify an ingested `clip.mp3`; scan twice | collection `clip.mp3` has the new bytes; the old bytes in `media.trash`; pending |
| case kept | write `Clip.mp3`; scan twice | stored as `Clip.mp3` |
| mid-write | append to a file between two scans; then stop | ingested only after two equal observations, with the full bytes |
| baseline | grant a folder holding 3 files; scan; edit one; scan | no write |
| baseline referenced | then `addNote` referencing one of them | only that file ingested |
| batch | `addNotes` with a note referencing a baseline file | ingested before the commit; a failing batch leaves none |
| rollback then scan | write `new.mp3`; `addNotes` whose first note references it and whose second fails; scan twice | the batch leaves no file and no index change; the two scans copy `new.mp3` |
| unrelated name | collection media holds `pic.png` = A (added in the editor); write `pic.png` = B into the folder; scan twice | collection `pic.png` still A; entry `conflict`; one `conflict` notice |
| transformed collision | collection media holds `ab.png` = A; write `a?b.png` = B; `addNote` referencing `a?b.png` | B stored under M3's hash name of `ab.png`; the field references it; `ab.png` still A; a `renamed` notice |
| grant then apply | grant P2 while the config names P1; scan; apply P2; scan twice | the first scan is `suspended`; then P2's new files only |
| apply then grant | apply P2 while the index is P1's; write a file into P2; scan; grant P2; scan twice | the first scan is `suspended`; the file written before the grant is `baseline`, not copied |
| moved folder | the config's path changes to P3 without a grant (a moved folder); write a file; scan; `addNote` referencing it | the scan is `suspended`; the note's file is ingested by the hook |
| crash after commit | stop the process between an ingest's media commit and its index write; relaunch; scan | nothing written twice; the entry becomes `ingested` |
| closed app | add a file while the engine is closed; launch scan twice | the file ingested; no new baseline at launch |
| path root | `storeMediaFile` with `path` inside the external folder | succeeds |

## X15. Open questions

- Q1. The X12 texts are proposed; no source fixes them.
- Q2. Closed: downloads take the shared fetcher's cap (`servers/host-security#H13`, the 256 MiB body limit of H8), and `path` reads use the same cap (X3.3 step 6). Confirm the `path` cap, which no source fixes.
- Q3. Whether removing or renaming a file in the external folder should ever affect collection media is not stated by R1771; X11.3 copies only and never removes. Confirm.
- Q4. Names starting with `.` are skipped by the scan (X11.3) so that editor and system temporary files (`.DS_Store`, `.clip.mp3.part`) are not copied; the hook (X11.6) still ingests such a name when a note references it explicitly. Confirm.
- Q5. U136's `ADDON_PIN` recording of the add-on build of 2025-11-09 confirms or overrides the replies of the recorded actions in this file; where a golden differs, the golden decides and this file changes.
- Q6. The X8 text names "Settings › AnkiConnect › Media Folder" as R1770 does, while U144's screen is titled Settings › Integrations. The text must name the screen and control exactly as U144 builds them; U144 confirms or changes it.
- Q7. An attachment that fails and has no `fields` member fails the note (X10.2 step 3), as AnkiConnect does, although R1762 says an attachment failure "does not fail the call". Confirm the add-on's behavior, or amend R1762's wording to exclude this case.
- Q8. The plan (U143) says ingest events are logged in the activity log, but `servers/ankiconnect-envelope#E16` keeps one entry per request with no names (R1780). X11.9 sends them as a separate host event. Decide whether E16 gains an entry kind for them (and whether file names may appear there), or whether U144 shows them elsewhere.
- Q9. A folder that moved or was renamed (its bookmark resolves to a new path) suspends the scans until the user picks it again, and a new pick records a new baseline, so files a tool wrote meanwhile and no note referenced are not copied. A relocation call that keeps the index (re-keying it to the new path) would avoid that. Decide whether U144 and U143 add one.
