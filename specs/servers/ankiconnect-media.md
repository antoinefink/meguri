# AnkiConnect media actions and the external media folder

Spec: servers/ankiconnect-media
Requirements: R1761, R1762, R1769, R1770, R1771, R1772, R1773; AE79 (core part); supports R1779, R1780, R1789, R1790
Anki: 26.09.3
Scenarios: conformance/recordings/servers/ankiconnect/media/ (X14; `servers/transcripts` T2; ids `servers/ankiconnect/media/<name>`). Oracle goldens recorded 2026-11-17. Declared Meguri-only scenarios: `dir-unset`, `non-files` and `ext-hook*`; `ext-ungranted` is held until `servers/transcripts` T20 states that Meguri's executor never grants an external folder. The external-folder tests that need a grant, a scan result or a crash stay Meguri's own (`ankiconnect_extmedia.rs`). X16 records the add-on's replies as probed on 2026-11-10.

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
- **Falsy**: a missing member and the JSON values `null`, `false`, `0` (also `0.0`), `""`, `[]` and `{}`. Every other value is **truthy**. The add-on tests its optional members this way (X16.1, X16.5).

## X2. Actions

| Action | Parameters | Result | Kind | `affected` (R1780, `servers/ankiconnect-envelope#E16`) |
|---|---|---|---|---|
| `storeMediaFile` | `filename`, `data`, `path`, `url`, `skipHash`, `deleteExisting` | stored name, or null | write | files written (0 or 1) |
| `retrieveMediaFile` | `filename` | base64 text, or false | read | 0 |
| `getMediaFilesNames` | `pattern` (default `*`) | list of names | read | 0 |
| `deleteMediaFile` | `filename` | null | write | files trashed (0 or 1) |
| `getMediaDirPath` | none | absolute path | read | 0 |

Registry types (`servers/ankiconnect-envelope#E12.2`): `filename` is a required string; `pattern` is a string, `*` when absent; `data`, `path`, `url`, `skipHash` and `deleteExisting` have the type any. The generic checks never name these five, so `data: 0`, `data: null` and `deleteExisting: null` reach the action, and X3.1 and X4 own their errors.

Write actions are refused in read-only mode (R1779) by the envelope. Each write action is one `LocalApi` op of the media store (`media/media-journal#J5`), so media sync and Check Media see it like an editor change (R1789).

## X3. Content sources

### X3.1 Choosing the source

The members are taken in the order `data`, `path`, `url`. A falsy member (X1) counts as absent, whatever its type. The first member that is not absent is the source, and only its type is checked: it must be a string, and a truthy non-string (`123`, `true`, `["YQo="]`) is `invalid parameter: <name>` (`servers/ankiconnect-envelope#E12.2`; the add-on gives its runtime's text, `ankiconnect-media-error-text`). Members after the source are never looked at, whatever their values: `{data: "YQo=", url: 5}` stores the `data` bytes, and `{data: 0, path: <a readable file>}` reads the file. When all three are absent: error `You must provide a "data", "path", or "url" field.` A later member is never tried when an earlier one fails. Settled by the 2026-11-10 probes (X16.1): the add-on behaves so, and Meguri follows it.

| Source | Bytes |
|---|---|
| `data` | the base64 decoding of the text (X3.2) |
| `path` | the file at that path (X3.3) |
| `url` | the body of the download (X3.4) |

### X3.2 `data`

Standard base64 (RFC 4648 §4 alphabet, `=` padding), decoded in three steps:

1. A text holding a character outside ASCII is an error `invalid base64 data`. The add-on refuses such a text too (X16.2).
2. Every other character that is not `A`–`Z`, `a`–`z`, `0`–`9`, `+`, `/` or `=` is removed (line breaks, spaces, and also the URL-safe `-` and `_`).
3. What remains must be a run of alphabet characters of length L followed by padding, with L mod 4 = 0 and no `=`, L mod 4 = 2 and exactly `==`, or L mod 4 = 3 and exactly `=`. It decodes to floor(L × 3 / 4) bytes. Anything else (no padding where it is needed, `=` inside the text, too many `=`, more text after the padding, L mod 4 = 1): error `invalid base64 data` (Meguri wording, X12).

| `data` | Bytes |
|---|---|
| `YQo=` | `61 0a` |
| `YQ==` | `61` |
| `YWJj` | `61 62 63` |
| `Y Q o =`, `YQo=` + LF | `61 0a` |
| `YQo`, `YQ` (unpadded) | error |
| `YQ=o`, `=YQo` | error |
| `YQo==`, `YQ==YQ==` | error (the add-on decodes them to `61 0a` and `61`; X13 `ankiconnect-media-base64`) |
| `YQo=garbage` | error (the add-on decodes `61 0a` and ignores the text after the padding; the same entry) |
| `=`, `==` | error (L = 0 takes no padding; the add-on decodes them to no bytes and stores an empty file; the same entry) |
| `-_8=` | error (after step 2, `8=` is not valid; the add-on refuses it too) |
| `YQ==é` | error (the add-on refuses it too) |

The empty string never reaches decoding: X3.1 treats it as absent. A `data` made only of removed characters (a single space) decodes to no bytes, which is valid; the add-on also stores an empty file for it (X16.2). Every row whose result is bytes, and every error row not marked otherwise, matches the add-on's outcome (X16.2); the error texts are X12's.

### X3.3 `path` (R1772)

The checks run in this order; the first that fails gives the error. `<path>` is the `path` member as given.

1. The path must be absolute; otherwise `cannot read file: <path> (not permitted)`.
2. **Root check.** Split the path into components. Resolve the longest prefix that exists, following symbolic links, to its canonical form (X1), and append the remaining components. When a remaining component is `.` or `..`, or the result is not inside an allowed root (X1): `cannot read file: <path> (not permitted)`. So a path outside every root is refused the same way whether or not it exists (`/etc/hosts`, `/etc/does-not-exist`), and nothing tells a caller whether a file outside the roots exists.
3. When the path does not exist (some components remained in step 2): `cannot read file: <path> (not found)`. Only paths inside a root reach this.
4. **Open and verify.** Open the canonical path for reading, not following a symbolic link in its last component. Ask the system for the opened file's own path (the descriptor's path, in the file system's spelling) and check it again against the allowed roots, as in step 2; a component replaced by a link between step 2 and the open is caught here. A failure: `cannot read file: <path> (not permitted)`.
5. When the opened file is not a regular file: `cannot read file: <path> (not a file)`.
6. When its size is above 268,435,456 bytes (the AnkiConnect body limit, `servers/host-security#H8`): `cannot read file: <path> (too large)`.
7. Read the bytes from the opened descriptor. A read failure: `cannot read file: <path> (<reason>)`, `<reason>` the system's description.

The add-on reads any path its process can open, a relative path from its working folder, and reports the runtime's error texts (X16.3; `ankiconnect-media-path-roots`). So, in Meguri, a symbolic link inside a readable folder that points outside every root is refused, and a root configured through a symbolic link (`/tmp` for `/private/tmp`) accepts its children, because both sides are canonical. This holds in every build, sandboxed or not.

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

`<url>` is the member as given. The body is taken as is; no content type is checked. A 2xx status other than 200 is accepted (H13); the add-on refuses it with the status text above (X13 `ankiconnect-media-url-limits`). That status text is the add-on's, observed for 201, 204, 404 and 500 (X16.4).

### X3.5 When the bytes are obtained

Decoding `data`, reading a `path` and downloading a `url` happen before the action's op opens, on the request's worker and never on the collection writer (`engine/op-model#O2`), so a 30 s download never holds up collection writes, the UI's included. Each source yields either its bytes or its error text (X3.2 to X3.4). The op then only names and writes files (X4 steps 3 to 5, X10.2) and builds fields; it reads no file outside the collection media folder and makes no network request.

## X4. `storeMediaFile` (R1769)

Parameters: `filename` (string, required), the content sources, `skipHash` (any value, default null; only a string can match), `deleteExisting` (true when the member is missing; otherwise true exactly when its value is truthy, X1, so `null`, `false` and `0` count as false and `"yes"` as true). These rules are the add-on's (X16.5).

1. Get the bytes (X3), before the op (X3.5).
2. When `skipHash` is a string equal to the MD5 hex of the bytes (exact comparison, so upper-case hex never matches): the result is `null` and nothing is written or trashed. No op runs.
3. Let D be the text after the last `/` of `filename`, or all of `filename` when it holds no `/`: the add-on's media layer keeps only the last component of a `/`-separated name (X16.5). Take N = transform(D) and its lowercase candidate L, as `media/media-files#M3` steps 1 and 3 define them. M3's refusals (N empty, N ignored) are errors with X12's texts, before anything is looked up. N is a flat name (`text/media-filenames#F1`), so nothing below ever looks outside the collection media folder.
4. When `deleteExisting` is true: resolve N (`media/media-files#M5`), and when it resolves to nothing, resolve L. When one resolves to a file: if that file's content equals the bytes, the result is that file's name and nothing is written (R1571); otherwise the file goes to the trash (`media/media-files#M10.1`, called with the resolved name), and step 5 follows in the same op.
5. Add the bytes under the desired name D (`media/media-files#M3`): the name is transformed and lowercased, identical content already present is reused, and other content under a taken name gets the hash name (M4). The result is the stored name.

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
| none | `{filename: "a/b.mp3", data: <A>}` | `"b.mp3"` | `b.mp3` = A |
| `clip.mp3` = A | `{filename: "Clip.mp3", data: <B>}` | `"clip.mp3"` | `clip.mp3` = B; A in the trash |

Rows 1 to 8 give the add-on's reply (X16.5). Row 5 differs only in the trash: the add-on moves A to the trash and writes it again (`ankiconnect-media-delete-existing`). Row 9 differs in the reply: step 4 resolves `Clip.mp3` to its case variant `clip.mp3` (M5 step 3), while the add-on trashes by the exact name, finds nothing on its case-sensitive volume, keeps A and returns `"clip-<sha1 of B>.mp3"` (`ankiconnect-media-case-variants`).

## X5. `retrieveMediaFile` (R1769)

`filename`: a string. When it is not a flat name (X1): error `invalid media file name: <filename>` (X12), before any file is looked at (`../collection.anki2`, `a/b.mp3`). Otherwise resolve it (`media/media-files#M5`): a file gives its bytes in standard base64 with padding; nothing gives `false`. M5 resolves only regular files, so `""` and the name of a directory give `false`; a case variant of a present name resolves to that file (M5 step 3), so with `a.mp3` present `A.MP3` returns its bytes. A case variant resolves only to a present file that the media database tracks (M7.1): a file that reached the folder without the store, as a scenario's setup file does, is tracked only once a registration has run (M7.2; M7.3 says when, and the `media_pending` observation runs one). The add-on gives error `[Errno 21] Is a directory: …` for `""` and a directory name (`ankiconnect-media-non-files`), and `false` for a case variant on its case-sensitive volume (`ankiconnect-media-case-variants`, X16.6). The add-on instead keeps the name's last path component, normalizes it to NFC and strips the characters Anki removes from media names, so `sub/a.mp3`, `/etc/a.mp3` and `a?.mp3` all return `a.mp3`'s bytes, and `../collection.anki2` returns `false` (X16.6); Meguri refuses such names (`ankiconnect-media-flat-names`).

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

The add-on gives these sets on a case-sensitive volume (X16.7), in directory order, and also lists directories; `""` gives `[""]`, and a pattern holding `/` lists other folders (`sub/*`, `../*`, `/etc/*`), returning the last component of each match (`ankiconnect-media-names-order`, `ankiconnect-media-flat-names`). Its behavior on a case-insensitive volume (X13) was not probed.

## X7. `deleteMediaFile` (R1769)

`filename`: a string. Not a flat name: error `invalid media file name: <filename>`. Otherwise one op trashes it (`media/media-files#M10.1` `trash([filename])`): a file it resolves to moves to the trash and its deletion becomes pending for media sync; a name that resolves to nothing changes nothing. The result is `null` either way, as in the add-on (X16.8). `""` and the name of a directory resolve to nothing (M5 resolves only regular files): `null`, nothing trashed. A case variant resolves to the present file (M5 step 3): with only `b.mp3` present, `B.MP3` trashes `b.mp3`, where the add-on removes nothing (`ankiconnect-media-case-variants`). The add-on trashes by the raw name: a directory name moves that directory to the trash (`ankiconnect-media-non-files`). `deleteMediaFile ""` moves the media folder itself into the trash when no trash folder exists yet: its files end up in `media.trash` and `collection.media` is gone. The reply is `null` when the media database was not in step with the folder before the request, and the error `No such file or directory (os error 2)` when it was, as after a registration, because the add-on then reads the moved folder again; the move happens either way (X16.8). With a trash folder present, `""` gives `Directory not empty (os error 39)` (the same entry); `sub/x` fails with `No such file or directory (os error 2)` and `../x` leaves the file in place (`ankiconnect-media-flat-names`). Meguri's flat-name check and M10.1 resolution keep all of these inside collection media.

## X8. `getMediaDirPath` (R1770)

- With an external folder configured (`ankiconnect.media_dir` not null) that is an allowed root (X1): its `path` as configured, without a trailing `/`. This holds also while its scans are suspended (X11.7): files written there still reach collection media through the note hook (X11.6).
- Otherwise (none configured, or a folder that is not an allowed root, such as one whose bookmark U144 could not resolve): error `no media folder is set up: choose one in Meguri Settings › AnkiConnect › Media Folder` (X12; the label is confirmed against U144's screen, X15 Q6).

The collection media folder is never returned: it lives in the app container, and tools writing there would meet macOS privacy prompts. The add-on returns the absolute path of the profile's `collection.media` folder (X16.9).

## X9. Name-only actions and other names

Only `retrieveMediaFile`, `getMediaFilesNames` and `deleteMediaFile` apply the flat-name check (R1769). `storeMediaFile` and attachments never use `filename` as a path: X4 step 3 keeps the text after its last `/`, as the add-on does, and transforms that into a flat name (`text/media-filenames#F1` removes `\`) before any lookup, trash call or write. So `a/b.mp3` is stored as `b.mp3`, `a\b.mp3` as `ab.mp3`, and `../collection.anki2` as `collection.anki2` inside collection media; the collection file can neither be read, replaced nor trashed through it. The add-on gives the same replies (X16.5).

## X10. Note attachments (R1761, R1762)

`addNote`, `addNotes`, `updateNoteFields` and `updateNote` (with `fields`) accept, in the note object, the members `audio`, `video` and `picture`, each `null`, one attachment object or a list of them. `updateNote` processes them only when it has `fields`; with `tags` alone the add-on ignores them too (X16.10). (`guiAddCards` takes the same members; `servers/ankiconnect-gui`.) The `canAdd*` actions ignore them (`servers/ankiconnect-actions` A7.7); the add-on stores their files (X16.10; `servers/ankiconnect-actions` A18 `ankiconnect-canadd-no-media`).

### X10.1 The attachment object

| Member | Meaning |
|---|---|
| `filename` | the desired name, as in X4 (required) |
| `data`, `path`, `url` | the content sources (X3) |
| `skipHash` | as in X4 |
| `deleteExisting` | its truthiness, as in X4, but **default false**: a missing or `null` member does not trash an existing file (the add-on's behavior, X16.10) |
| `fields` | optional list of field names to append the reference to (X10.2 steps 2 and 3) |

**Error paths.** An attachment's own member errors name the member by its own name within the attachment (`missing parameter: filename`, `invalid parameter: fields`), whether `audio` holds one object or a list: `servers/ankiconnect-actions` A1 treats an attachment like an element of a list parameter. An `audio` (`video`, `picture`) value or list entry that is neither `null` nor an object is `invalid parameter: <path>`, with `<path>` the path of that note member under A1: `note.audio` for `addNote`, `updateNoteFields` and `updateNote`, `audio` for an element of `notes`.

### X10.2 Processing

The attachments are processed after the fields are set and before the note is checked (`servers/ankiconnect-actions` A7.2 step 8, A8.1 step 3): first every `audio` entry, then every `video` entry, then every `picture` entry, each list in its order. A `null` entry is skipped; an entry that is neither `null` nor an object fails the note with `invalid parameter: note.audio` (`note.video`, `note.picture`; the path of X10.1); the add-on fails it too, with its runtime's text (X16.10). Before the op opens, every entry's source is read (X3.5), giving bytes or an error text per entry; an entry whose own members fail (below) reads nothing. Then, inside the op, for each entry:

1. Store the file as `storeMediaFile` does (X4 steps 2 to 5 with the entry's members and its pre-read bytes), staged (`media/media-files#M13`).
2. On success with a stored name `N` (not `null` from `skipHash`) and a `fields` member that is a list: for each string entry of the list that is exactly the name of one of the note's fields (case-sensitive, also for `addNote`, whose `fields` member matches case-insensitively), append to that field's current value `[sound:N]` for audio and video, or `<img src="N">` for a picture. No separator is added; a name listed twice gets two references. Entries that are numbers, booleans or `null`, and names that match no field, are skipped. This is the add-on's behavior (X16.10). An entry that is a list or an object fails the note with `invalid parameter: fields`, and nothing is appended; the add-on fails the note too, with its runtime's text (`unhashable type: 'list'`), after it has stored the file, which stays (X16.10); Meguri's op rolls back, as for any failed add (below).
3. On failure (the entry's pre-read error of X3, an error of X4, or a member error of the entry itself, such as a missing `filename`: `missing parameter: filename`, `servers/ankiconnect-actions` A1, where the add-on appends `'filename'`), the error text with `&`, `<`, `>` written as `&amp;`, `&lt;`, `&gt;` (in that order) is appended to each listed field, as in step 2 (a list or object entry fails the note here too, as in the add-on). The note does not fail and the other attachments go on (R1762). An empty list appends nothing. When this entry has no `fields` member, the note fails instead with `missing parameter: fields` (the add-on fails it with `'fields'`, X16.10), and with `invalid parameter: fields` when `fields` is not a list (`null` included). For non-list values the add-on differs (`ankiconnect-media-attach-fields-type`, X15 Q7, X16.10): a number, `true` or `null` fails the note with its runtime's text (`'int' object is not iterable`, `'bool' object is not iterable`, `'NoneType' object is not iterable`); a string it reads character by character, appending to any field whose name is one of those characters; an object it reads by its member names, appending to the fields those names give (`{"Gloss": 1}` appends to `Gloss`; `{}` appends nothing); in both of these it adds the note. For `addNote` and `updateNote*` that is the action's error; for `addNotes` it is that note's entry in the error list (`servers/ankiconnect-actions` A7.6).

On success, a `fields` member that is absent or not a list appends nothing and is not an error, as in the add-on (X16.10).

A `skipHash` match appends nothing. The X3.5 reads of an action that fails before step 1 are discarded. When the action later fails (a duplicate, a missing deck, any note of an `addNotes` batch), its op rolls back and none of its staged files is left (`servers/ankiconnect-actions` A18 `ankiconnect-failed-add-media`). The add-on leaves them: observed for a duplicate, an empty note and a failed `addNotes` batch; a missing deck or note type fails before any attachment is stored (X16.10).

| Note `fields` | Attachments | Field values after |
|---|---|---|
| `Term`: `word` | `audio: [{filename: "a.mp3", data: <A>, fields: ["Term"]}, {filename: "b.mp3", data: <B>, fields: ["Term", "Gloss"]}]` | Term `word[sound:a.mp3][sound:b.mp3]`, Gloss `[sound:b.mp3]` |
| `Term`: `word` | `picture: {filename: "Cat.JPG", data: <C>, fields: ["Gloss"]}` | Gloss `<img src="cat.jpg">` |
| `Term`: `word` | `audio: {filename: "x.mp3", url: "ftp://h/x", fields: ["Gloss"]}` | Gloss `cannot download ftp://h/x (only http and https are allowed)`; the note is added |
| `Term`: `word` | `audio: {filename: "x.mp3", url: "ftp://h/x"}` | error `missing parameter: fields`, nothing added |
| `Term`: `word` | `audio: {filename: "x.mp3", data: <A>, fields: ["term"]}` | unchanged (case-sensitive) |
| `Term`: `word` | `audio: {filename: "x.mp3", data: <A>, fields: ["Gloss", ["x"]]}` | error `invalid parameter: fields`, nothing added, no file |

The add-on gives these field values (X16.10), except for its own texts (`ankiconnect-media-error-text`): row 3 appends `No connection adapters were found for 'ftp://h/x'`, row 4's error is `'fields'`, and row 6's is `unhashable type: 'list'`, with `x.mp3` left in media (`servers/ankiconnect-actions` A18 `ankiconnect-failed-add-media`).

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

| External folder | Request | Effect | Scenario |
|---|---|---|---|
| `clip.mp3` written just now (not yet scanned) | `addNote` with Term `[sound:clip.mp3]` | `clip.mp3` copied into collection media in the add's op; pending for sync; Check Media reports no missing file (AE79) | `media/ext-hook` (declared) |
| `Clip.mp3` | `addNote` with `[sound:Clip.mp3]` | stored as `Clip.mp3` (case kept) | `media/ext-hook-case` (declared) |
| `doc.pdf` (baseline) | `addNote` with `<img src="doc.pdf">` | `doc.pdf` ingested; entry `ingested` | none (needs a grant step, X14) |
| `a?b.png` (a name F1 changes) | `addNote` with `<img src="a?b.png">` | stored as `ab.png`; the field holds `<img src="ab.png">`; a `renamed` notice | `media/ext-hook-renamed` (declared) |
| `pic.png`, while collection media holds an unrelated `pic.png` | `addNote` with `<img src="pic.png">` | the name resolves already, so nothing is ingested and the note uses the collection file | `media/ext-hook-resolved` (declared) |
| `a?b.png`, while collection media holds an unrelated `ab.png` | `addNote` with `<img src="a?b.png">` | stored under M3's hash name of `ab.png`; the field holds that name; a `renamed` notice | `media/ext-hook-collision` (declared) |

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

Texts without an AnkiConnect counterpart (proposed wording, X15 Q1). Implementers take them only from here. `<filename>` is the `filename` member as given, also in X4, where only its last component is used (`{filename: "a/"}` gives `invalid media file name: a/`); `<pattern>` is the `pattern` member as given; `<path>` and `<url>` are as X3.3 and X3.4 say.

| Where | Text |
|---|---|
| X3.2 | `invalid base64 data` |
| X3.3 | `cannot read file: <path> (not permitted)` (R1772), `cannot read file: <path> (not found)`, `cannot read file: <path> (not a file)`, `cannot read file: <path> (too large)`, `cannot read file: <path> (<reason>)` |
| X3.4 | `cannot download <url> (only http and https are allowed)`, `cannot download <url> (too many redirects)`, `cannot download <url> (timed out)`, `cannot download <url> (too large)`, `cannot download <url> (<reason>)` |
| X5, X7 | `invalid media file name: <filename>` |
| X6 | `invalid media file name: <pattern>` |
| X4: M3 `invalid_name` | `invalid media file name: <filename>` |
| X4: M3 `ignored_name` | `media file name is reserved: <filename>` |
| X8 | `no media folder is set up: choose one in Meguri Settings › AnkiConnect › Media Folder` |

X3.4's status failure keeps AnkiConnect's text `<url> download failed with return code <status>`, and X3.1's keeps `You must provide a "data", "path", or "url" field.`

## X13. Deviations from AnkiConnect

Proposed `conformance/deviations.yaml` entries for the U136 replay. "Scenario" names the X14 row whose golden shows the difference and whose Meguri expectation asserts Meguri's behavior.

| Proposed id | AnkiConnect | Meguri | Scenario |
|---|---|---|---|
| `ankiconnect-media-dir` | `getMediaDirPath` returns Anki's media folder | the external folder, or an error (X8) | `media/dir-unset` (declared, so its replay needs no `deviations.yaml` entry; in `media/dir` both sides give `<media-dir>`, X14) |
| `ankiconnect-media-flat-names` | `retrieveMediaFile` takes the last path component; `deleteMediaFile` and `getMediaFilesNames` follow separators and `..`, and `getMediaFilesNames` also absolute patterns, so it lists any folder (X16.6 to X16.8) | error (X5–X7) | `media/read` |
| `ankiconnect-media-path-roots` | `path` reads any file its process can open, a relative path from its working folder (X16.3) | allowed roots only (X3.3) | `media/path` |
| `ankiconnect-media-url-limits` | 10 s for each connection attempt and each socket read, not for the whole download (a body arriving in parts 4 s apart over 16 s is stored); up to 30 redirects (a 31st: `Exceeded 30 redirects.`); only status 200 accepted (201 and 204 refused); schemes other than `http` and `https` refused with the client library's text, also as a redirect target (X16.4) | 30 s, 5 redirects, `http`/`https` only, any 2xx accepted, 256 MiB cap (X3.4) | `media/attach-fail`; the stub tests below |
| `ankiconnect-media-names-order` | `getMediaFilesNames` in directory order, also matching directories; the empty pattern gives `[""]` (X16.7); on a case-insensitive volume a pattern without wildcards matches any case (not probed) | regular files only, sorted; the empty pattern gives `[]`; matching is case-sensitive everywhere (X6) | `media/names` |
| `ankiconnect-media-delete-existing` | with `deleteExisting`, the existing file is trashed by the exact given name and the same content is written again; a `filename` of `""` moves every file of the media folder into the trash when no trash folder exists yet; `sub/inner.mp3` naming an existing subfolder's file fails with `No such file or directory (os error 2)`, and `..` with `Device or resource busy (os error 16)` (X16.5) | the transformed name resolved by M5; identical content is kept, nothing trashed (X4 step 4); `""` is an error (M3 `invalid_name`, X12); `sub/inner.mp3` is stored as `"inner.mp3"` and `..` as `".._"` (X4 step 3, `text/media-filenames#F1` step 5) | `media/store-conflict` |
| `ankiconnect-media-case-variants` | names are matched exactly on the add-on's case-sensitive volume: with `clip.mp3` = A present, `storeMediaFile` `Clip.mp3` with B returns `"clip-<sha1 of B>.mp3"` and keeps A; `retrieveMediaFile` `CLIP.MP3` returns `false`; with `b.mp3` present, `deleteMediaFile` `B.MP3` removes nothing (X16.5, X16.6, X16.8) | case variants resolve to the present file (`media/media-files#M5` step 3): `"clip.mp3"` with B stored and A trashed (X4 row 9); A's bytes (X5); `b.mp3` trashed (X7) | `media/store-conflict`, `media/read`, `media/delete-names` |
| `ankiconnect-media-non-files` | `retrieveMediaFile` with `""` or a directory name: error `[Errno 21] Is a directory: …`; `deleteMediaFile` with a directory name moves that directory to the trash, and with `""` moves the media folder itself into the trash when no trash folder exists yet, replying `null`, or `No such file or directory (os error 2)` when the media database was in step with the folder, as after a registration (X7); `Directory not empty (os error 39)` once a trash folder exists (X16.6, X16.8) | `false` and `null`; nothing trashed (X5, X7) | `media/delete-names`; Meguri's side also `media/non-files` (declared) |
| `ankiconnect-media-base64` | the runtime's lenient decoder: decoding stops at the first complete padding (`YQo=garbage` gives `61 0a`), extra `=` are accepted, and padding alone (`=`, `==`) decodes to no bytes, so an empty file is stored (X16.2) | X3.2's rule: such texts are errors | `media/store-data` |
| `ankiconnect-media-error-text` | the runtime's error texts for decoding, file, download and parameter errors, also where they are appended to fields (X10.2 step 3); X16 lists the observed ones | X12's, and the envelope's parameter texts | `media/attach-fail`, `media/path` |
| `ankiconnect-media-attach-fields-type` | for a failing attachment: a string `fields` is matched character by character and an object `fields` by its member names, the error text appended to the fields so named, and the note is added (`{}` appends nothing); a number, `true` or `null` fails the note with the runtime's text (`'int' object is not iterable`, `'bool' object is not iterable`, `'NoneType' object is not iterable`) (X16.10) | any non-list `fields` of a failing attachment, `null` included, fails the note with `invalid parameter: fields` (X10.2 step 3; X15 Q7) | `media/attach-fail` |

The case rules of `media/media-files#M14` apply to every stored name; `ankiconnect-media-case-variants` names the cases the probe showed, and other cases need their own entries where a golden shows them. The failed-add and `canAdd*` differences are `servers/ankiconnect-actions` A18's (`ankiconnect-failed-add-media`: `media/attach-rollback`; `ankiconnect-canadd-no-media`: `notes/can-add-media`). A list or object entry in `fields` fails the note in the add-on and in Meguri (X10.2 step 2); the file the add-on leaves behind is `ankiconnect-failed-add-media`'s. The probe also showed that the add-on's `canAddNote` returns `false` when an attachment fails with no `fields` (X16.10), where Meguri's check, which ignores attachments, can return `true`; A18's `ankiconnect-canadd-no-media` should name that reply too.

## X14. Scenarios

Requests are `version` 6 without key or Origin; replies shown unwrapped. The add-on's replies to the same requests, where they were probed, are in X16. Byte contents: A = `61 0a` (base64 `YQo=`), B = `62 0a` (`Ygo=`). `Pair` and deck `D` are `servers/ankiconnect-actions` A20's. Scenarios that add a note end with `observe: [{db_dump: {tables: [notes, cards]}}, media_dir]` (R1948, `servers/transcripts` T14; the table form of T18, R1959). A row marked "(Meguri)" asserts Meguri's side of the X13 entry it names. A scenario marked "(recorded)" has an oracle golden of 2026-11-17 next to its transcript; one marked "(declared)" is Meguri-only (`servers/transcripts` T13). A setup file bypasses the store, so a scenario whose requests depend on the media database knowing it (a case variant, a pending deletion) registers the folder first with an `observe: [media_pending]` step, a full registration on both sides (`conformance/observations` O4, `media/media-files#M7.3`).

| Scenario | Setup | Request | Reply and media after |
|---|---|---|---|
| `media/store-data` (recorded) | empty media | X4's rows 1, 2, 6, 7; then `storeMediaFile {filename: "y.txt", data: "YQo=="}`; `{filename: "z.txt", data: "="}`; `{filename: "w.txt", data: "YQo=", url: 5}` | as X4; error `invalid base64 data` twice (Meguri, `ankiconnect-media-base64`); `"w.txt"` = A (X3.1) |
| `media/store-conflict` (recorded) | `clip.mp3` = A | X4's rows 3, 4, 5, then row 9 on a fresh `clip.mp3` = A | as X4 (trash contents checked by a `media_trash` observation, `conformance/observations` O4); row 9: Meguri, `ankiconnect-media-case-variants` |
| `media/store-traversal` (recorded) | `a.mp3` = A | `storeMediaFile {filename: "../collection.anki2", data: "Ygo="}` | `"collection.anki2"` (X4 step 3 keeps the last component, as the add-on does, X16.5); stored in collection media; the collection file untouched, nothing trashed |
| `media/read` (recorded) | `a.mp3` = A, registered by an `observe: [media_pending]` before the first request (a case variant resolves only to a tracked file, X5) | `retrieveMediaFile {filename: "a.mp3"}`; `{filename: "missing.mp3"}`; `{filename: "../collection.anki2"}`; `{filename: "a/b.mp3"}`; `{filename: "A.MP3"}` | `"YQo="`; `false`; error `invalid media file name: ../collection.anki2` (Meguri, `ankiconnect-media-flat-names`); error `invalid media file name: a/b.mp3` (the same); `"YQo="` (Meguri, `ankiconnect-media-case-variants`). `{filename: ""}` is not here: the add-on's error names its media folder inside the text, which `servers/transcripts` T10.3 does not normalize and T9 keeps out of goldens; it is in `media/non-files` |
| `media/non-files` (declared) | `a.mp3` = A, a directory `sub` | `retrieveMediaFile {filename: ""}`; `{filename: "sub"}`; `deleteMediaFile {filename: ""}`; `{filename: "sub"}` | `false`; `false`; `null`, no effect; `null`, no effect (Meguri, `ankiconnect-media-non-files`) |
| `media/names` (recorded) | X6's folder | X6's rows; `getMediaFilesNames {pattern: ""}` | as X6 (order, Meguri `ankiconnect-media-names-order`); `[]` |
| `media/delete` (recorded) | `a.mp3` = A, registered by an `observe: [media_pending]` before the first request (the add-on records no deletion for a file it never registered) | `deleteMediaFile {filename: "a.mp3"}`; again | `null`, file in the trash, deletion pending; `null`, nothing |
| `media/delete-names` (recorded) | `a.mp3` = A, `b.mp3` = B, registered by an `observe: [media_pending]` | `deleteMediaFile {filename: "B.MP3"}`; then `{filename: ""}`. The case variant comes first because the add-on's `""` moves the whole folder into the trash (X7), which would hide the case-variant reply; the observation after `""` reads only `media_trash` | Add-on: `null`, nothing trashed, no deletion pending; then error `No such file or directory (os error 2)`, both files in the trash. Meguri: `null`, `b.mp3` in the trash, its deletion pending, `a.mp3` kept (`ankiconnect-media-case-variants`); then `null`, nothing trashed (`ankiconnect-media-non-files`) |
| `media/dir` (recorded) | none | `getMediaDirPath` | `"<media-dir>"` (`servers/transcripts` T10.3; Meguri's executor configures `media-dir` by default, T4, so both sides normalize the same and no deviation entry applies) |
| `media/dir-unset` (declared) | `server_config: {media_dir: null}` | `getMediaDirPath` | error X8 (Meguri, `ankiconnect-media-dir`) |
| `media/attach` (recorded) | `Pair`, deck `D` | `addNote` with X10.2's rows 1 and 2 | fields as X10.2; files stored |
| `media/attach-fail` (recorded) | the same | X10.2's rows 3, 4 and 6 | as X10.2 (row 3's appended text: Meguri, `ankiconnect-media-error-text`; row 6's missing file: Meguri, `ankiconnect-failed-add-media`) |
| `media/attach-rollback` (recorded) | the same, with a note `x` already in `D` | `addNote` with Term `x` (a duplicate) and `audio: {filename: "n.mp3", data: "Ygo=", fields: ["Gloss"]}` | error `cannot create note because it is a duplicate`; no `n.mp3` in media (Meguri, `ankiconnect-failed-add-media`) |
| `media/attach-notes` (recorded) | the same | `addNotes` with two notes, the second's `audio` lacking `fields` and failing (`url: "ftp://h/x"`) | error `['missing parameter: fields']`; nothing added (the add-on: `["'fields'"]`, with the first note's file left in media, X16.10) |
| `media/skip-hash` (recorded) | the same | `addNote` with `audio: {filename: "x.mp3", data: <A>, skipHash: "60b725f10c9c85c70d97880dfe8191b3", fields: ["Term"]}` | note added, Term without a reference, no file (the MD5 of A) |
| `media/path` (recorded) | none (no readable folder) | `storeMediaFile {filename: "f.mp3", path: "/etc/does-not-exist"}`; `{…, path: "/tmp/r/missing.mp3"}`; `{…, path: "r/f.mp3"}` | the add-on: error `[Errno 2] No such file or directory: '<path>'` for each. Meguri: error `cannot read file: <path> (not permitted)` for each (X3.3 steps 1 and 2; `ankiconnect-media-path-roots`) |

The `path` rows with a readable folder (`/tmp/r` holding `f.mp3`: `/tmp/r/f.mp3` gives `"f.mp3"`, `/tmp/r/missing.mp3` gives `cannot read file: /tmp/r/missing.mp3 (not found)`), a symbolic link `out` → `/etc/hosts` inside it (`cannot read file: /tmp/r/out (not permitted)`) and an existing file outside every root (`/etc/hosts`: `cannot read file: /etc/hosts (not permitted)`) are Meguri's own tests (`ankiconnect_media.rs`): `readable_folders` is declared only (`servers/transcripts` T4), no setup writes files or links outside the media folder, and an outside file's bytes differ between containers. They become a declared scenario once `servers/transcripts` can create files and links outside the media folder and name their absolute paths.

The `url` source is tested against a local HTTP stub in Meguri's own tests (`core/crates/meguri-servers/tests/ankiconnect_media.rs`): 5 redirects succeed, 6 fail; `file://` and `ftp://` fail; a 404 gives `<url> download failed with return code 404`; a 204 succeeds with no bytes; a body above the cap fails with `too large`; a stalled body fails once the 30 s limit passes (real time: fetch limits do not follow the host's test clock, `servers/host-security#H21`); while a download stalls, a UI write and another API write commit without waiting for it (X3.5).

External-folder tests (Meguri only, `ankiconnect_extmedia.rs`, a temporary folder in Docker). The Scenario column names the declared transcript that covers a test, or its hook part; a test with none stays Rust-only, because it needs a grant step (`media_folder_granted`, X11.2), a scan result (X11.8) or a crash, which `servers/transcripts` T7 does not express yet:

| Test | Steps | Expected | Scenario |
|---|---|---|---|
| AE79 core | write `clip.mp3`, then `addNote` with `[sound:clip.mp3]` before any scan | `clip.mp3` in collection media before the commit, pending for sync; Check Media: used, nothing missing | `media/ext-hook` (declared) |
| idempotent scan | scan until no entry is `pending`; scan again with no change | the last scan writes nothing (media, journal, index) | none |
| re-ingest | modify an ingested `clip.mp3`; scan twice | collection `clip.mp3` has the new bytes; the old bytes in `media.trash`; pending | none |
| case kept | write `Clip.mp3`; scan twice | stored as `Clip.mp3` | hook form: `media/ext-hook-case` (declared) |
| mid-write | append to a file between two scans; then stop | ingested only after two equal observations, with the full bytes | none |
| baseline | grant a folder holding 3 files; scan; edit one; scan | no write | none |
| baseline referenced | then `addNote` referencing one of them | only that file ingested | none |
| batch | `addNotes` with a note referencing a baseline file | ingested before the commit; a failing batch leaves none | hook form: `media/ext-hook-batch` (declared) |
| rollback then scan | write `new.mp3`; `addNotes` whose first note references it and whose second fails; scan twice | the batch leaves no file and no index change; the two scans copy `new.mp3` | hook part: `media/ext-hook-batch` (declared) |
| unrelated name | collection media holds `pic.png` = A (added in the editor); write `pic.png` = B into the folder; scan twice | collection `pic.png` still A; entry `conflict`; one `conflict` notice | none |
| transformed collision | collection media holds `ab.png` = A; write `a?b.png` = B; `addNote` referencing `a?b.png` | B stored under M3's hash name of `ab.png`; the field references it; `ab.png` still A; a `renamed` notice | `media/ext-hook-collision` (declared) |
| grant then apply | grant P2 while the config names P1; scan; apply P2; scan twice | the first scan is `suspended`; then P2's new files only | none |
| apply then grant | apply P2 while the index is P1's; write a file into P2; scan; grant P2; scan twice | the first scan is `suspended`; the file written before the grant is `baseline`, not copied | none |
| moved folder | the config's path changes to P3 without a grant (a moved folder); write a file; scan; `addNote` referencing it | the scan is `suspended`; the note's file is ingested by the hook | `media/ext-ungranted`, held until `servers/transcripts` T20 states that Meguri's executor never grants a folder |
| crash after commit | stop the process between an ingest's media commit and its index write; relaunch; scan | nothing written twice; the entry becomes `ingested` | none |
| closed app | add a file while the engine is closed; launch scan twice | the file ingested; no new baseline at launch | none |
| path root | `storeMediaFile` with `path` inside the external folder | succeeds | none |

## X15. Open questions

- Q1. The X12 texts are proposed; no source fixes them.
- Q2. Closed: downloads take the shared fetcher's cap (`servers/host-security#H13`, the 256 MiB body limit of H8), and `path` reads use the same cap (X3.3 step 6). Confirm the `path` cap, which no source fixes.
- Q3. Whether removing or renaming a file in the external folder should ever affect collection media is not stated by R1771; X11.3 copies only and never removes. Confirm.
- Q4. Names starting with `.` are skipped by the scan (X11.3) so that editor and system temporary files (`.DS_Store`, `.clip.mp3.part`) are not copied; the hook (X11.6) still ingests such a name when a note references it explicitly. Confirm.
- Q5. U136's `ADDON_PIN` recording of the add-on build of 2025-11-09 confirms or overrides the replies of the recorded actions in this file; where a golden differs, the golden decides and this file changes. The 2026-11-10 probe (X16) ran every recorded action of this file against that build; this file now states its replies, and a later golden still decides.
- Q6. The X8 text names "Settings › AnkiConnect › Media Folder" as R1770 does, while U144's screen is titled Settings › Integrations. The text must name the screen and control exactly as U144 builds them; U144 confirms or changes it.
- Q7. An attachment that fails and has no `fields` member fails the note (X10.2 step 3), as AnkiConnect does, although R1762 says an attachment failure "does not fail the call". The add-on's behavior is settled by the probe (X16.10): it fails the note with `'fields'` (`["'fields'"]` for `addNotes`, which keeps the files of the batch's other notes). Open: amend R1762's wording to exclude this case (recommended), or let such an attachment fail silently. The non-list `fields` case (`ankiconnect-media-attach-fields-type`) belongs to the same decision.
- Q8. The plan (U143) says ingest events are logged in the activity log, but `servers/ankiconnect-envelope#E16` keeps one entry per request with no names (R1780). X11.9 sends them as a separate host event. Decide whether E16 gains an entry kind for them (and whether file names may appear there), or whether U144 shows them elsewhere.
- Q9. A folder that moved or was renamed (its bookmark resolves to a new path) suspends the scans until the user picks it again, and a new pick records a new baseline, so files a tool wrote meanwhile and no note referenced are not copied. A relocation call that keeps the index (re-keying it to the new path) would avoid that. Decide whether U144 and U143 add one.

## X16. Observed add-on behavior (probe of 2026-11-10)

The AnkiConnect add-on 25.11.9.0 (the build U136 pins) in Anki 26.9.3, run in the quarantined recorder image on Linux (a case-sensitive volume) with a new profile, answered the requests below. Requests were `version` 6 with no key and no `Origin`; replies are shown unwrapped; every reply had HTTP status 200. A = `61 0a` (`YQo=`), B = `62 0a` (`Ygo=`). `<media folder>` is the profile's `collection.media` folder, given as an absolute path in the real texts. Meguri follows these observations except where X12 gives a Meguri text or X13 a deviation. They do not replace U136's goldens (X15 Q5). A second run on the same day, in a new container (review fixes), added the rows marked "(re-probe)".

### X16.1 Content sources

| `storeMediaFile` members | Add-on reply |
|---|---|
| `data: ""`; no source member; all three `null`; `data: 0` | error `You must provide a "data", "path", or "url" field.` |
| `data: 123`, `data: true`, `data: ["YQo="]` | error `argument should be a bytes-like object or ASCII string, not 'int'` (`'bool'`, `'list'`) |
| `data: "Ygo="` and `path` of a readable file | the `data` bytes stored |
| `data: "YQo="` with `url: 5`, `path: 5`, `url: true` or `path: ["x"]` (re-probe) | the `data` bytes stored; the other member is not looked at |
| `data: 0` and `path` of a readable file (re-probe) | the file's bytes stored |
| `data: ""` and `path` of a readable file | the file's bytes stored |
| `path: "/nope"` and a working `url` | error `[Errno 2] No such file or directory: '/nope'`; the `url` is not fetched |
| no `filename` | a runtime error saying that the required argument `filename` is missing |
| `filename: 5` | error `bad argument type for built-in operation` |

### X16.2 `data`

| `data` | Add-on |
|---|---|
| `YQ==`, `YWJj`, `Y Q o =`, `YQo=` + LF | `61`; `61 62 63`; `61 0a`; `61 0a` |
| `YQo==`, `YQ==YQ==`, `YQo=garbage` | `61 0a`; `61`; `61 0a` |
| ` ` (one space) | no bytes: an empty file is stored |
| `=`, `==` (re-probe) | no bytes: an empty file is stored |
| `YQo`, `YQ`, `YQ=o`, `=YQo` | error `Incorrect padding` |
| `Y`, `-_8=` | error `Invalid base64-encoded string: number of data characters (1) cannot be 1 more than a multiple of 4` |
| `YQ==é`, `é` | error `string argument should contain only ASCII characters` |

### X16.3 `path`

| `path` | Add-on reply |
|---|---|
| `/etc/hostname` (outside every Meguri root) | the file's bytes stored |
| `/nope/missing.mp3` | error `[Errno 2] No such file or directory: '/nope/missing.mp3'` |
| `/tmp` (a directory) | error `[Errno 21] Is a directory: '/tmp'` |
| `relative.txt` | error `[Errno 2] No such file or directory: 'relative.txt'` (looked up from the process's working folder) |
| `7` (a number) | error `[Errno 22] Invalid argument` |

### X16.4 `url`

Against an HTTP stub on the container's loopback:

| Stub behavior or `url` | Add-on reply |
|---|---|
| status 200 | stored |
| status 201, 204, 404, 500 | error `<url> download failed with return code <status>` |
| 5, 6 and 30 redirects, then 200 | stored |
| 31 redirects | error `Exceeded 30 redirects.` |
| `ftp://…`, `file://…`, or a redirect to `ftp://…` | error `No connection adapters were found for '<url>'`, naming the redirect target for a redirect |
| `nonsense` (no scheme) | error starting `Invalid URL 'nonsense': No scheme supplied.`, then a question that names `https://nonsense` |
| connection refused, unknown host, TLS to a plain HTTP port | the client library's text, which starts `HTTPConnectionPool(host='<host>', port=<port>): Max retries exceeded with url: <path>` (`HTTPSConnectionPool` for `https`) and names the system error |
| no response for 12 s | after 10.0 s: error `HTTPConnectionPool(host='<host>', port=<port>): Read timed out. (read timeout=10.0)` |
| a 5-byte body sent one byte every 4 s | stored after 16 s (the limit applies to each read, not to the download) |

The add-on's request carried the headers `Host`, `User-Agent` (`Anki 26.09.3`), `Accept`, `Accept-Encoding` and `Connection`.

### X16.5 `storeMediaFile` names, `skipHash`, `deleteExisting`

| Collection media before | Request | Add-on reply | After |
|---|---|---|---|
| none | `Clip.MP3`, A | `"clip.mp3"` | |
| `clip.mp3` = A | `clip.mp3`, B | `"clip.mp3"` | B; A in `media.trash` |
| `clip.mp3` = A | `clip.mp3`, A | `"clip.mp3"` | A; a copy of A in `media.trash` |
| `clip.mp3` = A | `clip.mp3`, B, `deleteExisting` `false` or `null` | `"clip-<sha1 hex of B>.mp3"` | both files |
| `clip.mp3` = A | `clip.mp3`, B, `deleteExisting` `"yes"` | `"clip.mp3"` | B; A trashed |
| `clip.mp3` = A | `Clip.mp3`, B (`deleteExisting` missing) | `"clip-<sha1 hex of B>.mp3"` | A kept and nothing trashed: the trash call named `Clip.mp3`, which is not present |
| none | A, `skipHash` = MD5 hex of A | `null` | nothing |
| none | A, `skipHash` = the same in upper case, `""` or `5` | the stored name | stored |
| `x.mp3` = A | `x.mp3`, B, `skipHash` = MD5 hex of A | `"x.mp3"` | B; A trashed |
| none | `a/b.mp3`; `a\b.mp3` | `"b.mp3"`; `"ab.mp3"` | |
| none; `outside.txt` in the profile folder | `../outside.txt`, B | `"outside.txt"` | stored in collection media; the profile folder's file unchanged |
| `sub/inner.mp3` (a subfolder) | `sub/inner.mp3`, B | error `No such file or directory (os error 2)` | nothing |
| none | `a?b:c*.mp3`; `e` + U+0301 + `.mp3` | `"abc.mp3"`; `"é.mp3"` (NFC) | |
| files, no `media.trash` folder yet | `""`, A | error `Failed to read '<media folder>/': Is a directory (os error 21)` | every file of collection media moved into `media.trash` |
| files, `media.trash` present | `""`, A | error `Directory not empty (os error 39)` | nothing |
| | `???`, `deleteExisting: false` | error `Failed to read '<media folder>/': Is a directory (os error 21)` | nothing |
| | `..`, A | error `Device or resource busy (os error 16)` | nothing |

### X16.6 `retrieveMediaFile`

With `a.mp3` = A and `été.mp3` present: `a.mp3`, `sub/a.mp3`, `x/../a.mp3`, `/etc/a.mp3` and `a?.mp3` return `"YQo="`; `missing.mp3` and `../collection.anki2` return `false`; the decomposed spelling of `été.mp3` returns that file; `sub` (a directory) gives error `[Errno 21] Is a directory: '<media folder>/sub'`, `""` gives `[Errno 21] Is a directory: '<media folder>/'`, and `null` gives `expected str, bytes or os.PathLike object, not NoneType`. Re-probe, with `clip.mp3` present: `CLIP.MP3` and `Clip.mp3` return `false`.

### X16.7 `getMediaFilesNames`

Folder: `b.mp3`, `a.mp3`, `c.jpg`, `.hidden.mp3`, `A.MP3x`, a directory `dir.mp3` and a directory `sub` holding `inner.mp3`. Results came in the folder's directory order; shown here as sets.

| `pattern` | Add-on result |
|---|---|
| missing, `*`, `**` | every entry except `.hidden.mp3`, the two directories included |
| `*.mp3` | `dir.mp3`, `a.mp3`, `b.mp3` |
| `[ab].*`, `?.mp3` | `a.mp3`, `b.mp3` |
| `[!a]*` | every entry except `a.mp3` and `.hidden.mp3` |
| `A.MP3`, `[a` | `[]` |
| `.*` | `.hidden.mp3` |
| `""` | `[""]` |
| `sub/*`, `*/*` | `inner.mp3` |
| `../*` | the profile folder's entries (`collection.anki2`, `collection.media`, `media.trash`, …) |
| `/etc/*` | the names in `/etc` |

### X16.8 `deleteMediaFile`

`a.mp3` returns `null` and moves the file to `media.trash`; the same call again returns `null`. `B.MP3` with only `b.mp3` present returns `null` and removes nothing (confirmed by the re-probe). `dir.mp3` (a directory) returns `null` and moves the directory to `media.trash`. `sub/inner.mp3` gives error `No such file or directory (os error 2)` and removes nothing. `../outside2.txt` returns `null` and the profile folder's file stays. `""` with `media.trash` present gives `Directory not empty (os error 39)`. `5` gives `bad argument type for built-in operation`. Recorded on 2026-11-17 (`media/delete-names`, after a registration): `""` with no `media.trash` folder yet gives error `No such file or directory (os error 2)` and moves the `collection.media` folder itself into the trash, so both files are in `media.trash`; a first recording without the registration replied `null` with the same move (X7).

### X16.9 `getMediaDirPath`

The absolute path of `<profile folder>/collection.media`, without a trailing `/`.

### X16.10 Attachments

Note type `Pair` (fields `Term`, `Gloss`), deck `D`; `Gloss` empty unless stated.

| Note request | Add-on outcome |
|---|---|
| X10.2's rows 1, 2 and 5 | the field values X10.2 shows (row 2: `<img src="cat.jpg">`) |
| X10.2's row 3 (`url: "ftp://h/x"`, `fields: ["Gloss"]`) | note added; Gloss `No connection adapters were found for 'ftp://h/x'` |
| row 4 (`url` fails, no `fields`) | error `'fields'` |
| `picture`, `video` and `audio`, each to `Term` | Term `w7` became `w7[sound:a7.mp3][sound:v7.mp4]<img src="p7.png">`: audio, then video, then picture |
| `audio: [null, {…, fields: ["Term"]}]` | the `null` skipped; one reference |
| `audio: ["x", {…}]`; `audio: 5` | error `string indices must be integers, not 'str'`; error `'int' object is not subscriptable` |
| an entry without `filename`, `fields: ["Gloss"]` | note added; Gloss `'filename'` |
| `fields: "Term"` on success; `fields: 5` on success | note added; nothing appended |
| `fields: "Gloss"` with a failing `data` | note added; nothing appended (no field has a one-letter name) |
| `fields: 5` with a failing `data` | error `'int' object is not iterable` |
| `fields: ["Gloss", "Gloss"]` | Gloss `[sound:a19.mp3][sound:a19.mp3]` |
| `fields: [5, "Gloss"]` | Gloss `[sound:a20.mp3]` |
| `fields: ["Gloss", null, true, 1.5]` (re-probe) | Gloss `[sound:u8.mp3]` |
| `fields: ["Gloss", ["x"]]`, `fields: ["Gloss", {"a": 1}]` (re-probe) | error `unhashable type: 'list'` (`'dict'`); no note added; the file stays in collection media |
| `fields: ["Gloss", ["x"]]` with a failing `data` (re-probe) | error `unhashable type: 'list'`; no note added |
| `fields: [null, "Gloss"]` with a failing `data` (re-probe) | note added; Gloss holds the error text |
| `fields: {"Gloss": 1}` with a failing `data` (`YQ`) (re-probe) | note added; Gloss `Incorrect padding` |
| `fields: {"Gloss": 1}` on success (re-probe) | note added; nothing appended; the file stored |
| `fields: null`, `fields: true` with a failing `data` (re-probe) | error `'NoneType' object is not iterable` (`'bool' object is not iterable`) |
| `fields: {}`, `fields: []` with a failing `data` (re-probe) | note added; nothing appended |
| `url: "ftp://h/<x>&y"`, `fields: ["Gloss"]` | Gloss `No connection adapters were found for 'ftp://h/&lt;x&gt;&amp;y'` |
| `clip9.mp3` = A present; attachment `clip9.mp3`, B, no `deleteExisting` | Gloss `[sound:clip9-<sha1 hex of B>.mp3]`; A kept |
| the same with `deleteExisting: true` | Gloss `[sound:clip9.mp3]`; B stored, A trashed |
| `skipHash` = MD5 hex of the bytes, `fields: ["Term"]` | note added; Term unchanged; no file |
| a duplicate of an existing note, with an attachment | error `cannot create note because it is a duplicate`; the attachment's file stays in collection media |
| an empty first field, with an attachment | error `cannot create note because it is empty`; the file stays |
| a missing deck, with an attachment | error `deck was not found: Nope`; no file |
| `addNotes`: a note with a good attachment, then one whose attachment fails with no `fields` | error `["'fields'"]`; no note added; the first note's file stays |
| `addNotes`: one note whose failing attachment has `fields: ["Gloss"]` | the note added; Gloss holds the error text |
| `updateNoteFields` with `fields: {"Gloss": "g"}` and an audio attachment to `Gloss`; with `fields: {}` | Gloss `g[sound:u1.mp3]`; the reference appended to the current value |
| `updateNote` with `fields` and a picture | the field set, then `<img src="u2.png">` appended |
| `updateNote` with `tags` only and an attachment | tags set; the attachment ignored, no file |
| `updateNoteFields` whose attachment fails with no `fields` | error `'fields'`; the note's fields unchanged |
| `canAddNote`, `canAddNoteWithErrorDetail`, `canAddNotes` with an attachment | `true`, `{"canAdd": true}`, `[true]`; each attachment's file stored in collection media |
| `canAddNote` whose attachment fails with no `fields` | `false` |
