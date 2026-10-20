# Native card page behaviors

Spec: render/card-behaviors
Requirements: R454, R523, R524, R538, R560, R566–R577, R579, R580, R2040; AE20
Anki: none. This is an original Meguri contract joining Product Contract policies to the accepted native page boundary.
Scenarios: none. Native and WebKit tests use independently authored synthetic pages.

## B1. Scope and identity

U70 supplies reusable page behavior. U175 owns the session and presentation host; U176 owns prepared slots; U160 owns device settings; U180 owns review tap decisions. U187 owns durable continuity snapshots and relaunch routing. This contract adds no page global, generic bridge, host-report kind or collection operation.

Use the actual native page and its current card, generation and side binding from `render/media-serving#M7`–M8 and `render/card-runtime#C6.8`. Capture that binding when native work starts. Before applying an asynchronous result, require the same live binding; an old page, prior side or superseded generation cannot affect the replacement. Recovery explicitly creates a new binding as B5 describes. Invalidate host geometry under `render/host-reports#H4`–H5 when layout, zoom or page lifetime changes.

## B2. Native dialogs

A dialog may present only for the current presented page while its owning surface is active and not suspended. Native alert, confirm and prompt retain their ordinary results: alert has no value, confirm returns the selected Boolean, and prompt returns the entered string or null on cancellation. Accepting an empty prompt returns an empty string, not null.

For an unpresented, stale, detached, suspended or closed page, or when no valid native presenter exists, complete immediately without a dialog:

| Request | Completion |
|---|---|
| alert | void |
| confirm | false |
| prompt | nil at the native callback, yielding JavaScript null |

Complete every WebKit dialog callback exactly once. If its page binding loses validity while a visible dialog is open, dismiss that dialog and finish with the neutral result above; a later UI response does nothing. Do not queue a dialog for later presentation or replay it after neutral completion. An idle page's script must continue without waiting for that page to be presented, so its dialog cannot prevent C6.6 settling. This is Meguri's native policy under C19.4, not reference-client behavior.

## B3. Links and the root document

Apply R566–R570 through one routing policy for links, target=_blank and window.open. The host receives typed open requests and presents the permitted browser or scheme confirmation. The component does not create another card page. Navigation handling never reveals or answers a card; U180 separately treats links as exempt input targets.

For a same-document fragment, keep the root document URL unchanged and scroll to the fragment target through a fixed native-owned operation. An empty fragment scrolls to the top; an absent target leaves the scroll position unchanged. Pass fragment data as arguments, never as executable source. This preserves in-page movement without navigating or replacing the review document. javascript: links retain actual page-context execution through normal WebKit handling; do not replace them with an external-open request or a new native evaluator. Other same-document activity stays in the page. A document navigation away from the bound root is refused and routed only when R568/R569 permit an external open.

The file scheme is always refused, regardless of stored approvals. Scheme approval and link-opening preferences remain device-local, supplied by their owner; R569's confirmation is remembered per scheme, not per URL. This does not introduce a second settings store or expand the native message allowlist.

## B4. Zoom and live layout continuity

Saved card zoom is pageZoom, in R571's 50–300% range with default100%. It is supplied by the device setting owner and applied to both pages. Never implement it by body zoom, root font-size changes or Dynamic Type scaling of card content. Temporary iPhone/iPad pinch zoom is separate: preserve it from a card's question to its answer, and reset it for the next card without changing saved zoom.

The native host supplies the content rectangle excluding its bars and system safe areas. Disable automatic scroll inset adjustment; do not subtract those exclusions again. Both pages fill that rectangle. This preserves R575's viewport height and fixed-bottom behavior. Review chrome still follows Dynamic Type.

On a live size change, preserve the page objects, DOM, scripts, current side, typed input and playing media; reflow and deliver resize without a reload. Preserve scroll proportionally on each axis using CSS-pixel scroll metrics: the scrollable extent is max(0, scroll size minus client size). Before the change, divide the offset clamped to that extent by the extent; use fraction0 for a zero extent. After layout, multiply the fraction by the new extent and clamp to that extent. Do not mix these metrics with native points or backing pixels. No change of session/card/side follows rotation, keyboard connection or a size-class switch.

Geometry changes invalidate prior host input/maps before further native input decisions. They do not recreate the page or grant a fast path while a new map is absent. U176/U180 own fresh DOM observations; U70 only supplies its native bounds/zoom invalidation. Fold-specific APIs remain conditional on the planned SDK; existing size-class continuity is unchanged.

## B5. Process recovery and typed text

Use C14, C6.6–C6.8, C7.4, media-serving M7 and host-reports H4.

| State / input | Required behavior |
|---|---|
| Typed text | Store accepted text outside WebContent. Match page, card, generation and side at emission and receipt. Reject old-process updates. |
| Visible process terminated | New document skeleton and generation; render the owner's current side; run its scripts again. Keep the other live page and session. |
| Idle process terminated | Clear settled/readiness. Prepare lazily. No presentation from old receipts. |
| Restore text | Owner authorizes the same card visit and side. After matching DOM installation, before ready, set #typeans value. No input/change events or focus request. Missing input is allowed. |
| Cancel, close or binding change | Cancel pending work, dialogs, host reports and confirmations. Discard late callbacks, text writes and readiness results. Recheck identity after async work. |
| Foreground / probe | Healthy page: keep DOM, no rerender. Match probe identity. Missing host or probe timeout means unavailable, not process death. Keep C7.4 dependency ordering; no false completion. U187 owns lifecycle order. |
| Recovery result | Normal settled/visible receipts only. No error UI on success; no grade, reveal or audio replay. Preserve C6.4 keyboard ownership. |
| Storage / comparison | U175/U187 own persistence, snapshots and relaunch. No second U70 store or lifecycle coordinator. C10.3 comparison and template overrides stay unchanged. |

## B6. Focus, accessibility and backgrounding

Only the visible page belongs in the accessibility tree and key-view loop, as media-serving M7 requires. On reveal, move VoiceOver focus to #answer when it exists, otherwise to the start of #qa, once the matching answer DOM is available. Discard that request if its binding changes. Answer-button grade/interval labels remain U178's responsibility.

VoiceOver movement is not permission to make the web view first responder or raise its keyboard while an external editor, panel or sheet owns keyboard focus. Preserve C6.4's native gate for stock automatic type-answer focus and its existing explicit-card-focus behavior. A resize or recovery is not a new explicit user focus request.

Background suspension pauses in-page audio/video through the existing U67/C8 lifecycle. U175/U187 coordinate native audio stop and snapshot ordering. An ordinary foreground return does not automatically replay audio. Neither backgrounding nor accessibility changes the session's card or side.

## B7. Inspector eligibility

Inspector eligibility is an explicit build/distribution policy input, separate from the R523 device setting. **E-20261020T122806Z-8ab6 is resolved by the orchestrator under Antoine's delegated judgment (2026-10-20):** follow R523/R2040's distribution rule. Non-AppStore Debug, Profile and personal Release builds follow the device setting; AppStore builds are always ineligible. Unknown eligibility is false.

| Build/distribution | Setting off | Setting on |
|---|---|---|
| AppStore, any configuration | false | false |
| Known non-AppStore Debug/Profile/personal Release | false | true |
| Unknown eligibility | false | false |

Keep R523's declared setting defaults unchanged: default off, on in personal builds. A setting value alone cannot override AppStore or unknown eligibility. U70 consumes this policy; the release-audit and U255 checks must validate the same distribution matrix rather than the superseded Debug-only/Release-off wording.

## B8. Synthetic verification obligations

| Case | Required result |
|---|---|
| An idle question calls confirm during preparation | false immediately; no modal or queued replay; normal settling can complete |
| A visible prompt loses its page binding, then the UI returns a value | neutral completion once; late value ignored |
| A fragment link targets a lower element, then javascript:void(0) runs | in-page handling; root document URL unchanged; no reveal, answer or replacement page |
| Saved zoom150%, temporary pinch2×, reveal, then next card | saved zoom unchanged; pinch survives reveal and resets for the next card |
| Size change with text, scroll and playing media | same live page/side; text/audio continue; proportional scroll; old geometry invalidated |
| Current question contains typed ka, process terminates | current side scripts rerun in a new generation; ka restored silently; no stale completion, replay or error UI |
| Idle termination or missing future host function | lazy invalidation for actual termination; host-unavailable alone does not recreate a live page |
| Reveal with #answer, reveal without it, then stale reveal completion | VoiceOver targets answer or start of qa respectively; stale completion ignored; external keyboard owner preserved |
| AppStore or unknown inspector eligibility with setting on | inspection remains disabled |
| Known non-AppStore Debug, Profile and personal Release, setting off then on | inspection changes from disabled to enabled in each configuration; R523 setting defaults unchanged |

These are component obligations, not recorded test or physical-device results. Native/WebKit checks run through tools/mg in the VM. Final device and reference evidence retains its existing owners and pending status.
