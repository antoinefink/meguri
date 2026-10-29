# U114 registration failure

Status: narrow repair verified; no Unit trailer.
Prepared from `f94238ea`; applied after `43b6f6dc`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

This closes retained P2-REG. Registration installs SDK authority after pending
discovery succeeds. A failed attempt leaves no hidden owner. Root confirmation
is still cleared before discovery, and identifiers remain monotonic.

Filtered fixer: `cleanroom/20261029T062352Z-43100`.
Independent narrow review found no P0-P2. The new real-CoreHost test injects a
failure at pending discovery and checks the error, absent SDK authority, unchanged
updates, direct retry without retire/release, and the pending lists before and
after root confirmation. The stable-binding fixture remains confined to this
registration test; it does not prove first publication.

- `core-format/20261029T063156Z-48388`: pass.
- `core-test/20261029T063252Z-49102`: new regression passes, none skipped.
- `core-test/20261029T063430Z-50719`: all 10 adjacent adapter and review tests
  pass, none skipped, including the 400-record/eight-call case.
- `private/20261029T063255Z-49364`: two files pass.
- `similarity/20261029T063257Z-49612`: two files, zero findings or exemptions.
- `swift-test/20261029T063634Z-52815`: all 33 Swift CloudKit adapter cases pass
  against the rebuilt core, none skipped.

Publication targets remain enabled RED pending their production dependencies.
Full lint/gates, signed SDK checks, adoption, incoming data and media remain open.
