# Leased Duo posture (#205)

`tools/mg sim-posture open <leased-udid>` and `closed <leased-udid>` use the
existing persistent VNC FIFO holder. `prepare [--vm <name>]`,
`status [--vm <name>]` and `inspect <leased-udid>` keep their invocation.
Posture never prepares/reconnects VNC, boots a device, starts/restarts the VM,
changes consent or uses the coordinator's GUI. There is no host simulator
fallback. The former unleased `hub`, fixed-coordinate `fold` and generic
`click` helper routes have been removed.

All commands share the transport's state selection: normally `build/vm-fold`,
or `build/vmf2` after verified canonical old-orphan admission. Both use
`build/vm-fold/venv` and the primary M5 scheduler lease; mutating commands
recompute routing and revalidate selected VM metadata under that lease.

Native evidence is **pending**: selected Device Hub process identity, Swift
helper compilation, live full-frame VNC refresh, compact title-bar OCR,
CG/RFB coordinate scale, exact control labels, tooltip localization/placement
at 1x/2x, the synchronous holder input path, and both pose transitions need the
coordinator's M3 lab run. Synthetic tests exercise the production controller
with fake guest evidence; they do not establish native acceptance. The
cleanroom's MG test attempt could not start because `jq` is absent.

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
   the existing `build/vm-fold/venv` must already have vncdotool and Pillow.
2. Acquire `foldable` through MG from this checkout. Keep that exact lease
   unexpired, with sufficient time for calibration (the terminal input budget
   is three minutes, the guest guard is bounded to eight). An actual click
   requires five minutes of exact remaining lease lifetime for the bounded
   command and readback; neither expiry nor ownership is changed. Do not release,
   renew or run another guest job during calibration.
3. Run `tools/mg sim-posture inspect <leased-udid>`. Privately inspect its saved
   guest PNG. The selected signed Device Hub must show the unique booted
   leased simulator, awake, in **COMPACT** view, titled exactly
   `Meguri-Pool-foldable-N`. Both external display 1 and internal display 3
   must be readable, with exactly one lit and the other black. Close other
   normal Hub windows in the guest, including its library. If arrangement or
   selecting compact view needs operator input, use only the already connected
   holder's FIFO with fresh private captures. Never guess that a library,
   disconnected device or physical device is the simulator. Agents do not
   perform this manual lab step. Remove the inspection PNG afterwards.
4. In the operator's **foreground local terminal**, run
   `tools/mg sim-posture calibrate <leased-udid>`. The CLI launches/selects the
   exact UDID through the selected Hub's `-CurrentDeviceUDID` argument, then
   requires the selected signed frontmost Hub and exactly one unobscured
   normal window. It saves its own private guest screenshot, reports the
   guest CG window rectangle in points, and displays its capture path directly
   in `/dev/tty`. No input file or executable path can be supplied.
5. Open that saved image privately and measure the **centers** of Closed and
   Open below the bezel. Exclude Rotate Right and the middle Partially Open
   button. Derive scale `s` from screenshot dimensions divided by the single
   guest display dimensions: both axes must equal 1 or 2. Convert each PNG
   center `(px, py)` to window-relative points
   `(px/s - window.x, py/s - window.y)`. Confirm the two controls share a row
   (within two points) and Closed is left of Open. At the terminal, confirm
   `COMPACT <exact simulator name>`, then enter the two measured `x y` pairs.
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
SHA-256, VM, UDID, device name/runtime, compact window dimensions, single-monitor
geometry and VNC scale. It contains no executable instructions or supplied
paths. Every pose request repeats tooltip/title verification. Window movement
works through relative coordinates; resizing, scale changes, another Xcode,
target, monitor arrangement or view require fresh calibration. Multiple guest
monitors and nonzero display origins are currently unsupported and fail closed.
If the selected Hub trampoline runs with a different frontmost bundle identity,
this version refuses it; the coordinator must record native identity evidence
before any admission change. Do not broaden matching to arbitrary Apple apps.

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
