# Isolated card host reports

Spec: render/host-reports
Requirements: R335, R350, R405–R407, R415, R425–R427, R443, R449, R465, R1862, R2038; U67/U175/U176 transport boundary
Anki: none. This is an original Meguri protocol, designed from the Product Contract and accepted native boundaries.
Scenarios: none. Isolated-world native handler tests use synthetic reports; producer tests belong to U176/U180/U181.

## H1. Scope and ownership

The `render/media-serving#M8` host-report boundary carries observations from the pair-installed isolated script. This contract supplies transport and validation. U176/U180/U181 still own listeners and touch/key decisions; C66 and the native action allowlist are unchanged.

Neutral requirements used:

- U175 owns the single phase/input gate; a report cannot itself grade a card or change the collection.
- U176 requires isolated pair-only host script execution, side resources/preload/settle observations, current body background, and scrolling after image readiness. Its visible-page input/hit-map rule already rejects idle input.
- R405–R407 require conservative exempt-target data and final click information after page handlers; R415 requires an input to remain associated with the card at its start. R425–R427 require editable/composition/key observations without exposing page code to native APIs.
- C66 C13 owns whether card listeners exist; the new hit-map message does not duplicate or override that registry. Existing settled/visible signals remain the paint evidence.

## H2. Isolated world and channel

Use the existing isolated `meguri.card` WKContentWorld, but a distinct handler named `meguriReviewHost`, with three exact signal names: `hostInput`, `hostHitMap`, `hostProbe`. Never admit these through `meguriCard`, whose generic page-event relay can carry untrusted page-created dictionaries. “Its own content world” means a Meguri-owned world distinct from the page world; it need not mean another world distinct from interaction.js. KTD11 already places probes and input observers together. An additional world would add crossing/state synchronization without a security benefit required by this contract.

The host script calls `meguriReviewHost` directly from the isolated world. It must not take these reports from a DOM event, runtime relay or page message. `meguriCard` rejects all three host signal names even if the page has host reporting enabled; it never forwards/promotes them to the host decoder. Register the separate handler only with the isolated world and require native pair opt-in for the actual sending page after installHostScript. A shared content controller registration does not opt previews or other pairs in. Install the trusted host script into only the two pages of its pair through installHostScript. It is not a new shared user script on every card surface. No page-world handler/global is added. C66 runtime and cb/meguri protocols remain unchanged. Reject host signals arriving through a page-world adapter, the generic meguriCard relay, another world/frame/origin or a page not opted in by its native pair.

A host report is an observation emitted by the installed isolated script. It cannot contain an operation name, selector, function name, source text, URL, filesystem path, native object or grading/reveal instruction. Native delivers typed events to the pair owner; U175/U180/U181 decide actions through the existing review gate.

## H3. Envelope and hard limits

All three reports are dictionaries, not a JSON string or arbitrary array. Required common keys:

| Key | Value |
|---|---|
| `signal` | one of the three exact names above |
| `version` | integer 1; other versions rejected |
| `card`, `generation`, `side` | exact identity encoding and precision checks already used by media-serving M8; side question or answer |
| `viewRevision` | native-issued positive integer, at most 2^53−1, matching the current page binding/geometry epoch |
| `payload` | exactly the variant below |

Identify sender page, main frame, world and origin through WebKit metadata. Refuse a `page` or world-id field in the body. Match the captured event/probe identity and native viewRevision exactly. Preserve M8 card-id encoding without rounding; delayed reports cannot acquire a newer card identity.

Reject unknown keys at every level, wrong types (including booleans masquerading as numbers), nonfinite/fractional integer values, excess nesting or payload size over 64 KiB in its UTF-8 JSON form. Nesting is only the fixed objects/arrays below. No arbitrary strings/dictionaries/arrays are forwarded. String limits below count UTF-8 bytes. Exceeding a report limit disables that report, not card rendering or normal browser input; take the conservative web-confirmed route where needed.

For hostInput and hostHitMap include common `serial`: a positive safe integer increasing per signal per page revision. Reject duplicates/out-of-order values; reset admission only with a new native viewRevision. Probe reports instead use an outstanding native request id (H8), not a caller-chosen serial.

## H4. Native-to-host ABI and uncertain updates

U176's installed script exposes one object in the isolated world only: `window.meguriReviewHost`, with a `receive(message)` function. U67 calls that fixed entry point with structured arguments in the `meguri.card` world of one actual page. It never evaluates a caller-chosen function name or card-provided source. This object is distinct from `window.webkit.messageHandlers.meguriReviewHost`, the outbound isolated handler. Neither exists in the page world.

The inbound dictionary has exactly one of these shapes:

| op | Other fields |
|---|---|
| `clear` | `viewRevision`: newly issued positive safe integer |
| `context` | `card`, `generation`, `side`, `viewRevision` as above; `visibility`: `preparing` or `presented`; `bounds`: `{width,height}` in native content-rectangle points; `pageZoom`: finite positive number |
| `probe` | `kind`: `background` or `resources`; `request`: native positive safe integer; `viewRevision`; `timeoutMs`: integer 1–2000; only for resources, `purpose`: `warmup` or `preload` |

Report bounds must be finite and positive, ≤1,000,000 points each; pageZoom must be finite, >0 and ≤1,000. Reject larger report geometry without clamping. UI zoom settings are unchanged. Probe identity comes from the active context and its revision. No extra query/options are allowed.

Native starts with reporting disabled. Before a prepare, side replacement, swap/unpresentation, geometry change or suspension, native immediately invalidates admission state, advances viewRevision, cancels pending probes and calls clear where the function exists. The script clears active context, map caches and pending async work. It cannot retain a prior context as a fallback.

While the native binding and installed DOM update are uncertain, publish no active context and admit no host reports. Publish context only after the owner knows which matching DOM update is installed, with current native bounds/zoom and visibility; an acknowledgement does not prove layout freshness. A preparing context permits only its requested slot probes, never input/hit maps. At presentation, clear old input/map admission, consume already matched preparation data as appropriate for the same card, then publish the presented context at the new revision. A prepare-time background observation may supply that card's pre-swap chrome; it is native slot data, not replayed as a new input report. Side/content/appearance invalidation discards stale slot data.

Keep an immutable context snapshot from event/probe start through emission. Check it again before sending. Discard input that began before that context became active. A future bubble-phase or async observer must retain that start identity, never read a newer context at send time. Native also checks the revision and R415 input-start gate. U176/U180/U181 implement these observers; U67 cannot relabel their pending events.

`receive` returns true only to acknowledge that the installed host accepted the context/request, false on malformed/obsolete input. Neither means a probe is ready or a map is fresh. If the object/function is absent, throws or has an unexpected return type, U67 leaves reporting disabled and reports host-unavailable to its owner; a probe completes as unavailable, not ready. No fallback shim, fake successful function or synthetic result is installed. After later successful installation the owner may republish only its still-current known context. clear/invalidation is effective natively even if the script is absent or fails.

Probe deadlines use native monotonic time. Default deadline: 250 ms for background and 1000 ms for resources; the owner may choose 1–2000 ms. These observation deadlines do not extend C66's independent resource/preload budgets. Expiry returns a typed timeout/unavailable observation to the requesting native owner, clears the outstanding request and drops later replies; it does not create ready/budgetExpired resource evidence. Cancellation, context loss, suspension or missing host clears requests promptly. At most one outstanding request per kind per page, with no automatic retry or unbounded pending queue.

## H5. View revision and coordinates

Native advances viewRevision on prepare/re-prepare, side replacement, presentation/unpresentation, page/root closure, suspend, layout/size change, page-zoom change or a changed native content rectangle. It invalidates cached input state/hit maps immediately. Capture the new revision in the isolated script before reports for that revision are accepted. A temporarily missing valid report never grants a fast path.

A geometry object has exactly:

- `width`, `height`: finite visual-viewport dimensions in CSS pixels, >0 and ≤1,000,000 each.
- `offsetX`, `offsetY`: finite visualViewport offsets relative to the layout viewport, absolute value ≤1,000,000,000 each.
- `scale`: finite visualViewport scale, >0 and ≤1,000.
- `layoutRevision`: positive safe integer from the producer, increasing whenever the observed geometry or exempt regions may have changed.

Coordinates are visual-viewport-local CSS pixels: origin at the currently visible viewport's top-left, x rightward, y downward. A client rectangle is translated by visualViewport.offsetLeft/offsetTop, then clipped to width/height. Rectangles use x/y/width/height, not ambiguous min/max corners. Scroll offsets are not added again.

Native converts a point in the page's current visible content rectangle into these CSS coordinates using the viewport/native-content size ratio, after converting from the host/window to page-local coordinates. Do not treat CSS pixels, backing pixels and points as equal. A changed content inset, viewport, zoom, rotation or scroll invalidates the previous geometry. Reject inconsistent/zero geometry rather than guessing a transform. Negative coordinates outside the clipped viewport are not actionable points.

The future producer marks layoutRevision invalid on scroll, viewport/DOM/resource/layout change or uncertain animation geometry. Publish a new complete snapshot at most once per animation frame. Native also invalidates on known scroll/geometry changes. With no complete current map, use the web-confirmed path. U180 must prove freshness in its tap-path spike; U67 validation alone cannot prove the DOM map current.

## H6. Input observations

Accept input only for the visible page. The producer checks engine isTrusted, discarding false. Refuse a trusted payload field and any host report relayed from a page event. Validate sender, world, identity and revision natively. U175/U180/U181 then apply R415 using native input-start data. An observation is never grading authority.

Common payload keys:

- `kind`: `click` or `key`.
- `timeMs`: finite performance-time value in milliseconds, 0 through 2^53−1. Preserve the event timestamp; native owns clock correlation, not subtraction from wall-clock time.
- `defaultPrevented`, `exempt`, `selection`, `editable`, `typeans`, `composing`: strict booleans. `typeans` implies editable. Exempt/selection/editable/composing are observations, not permissions.

Click-only keys:

- `geometry`: H5 object.
- `x`, `y`: finite coordinates in that visual viewport, 0≤x≤width and 0≤y≤height.
- `button`: integer 0–4; no arbitrary bit flags.
- `pointer`: one of `touch`, `mouse`, `pen`, `unknown`.

This is a final bubble-phase click observation after page handlers, not a synthetic touch-down or native fast-path decision. Exemption includes the R405 target ancestors. No movement/duration/answer rating is supplied here: native gesture tracking and U180 own them.

Key-only keys:

- `event`: `keydown` or `keyup`.
- `key`, `code`: strings each at most 128 UTF-8 bytes; an empty value is valid when the engine has no value. No target text/value or HTML is sent.
- `repeat`, `shift`, `control`, `alt`, `meta`: strict booleans.

Key observation occurs after card handlers; it reports cancellation, repeat and composition without choosing a shortcut. U181 owns shortcut registration, physical-key mapping, IME and interception. Existing C66 typedText/focus/composition protocols remain separate; this does not provide a second arbitrary typed-answer text channel.

No report buffering/replay on an idle page. Native drops an input even if its revision was once visible but is no longer current. Record at most bounded diagnostic rejection counters, not the raw key text.

## H7. Exempt-region maps

Exactly `geometry`, `complete`, `rects`:

- geometry: H5 object.
- complete: strict boolean; true means the producer covered all currently known R405 exempt regions in that snapshot. False makes the whole viewport uncertain.
- rects: 0–256 rectangles, each exactly `{x,y,width,height}`. Numbers finite; x/y≥0, width/height>0; rectangle wholly within the visual viewport after clipping. No DOM id, class, selector, text or URL accompanies a rectangle.

A region marks only “do not use native non-exempt fast path here.” It never marks a reveal/answer zone. Overlap and conservative unions are allowed. If there are too many regions, combine them conservatively (including one whole-viewport rectangle) or publish complete=false; never truncate and accidentally classify an exempt point as safe. An empty complete map means no known exempt region, not no card event listener. C66's listener registry and current selection/focus/composition state remain independent gates.

Update the map cache only from the visible page. Within a viewRevision, reject older layoutRevision. Clear the cache before further input on swap, side replacement, scroll, geometry change, suspension or preparation. Discard idle maps; do not save them for presentation. The next visible map needs the new revision. Tap/tint decisions remain outside this protocol.

## H8. Probe results

Only replies to native-created outstanding requests are admitted. Native has at most one request of each probe kind per page (two total), with a positive safe-integer `request` id; a newer request cancels the prior one of that kind. A request records page/card/generation/side/viewRevision and a native deadline. Unknown, duplicate, expired or mismatched replies are discarded. The reply cannot start a probe/native operation by itself.

Payload keys are exactly `kind`, `request` and the fields of one variant:

| kind | Other fields | Use |
|---|---|---|
| `background` | `rgba`: exactly four finite numbers in [0,1], in sRGB | Computed body background for the matching slot; alpha zero allows U177's canvas fallback. No CSS source/string, image or arbitrary color expression. |
| `resources` | `purpose`: `warmup` or `preload`; `state`: `ready`, `budgetExpired` or `failed`; `imagesPending`, `fontsPending`, `stylesPending`: integers 0–1,000,000 | Bounded resource readiness summary for U176. Purpose must match the native request. No media URLs, font names or per-element list. |

A matching idle preparation may receive background/resource results for its own slot only. Do not change visible chrome or input state from that result. U177 can retain that slot color before swapping it forward. Consume matched data at presentation; discard it on slot invalidation. Idle input and hit maps remain refused.

`resources` does not replace C66 settled/visible confirmation or override preload budgets. It cannot force a native slot settled, produce a frame timestamp or imply a physical-device performance pass. Native owns deadline expiration; a script's claimed state is evidence for that outstanding observation only. No generic named probe or arbitrary metrics dictionary is accepted. U176's scroll behavior stays in its host script; no native scroll command is added here.

## H9. Replies and required tests

A handler-with-reply completes exactly once with actual JavaScript null for every outcome: accepted, malformed, unknown, wrong sender, disabled, stale or oversized. Native supplies NSNull to the reply callback, not Swift nil/undefined. Its reply contains no native data, denial detail or authorization flag. Typed observation delivery/probe completion occurs through the owner's callback/event seam after validation; callback exceptions cannot generate a second reply. No waiting for review/core work is needed to acknowledge the transport. Existing pycmd asynchronous-null semantics are untouched.

Test matrix:

| Area | Cases |
|---|---|
| Schema | Valid variants; byte/count boundaries; extra keys; unknown version/enum; NaN, infinity, fractions and boolean numbers |
| Binding | Wrong page/world/frame/origin; stale card/generation/side/viewRevision; duplicate serial; older layoutRevision |
| Visibility | Drop idle input/maps; retain only matching idle-slot probes; clear cache on swap |
| Probes | Cancellation, timeout and duplicate result; absent host never reports ready |
| Geometry | 257 regions handled conservatively; coordinate conversion with scrolling, zoom and insets |
| Isolation | Host-shaped page/relay dictionaries and synthetic events refused; installation limited to the pair |
| Reply | Exactly one JavaScript null for every outcome |

## H10. Delivery boundary and evidence

U67 implements the strict host handler, native pair opt-in, context/probe invocation API, bounded request/result types, exactly-once null replies, revision/visibility admission and native invalidation on known bounds/zoom/lifecycle changes. It may test these with isolated synthetic host functions. An absent production host is explicitly unavailable.

U176 supplies the real pair-installed host script and background/resource observers. U180/U181 add DOM mutation/layout/scroll freshness, target exemption, event-start tracking and click/key observers under their existing ownership. This supplement does not require U67 to implement those units' gesture/layout engine or claim a fresh map before the real producer exists. The native consumer must use a safe non-map route when reporting is absent/uncertain. All future producers must satisfy this protocol and freshness proof before native fast-path use.

This specification records no implementation or test pass. Existing physical/device obligations are unchanged.
