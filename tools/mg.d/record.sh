# Screen recording and contact sheets (U8, R1976). Sourced by tools/mg.
#
# `record start --device <udid>` starts `simctl io recordVideo` on a pool
# simulator this checkout leased and returns; `record stop --device <udid>`
# stops it and reports the video. `drive --record` does both around a drive
# and makes the sheets itself. `contact-sheet` runs
# tools/scripts/contact-sheet in the image-tools container (R1952): about 12
# frames per marked window tiled into a PNG, plus jump cuts and stalls as data
# for the critic. Mac recording is unavailable: it needs Screen Recording, and
# XCUITest cannot sample frames while an action runs.

mg_job record host mg_record \
  "Record a simulator's screen: start --device <leased udid> | stop --device <udid> (video under build/mg/record/; then tools/mg contact-sheet)"
mg_job contact-sheet host mg_contact_sheet \
  "Contact sheets, jump cuts and stalls from a recording, in the image-tools container: <video> [--window <name>=<start>-<end>]... [--marks marks.json] [--fit] [--frames 12] [--columns 4]"

# Active recordings of this checkout, one state file per simulator.
MG_RECORD_ACTIVE=build/mg/record/active

mg__hires_now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }

# mg_record__wait_started <log> <pid>: waits (10 s) for simctl to say it is
# recording; sets MG_RECORD_STARTED to that moment (epoch seconds).
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

mg_record() {
  local sub="${1:-}" udid=""
  [ "$#" -gt 0 ] && shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --device) [ "$#" -ge 2 ] && udid="$2" && shift 2 || { mg_drive__usage "--device needs a UDID"; return 0; } ;;
      *) mg_drive__usage "usage: tools/mg record start|stop --device <udid>"; return 0 ;;
    esac
  done
  case "$sub" in start | stop) ;; *) mg_drive__usage "usage: tools/mg record start|stop --device <udid>"; return 0 ;; esac
  [ -n "$udid" ] || { mg_drive__usage "record $sub needs --device <udid> (a pool simulator this checkout leased)"; return 0; }
  local state="$MG_RECORD_ACTIVE/$udid.json"
  mkdir -p "$MG_RECORD_ACTIVE"
  if [ "$sub" = start ]; then
    if [ -f "$state" ] && kill -0 "$(jq -r .pid "$state")" 2>/dev/null; then
      mg_failure recording "$udid is already being recorded into $(jq -r .video "$state")" "tools/mg record stop --device $udid"
      mg_status error
      return 0
    fi
    mg_sim_check_owned "$udid" || return 0
    mg_sim_prepare "$udid"
    local video="$MG_RUN_DIR/video.mp4" log="$MG_RUN_DIR/recorder.log" pid
    # Its own session, so it outlives this job; record stop ends it.
    perl -e 'use POSIX qw(setsid); setsid(); exec @ARGV or die "exec: $!"' \
      xcrun simctl io "$udid" recordVideo --codec h264 --force "$video" </dev/null >"$log" 2>&1 &
    pid=$!
    if ! mg_record__wait_started "$log" "$pid"; then
      kill -INT "$pid" 2>/dev/null
      mg_failure record "simctl io recordVideo did not start; see $log"
      mg_status error
      return 0
    fi
    jq -n --arg u "$udid" --argjson pid "$pid" --arg v "$video" --argjson t "$MG_RECORD_STARTED" --arg run "$MG_RUN_ID" \
      '{udid: $u, pid: $pid, video: $v, started_epoch: $t, run_id: $run}' >"$state"
    mg_detail recording "$(cat "$state")"
    mg_message "record: recording $udid into $video; stop with tools/mg record stop --device $udid"
    mg_status pass
    return 0
  fi

  if [ ! -f "$state" ]; then
    mg_failure not-recording "no recording of $udid was started from this checkout" "tools/mg record start --device $udid"
    mg_status error
    return 0
  fi
  local pid video i
  pid="$(jq -r .pid "$state")"
  video="$(jq -r .video "$state")"
  kill -INT "$pid" 2>/dev/null
  for i in $(seq 1 150); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.1
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null
    mg_failure record "the recorder did not stop within 15 s; the video may be unusable"
  fi
  mg_detail recording "$(jq -c --arg at "$(mg__hires_now)" '. + {stopped_epoch: ($at | tonumber)}' "$state")"
  rm -f "$state"
  if [ ! -s "$video" ]; then
    mg_failure record "no video at $video"
    mg_status error
    return 0
  fi
  mg_artifact video "$video" "recording of $udid"
  mg_message "record: $video; make sheets with tools/mg contact-sheet $video --window <name>=<start>-<end>"
  mg_status pass
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
