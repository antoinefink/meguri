# Leased Duo posture (#205)

`tools/mg sim-posture open <leased-udid>` and `closed <leased-udid>` use the
existing persistent VNC FIFO holder. `prepare [--vm <name>]`,
`status [--vm <name>]` and `inspect <leased-udid>` keep their invocation.
Posture never prepares/reconnects VNC, boots a device, starts/restarts the VM,
changes consent or uses the coordinator's GUI. There is no host simulator
fallback. The former unleased `hub`, fixed-coordinate `fold` and generic
`click` helper routes have been removed.

New configured-remote sessions use coordinator state `build/vmh3` and a
resident M3 owner. Local and already connected legacy sessions retain
`build/vm-fold` or `build/vmf2`. M5 alone schedules folding; both Macs can
schedule other VM jobs. Folding waits on MG's guest-wide claim before guest
access, including when reusing the resident VNC connection. After a guest
reboot, it waits for the claim to be reacquired before readiness checks or
drain cleanup (`tools/vm/AGENTS.md`).
M5 posture analysis still uses `build/vm-fold/venv`. Mutations recheck routing
and VM metadata under the scheduler lease.

## Resident deployment and the one drained migration

Integrator/operator only, from the authorized M5 scheduler checkout, with its
existing private `mac-vm.json` / wrapper-adjacent `remote.json`. No endpoint,
credential, command or remote output path is supplied to these commands:

```sh
tools/mg image-tools-selftest -k hosted
tools/mg image-tools-selftest -k posture
tools/mg selftest tools/tests/vnc_session.bats tools/tests/remote_wrapper.bats
tools/mg sim-posture deploy
tools/mg sim-posture preflight
tools/mg sim-posture status
# Operator only: suspend dispatch externally on BOTH M5 and M3, then drain
# outstanding jobs and simulator leases on both. MG cannot verify this pause.
tools/mg sim-posture maintenance-prepare
tools/mg sim-posture status
# Resume dispatch on both Macs after maintenance. Everyday automated reuse:
tools/mg sim-posture prepare
```

Use MG result status/details to verify each job; its process exit alone is not
a pass. `maintenance-prepare` is the operator's declaration for that invocation
only; ordinary prepare and other jobs never inherit it. It adds no host lock to
ordinary jobs and changes none of their master boot behavior. Both Macs may
continue scheduling normal jobs outside the maintenance window.

`deploy` installs only the six shipped Python components plus a private venv
with `vncdotool==1.4.2` and `Pillow==11.3.0`, under the fixed kit-relative
`vnc-owner-v1/`. It uses the configured endpoint and existing strict SSH
validation/options; only hosted VNC control opts into dedicated SSH connections.
All code/state is owned, unsymlinked and private. Under the install lock,
`deploy` can atomically replace partial or changed components and rebuild an
incomplete venv only before any drain, launch or operation evidence exists.
For an interrupted pre-connection install or a new release in that vacant
installation, rerun `deploy`, then `preflight`. Do not delete reservations or
owner records to enable an update. Matching ready components remain usable
with an existing owner; deployment refuses code or dependency repair there.
Deployment never changes system settings, provisions the guest or accesses
user credentials. `preflight` verifies digests, pinned versions and the actual
posture sidecar imports, exercises private state/operation storage and a
unique control-socket bind/listen probe, and parses the configured guest's
local Tart inventory before drain admission. Its probes remove only their own
files. Neither command starts Tart or connects to RFB.

The configured M3 kit must already be mounted, its actual `runtime/bin/tart`
locally executable, strict-key batch SSH working, Python 3.9+ with venv/pip
available, and the two pinned packages installable (network/package availability
only during deployment). All kit ancestors must be unsymlinked and protected
from other writers. `runtime/vnc-owner-v1/state/control.sock` must fit Darwin's
103-byte pathname budget. M5 still needs the existing Pillow operator venv.
These are real prerequisites; a Docker fixture does not establish them.

The guarded migration admits the actual disconnected authenticated supervisor
in `vmf2`: two positive disconnected replies, unchanged private owner/VM/peer
records, all former transport children absent or stably kernel-confirmed
zombies parented by that same live supervisor, no shutdown/recovery/drain/posture
uncertainty, and the existing strict canonical pre-supervisor orphan proof.
The live same-user supervisor must own the authenticated control socket;
Darwin `ps` observations of parent, state and command repeat without change.
Live, unrelated or ambiguous children refuse migration. This zombie admission
applies only to managed `vmf2`, never the canonical pre-supervisor orphan.
Timeout, EOF, unknown process evidence or a lost supervisor response refuse.
M3 must independently report no resident reservation. The canonical directory
and `vmf2` evidence are preserved. No old PID is signalled and no old holder is
reconnected. Admission also works for a genuinely empty legacy route.

After the ordinary M5 lease, guest-wide claim, idle-test check and committed
guest drain, M3 reserves one token-bound detached stop peer. Its single terminal
receipt requires the actual stop command to join **and** M3 to confirm Tart stopped.
Only that receipt admits one detached resident start with the actual local
Tart binary. Stop/start reservations, logs and receipts stay in the private
kit. Controller loss cannot kill these peers. Lost replies use fresh receipt
or status calls; an unknown reservation is never vacant. A completed stop
followed by an interrupted start can resume that reservation only through
`maintenance-prepare` in another externally paused and drained maintenance window.
Before boot, the local scheduler may admit only a positively stopped guest with
matching private local VM/endpoint, recovery and drain records, M3's original
stop reservation and its positive joined/stopped terminal receipt. This host-only
check needs no guest SSH. It never repeats the stop. Running or unknown guests
require the normal shared claim before guest work. After boot, preparation waits
for normal shared admission (also when no prior keeper exists) before readiness
checks or drain cleanup. Uncertain markers, retained scheduler fences and
incomplete/mismatching records refuse; the declaration overrides none of them.
A caught log-open or `Popen` failure durably marks a proven pre-spawn failure;
only the original VM/drain token may retry it. The active drain caller queries
that marker before retrying its stop reservation; maintenance can resume a marked
start failure. A missing failure marker always retains uncertain ownership,
even if no child can be observed. Persistent stop failure retains the scheduler
and guest fences for token-bound reconciliation; a new token cannot replace it.
There is no rollback restart or automatic replacement of a disconnected owner,
and this versioned runtime refuses a second migration stop. Hosted `stop`
refuses because it would require a further authorized restart.
An actual disconnected hosted owner cannot be replaced by this runtime, even
inside maintenance. Retain its evidence for operator reconciliation; this command
does not implement arbitrary replacement or additional VM reboots.

An established hosted connection always returns `reused` from ordinary prepare,
including after M5 SSH/client loss. It never stops, starts or reconnects Tart.
An empty, disconnected or stopped hosted route refuses ordinary prepare with
instructions to schedule maintenance; automation never enters maintenance itself.
An already connected legacy/local route is likewise preserved. The resident
supervisor owns its original children and authenticated loopback RFB connection
on M3; no new remote session uses a foreground M5 VNC tunnel. No other RFB
client may connect to running Tart.

Capture and move+capture requests reuse that connection and the existing local
Holder's FIFO/log/complete-PNG barrier. Only bounded PNG bytes cross SSH into
exclusive owner-only files in unique private coordinator directories. The
service accepts typed coordinates, never supplied paths or executable commands.

For a click, M5 still holds its scheduler, guest-wide claim and exact guest
lease/window guard.
M3 first reserves a unique request bound to that resident session and guest
guard, sampling a five-second deadline on **M3's clock before publication**.
Before sending input, M5 requires headroom for a full five-second resident
ticket from receipt of the reservation response, plus the measured reservation
round trip, all before its tighter guest/lease deadline. A short lease refuses
input; the abandoned resident ticket expires without input. No bare M5 monotonic
timestamp is sent. The resident latches an attempt before the FIFO write and
the existing synchronous holder callback checks the M3 deadline. A lost click
reply triggers only receipt queries, never a click resend. Terminal receipts
survive client loss, are independent of screenshots and cannot be overwritten
by a repeated attempt. Uncertain publication retains the scheduler/guest fences
and private evidence. `tools/mg sim-posture receipt` reads only the exact
outstanding scheduler intent and refreshes that receipt; it emits bounded
joined/executed/error data and never clears fences or emits input. A null
receipt remains unknown. Receipt recovery alone is not a pose/display proof.

Native acceptance remains with the integrator: verify the ready M3 kit and
connected reuse. Interrupt the M5 client/SSH between joined read-only operations,
verify the same resident session/children
and one connection with fresh private captures, then acquire a Duo and run the
calibration below. Verify real full-frame refresh, signed Hub/window identity,
scale, tooltips and open → closed → open display pairs through production MG.
Slow SSH must still fit the five-second click reservation; otherwise refuse
input and report that latency prerequisite. Do not clear uncertain fences,
reconnect RFB or consume another restart to manufacture acceptance.

M3 evidence identifies the selected `DeviceHub.app` as `com.apple.dt.Devices`,
declaring `DevicesTrampoline`, with the visible window owned by
`Contents/MacOS/DeviceHub`. Both executable and bundle have the strict Apple
anchor and exact code identifier. Frontmost can be either that window owner or
the selected installation's launcher with a different PID. Admission proves
both identities separately; it never equates those transient PIDs. Only these
two exact binaries beneath the selected bundle are admitted, with
`codesign --verify --strict -R '=anchor apple and identifier "com.apple.dt.Devices"'`.
Admission binds CG's globally first normal window directly, with a positive
CG PID and a unique positive integer window ID. It never searches the window
list for a matching title. Apple's native URL route can open the leased compact
device canvas in front of an existing device/management window showing the
previous iPhone. That layout is allowed: each lower selected-Hub window must
have a distinct positive integer ID and complete AppKit identity, a positive CG
PID whose kernel executable matches the exact selected `DeviceHub`, and the
same strict signed-installation verification. IDs must be positive and unique
across the normal-window list. Launcher or companion windows, unidentified
secondary selected windows, conflicting identities and unresolved PIDs still
refuse. Unrelated lower processes retain kernel identity/exclusion checks.
A foreign front app or foreign, incorrect or unknown top window refuses input,
even when the requested device/title appears behind it. No other device is
closed or selected through guest input to obtain this layout.

Native validation of this revision is **pending**: local OCR/guest observer
compilation, live full-frame VNC refresh, compact/expanded title-bar OCR,
CG/RFB coordinate scale, exact control labels, tooltip localization/placement
at 1x/2x, the synchronous holder input path, and both pose transitions need the
coordinator's M5/M3 lab run. Synthetic tests exercise the production controller
with fake guest evidence and OCR subprocesses; they do not establish native acceptance.

The supplied Apple Device Hub documentation describes three buttons below
the bezel: Closed immediately right of Rotate Right, Partially Open in the
middle, Open at the far right. It supplies no supported headless posture API.
This implementation uses those controls; display 3 being lit also occurs in
Partially Open and never independently establishes Open.

## One-time coordinator calibration

Use the authorized scheduler checkout and its existing private remote-wrapper
configuration. M5 remains the scheduler even when the coordinator is at M3.
Do not open another VNC client on a running VM. Do not perform provisioning,
permission setup or recovery as part of this procedure.

1. Before acquiring a device, the coordinator can use the existing
   `tools/mg sim-posture prepare` route if preparation is needed. Posture
   itself never calls it. Confirm `tools/mg sim-posture status` is connected;
   the M3 venv must pass preflight, and the existing M5 `build/vm-fold/venv` must have Pillow.
2. Acquire `foldable` through MG from this checkout. Keep that exact lease
   unexpired, with sufficient time for calibration (the terminal input budget
   is three minutes, the guest guard is bounded to eight). An actual click
   requires five minutes of exact remaining lease lifetime for the bounded
   command and readback; neither expiry nor ownership is changed. Do not release,
   renew or run another guest job during calibration.
3. Run `tools/mg sim-posture inspect <leased-udid>`. Privately inspect its saved
   guest PNG. The selected signed Device Hub must show the unique booted
   leased simulator, awake, in **compact or expanded canvas**, titled exactly
   `Meguri-Pool-foldable-N`. Both external display 1 and internal display 3
   must be readable, with exactly one lit and the other black. Fully identified
   signed same-Hub device/management windows may remain behind the leased
   canvas under the admission contract above. The exact-UDID URL selects the
   target; do not close other devices or issue guest input to select them.
   If it leaves a different canvas on top, stop. Never guess that a library,
   disconnected device or physical device is the simulator. Agents do not
   perform this manual lab step. Remove the inspection PNG afterwards.
4. In the operator's **foreground local terminal**, run
   `tools/mg sim-posture calibrate <leased-udid>`. The CLI launches/selects the
   exact UDID with `/usr/bin/open -a <exact validated Hub bundle>` followed by
   the ordinary URL argument `devices://device/open?id=<leased-udid>`, after
   validation and an immediate lease recheck. This also routes an already running
   Hub to the booted target. Inspection uses the same route. The CLI then
   requires the selected signed frontmost Hub and the exact leased canvas as
   the globally frontmost normal window, allowing verified lower same-Hub
   windows as described above. The front app and window
   owner may have different PIDs, but each must prove the exact selected bundle,
   identifier and approved signed executable. After proving this initial window
   and its exact leased title in a joined capture, the CLI uses the existing held
   VNC connection to move the guest pointer to the safe noninteractive titlebar
   point used by hover (horizontal center, 12 points below the window top).
   It verifies the actual pointer and unchanged signed window, waits for old
   tooltips to disappear, then acquires a fresh joined image and repeats title,
   pointer and window proof **before** asking for approval. There is no click,
   keyboard input or focus change during neutralization. It reports the guest CG
   window rectangle in points and displays this fresh image's private capture
   path directly in `/dev/tty`. No input file or executable path can be supplied.
5. Open the saved neutral image privately and confirm all
   three small glyphs are neutral and unoccluded. Xcode documents their order
   below the bezel as **Closed (left), Partially Open (middle), Open (right)**.
   Measure the **centers** of Closed and Open; the CLI derives the middle center
   halfway between them. Exclude Rotate Right. Derive scale `s` from screenshot
   dimensions divided by the single guest display dimensions: both axes must
   equal 1 or 2. Convert each PNG
   center `(px, py)` to window-relative points
   `(px/s - window.x, py/s - window.y)`. Confirm the two controls share a row
   (within two points) and Closed is left of Open. At the terminal, explicitly
   confirm `COMPACT <exact simulator name>` or `EXPANDED <exact simulator name>`,
   matching the saved canvas. This confirms the **named leased target itself**,
   the documented three-glyph order and a neutral, unoccluded toolbar with the
   pointer away and no tooltip or overlay; a selected sidebar row or a
   generic library is insufficient. Then enter the two measured `x y` pairs.
   Expanded canvas may keep the same Mac window size across pose changes;
   measure its actual controls without assuming compact proportions. No layout,
   target or control location is discovered from a sidebar or window dimensions.
6. Calibration emits **zero clicks and no keyboard input**; its only pointer
   movement is the verified titlebar neutralization before approval. A pointer
   left on a fold icon by normal use or legacy hover-only calibration needs no
   separate movement channel. The CLI checks
   the operator-approved image and two fresh joined captures for exact title-bar
   OCR, stable signed window/geometry/pointer and the same three distinct,
   nonempty glyphs. Every approved/fresh frame also needs neutral full-frame
   and localized crop OCR around all three controls. Movement away from the
   verified neutral pointer, visible pose labels anywhere in a control's wide
   tooltip ROI or nonempty high-confidence text within its precise tooltip
   scope that intersects the signed window refuses the calibration write.
   Unknown background text wholly outside the signed window may remain, but
   each OCR view's nearby text must be unchanged between the approved image
   and both fresh parked captures. Contents and original desktop-pixel bounds
   are compared only in memory and never saved in calibration. The row
   must be within two points, 12–90 points above the
   window bottom, with equal 24–48 point spacing. Each glyph must fit wholly
   inside its 22-point square patch and the signed window. Blank, clipped,
   duplicate, ambiguous or changed shapes refuse publication. Successful
   calibration writes only bounded JSON with three normalized binary glyph
   masks to the owner-only directory
   `~/Library/Application Support/meguri/sim-posture/` outside Git. Temporary
   desktop images are removed on joined completion; no raw frame or crop is
   saved in calibration, shipped as an asset or accepted as a global template.

The record binds exact selected Xcode path/version/build, signed Hub trampoline
SHA-256, VM, UDID, device name/runtime, confirmed viewport, window dimensions, single-monitor
geometry and VNC scale. It contains no executable instructions or supplied
paths. Every pose request repeats fresh title and control verification. Window
movement works through relative coordinates; resizing, scale changes, another Xcode,
target, monitor arrangement or layout require fresh calibration. New records
use schema 3 with an explicit `viewport` of `compact` or `expanded` and private
glyph proof. Existing schema-1 (`compact: true`) and schema-2 (`viewport`) records
keep their exact bindings and require the existing positive fresh localized
tooltip and changed-pixel path; coordinates alone never acquire fallback
authority. Explicit operator recalibration is required for glyph proof.
Unknown schemas, mixed fields or other viewport values refuse. Both layouts
require the exact leased title as high-confidence OCR in the top 40 points and
stable signed owner/window/geometry and pointer. Schema 3 also requires both
endpoints **and the middle glyph** in pointer-away and hovered fresh captures.
Each mask must match its private calibrated position with at least 95% foreground
intersection-over-union and remain distinct from either other glyph (at most
80% overlap). Local background-relative contrast removes uniform background or
selection tint and allows foreground polarity changes; there is no position
search, shifted matching or screen-coordinate fallback. The three-glyph proof
distinguishes full Open from Partially Open even when the display state cannot.

The existing strong exact localized tooltip path remains preferred when present.
Before hovering, the pointer-away baseline must have no recognized nearby pose
labels in either its full frame or its accurate localized crop, and no nonempty
high-confidence text within the candidate control's precise tooltip scope that
intersects the actual signed window. Unknown background text wholly outside
that window is not a baseline veto solely by position. Intersection uses the
original bounds in desktop pixels; a partially overlapping row intersects.
This exception applies only to unknown baseline text; pose-label vetoes still
cover the whole wide ROI. A stale visible label, including the
correct requested tooltip, refuses the request; this
candidate does not retry the baseline. In every hover sample, wrong or ambiguous
nearby pose labels in either OCR view fail immediately, even if all glyphs agree.
Conflicting labels, duplicate labels and split partial-open text refuse. Every
hover sample also refuses any new nonempty high-confidence nearby text other than the
exact requested label. Each OCR view is compared with its own baseline using
original text and bounds across the entire wide ROI, including outside the
signed window: new, relocated, resized or mutated unknown text always refuses.
Unchanged canvas text outside the precise tooltip scope or background text
wholly outside the signed window may remain. Unknown OCR strings
never enter diagnostics or receipts. A later
unlabelled sample can never erase any recognized label: even a correct label
that fails freshness or precise tooltip scope blocks glyph fallback for that
request. Exact title proof always uses the same saved full frame, and positive
tooltip proof still needs the precise control scope and changed pixels within
that frame's localized rectangle. The joined frame returned by the actual move
onto the target is the first observation; it is never discarded. Schema 3 may
return immediately on that single complete observation when all three fresh
matching glyphs, exact leased title/window/pointer and a fresh neutral baseline
are proved, with no recognized nearby pose label in any sample. It does not
start another acquisition after complete adequate proof. A correct label that
fails tooltip freshness or scope still blocks glyph fallback for the request.
For schema 3, every baseline and hover frame must match all three private glyphs.

Acquisition has a 240-second budget beginning before the target move, including
joined capture, transfer of the large PNG, both offscreen OCR calls, glyph checks
and signed pointer/window observations. Local FIFO captures retain their
130-second join bound and hosted capture RPCs their 145-second transport bound;
each OCR call remains bounded to 30 seconds. Legacy tooltip-only requests (and
requests whose recognized correct label needs stronger tooltip proof) may
observe again, with at most eight seconds of polling delays (32 additional
samples), within the acquisition budget. Transport and OCR time count against
acquisition rather than that polling allowance. A repeat starts only with room
for the previous complete sample's duration plus the polling delay. Expired or incomplete
acquisition evidence permits zero clicks. All checks of a successful observation,
including changed pixels when using a tooltip, must finish before the acquisition
deadline. Acquisition reserves no input ticket. Publication still requires all
five-second resident ticket checks, arm/publication acknowledgments, the final
signed exact pointer callback, joined click and two consecutive post-click display
proofs. No resident protocol change is required.
Title and tooltip OCR use offscreen Apple Vision Accurate on M5, reading only
the saved frame captured through the same bound resident. The reviewed
`sim-posture-ocr.swift` helper compiles once per invocation in its private
mode-0700 temporary folder with M5's default selected Xcode; compilation and
recognition use bounded quiet subprocesses. No frame returns to the guest for
OCR. Title proof continues to use the full frame. Tooltip proof reuses that
compiled helper on the existing candidate rectangle in each saved before/after
frame, mapping crop boxes back to guest desktop pixels with the clamped crop
origin. Mode-0600 crops live in private temporary folders and are removed after
recognition, including on failure; originals remain for title and local changed-pixel
proof. Both OCR calls count toward the separate acquisition deadline; neither
extends or consumes a click publication ticket.
Failure-only hover diagnostics contain only the requested allowlisted pose,
whether the baseline had a known pose, the last sample's allowlisted pose labels,
sample count and a fixed rejection reason. They contain no recognized arbitrary
text, glyph masks, pixels or desktop geometry.
Unavailable recognition refuses input; exact text and confidence >=0.9
remain required. Window/process/geometry observation stays guest-only, with
its observer source sent per request. M5 performs no desktop capture or UI.
The actual window owner PID, window ID and geometry stay bound throughout each
request; a newly admitted launcher PID does not invalidate an unchanged owner.
Both app identities must be active, nonterminated and finished launching. The
front entry and the AppKit object looked up by the positive CG owner PID may
report Apple's documented PID `-1`. A positive lookup PID must equal CG's PID;
zero, other values and missing lookups refuse. Direct `proc_pidpath` on the CG
PID must match both AppKit's executable and the exact selected Apple-signed
`DeviceHub` binary. The positive CG PID stays bound throughout; sentinel-valued
lookups never hide secondary Hub windows: known lower actual owners require
the same identity checks, while missing selected/companion identity refuses.
Exact selected paths and signatures remain required for both front and owner.
Only exact high-confidence title evidence within the bound top window's top
40 points can admit the leased target; global OCR hits or previous-device titles
behind it cannot supply evidence. Switching active canvases fails the stable
PID, ID and rectangle binding. Fresh keyboard-free pointer, control evidence
as above and single-click proof remain required. The first pre-input window observation
alone may poll read-only for five seconds after opening the leased target; an
unrelated top window is never admitted. Later observations fail immediately on
mismatch, and no click is retried.
Multiple guest
monitors and nonzero display origins are currently unsupported and fail closed.
Do not broaden matching to arbitrary Apple apps.

## Per-request proof and failure handling

The scheduler lease and guest `pool.lock` cover exact live checkout/VM/UDID
lease admission, pinned booted Duo validation, signed selected Hub/window
evidence, input and display proof. The lease inode/content is pinned for the
request. Legacy renew/release/reassignment cannot pass the same pool mutex.
The selected Xcode's simctl is used explicitly; an ambient `DEVELOPER_DIR`
cannot redirect it.

Read-only FIFO moves/captures use the unique private PNG and exact pinned
holder-log suffix as their execution barrier. They never arm durable fences.
Actual clicks require the current holder's `posture-click-v1` capability; old
holders still support inspection/calibration, but refuse actual input. Only the
coordinator's existing preparation route may replace a holder; posture does
not reconnect or upgrade it.

The one click uses a bounded synchronous callback on that same holder's reactor
thread. It checks the conservative lease handoff deadline before sending the
button events and writes an atomic, unique terminal receipt independently of
screenshots. A failed screenshot cannot make an already joined click unknown.
The scheduler fence is created under its existing mutex immediately around the
atomic nonblocking FIFO publication. A delayed callback refuses input after
its deadline. Missing/malformed terminal proof retains isolation and private
evidence; no second click is attempted. Do not write to the FIFO concurrently.

After the verified exact Open/Closed click, read-only simctl screenshots of
the **same leased UDID**, displays **1 and 3**, must show the requested active
display and the opposite black display in two consecutive samples. Sleeping,
dim, both-lit, both-black and ambiguous classifications remain transitional
during the bounded settling window and reset the consecutive-sample count.
Initial display admission remains strict. An unreadable frame fails. A
transition that never supplies two consecutive expected pairs times out.
Success receipts contain only requested control, joined click count and
per-display dimensions, coarse black/lit percentages and classification before
and after. No pixels, OCR/card content, desktop titles or pixel hashes enter
MG results. Open acceptance combines the exact freshly verified Open control
with the display pair; it is not inferred from inner-display light alone.

Read-only/abandoned hover guards release on EOF or their eight-minute alarm.
Calibration refuses a missing/nonforeground terminal before any guest work.
Ordinary refusals join the guest guard and remove private captures. Arm/release
requests use exact framed JSON IDs; lost transport is never a click join. A
known terminal/nonexecution path can disarm that same token-bound guard through
one bounded fresh SSH control request when the original channel has failed.
It never reclaims an unrelated lock or emits input. Uncertain actual click
completion retains private evidence and
`sim-posture-pending` fences in the scheduler lease and guest pool lock. Neither
stale-holder cleanup nor pool stale-lock reclamation removes those fences.
Only owned child transports receive termination signals. Reconcile the exact
outstanding FIFO command and remote guard on M3/M5 before coordinator cleanup;
never clear a fence on PID disappearance alone, restart/bootstrap the guest,
or rerun a pose action to resolve an unknown click.

Coordinator checks: `tools/mg image-tools-selftest -k posture`,
`tools/mg selftest tools/tests/vnc_session.bats`, then the lab procedure and
both production commands, including deliberately moved/resized/wrong-target
windows, a missing tooltip, sleeping displays and an expired/foreign lease.
Record the native outcomes privately and publish only sanitized classifications.
