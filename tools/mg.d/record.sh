# Screen recording and contact sheets (U8, R1976). Sourced by tools/mg.
#
# `record start --device <udid>` records a pool simulator this checkout
# leased: it takes the host lease, checks the device's runtime against the pin
# and hands the lease to a recording worker (tools/scripts/sim-record.sh,
# its own session) that keeps it, and the display awake, until `record stop
# --device <udid>` or a limit (MEGURI_RECORD_MAX_SECONDS, 600). While it
# records, this checkout's jobs (run, drive) run nested in its lease so it can
# film them; other checkouts queue. `drive --record` records itself around a
# drive. `contact-sheet` runs tools/scripts/contact-sheet in the image-tools
# container (R1952): about 12 frames per marked window tiled into a PNG, plus
# jump cuts and stalls as data for the critic. The Mac is recorded only
# through `drive --platform mac --record` (drive.sh).

mg_job record host mg_record \
  "Record a simulator's screen: start --device <leased udid> | stop --device <udid> (video under build/mg/record/; the recording holds the host lease until stop or MEGURI_RECORD_MAX_SECONDS, 600; then tools/mg contact-sheet). The Mac: tools/mg drive --platform mac --record"
mg_job contact-sheet host mg_contact_sheet \
  "Contact sheets, jump cuts and stalls from a recording, in the image-tools container: <video> [--window <name>=<start>-<end>]... [--marks marks.json] [--fit] [--frames 12] [--columns 4]"

# Active recordings of this checkout, one state file per simulator.
MG_RECORD_ACTIVE=build/mg/record/active

mg__hires_now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }

# mg_record__wait_started <log> <pid>: waits (10 s) for simctl to say it is
# recording while <pid> lives; sets MG_RECORD_STARTED to that moment (epoch
# seconds).
mg_record__wait_started() {
  local i
  for i in $(seq 1 100); do
    if grep -q 'Recording started' "$1" 2>/dev/null; then
      MG_RECORD_STARTED="$(mg__hires_now)"
      return 0
    fi
    kill -0 "$2" 2>/dev/null || return 1
    sleep 0.1
  done
  return 1
}

# mg_record__start_recorder <udid> <video> <log>: simctl io recordVideo in the
# background; sets MG_RECORDER_PID. A background command starts with SIGINT
# ignored, and simctl finishes the file on SIGINT: the default comes back.
mg_record__start_recorder() {
  perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die "exec: $!"' \
    xcrun simctl io "$1" recordVideo --codec h264 --force "$2" </dev/null >"$3" 2>&1 &
  MG_RECORDER_PID=$!
}

# mg_record__stop_recorder <pid>: SIGINT, then SIGKILL after 15 s; returns
# once it has exited.
mg_record__stop_recorder() {
  local i
  kill -INT "$1" 2>/dev/null
  for i in $(seq 1 150); do
    kill -0 "$1" 2>/dev/null || break
    sleep 0.1
  done
  kill -9 "$1" 2>/dev/null
  wait "$1" 2>/dev/null
}

# mg_record__proc_start <pid>: when the process started (ps lstart); empty
# when no such process runs.
mg_record__proc_start() { ps -o lstart= -p "$1" 2>/dev/null | sed 's/[[:space:]]*$//'; }

# mg_record__worker_alive <state file>: whether the recording worker the
# state names still runs. A worker that stopped at the limit leaves its state
# until record stop, and its PID may belong to another process by then: the
# process must have the worker's start time and run sim-record.sh.
mg_record__worker_alive() {
  local pid started
  pid="$(jq -r '.pid // empty' "$1" 2>/dev/null)"
  started="$(jq -r '.pid_started // empty' "$1" 2>/dev/null)"
  [ -n "$pid" ] && [ -n "$started" ] && [ "$(mg_record__proc_start "$pid")" = "$started" ] || return 1
  case "$(ps -o command= -p "$pid" 2>/dev/null)" in *sim-record.sh*) return 0 ;; esac
  return 1
}

# Set once record start has handed the lease and the recording to the worker;
# until then, the job's exit stops the worker (cancellation, failures).
MG_RECORD_HANDED=""

mg_record() {
  local sub="${1:-}" udid=""
  [ "$#" -gt 0 ] && shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --device) [ "$#" -ge 2 ] && udid="$2" && shift 2 || { mg_drive__usage "--device needs a UDID"; return 0; } ;;
      --platform) mg_drive__usage "record takes simulators only (--device <udid>); record the Mac with tools/mg drive <script> --platform mac --record"; return 0 ;;
      *) mg_drive__usage "usage: tools/mg record start|stop --device <udid>"; return 0 ;;
    esac
  done
  case "$sub" in start | stop) ;; *) mg_drive__usage "usage: tools/mg record start|stop --device <udid>"; return 0 ;; esac
  [ -n "$udid" ] || { mg_drive__usage "record $sub needs --device <udid> (a pool simulator this checkout leased)"; return 0; }
  mkdir -p "$MG_RECORD_ACTIVE"
  if [ "$sub" = start ]; then
    mg_record__start "$udid"
  else
    mg_record__stop "$udid"
  fi
}

mg_record__start() {
  local udid="$1" state="$MG_RECORD_ACTIVE/$1.json"
  if [ -f "$state" ] && mg_record__worker_alive "$state"; then
    mg_failure recording "$udid is already being recorded into $(jq -r .video "$state")" "tools/mg record stop --device $udid"
    mg_status error
    return 0
  fi
  mg_sim_check_owned "$udid" || return 0
  mg_sim_check_runtime "$udid" || return 0
  mg_host_lease || return 0
  mg_sim_prepare "$udid" || return 0
  local video="$MG_RUN_DIR/video.mp4" log="$MG_RUN_DIR/recorder.log" status="$MG_RUN_DIR/recording-status.json"
  local max="${MEGURI_RECORD_MAX_SECONDS:-600}" pid
  case "$max" in "" | *[!0-9]* | 0) max=600 ;; esac
  # Its own session, so it outlives this job; record stop ends it. No
  # descriptor of this job: tools/mg waits for its output pipe to close.
  perl -e 'use POSIX qw(setsid); setsid(); exec @ARGV or die "exec: $!"' \
    /bin/bash "$MG_TOOLS/scripts/sim-record.sh" "$udid" "$video" "$log" "$(mg_host_lease_dir)" "$status" "$max" \
    </dev/null >"$MG_RUN_DIR/worker.log" 2>&1 &
  pid=$!
  mg_on_exit "[ -n \"\$MG_RECORD_HANDED\" ] || kill -TERM $pid 2>/dev/null"
  if ! mg_record__wait_started "$log" "$pid"; then
    mg_failure record "simctl io recordVideo did not start; see $log"
    mg_status error
    return 0
  fi
  if ! mg_host_lease_transfer "$pid" "$(jq -nc --arg u "$udid" --arg v "$video" '{kind: "recording", job: "record", udid: $u, video: $v}')"; then
    mg_failure record "could not hand the host lease to the recording worker; the recording was stopped"
    mg_status error
    return 0
  fi
  MG_RECORD_HANDED=1
  jq -n --arg u "$udid" --argjson pid "$pid" --arg ps "$(mg_record__proc_start "$pid")" --arg v "$video" --arg log "$log" --arg st "$status" \
    --argjson t "$MG_RECORD_STARTED" --arg run "$MG_RUN_ID" --argjson max "$max" \
    '{udid: $u, pid: $pid, pid_started: $ps, video: $v, log: $log, status: $st, started_epoch: $t, run_id: $run, max_seconds: $max}' >"$state"
  mg_detail recording "$(cat "$state")"
  mg_message "record: recording $udid into $video (it holds the host lease; this checkout's jobs run inside it); stop with tools/mg record stop --device $udid (at the latest after $max s)"
  mg_status pass
}

mg_record__stop() {
  local udid="$1" state="$MG_RECORD_ACTIVE/$1.json"
  if [ ! -f "$state" ]; then
    mg_failure not-recording "no recording of $udid was started from this checkout" "tools/mg record start --device $udid"
    mg_status error
    return 0
  fi
  local pid video status max i reason
  pid="$(jq -r .pid "$state")"
  video="$(jq -r .video "$state")"
  status="$(jq -r '.status // empty' "$state")"
  max="$(jq -r '.max_seconds // "?"' "$state")"
  if [ -s "$status" ]; then
    # The worker ended on its own (the limit, or the recorder exited) or is
    # ending: it writes its status, then releases its slot. Nothing to signal.
    for i in $(seq 1 50); do
      mg_record__worker_alive "$state" || break
      sleep 0.1
    done
  elif mg_record__worker_alive "$state"; then
    kill -TERM "$pid" 2>/dev/null
    for i in $(seq 1 200); do
      mg_record__worker_alive "$state" || break
      sleep 0.1
    done
    if mg_record__worker_alive "$state"; then
      # The worker leads its own process group: end it with the recorder.
      kill -9 -- "-$pid" 2>/dev/null || kill -9 "$pid" 2>/dev/null
      mg__host_lease_release "$(mg_host_lease_dir)" "$pid"
      mg_failure record "the recording worker did not stop within 20 s; the video may be unusable"
    fi
  else
    # The worker died without a status (killed): release the slot it held.
    mg__host_lease_release "$(mg_host_lease_dir)" "$pid"
  fi
  reason="$(jq -r '.reason // empty' "$status" 2>/dev/null)"
  mg_detail recording "$(jq -c --arg at "$(mg__hires_now)" --arg r "${reason:-unknown}" \
    '. + {stopped_epoch: ($at | tonumber), reason: $r}' "$state")"
  rm -f "$state"
  if [ ! -s "$video" ]; then
    mg_failure record "no video at $video (the recording ended: ${reason:-unknown})"
    mg_status error
    return 0
  fi
  mg_artifact video "$video" "recording of $udid"
  case "$reason" in
    stopped)
      mg_message "record: $video; make sheets with tools/mg contact-sheet $video --window <name>=<start>-<end>"
      mg_status pass
      ;;
    limit)
      mg_message "record: $video stopped at the $max s limit, before record stop"
      mg_status warn
      ;;
    *)
      mg_failure record "the recording ended before record stop (${reason:-the worker died}); $video may be cut short; see $(dirname "$video")/recorder.log"
      mg_message "record: the recording of $udid ended early (${reason:-worker died})"
      mg_status error
      ;;
  esac
}

mg_contact_sheet() {
  local video="" args=() p vp
  [ "$#" -gt 0 ] || { mg_drive__usage "usage: tools/mg contact-sheet <video> [--window name=start-end]... [--marks file] [--frames N] [--columns N]"; return 0; }
  video="$1"
  shift
  [ -f "$video" ] || { mg_drive__usage "no video at $video"; return 0; }
  vp="$(mg_container_path "$(cd "$(dirname "$video")" && pwd -P)/$(basename "$video")")" ||
    { mg_drive__usage "$video is outside the checkout; the container sees only the checkout"; return 0; }
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --marks)
        [ "$#" -ge 2 ] && [ -f "$2" ] || { mg_drive__usage "--marks needs a marks file"; return 0; }
        if ! p="$(mg_container_path "$(cd "$(dirname "$2")" && pwd -P)/$(basename "$2")")"; then
          mg_drive__usage "$2 is outside the checkout"
          return 0
        fi
        args=(${args[0]+"${args[@]}"} --marks "$p")
        shift 2
        ;;
      --fit) args=(${args[0]+"${args[@]}"} --fit) && shift ;;
      --window | --frames | --columns | --cut-threshold | --stall-seconds)
        [ "$#" -ge 2 ] || { mg_drive__usage "$1 needs a value"; return 0; }
        args=(${args[0]+"${args[@]}"} "$1" "$2")
        shift 2
        ;;
      *) mg_drive__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  local rc
  mg_image_tools python3 tools/scripts/contact-sheet/contact_sheet.py "$vp" --out "$(mg_container_path "$MG_RUN_DIR")" ${args[0]+"${args[@]}"} >&2
  rc=$?
  if [ "$rc" -ne 0 ] || [ ! -f "$MG_RUN_DIR/report.json" ]; then
    mg_failure contact-sheet "contact_sheet.py exited with code $rc; see log.txt"
    mg_status error
    return 0
  fi
  local sheet
  for sheet in $(jq -r '.windows[].name' "$MG_RUN_DIR/report.json"); do
    mg_artifact contact-sheet "$MG_RUN_DIR/$sheet.png" "$sheet"
  done
  mg_artifact report "$MG_RUN_DIR/report.json" "frames, jump cuts and stalls"
  mg_detail motion "$(jq -c --arg root /work/ --arg host "$MG_ROOT/" '{video: .video, jump_cuts, windows: [.windows[] | {name, start, end, sheet, jump_cuts, stalls}]}
    | walk(if type == "string" and startswith($root) then $host + ltrimstr($root) else . end)' "$MG_RUN_DIR/report.json")"
  mg_count total "$(jq '.windows | length' "$MG_RUN_DIR/report.json")"
  mg_count jump_cuts "$(jq '.jump_cuts | length' "$MG_RUN_DIR/report.json")"
  mg_count stalls "$(jq '[.windows[].stalls[]] | length' "$MG_RUN_DIR/report.json")"
  mg_message "contact-sheet: $(jq -r '"\(.windows | length) sheet(s), \(.jump_cuts | length) jump cut(s), \([.windows[].stalls[]] | length) stall(s)"' "$MG_RUN_DIR/report.json")"
  mg_status pass
}
