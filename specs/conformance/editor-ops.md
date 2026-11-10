# Editor ops for scenarios: media fetch and config keys

Spec: conformance/editor-ops
Requirements: R1883, R1887, R1876, R870, R871, R115, R138, R139, R140, R149, R848
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/paste/url/, conformance/scenarios/editor/config-keys/

The scenario vocabulary for a media fetch (U197): the setup sections `http_fixtures` and `url_files` and the op `add_media_from_url`. The paste of `editor/paste` downloads the URLs of `editor/paste-sources` S4, S5 and S10 and the `img` sources of P14.2 into the media folder. Anki's page editor (the experimental one, `editor/paste-sources` S1) does it through one call of the collection library, which downloads a URL, derives a file name from it, adds the bytes and returns the name. This op runs that call on the oracle and Meguri's fetcher and importer on Meguri, so that a scenario observes the downloaded name, the stored name and the failures without an editor. The behavior they observe is `editor/paste-sources` S5, S6 and S6.1 (the page editor's column) and `editor/paste` P10. `conformance/schema/scenario.schema.json` and `observation.schema.json` hold the shapes. Draft (`dsl.md` D7.2): the oracle implements them and the scenarios of E5 have goldens. Meguri's executor and the typed model do not have them yet. E6 and E7 add a config write op and an observation kind for the editing keys of `editor/config-keys` (E8: draft, no driver op yet).

## E1. Scope

- No scenario reaches a host outside its executor (R1876). A URL names the scenario's own fixture server or its own file folder through a placeholder (E4). The executor replaces it.
- The op covers one fetch and the add that follows it. The source order, the link fallbacks, the alerts and the snippet a paste inserts stay in the editor and are not observed (`editor/paste-sources` S15).
- Both executors start their fixture server and write their file folder in every segment of a run. No live scenario uses this vocabulary yet, and a handoff (`dsl.md` D9.2) carries none of it.

## E2. Setup section `http_fixtures`

`setup.http_fixtures` is a list of responses. The executor serves them from an HTTP/1.0 server on the loopback interface, which it starts for the scenario after the setup and stops when the scenario ends. Only the executor's own fetches reach it. An entry is a mapping:

| Key | Value | Default |
|---|---|---|
| `path` | the request path it answers: printable ASCII (`!` to `~`), starting with `/`, without `?` and `#` | required |
| `status` | the status code, an integer from 200 to 599 | 302 with `redirect_to`, else 200 |
| `content_type` | the exact `Content-Type` value sent, printable ASCII with no leading or trailing space | no `Content-Type` header |
| `text` or `fill` | the body, as `media-ops.md` F2; at most one | an empty body |
| `delay_ms` | milliseconds to wait before the status line, 0 to 35,000 | 0 |
| `byte_delay_ms` | milliseconds to wait before each body byte, after the headers, 0 to 15,000 | 0 (the body at once) |
| `redirect_to` | the `path` of another entry, sent as an absolute `Location` on the server's origin; only with a status of 301, 302, 303, 307 or 308. Such a status without it is sent with no `Location` | none |

- **Matching.** A request matches the entry whose `path` equals the request target up to its first `?`, byte for byte. The query is ignored, and escapes are not decoded: `/a%2Fb.png` and `/a/b.png` are two paths. A request no entry matches gets 404 with an empty body. Scenarios write URLs in the form every client sends unchanged: ASCII, with each other byte as `%` and two upper-case hex digits, and no `.` or `..` segment that a client resolves (`/x/../y.png` reaches the server as `/y.png`).
- **Response.** The headers are the status line, `Content-Type` when given, `Location` when the entry has `redirect_to` and `Content-Length` (except for 204), then the body. A 204 entry has no body. With `byte_delay_ms`, the server sends the headers at once and then each body byte after the wait, so the exchange lasts `byte_delay_ms` times the body size while no gap exceeds `byte_delay_ms`.
- **Refused as `invalid_scenario`** when the setup runs: a malformed entry, two entries with one `path`, or a `redirect_to` that names no entry.

## E3. Setup section `url_files`

`setup.url_files` is a list of `{name, text}` or `{name, fill}` (`media-ops.md` F2). Names follow `media-ops.md` F3 and appear once. The executor writes the files into a folder of its own, outside the media folder, which `{files}` names (E4). The section runs after every other setup section. Two names that are case variants of each other are not refused, but the scenarios use none.

## E4. Op `add_media_from_url`

One fetch and the add of its bytes.

- **Argument** `{url}`. `url` starts with one of these placeholders, and no scenario writes any other URL:
  - `{fixture}/…`: the executor replaces `{fixture}` with its fixture server's origin, `http://127.0.0.1:<port>`. Without `setup.http_fixtures` the step is `invalid_scenario`.
  - `{files}/…`: the executor replaces `{files}` with the `file://` URL of the `url_files` folder (an absolute path, with no trailing `/`). The rest is written as a file URL path: `{files}/b%20c.JPG` names the file `b c.JPG`.
- **Output** `{add_media_from_url: {name}}`. `name` is the name the fetch derived from the URL, before the media store's transform. The stored name may differ: `editor/paste-sources` S6.1 gives the cases. `media_dir` (`media-ops.md` F9) shows what was stored.
  - Oracle: Anki's add-media-from-URL call of the collection library, which the page editor uses. It returns the derived name (`editor/paste-sources` S6, page column).
  - Meguri: the Rust core's media fetch service (`editor/paste` P10.2, P10.3), the same code the app's fetcher calls, then the media importer (P4). P4's size question (a 0-byte or oversized file) is answered Add Anyway, with no UI. Meguri's output is its stored name (P10.1). The rows where the two names differ carry the deviation `editor-paste-url-stored-name` (`editor/paste-sources` S6.1, S14). Decided 2026-11-10 (`editor/paste` P10.2): the fetch and naming live in the core so that `meguri-cli`, which runs the core in-process (`dsl.md` D12), executes this op; the plan's U197 file list gains that core module (follow-up for the plan).
- **Rejection** `fetch_failed` (`dsl.md` D7.3): the fetch failed and nothing was added. This covers a status the fetch refuses, a timeout, a network error, a missing or unreadable file, and a URL or name the fetch refuses as invalid. Anki gives only a localized message for these. That message is not compared (R1887), so all of them are this one kind.
- **Run errors.** An add that fails after a successful download is a run error, as for `add_media` (`media-ops.md` F5). So is a URL without a placeholder.

## E5. Scenarios

In `conformance/scenarios/editor/paste/url/`, inactive:

| Scenario | Covers |
|---|---|
| `editor/paste/url/names` | `editor/paste-sources` S6: the query and fragment dropped, the last segment, `%2F` decoded after the split, `%FF` refused, a trailing `/`, a whitespace-only segment, `.hidden`, `a.`, the media-type table of `editor/paste` P10.3 row by row, types that do not match exactly (parameters, case, types outside the table, no header), a name that has an extension, `..png` |
| `editor/paste/url/status` | `editor/paste-sources` S5: 200, 204, 206 and an empty 200; 404, 500 and a path with no entry; one redirect (name from the URL asked for, type from the final response); a chain of 10 redirects and a chain of 11; a 302 without `Location` and a 304, neither followed |
| `editor/paste/url/timeout` | `editor/paste-sources` S5: 31 s before the first byte; a body sent byte by byte over 33 s with gaps of 11 s |
| `editor/paste/url/stored-name` | `editor/paste-sources` S6.1: the asked-for name against the stored name (lower case, the hash suffix over a present file, `a/b.png`) |
| `editor/paste/url/file` | `editor/paste-sources` S5, S6: `file://` URLs of a present file, one with an encoded space, a missing one, one whose escapes are not UTF-8, an encoded `#` whose truncated path `a` exists, and one whose truncated path `c` does not |

### E5.1 Meguri's expected values

What Meguri's executor must record for each step, by `editor/paste` P4 and P10. Where it differs from the golden, the deviation named is entered in `conformance/deviations.yaml`, with a `replace` holding these values, in the change that lands Meguri's adapter and activates the scenario; until then the scenarios stay inactive. Steps are numbered from 1; `failed` is `{error: fetch_failed}`; a hash is the SHA-1 of the step's fixture body.

`editor/paste/url/names` (`editor-paste-url-stored-name`, `editor-paste-mime-params`, `editor-paste-url-slash`, `editor-paste-url-invalid-escape`, `editor-paste-leading-dots`):

| Steps | Meguri's `name` |
|---|---|
| 1 to 6 | `pic.png`, `img.png`, `paste.jpg`, `x.png`, `paste.png`, `paste-7a4c5950371ac7a4b2c077c2e96fd9f567890c7e.png` |
| 7 to 12 | `a.png`, `b.png`, `ab.png`, `a b.png`, `日.png`, U+FFFD `.png` |
| 13 to 18 | `y.png`, `.hidden.png`, `a._`, `play.weba`, `play`, `notes.tar` |
| 19 to 29 | as the golden (`mpeg.mp3` to `avif.avif`) |
| 30 to 35 | `gif`, `mp4`, `upper.jpg`, `params.mp3`, `none`, `..png.png` |

`media_dir` is the golden's with `paste` named `paste.jpg`, `x` named `x.png`, `upper` named `upper.jpg`, `params` named `params.mp3`, `ab.png` (the `slash page bytes`) named `b.png`, `ab-bee1e6c9fbdfd6ea5572502b4ae589d5e8df2e37.png` named `ab.png`, `..png` named `..png.png`, and one more file, U+FFFD `.png` (SHA-1 `a230061f695616289efebb26eac66b71ebdc7263`, 20 bytes).

`editor/paste/url/status` (`editor-paste-fetch-status`): steps 1 to 14 are `ok.png`, failed, failed, failed, `empty.png`, failed, failed, failed, `moved.png`, `seeother.png`, `r1.png`, failed, failed, failed. `media_dir` is the golden's without `nocontent.png`, `partial.png`, `accepted.png`, `bare.png` and `notmodified.png`.

`editor/paste/url/timeout` (`editor-paste-fetch-timeout`): failed, `trickle.png`, `quick.png`. `media_dir` is the golden's plus `trickle.png` (SHA-1 `a9993e364706816aba3e25717850c26c9cd0d89d`, 3 bytes).

`editor/paste/url/stored-name` (`editor-paste-url-stored-name`, `editor-paste-url-slash`): `pic.png`, `pic-08811aba945e9bf73dd3e12f16d85ceb8863779e.png`, `b.png`. `media_dir` is the golden's with `ab.png` named `b.png`.

`editor/paste/url/file` (`editor-paste-url-stored-name`, `editor-paste-file-url-hash`): `b c.jpg`, `plain`, failed, failed, `a#b.png`, `c#d.png`. `media_dir`: `b c.jpg` and `plain` as the golden's, `a#b.png` with SHA-1 `3b20c52c8420fb9a95a52b0c8b07b332ca989b78` (10 bytes, the file `a#b.png`), and `c#d.png` with SHA-1 `07414acc1f28e4516ab9bb54c72a559fb4017aaf` (17 bytes).

## E6. Op `editor_config_set`

Added 2026-11-10 for `editor/config-keys` K7 and K8, modelled on `conformance/browser-config-ops` B1. It writes one of the editing keys through the write the app uses, so that a scenario observes the stored bytes and stamps of Meguri's own write.

- **Argument** `{key, value}`:

  | Keys | Value |
  |---|---|
  | `pasteImagesAsPng`, `pasteStripsFormatting`, `addToCur`, `shrinkEditorImages`, `closeHTMLTags` | boolean |
  | `firstDayOfWeek` | one of the integers 0, 1, 5 and 6 (K5) |

  Any other key, or another value, is `invalid_scenario`. `customColorPickerPalette` is not in the list: Meguri's palette write applies K9's add or remove to the stored value, and Anki's collection library has no such write, so scenarios write the palette's bytes with `set_config` (`record-ops.md` W4; scenario `editor/formatting/options/key-writes`).
- **Effect.** The key is set to the value as one operation with no undo entry (`storage/config-table` C4): it keeps the undo steps and empties the redo history (`engine/undo` A2).
  - Oracle: the collection's boolean setter for `pasteImagesAsPng`, `pasteStripsFormatting` and `addToCur`, its generic config setter for the other three, each without an undo entry. `firstDayOfWeek` is not written through Anki's graph preferences save, which also writes two other keys (`editor/config-keys` K6). Anki writes when the value's compact bytes differ from the stored bytes.
  - Meguri: the whole-value write of `editor/config-keys` K1 (K8 for its two keys), the operation the Settings pages, the Stats menu and the editor's options menu run once they have decided to write. The app's own test, that the user picked a value different from the one read (K1), happens before that operation and is not part of the op: the op writes whenever the value differs from the stored value (deviation `config-same-value-other-bytes`, as B1 says), so a value of another type is replaced.
- **Output** none (`{}`).

## E7. Observation kind `editor_config`

- **Request** `editor_config`, no arguments.
- **Value** an object with exactly these four members, each the value Anki's own read gives:

  | Member | Value |
  |---|---|
  | `pasteImagesAsPng`, `pasteStripsFormatting`, `addToCur` | boolean: Anki's boolean read with the default of `editor/config-keys` K1 (`false`, `false`, `true`) for a missing or unreadable value |
  | `firstDayOfWeek` | integer: the week start of Anki's graph preferences read (`calendar_first_day_of_week`), one of 0, 1, 5 and 6; any other stored value reads 0 (K5) |

  Meguri's adapter reports its typed reads of K1 and K5, the ones the app uses.
- `shrinkEditorImages`, `closeHTMLTags` and `customColorPickerPalette` are not members: Anki reads them only in its editor page (`editor/config-keys` K10), so its collection library has no read to compare. Scenarios read their stored bytes with `db_exec`; Meguri's tests cover K8's and K9's reads.
- Nothing in the collection changes: no row is written and `col.mod` does not move.

## E8. Status of E6 and E7

Draft (`dsl.md` D7.2, `observations.md` O4): no driver op, schema shape or typed model exists yet. The oracle maintainer adds them together, as `conformance/AGENTS.md` asks. Once they exist, `editor/config-keys` K7's `bool-defaults`, `first-day-of-week` and `write-scalar` are written in `conformance/scenarios/editor/config-keys/`, and they stay inactive until Meguri's adapter implements both.
