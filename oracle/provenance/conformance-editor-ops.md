# Editor fetch ops (U197)

Oracle-maintainer notes for `specs/conformance/editor-ops.md` and the driver
module `oracle/driver/ops/url_media.py`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied into specs or scenarios. Written
2026-11-10 (session s-20261110T085429Z-165d83d3).

Meguri's own design, with no Anki source: the setup sections `http_fixtures`
and `url_files`, the `{fixture}` and `{files}` placeholders, the fixture
server's matching and response rules (E2), and the single rejection kind.

| Spec section | Anki files read | Read for |
|---|---|---|
| editor-ops E1, E4 | `proto/anki/media.proto` (add-from-URL request and response), `rslib/src/backend/media.rs` (the call: download, add, the derived name returned, error text in the response, the add's failure as a backend error), `pylib/rsbridge/lib.rs` (backend calls release the GIL, so an in-process fixture server answers during the call), `qt/aqt/mediasrv.py` (the page editor reaches the call through the media server's exposed backend methods) | that the page editor's fetch is one collection-library call; that every fetch failure is a response text (so one kind) |
| editor-ops E2 | `rslib/src/editor.rs` (client timeout over the whole request, User-Agent, status check) | which fixture behaviors tell the rules apart: a delay before the first byte, a slow body, redirect chains (the redirect limit is from the recording) |
| editor-ops E3 | `rslib/src/editor.rs` (local file read: whole-URL decode, then parse and read) | why a `{files}` URL is written in file-URL form |

Recording 2026-11-10 (`tools/mg oracle record editor/paste/url`): every
page-editor row that `specs/editor/paste-sources.md` S5, S6 and S6.1 gave as draft
matched. New facts recorded and added to the spec: the name after a redirect
comes from the requested URL and the type from the final response (S6); an
encoded `#` in a `file://` URL reads another path (S5, S14).

Review 2026-11-10: E5.1 (Meguri's expected value per step) and E4's choice of
a core fetch service are Meguri's own design, derived from `editor/paste` P4,
P10.2 and P10.3 with no Anki source. The `file` scenario gained a present `a`
and a second encoded `#` fetch, and `names` a `..png` step (`rslib/src/editor.rs`
and `pylib/anki/media.py` extension tests); both need re-recording.

Addition 2026-11-10 (spec writer, same session): E6 to E8, the editing-key
write op and observation kind that `specs/editor/config-keys.md` K7 requests.

| Spec section | Anki files read | Read for |
|---|---|---|
| editor-ops E6 | `rslib/src/config/bool.rs` (the three typed boolean keys), `rslib/src/backend/config.rs` and `pylib/anki/collection.py` (boolean and generic setters with an `undoable` flag), `rslib/src/stats/graphs/mod.rs` (the graph preferences save writes the week start and two other keys) | which setter the oracle uses per key; why `firstDayOfWeek` is not written through the graph preferences save |
| editor-ops E7 | `rslib/src/config/bool.rs` (boolean read with per-key default, any decode failure giving the default), `rslib/src/config/mod.rs` (optional typed read; the week-start read and its four-value enum), `rslib/src/stats/graphs/mod.rs` and `proto/anki/stats.proto` (graph preferences read field) | the four reads and their defaults; that the two option keys and the palette have no collection-library read |
