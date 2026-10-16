# Deleted-notes log

Spec: notes/deleted-notes-log
Requirements: R781, R1594
Anki: 26.09.3
Scenarios: none (the oracle runs without Anki's desktop shell, which writes this file; L6)

A text file next to the collection to which every note deletion appends the deleted notes, so the user can recover their text (R781, R1594). Its layout is the one of Anki's `deleted.txt`, which Anki's text importer reads (U121).

## L1. The file

- Name `deleted.txt`, in the directory that holds the collection file (`collection.anki2`), next to it.
- Device-local: it never syncs and is not part of backups, exports or `.colpkg` packages.
- Only appended to. Nothing in Meguri reads, rewrites or truncates it.

## L2. Layout

UTF-8 text, every line ending with LF (U+000A). When the file does not exist at the moment of an append, the append first writes these three lines (TAB is U+0009):

```
#guid column:1
#notetype column:2
#nid<TAB>mid<TAB>fields
```

Then one line per deleted note:

```
<guid><TAB><note type id><TAB><field 1><TAB><field 2>...<TAB><field n>
```

- `<guid>`: the note's `guid` as stored.
- `<note type id>`: the note's `mid` in decimal.
- The fields: the note's stored `flds` split at U+001F, joined with TAB, as stored, without quoting or escaping. A field holding TAB or LF therefore breaks the line's columns, as in Anki's file.

A file that exists but is empty gets no header lines.

Example: deleting a note with GUID `c~MJPzD`, note type id 1894701600006 and fields `私` and `I` from a collection with no log yet writes

```
#guid column:1
#notetype column:2
#nid	mid	fields
c~MJPzD	1894701600006	私	I
```

Anki writes the note's id (decimal) in the first column, under the header that names it the GUID column. Meguri writes the GUID, as R781 asks and as the header says, so that importing the file brings the note back with its identity. No scenario observes the file (L6); the difference is recorded here, not in `conformance/deviations.yaml`.

## L3. When

- The note deletion of `notes/note-records` D1 appends one line per note that exists when it runs, in ascending note id order, before it removes the rows. A deletion whose notes all no longer exist appends nothing and does not create the file.
- One deletion opens the file once, writes the header when needed and all its lines, and closes it.
- Nothing else appends: deleting a deck or a note type, Empty Cards, removing cards that leave a note empty (`notes/note-records` D2), incoming sync deletions, imports, undo and redo.

## L4. Failure

When the append fails (no space, no permission), the deletion fails with an I/O error and removes nothing. Lines an append wrote before the failure stay. A deletion that fails after a successful append leaves its lines in the file.

## L5. Undo

Undoing a deletion leaves the file as it is: the lines stay (the coverage note on R781). Redoing the deletion appends nothing.

## L6. Testing

Anki writes this file from its desktop shell, not from the library the oracle runs, so no golden exists. Meguri's tests check L2 and L3 with the example above, a second deletion appending without headers, notes in ascending id order whatever the order of the request, a skipped missing note, an undo that leaves the file unchanged, and L4 with a read-only directory.
