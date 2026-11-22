# Card media origin, page pair and native bridge

Spec: render/media-serving
Requirements: R465, R475, R477, R478, R490, R493–R504, R511, R522, R560, R1857, R1862, R2038
Anki: 26.09.3 (interoperability); native component and security rules are Meguri design
Scenarios: none. Native handler and WebKit tests use synthetic files; the headless oracle has no card page.

This contract joins `render/card-runtime` (C66 below), U65's S1/S2 decisions and the U67 component. It defines loading and native boundaries, not new card rendering or a copied web asset. HTML, AV lists and type-in data come from U62/U64; audio queue policy is `render/audio-playback`. U69 owns additional codecs and video tags, U70 navigation and recovery, U175 session policy, and U176 review layout.

## M1. Fixed origin and resource routes

The document URL and base URL are `meguri-media://media/`. Relative and root-relative media, CSS URLs, `fetch()` and inline dynamic module imports therefore start at the media root. The scheme and lower-case host stay fixed over sessions, launches and upgrades. A page at a bundled-asset URL plus a base element is insufficient for inline module imports (S2's origin-root amendment).

| Path | Resource |
|---|---|
| `/` | Meguri's warm card document, never a media file or directory listing |
| `/_anki/…` | named bundled assets only; no fallback to collection media |
| `/_addons/…` | 404, including when a same-named media entry exists |
| other allowed path | one regular media file from the active collection's flat `collection.media/` folder |

Routes are selected after safe decoding (M2), before file lookup. Reserved names cannot be reached through encoded spelling or fallback case matching. Reserve the bare `/_anki` and `/_addons` paths too. ASCII case variants of either reserved first component cannot select collection media. Bundle lookup is an explicit asset map, not arbitrary bundle-file access.

Only this scheme/host, without user information or an explicit port, is served. Reject other authorities; never turn their URL text into a local path. Queries do not name files and are ignored for lookup; fragments are browser-side and never select another file. An encoded `%3F` or `%23` is instead a literal file-name character after decoding. The document root remains the document even with a query.

The root is supplied by the open collection owner, not by a card message. Requests retain their collection/root binding through completion. Closing or replacing that context cancels old tasks; a delayed request must not read the next collection's folder.

## M2. Path validation and lookup

URL paths and native AV file names are different inputs. Decode valid percent triplets once only, reading the resulting bytes as UTF-8. Leave a percent sign that has no two following hex digits literal, as the display encoder can leave raw percent text (`render/media-display#D3`). Reject invalid decoded UTF-8 rather than aliasing it to another file. A plus sign is a plus sign, not a space. Do not apply form decoding or decode a second time.

The media folder is flat (`media/media-files#M1`). After removing the one origin-root slash, a media request names one nonempty component. Refuse NUL, backslash, slash within that component, a component equal to `.` or `..`, and any path containing more components. Reject encoded separators `%2F` and `%5C`, in either hex case. Check the path available from the URL parser before joining it to a filesystem path; any dot segments visible before URL normalization are refused. Browser normalization that has already removed a segment grants no filesystem access beyond this one-component rule.

File lookup is read-only. For the decoded component:

1. Prefer the exact NFC spelling.
2. Then try the exact NFD spelling, for an imported file on a volume that distinguishes them.
3. Then use the existing media store's case key and tracked-name index (`media/media-files#M2`, M5): prefer the exact lowercase candidate when present, else the smallest matching name by UTF-8 bytes. Do not use locale-sensitive collation or full case folding.

At every step the result must be a regular file directly inside the bound media root. Never follow symbolic links, including a link resolving back into that root. A linked root or a root whose identity changed must be revalidated by its owner before use. Check/open without following a substituted link; merely checking a canonical string and later opening an unchecked path is insufficient. Directories, device files and sockets cannot be responses. A denied path returns 404 without exposing the absolute path or bytes outside the root.

The component takes a root plus tracked names and update notifications from its collection owner. U31 defines their case keys. Never list the directory for each request. Unit tests inject names; the real native connection is still pending. As media/media-files M5 permits, direct NFC/NFD probes can also read an untracked regular file. Order: NFC, NFD, case match. Content may differ between colliding names; return the winner without a write and let Check Media handle repair.

| Synthetic request / folder | Result |
|---|---|
| `/my%20file.jpg`; `my file.jpg` exists | that file |
| `/caf%C3%A9.png`; only NFD `cafe` + U+0301 + `.png` exists | that file |
| `/PHOTO.JPG`; tracked `photo.jpg` exists | `photo.jpg` |
| `/MiXeD.png`; tracked `MIXED.png`, `Mixed.png` exist | `MIXED.png` (UTF-8 tie-break) |
| `/x%2520y.png`; file `x%20y.png` exists | that file, not `x y.png` |
| `/a+b.png` | file named `a+b.png` |
| `/100%%20sure.png` | file named `100% sure.png` |
| `/a.png?revision=2#part` | `a.png` |
| `/%2e%2e/`, `/a%2Fb.png`, `/a%5Cb.png`, `/a/b.png` | 404, no file read |
| `/escape.png` is a symlink outside the root | 404 |
| `/_addons/x.js`, `/%5Faddons/x.js` | 404, no media fallback |

For native AV input, field text is already decoded (`render/av-tags#A6`). Use its last slash-delimited component and the safe filename lookup; do not URL-decode it. AV `%20` is literal percent text; URL `%20` is a space. Refuse empty names, dot/dot-dot, NUL and backslash. A filename cannot request remote content, another directory or native execution.

## M3. Types, cross-origin access and script policy

Use the extension of the resolved file name, ASCII case-insensitively. The following are required mappings; no whole-file read is needed to choose them.

| Extension | Content-Type |
|---|---|
| html, htm | `text/html; charset=utf-8` |
| xhtml | `application/xhtml+xml; charset=utf-8` |
| svg | `image/svg+xml` |
| css | `text/css; charset=utf-8` |
| js, mjs | `text/javascript; charset=utf-8` |
| json | `application/json; charset=utf-8` |
| txt | `text/plain; charset=utf-8` |
| png / jpg, jpeg / gif | `image/png` / `image/jpeg` / `image/gif` |
| webp / avif | `image/webp` / `image/avif` |
| ttf / otf / woff / woff2 | `font/ttf` / `font/otf` / `font/woff` / `font/woff2` |
| mp3 / m4a / aac / wav / flac | `audio/mpeg` / `audio/mp4` / `audio/aac` / `audio/wav` / `audio/flac` |
| mp4, m4v / mov / 3gp | `video/mp4` / `video/quicktime` / `video/3gpp` |
| ogg, oga / opus / spx / ogv / webm | `audio/ogg` / `audio/ogg` / `audio/ogg` / `video/ogg` / `video/webm` |
| other | `application/octet-stream` |

MIME support does not prove decoding support; U69 owns non-native codecs and video-tag dispatch. SVG fonts remain unsupported. There is no app-imposed font count or face-size cap (R501).

Return `Access-Control-Allow-Origin: *` for fonts and modules; other read-only responses may use it too. Where cross-origin clients read range metadata, expose `Accept-Ranges`, `Content-Range`, `Content-Length`. Do not reflect request header values or introduce cookies/credentials.

Set `Content-Security-Policy: script-src 'none'` on media of type HTML, XHTML or SVG. Omit CSP from the root document and runtime assets, retaining inline handlers, evaluation and card-referenced scripts. Media also gets `X-Content-Type-Options: nosniff` so unknown extensions are not executable documents. Keep S2's separate positive/negative controls: scripts blocked in embedded/top-level HTML/SVG media, yet a card src script runs. Do not restrict the page to fix a media response.

## M4. GET, HEAD and one byte range

Successful file responses include `Accept-Ranges: bytes`. GET without a range returns 200, full length and the unchanged file bytes. HEAD returns the same metadata as an unranged GET and no body; ignore its Range header. Other methods return 405 with `Allow: GET, HEAD` and no file body. Resolve/refuse the resource before evaluating its range, so a missing file stays 404.

For a single valid byte range, positions count from zero and include both ends. Return 206, `Content-Range: bytes first-last/total`, `Content-Length` equal to the returned byte count and exactly that slice. A bounded end past EOF is clipped. An open end means EOF. A suffix chooses the last requested number of bytes, or the whole nonempty file if shorter. A range whose start is at/past EOF, or a zero suffix, returns 416 with `Content-Range: bytes */total` and an empty body. A zero-length file returns 200 with an empty body regardless of Range.

These details follow [HTTP Semantics, sections14 and15.5.17](https://www.rfc-editor.org/rfc/rfc9110.html#section-14); the custom handler's bounded policy below is Meguri's choice within that protocol, not a new observation about the reference app.

- Only one bytes range is implemented; ignore a multiple-range request and send the normal 200 representation, never an incorrect concatenation or a partial first range.
- Ignore an unknown range unit, malformed syntax, reversed bounds or an incomplete number; send 200. Strip surrounding SP/TAB and allow SP/TAB around the one range item, not inside its digits or hyphen. The `bytes` unit is ASCII case-insensitive.
- Parse arbitrarily long unsigned decimal inputs without overflow or traps. A huge first position is unsatisfiable; a huge last position clips to EOF; a huge nonzero suffix chooses the whole file. Do not parse through floating point or wrap integers.
- No conditional cache validator is offered. A request with `If-Range` gets the full 200 response, since the handler cannot establish that condition. Do not report a fabricated ETag.

The table uses the independent ten-byte fixture `0123456789`, not the production range parser as an oracle. All rows require exact headers and body bytes; retain S2's existing hand-worked tests.

| GET Range | Status | Content-Range | Content-Length | Body |
|---|---|---|---|---|
| absent | 200 | absent | 10 | `0123456789` |
| `bytes=2-5` | 206 | `bytes 2-5/10` | 4 | `2345` |
| `bytes=0-0` | 206 | `bytes 0-0/10` | 1 | `0` |
| `bytes=8-20` | 206 | `bytes 8-9/10` | 2 | `89` |
| `bytes=7-` | 206 | `bytes 7-9/10` | 3 | `789` |
| `bytes=0-` | 206 | `bytes 0-9/10` | 10 | `0123456789` |
| `bytes=-3` | 206 | `bytes 7-9/10` | 3 | `789` |
| `bytes=-20` | 206 | `bytes 0-9/10` | 10 | `0123456789` |
| `bytes=10-`, `bytes=-0` | 416 | `bytes */10` | 0 | empty |
| `bytes=5-2`, `bytes=x-`, `items=1-2`, `bytes=0-1,8-9` | 200 | absent | 10 | `0123456789` |

HEAD of this file, with or without Range: 200, length10, no Content-Range, no body. Missing file:404; invalid path:404. Tests must also cover huge numerals, empty files and a cancellation while streaming a range.

## M5. Stream lifetime and live files

Filesystem lookup, stat, open, seek, read and index work run off the main actor. Deliver ordered response/data/completion callbacks on the required WebKit executor. A response streams bounded chunks (at most64KiB per read), with bounded outstanding work per task; never load a whole audio/video file into a Data value or enqueue the whole file as pending callbacks. Memory use must be independent of file length. HEAD does not read the body. File lengths and ranges use checked wide arithmetic.

A task has one terminal outcome. On WebKit stop/cancellation, invalidate its delivery permission before closing its file and discarding pending work. No response, data, finish or failure callback may occur after stop, including a worker result already queued for delivery. Concurrent stop, finish, root closure and read error must neither double-complete nor retain the handler/page. A truncated file cannot finish successfully after fewer bytes than advertised. A replacement by atomic rename may finish from its already-open old inode; a later request sees the new file.

Do not negatively cache 404s. Each request checks the current bound root/index snapshot; owner-supplied media-arrival/index updates refresh lookup state without rebuilding a page or scanning the folder per fetch. Return `Cache-Control: no-store` for collection media and failures, so stale success or failure caching does not hide an arriving/replaced file. Bundle caching may use its build identity.

`mediaArrived(names)` retries matching failed resources in the visible side in place, preserving DOM/script/input state. Reset the failed resource's load attribute or reload its resource element; do not call the side renderer. CSS/font failures must also be eligible for a focused reload where WebKit allows it. If a resource cannot be retried in place, report that limitation rather than silently claiming a reload. Idle pre-render invalidation/re-prepare belongs to U175. A retried resource that still fails keeps a diagnostic.

Missing-resource diagnostics retain logical name/URL, card and side, including CSS loads without an element error. Bind a request to its page/update at start; a later current-card value cannot identify it. Exclude absolute app paths and raw card text. Read no extra file merely to explain an error. Combine repeats per card/side while retaining each unresolved name.

## M6. Configuration and website state

All review and preview surfaces share one persistent `WKWebsiteDataStore` identifier, origin and normal WebKit storage. Choose one production identifier and keep it stable; never use the S2 spike's identifier, a fresh UUID per launch, or a nonpersistent store. A fresh page reads the same localStorage and IndexedDB as another surface and after a normal relaunch; another data-store identifier is isolated. `sessionStorage` is per-page: retained across sides and later cards on that page, empty on a fresh page. No code clears it per card. Persisted storage is not collection config or a sync upload.

Both pages share the scheme handler, base configuration and content controller. Pair-only host-script installation must not add scripts to another pair/preview through that shared controller. U176's host script runs in an isolated world in exactly the two pages of its pair, never as a page global. Reparenting due to SwiftUI layout changes preserves page objects, store, origin and globals.

Use mobile content mode on iPad; otherwise leave the system user agent unmodified. Native device idiom sets C66 platform classes; a foldable remains `iphone`. Pass current appearance/flag/mark to C66, with no duplicate native flag/mark. Appearance changes update both pages' classes, `color-scheme` and WebKit appearance without rerendering or rerunning scripts; a prepared page cannot reveal stale appearance.

On iOS/iPadOS reject loopback HTTP(S) before connecting: localhost, IPv4/IPv6 loopback literals, equivalent numeric forms and loopback redirect targets. The synthetic `http://127.0.0.1:8765` request must fail within50ms. Apply the rule before card execution; do not expose an alternative local-server route. Mac local-server requests remain subject to server origin/auth checks. Normal remote HTTP(S) uses WebKit, within C66 budgets. App transport configuration must permit R499 web-content requests.

## M7. Persistent page pair and presentation

The owning session model, on the main actor, owns exactly two persistent full-frame opaque pages, identifiers `review.card.page.a` and `review.card.page.b`. The representable hosts them; rebuilding or moving the view does not create new WKWebViews. Idle stays beneath visible in the same frame: no `isHidden`, offscreen translation, removal/readdition or zero size to simulate pre-rendering. Both sides of one card use one page. New cards alternate pages by the session's policy (U175).

`prepare(payload)` announces card id and a fresh prepare generation before starting C66's question update in the idle page. It waits for that update's settled signal: C66's scripts/hooks, image/font readiness budgets and double animation frame beneath the visible page. An old generation cannot mark a newer payload settled. Re-preparing invalidates the prior payload, pending actions, hit maps and confirmations. There is no native force-settled timer that bypasses C66.

`present()` is allowed only for the intended prepared card/generation. In the input's main-thread turn it reorders the two native subviews in place, without a JavaScript/web-process call before the swap and without starting its own explicit Core Animation transaction. The enclosing review/chrome transaction owns the commit. After that commit, notify the new page of presentation so C66 shown-hooks and its post-swap double-rAF confirmation can run. The confirmation names the same card and generation as the swap; it confirms identity/paint, not the endpoint of input-to-swap latency (KTD11, U12).

In that same swap turn, move first responder to the new page only while review owns keyboard focus; never steal it from a panel, sheet or other native editor. Hide the idle page's content from accessibility and remove it from the key-view loop; restore both for the new visible page. After repeated swaps, a real key reaches only the visible page and Tab never reaches idle content. Keep the old page unpresented for C66's media-holding rules, without destroying its globals.

`showAnswer` reads typed text, applies U64 comparison and replaces the answer in the current page. No page swap occurs. Record reveal-to-answer-visible and the matching side/generation confirmation. Suspension stops native audio and pauses HTML media; resume keeps the pages and current appearance without automatic replay. U70 handles process loss/recovery; report failed identity-bound updates.

## M8. Message boundary and allowed actions

Every native receipt first binds the actual sending WKWebView to its pair/page. Validate the message world, main frame and origin from WebKit metadata, not a body-supplied page identifier or URL. No subframe, remote page or detached/old page can invoke a native action. Private channels exist only in Meguri's isolated world; no generic evaluator, selector invocation, filesystem operation, collection op, server control, sync or native object is callable by card content (R2038).

The isolated `meguriCard` channel has a string `signal`; each update receipt also has `card`, `generation`, `side`. Require finite integral ids and nonnegative generations, with no precision lost in the bridge representation. Valid sides: `question`, `answer`. Validate the recognized message kind, supported fields, types and value/size bounds before comparing generations. Malformed input and unknown signals or confirmation phases remain rejected, even when they carry an old number. Pass native call data as arguments; never build JavaScript source from card text.

For a well-formed supported runtime message or confirmation, compare its emission generation with the page's current runtime generation. A smaller generation is a harmless no-op: produce no diagnostic, state change, action or paint/readiness confirmation. Do this before comparing its card, side or token with current values. A larger generation is rejected. Only an equal generation may proceed to the existing exact card/side/page and signal-specific admission checks, including current-payload indexes. An old number grants no admission or action. Use the native global update counter for this comparison; add no ledger of past generations or tokens.

| Channel / input | Accepted native effect |
|---|---|
| isolated signals `settled`, `shown`, `visible`, `updateFailed` | update/paint bookkeeping for the matching page/card/generation/side; failure reports diagnostics |
| isolated `command` | Mac `ans` reveal on question; `play:q:N` or `play:a:N` single-entry AV playback; all other commands ignored and logged |
| isolated `openAttachmentPDF` | native synced PDF viewer under [card behaviors B9](card-behaviors.md#b9-open-a-synced-pdf). Accept only an engine-trusted click. Require the exact generated filename in the bounded receipt and the current visible page, card, generation and side. |
| isolated `error`, `mediaError` | bounded card-scoped diagnostics; C66 C12 fields, never native execution |
| isolated `selection`, `focus`, `composition`, `typedText`, `listeners` | C66 current visible page interaction state only; no grading or collection mutation |
| isolated pair-only `meguriReviewHost` | typed input/map/probe observations under `render/host-reports`; never admitted through the generic `meguriCard` page-event relay |
| isolated `meguriVisible` confirmation | emission `card`, `generation`, `side`, with unchanged `token` and `phase` of `settled` or `visible`; accepted only for the exact current identity and outstanding native token/page |
| iOS page-world `meguri`, string `play:q:N` / `play:a:N` | original replay button's bounded single-entry playback (U62 D4) |
| iOS page-world `cb`, JSON string with `scheme: ankitap`, `msg: zone` | R511 tap action below |

The two legacy page adapters lack a card token. Bind them to the actual sending main-frame page's current native payload at receipt; card content cannot name a different page/card. Runtime and review-host messages must instead retain their emission identity. Page-supplied DOM events remain untrusted even when an isolated observer forwards them. They cannot create a new prepare, confirm another native token or bypass current-card validation. A confirmation is internal paint evidence, not authorization to modify collection data.

The native confirmation adapter carries the actual emitting update's card, generation and side through queued delivery; never fill them from the current page at receipt. The `settled` token remains that update's generation. The `visible` token remains the pair's independent presentation counter, so its numeric age cannot classify runtime age. Paint still requires the exact current card, generation, side, token, phase and page. This adapter rule preserves standalone C66 behavior and the legacy iOS sender binding above; it adds no collection-mutation authority.

For `play`, `N` is a nonnegative ASCII decimal index; accept leading zeros, reject negative, fractional, overflowed or out-of-range indexes. It selects the named side's AV list without reindexing TTS. For `cb`, only these nine exact zones exist: `topLeft`, `topCenter`, `topRight`, `midLeft`, `midCenter`, `midRight`, `bottomLeft`, `bottomCenter`, `bottomRight`. Any zone reveals on question; on answer, left-column zones request Again, right-column zones request Good, middle-column zones do nothing. U175 performs answering through its normal session action; the bridge does not directly write a grade. Unknown messages do nothing and are logged without a page exception. Mac has neither page-world handler; `window.webkit` remains absent there, per C66.

Drop input, hit-map and editable/selection/composition state from idle pages; they cannot replace visible interaction state. A card-scoped reveal/play request from the prepared idle page is retained only for that page's current card/generation, with bounded storage. Replay those actions after its `present` commit only if that exact card/generation is now visible; discard on re-prepare, invalidation, suspend/close or a different card. A queued reveal is applied once and only while still on question. Never reveal or grade the currently visible old card on behalf of idle content. All other stale receipts are ignored. Do not promote an old signal by stamping the new generation onto it.

Diagnostics and media errors may be recorded for a preparing page if their binding matches; they do not perform an idle action. Pair host scripts may report probes/input state, but cannot expand the card's action allowlist. When a reply handler is used, complete every callback once even for an ignored command; Mac's page callback contract remains asynchronous `null` (C66 C10.5).

The pair-host ABI, strict payloads, native context/revision lifecycle, probe deadlines and actual-null replies are `render/host-reports#H2`–H9. Native pair opt-in and the separate isolated handler prevent the generic page-event relay from impersonating host reports. U67 supplies admission/invalidation; real DOM/input/freshness producers remain U176/U180/U181 (H10).

## M9. Native test obligations and current interface gaps

Use deterministic fake task/player cases plus real WebKit cases for storage, module imports, CSP and page presentation.

- Over1,000 alternating cards: exactly two page objects, unchanged window attachments; both sides stay on one page; a synthetic page counter advances on every other card. Verify settled and visible identities, stale-generation rejection and no pre-swap JS call/own transaction.
- iPhone/iPad/Mac platform classes, normal user agent, live appearance without an incremented script counter; accessibility/key-view exclusion and first-responder ownership with a panel open. The external editor must retain focus through the type-in question's shown callback and later automatic-focus tasks, not only immediately after the swap. With review ownership, automatic type-in focus still runs after the commit (R534). Verify explicit card focus and standalone-runtime autofocus remain unchanged (`render/card-runtime#C6.4`).
- Synthetic resolver matrix M2, collision order on a case-sensitive fixture, encoded reserved paths, nested traversal, link substitution and root closure; no external file read or directory scan per request.
- M4 exact byte/HEAD/error table; mutate one suffix boundary in a test control so independent expectations prove they detect a one-byte error. Large file streaming has bounded memory and cancellation leaves no callbacks.
- `.woff2`/module MIME+CORS; root-relative fonts and dynamic import; HTML/SVG document script blocked while card scripts, inline handlers and evaluation work.
- Missing image diagnostic, atomic arrival, in-place retry with typed value and script counter preserved; new requests see arrivals. Cover CSS/font misses without attributing them to the wrong card.
- Shared localStorage/IndexedDB across two surfaces and a normal relaunch; fresh-page sessionStorage empty, existing-page storage retained. Never use the spike store identifier.
- Host reports: enforce `render/host-reports#H9` with synthetic isolated hosts, including forbidden generic-relay delivery, pair opt-in, context loss, limits, request expiry and exactly-once actual JavaScript null. Missing future producer is unavailable, never fabricated ready.
- Reject an unknown action, a bad world/frame/origin, an incorrect card/page/generation or token, idle input/state and an invalid AV index. Verify valid idle reveal/play is delivered only on matching presentation. AE18: a synthetic Mac front reveals its own card then; the iOS front stays on question because pycmd is undefined.
- iOS loopback failure within50ms; normal permitted remote/Mac local-server routing; no real integration/sync server is contacted for a test.

Integration gaps in the accepted C66 text must be resolved by the component's runtime adapter, with focused regression tests, not by weakening M8:

1. C66 C9.3/C14 now agree with the accepted replay-button transform: exactly `cb` and `meguri` are page-visible on iOS. The component must register those narrow protocols only; this adds no page global or general native handler.
2. Some DOM-observer signals in the existing interaction script omit update identity. State crossing a queued update needs the identity of emission, not receipt. The adapter/runtime must retain it and test a delayed event around re-prepare. Page-only legacy inputs are bound as described in M8.
3. A content controller shared by every card surface cannot be used to install a pair-only host script globally. Installation must remain scoped to that pair.
4. No native CoreBridge media-index reader is present at intake. U67 exposes the root/tracked-name snapshot-update input, with synthetic test providers; the collection owner/U175 must supply real U31 snapshots and events when it wires review. Adding an unrelated Rust provider or silently scanning the folder per fetch is outside this unit. Report that production wiring as pending until its owner exists.

## M10. Evidence limits and follow-up owners

The current S1 record reports passing VM function/paint/focus checks, but no120Hz host display/Instruments proof. Its host gate remains open; no approval or deferral is supplied by this spec. Only Antoine may perform that screen-owning check. The S1(f) WebContent footprint failure remains escalated for a product decision before U176/U245; successful card retention and a fixed page count do not prove the footprint limit. U66/U67 do not depend on that memory choice.

S2 records Range/CSP/storage/module/font successes and video seek function in the VM. The iOS27 **physical-device audio and video seek checks both remain open**. Keep the selected custom-scheme configuration conditional. If either real-device check fails, stop/escalate the media configuration and evaluate the specified file-URL fallback with fresh storage/module/CSP checks; do not silently substitute it or label VM evidence as device evidence. Immediate-kill durability of a last-moment website storage write is also not established by S2's relaunch test.

R1913's hand-captured reference screenshots remain an explicit human step in the milestone manifest. Antoine approved deferring nonblocking hands-on checks to the final batch during WIP (2026-10-20), including these references. The missing index stays pending evidence; final comparison and resulting fixes remain mandatory. No agent may take over Antoine's screen to obtain it. Device confirmation owners and evidence belong in the existing U256/milestone procedure, not fabricated metadata here.

## M11. Private native-audio playback copy

These rules define Meguri's Apple audio handoff; they add no Anki behavior.
During asynchronous preparation, read the file already opened under M2 into a private copy.
Give the existing player that copy's local URL, as supported by Apple's [audio-file initializer](https://developer.apple.com/documentation/avfaudio/avaudioplayer/init(contentsof:)).
Do not open the collection path again or give AVFoundation a descriptor pseudo-path.

Creation and copying follow this order:

1. Within storage reserved for this app's temporary files, create a directory under a newly generated, unguessable name.
   Require exclusive creation, mode `0700`, verified ownership and verified identity; refuse links.
   Acquire the directory's exclusive lifetime lock before creating output bytes or a publication marker.
   Create a new regular file exclusively, without following links, with mode `0600`.
   Generate its basename in the app; choose its lower-case suffix from the existing native-audio allowlist.
   No part of that output path may come from card text.
2. Perform the copy away from the main actor, using the existing bounded file reader.
   Limit each read to 64 KiB and limit pending read/write work.
   Check length arithmetic and finish short writes correctly.
   Copy exactly the length recorded at open.
   Before and after copying, compare the descriptor's identity, regular-file type, length, modification time and change time with the opening record.
   Reject changed metadata, early EOF and I/O failures.
   An atomic replacement of the pathname may leave the old opened bytes usable or cause rejection; it must never select replacement bytes through another path lookup.
   Allocate a fixed amount of memory for the transfer, regardless of input size.
   Do not map the input, place all its bytes in a `Data` value, or collect chunks for later.
3. Check the completed output's exact length and all required input metadata.
   Close every handle that can write the output and set its mode to `0400`.
   Only then create the completed-publication marker exclusively, with the lifetime lock still held.
   Release the URL for native use only after these steps.
   Never edit, replace or reuse a released copy.
   Keep its directory private from card content and keep its identity until removal.
   Initialize and prepare the player away from the main actor; copying alone proves no decoding result.

For every prepared or active native clip, ownership includes exactly one copied file and the original descriptor.
Keep the established question/answer preparation and lookahead limits, without adding a limit, replay storage or advance copies for a whole queue.
Both files remain owned when playback pauses or seeks.
End playback and revoke callback delivery first; only afterward close the descriptor and delete its copy.
The directory lock remains exclusively owned from the copy operation until removal finishes, including publication and player shutdown.
Retire the work if preparation is cancelled, preparation or decoding fails, a replacement arrives, the owner closes or the clip is released.
Every resource created for that work must be relinquished once, whether its output is complete or partial.
Observe cancellation after each bounded operation and once more before returning a prepared player.
No obsolete or cancelled preparation may deliver a playable object, start playback or enable an audio session.
Do not alter collection media or request additional entitlements.
Do not reveal the private file URL to a page, an exported interface or a diagnostic.

Recovery after a crash may inspect only the app's own copy area.
Without following links, verify ownership, directory/file identities, generated names and the completed-publication marker.
Try the exclusive lock without waiting, then check those conditions again before deleting anything.
Skip live entries and every uncertain state, including incomplete creation, a lock alone or a missing publication marker, even if locking succeeds.
Delete only the expected copy, lock and marker files.
Never traverse unrecognized children.
Exclude every shared cache or temporary folder from this operation.
If removal fails, retain the unused files for a later safe attempt and emit only a bounded diagnostic with no path.
That failure grants no right to reuse or play those files.

Keep `render/audio-playback#A2`–A7 unchanged: controller transitions, completion epochs, seek, preload and session rules still apply.
Use A5 for missing/failed items and their diagnostics.
Neither copying nor preparation may hold up page presentation or activate ducking.
R554 remains ≤50 ms from presentation to play and ≤100 ms between clips, including a missing middle item.
Measure copy/preparation time, memory at increasing file sizes and native playback independently.
No performance result follows from this design alone.
The existing timing exception excludes decoder initialization failure.
U69 codecs, real-device checks and final acceptance retain their existing owners and pending status.

Required evidence has three parts:

- Synthetic file tests cover refused links, mutation, atomic replacement, exact bytes, bounded reads/memory, short writes, I/O failure and write/decode/cancel/close races.
  Deterministically pause creation and run recovery before lock acquisition, during copying, after publication and during removal.
  Show that live/incomplete states survive recovery, while only a fully published abandoned copy with acquired exclusive authority can be removed.
- Generated supported clips exercise the actual backend: expected finite duration, real play, one completion, pause/seek and native stop before both resources are released.
  Show that an old generation cannot restart playback.
- For integration proof, run the strict review flow after media import using the production owner.
  Initializing a decoder on an unrelated regular file proves neither successful backend playback nor completion of this flow.

Report VM file/decoder results separately from platform and hardware timing/session proof.
