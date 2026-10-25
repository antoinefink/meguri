# Repeated row IDs in normal-sync chunks

Spec: sync/normal-sync N6.5, sync/wire-format W7.1, sync/hold-back H6/H10.2.
Session: s-20261025T132312Z-c047b13c

The existing neutral N6/W7 contracts impose no unique-ID requirement on
tuple arrays. A Meguri-owned bulk-stage failure with repeated fresh IDs
prompted this clarification; the diagnostic fixture was not changed.

Only the relevant merge functions were read at the verified exact tag
26.09.3 in refs/anki-26.09.3: rslib/src/sync/collection/chunks.rs, the
apply_chunk/merge_revlog/merge_cards/merge_notes paths and their pending-USN
predicate; plus rslib/src/storage/revlog/add.sql for insert-or-ignore.
No other reference implementation or runtime evidence was used, and no code
or reference fixture was copied. The example stamps are original synthetic
values, not reference test data.

Those paths process each table's incoming tuples in order, querying the
current row for each card/note acceptance decision, and retain the first
existing review row. Therefore a clean accepted incoming row can remove the
pending condition before a later tuple with a lower modification time.

H6/H10's bounded staging and live-row rules are Meguri-owned design. They
must reproduce that ordering while preserving held/post-snapshot changes,
pending graves, unique target reporting and complete commit journals. A
single final candidate does not suffice for every possible live pending row.
The bounded summary in H6 is a Meguri-owned equivalence argument: all initial
pending rows below the stream's maximum modification time converge at its
first maximum, then have identical later decisions. Its effective payload
is evaluated from absent, with the original pending predicate. This covers
received pending USNs too, which the strict i32 wire reader does not exclude.

Independent review U107-REPEATED-CONTRACT-R1-01 showed that this row-choice
summary is insufficient for conditional validation errors. Current H10.2 and
the plan's U107 Approach 6 apply the pending/mtime rule to live post-S rows;
only marking and config have separate post-S touch protection. Therefore a
fixed-S first-fault cache cannot generally replace validation for a different
live threshold. The bounded correction stores scalar validation records in the
existing scratch stage, not repeated row bodies or a general history service.
This correction changes no reference behavior or post-S precedence rule.
This note supplies no implementation, golden freshness or unit acceptance.
