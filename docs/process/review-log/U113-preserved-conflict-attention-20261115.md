# U113 preserved conflict attention

Starting commit: `0f1ce99aee1195345dba90a42d89b0dbecf11abe`. Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

P14.4/P22 and V11/V17.2 keep unresolved copies in attention until resolution. Runtime counted only copies of its bound binding. It now reads preserved copies before that branch, including isolated, salvage and other-build owners. Resolved copies are excluded. Read failures propagate. Publication counts and write authority keep their existing scope. Global attention completeness stays false.

Filtered author: `cleanroom/20261115T111812Z-64680`. Independent review: `cleanroom/20261115T112329Z-67367`, CLEAR, no P0–P2 findings. Root read the entire patch. The earlier review setup `112159Z-65491` refused a changing input before review; its receipt is retained.

Evidence:

- Tests alone: `core-test/20261115T112204Z-65888`, five expected failures. Four preserved copies produced one; unbound copies produced none.
- Production fix: `core-test/20261115T112233Z-66244`, all nine projection tests passed. Coverage includes reopen, resolution, removal, salvage, other builds and damaged tracking input.
- Full host policy target: `core-test/20261115T112523Z-68649`, 97 passed, zero failed or skipped.
- Privacy: `private/20261115T112502Z-68319`, two files passed.
- Similarity: `similarity/20261115T112254Z-66918`, two files, zero flags. Root read only the implementer view.

Copy fixtures are inputs to real Runtime projection. They do not prove the conflict-resolution interface. Listing still reads retained payloads; no performance improvement is claimed. Unit and release acceptance remain pending.
