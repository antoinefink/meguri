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
`build/vm-fold` or `build/vmf2`. M5 remains the only scheduler, with the same
primary lease, guest acquisition/window/OCR guards and display readback.
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
# Only after both readiness jobs pass and normal jobs/acquisitions have drained:
tools/mg sim-posture prepare
tools/mg sim-posture status
# Repeat: must report reused, with no stop, start or new RFB connection.
tools/mg sim-posture prepare
```

Use MG result status/details to verify each job; its process exit alone is not
a pass.

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

After the ordinary M5 lease, idle-test check and committed guest drain, M3
reserves one token-bound detached stop peer. Its single terminal receipt
requires the actual stop command to join **and** M3 to confirm Tart stopped.
Only that receipt admits one detached resident start with the actual local
Tart binary. Stop/start reservations, logs and receipts stay in the private
kit. Controller loss cannot kill these peers. Lost replies use fresh receipt
or status calls; an unknown reservation is never vacant. A completed stop
followed by an interrupted start can resume that reservation through prepare.
A caught log-open or `Popen` failure durably marks a proven pre-spawn failure;
only the original VM/drain token may retry it. The active drain caller queries
that marker before retrying its stop reservation; prepare can resume a marked
start failure. A missing failure marker always retains uncertain ownership,
even if no child can be observed. Persistent stop failure retains the scheduler
and guest fences for token-bound reconciliation; a new token cannot replace it.
There is no rollback restart or automatic replacement of a disconnected owner,
and this versioned runtime refuses a second migration stop. Hosted `stop`
refuses because it would require a further authorized restart.

An established hosted connection always returns `reused` from ordinary prepare,
including after M5 SSH/client loss. It never stops, starts or reconnects Tart.
An already connected legacy/local route is likewise preserved. The resident
supervisor owns its original children and authenticated loopback RFB connection
on M3; no new remote session uses a foreground M5 VNC tunnel. No other RFB
client may connect to running Tart.

Capture and move+capture requests reuse that connection and the existing local
Holder's FIFO/log/complete-PNG barrier. Only bounded PNG bytes cross SSH into
exclusive owner-only files in unique private coordinator directories. The
service accepts typed coordinates, never supplied paths or executable commands.

For a click, M5 still holds its scheduler and exact guest lease/window guard.
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

Native acceptance remains with the integrator: first verify the ready M3 kit,
then use the single authorized drained migration. Interrupt the M5 client/SSH
between joined read-only operations, verify the same resident session/children
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
It counts normal Hub windows across that bundle, requires exactly one and
requires it to be the top normal window globally. A foreign front app, another
installation, another binary or an additional normal Hub window refuses input.

Native validation of this revision is **pending**: Swift
helper compilation, live full-frame VNC refresh, compact/expanded title-bar OCR,
CG/RFB coordinate scale, exact control labels, tooltip localization/placement
at 1x/2x, the synchronous holder input path, and both pose transitions need the
coordinator's M3 lab run. Synthetic tests exercise the production controller
with fake guest evidence; they do not establish native acceptance.

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
   must be readable, with exactly one lit and the other black. Close other
   normal Hub windows in the guest, including its library. If arrangement or
   selecting the target canvas needs operator input, use only the already connected
   holder's FIFO with fresh private captures. Never guess that a library,
   disconnected device or physical device is the simulator. Agents do not
   perform this manual lab step. Remove the inspection PNG afterwards.
4. In the operator's **foreground local terminal**, run
   `tools/mg sim-posture calibrate <leased-udid>`. The CLI launches/selects the
   exact UDID through the selected Hub's `-CurrentDeviceUDID` argument, then
   requires the selected signed frontmost Hub and exactly one unobscured
   normal window across the selected signed bundle. The front app and window
   owner may have different PIDs, but each must prove the exact selected bundle,
   identifier and approved signed executable. It saves its own private guest screenshot, reports the
   guest CG window rectangle in points, and displays its capture path directly
   in `/dev/tty`. No input file or executable path can be supplied.
5. Open that saved image privately and measure the **centers** of Closed and
   Open below the bezel. Exclude Rotate Right and the middle Partially Open
   button. Derive scale `s` from screenshot dimensions divided by the single
   guest display dimensions: both axes must equal 1 or 2. Convert each PNG
   center `(px, py)` to window-relative points
   `(px/s - window.x, py/s - window.y)`. Confirm the two controls share a row
   (within two points) and Closed is left of Open. At the terminal, explicitly
   confirm `COMPACT <exact simulator name>` or `EXPANDED <exact simulator name>`,
   matching the saved canvas. This confirms the **named leased target itself**
   and its visible pose buttons below its bezel; a selected sidebar row or a
   generic library is insufficient. Then enter the two measured `x y` pairs.
   Expanded canvas may keep the same Mac window size across pose changes;
   measure its actual controls without assuming compact proportions. No layout,
   target or control location is discovered from a sidebar or window dimensions.
6. The CLI verifies both candidates **without clicking**: move away inside the
   validated window, capture a baseline, hover the candidate, then require a
   newly visible high-confidence en-US `Closed` or `Open` tooltip near that
   point. It requires changed local pixels, exact title-bar OCR and unchanged
   guest foreground/window geometry, plus the actual guest pointer position.
   Missing/localized/ambiguous/stale evidence refuses publication. It never
   substitutes the Partially Open tooltip. Successful calibration writes only
   bounded JSON to the owner-only directory
   `~/Library/Application Support/meguri/sim-posture/` outside Git. Temporary
   desktop images are removed on joined completion.

The record binds exact selected Xcode path/version/build, signed Hub trampoline
SHA-256, VM, UDID, device name/runtime, confirmed viewport, window dimensions, single-monitor
geometry and VNC scale. It contains no executable instructions or supplied
paths. Every pose request repeats tooltip/title verification. Window movement
works through relative coordinates; resizing, scale changes, another Xcode,
target, monitor arrangement or layout require fresh calibration. New records
use schema 2 with an explicit `viewport` of `compact` or `expanded`. Existing
valid schema-1 records (`compact: true`) retain their compact-only meaning and
exact binding/geometry checks; they are never migrated into expanded records.
Unknown schemas, mixed fields or other viewport values refuse. Both layouts
require the exact leased title as high-confidence OCR in the top 40 points and
the same fresh exact tooltip, changed-pixel and pointer proofs for each control.
The actual window owner PID, window ID and geometry stay bound throughout each
request; a newly admitted launcher PID does not invalidate an unchanged owner.
Both app identities must be active, nonterminated and finished launching. The
front entry and the AppKit object looked up by the positive CG owner PID may
report Apple's documented PID `-1`. A positive lookup PID must equal CG's PID;
zero, other values and missing lookups refuse. Direct `proc_pidpath` on the CG
PID must match both AppKit's executable and the exact selected Apple-signed
`DeviceHub` binary. The positive CG PID stays bound throughout; sentinel-valued
lookups still count when excluding other Hub windows. Exact selected paths
and signatures remain required for both. The first pre-input window observation
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
