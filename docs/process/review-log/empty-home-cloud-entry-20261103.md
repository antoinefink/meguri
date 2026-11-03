# Empty collection iCloud entry

Status: WIP; no unit acceptance.
Start:9f700afcbd63d3e68e97131e16da91d011e7c496.
Implementer:s-20261103T155953Z-89699f89.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.

R161 requires Connect to iCloud on Empty Home. The actual phone showed Import
alone; the toolbar route did not meet that requirement. Export78169 adds a native
button using the existing entry, availability reasons and localized title.
Independent review has no P0-P2 findings. Import retains its existing action.

Private81498 flags one comment. Export82993 removes only that comment; a separate
narrow review confirms identical executable code. Private84233 and Similarity84891
pass the final file. Compilation and actual phone action/layout checks remain
pending. No new connection flow, native ownership or Core behavior is introduced.

Phone86370 compiles the combined source and shows both empty-state buttons. The
direct Connect action and readiness wait succeed. The final capture has no active
reading indicator and shows the generic operation-error branch with no preview.
No adoption, download or replacement is confirmed. Hierarchy findings keep the
drive failed, including the existing Try Again button without an identifier.
Native appearance was inspected; final design agreement and full gates are open.

Session74365 could not create its Git lock in the sandbox and recorded no session.
The permitted host session76059 succeeds; no lock was removed or guard bypassed.
