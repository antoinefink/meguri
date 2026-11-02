# CloudKit scoped type-change contract review

Status: inactive proposal; no specification, code or Unit acceptance.
Revision start: 6759694af7815c51641331ea0f63e0f74293abbb.
Original proof start: 06f93ebf000fe0cb6262d88d43af5a78fd86d16a.
Spec-writer session: s-20261102T213243Z-4ad02362.
Integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.

The existing draft proves that a joined generation can need different maps
for two notes with the same source and destination types. This work completes
the proposed representation and recovery rules before implementation. Ordinary
unchanged-layout reviews and media use the existing path and do not await it.

Independent review of the first complete proposal finds three P1 issues:
the source point must account for notes created and sent during an older
unrelated generation; duplicated field sources need defined reverse mapping
and preservation of competing values; a collision after claim needs to retain
the correct member identity. All three must be fixed before integration.

The second proposal fixes the first two issues. Its single-manifest identity
repair leaves two P1 cases: restoring a scoped note whose payload never arrived,
and settling one identity shared by overlapping claimed generations. The third
revision fixes both; independent review finds no P0–P2 issues. It uses validated
history references for absent notes and holds later overlapping claims until
the earlier identity is settled. Retained payloads keep earlier work sendable
without losing later local changes. No runtime result is claimed.

Privacy checks of the first two proposals fail anonymously. They remain under
build, outside accepted source. Whole authored regions are rewritten; no
private words are disclosed and no guard is changed. The original packets and
review findings remain retained. Session60897 reports a lock error; the later
authorized session62836 succeeds. No lock directory is removed or bypassed.

Later privacy revisions preserve the reviewed protocol. Independent comparison
of v4 and final v11 finds no meaning, schema, limit or test loss. V11 still fails
the private-word check in two authored regions. Publication is deferred; the
full packet and exact patch remain in build, with all prior failures retained.
No guard change, exception, normative promotion or Unit acceptance occurs.
Reuse this prepared draft when finishing integration; do not repeat its research.

All implementation, required Q11 tests, signed integration and milestone
evidence remain outstanding for this amendment. The live ordinary-review
check separately awaits an unlocked host session; see the C12 review log.
