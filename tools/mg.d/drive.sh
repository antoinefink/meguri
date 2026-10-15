# Agent drive loop (U8, R1961, R1964, R1965, R1968). Sourced by tools/mg.
#
# `drive` runs a drive script (tools/drive-scripts/AGENTS.md) through the
# XCUITest AgentDriverTests on a simulator from the pool or for the Mac, both
# in the macOS test VM (mac-vm.sh), exports the hierarchy and
# screenshot taken before and after every step from the result bundle into
# build/mg/<job>/<run>/steps/, runs hierarchy-check over every hierarchy, and
# writes manifest.json. `ui-tree` and `screenshot` drive a one-step script
# (launch, snapshot). Layout and identifier issues fail the run (R1964,
# R1965); unavailable steps (an iPhone resize) do not.

mg_job drive host mg_drive \
  "Run a drive script: <script.json> [--platform iphone|iphone-small|ipad|ipad-small|foldable|mac] [--device <leased udid>] [--config Debug|Profile] [--record] (per-step hierarchy JSON and PNGs, hierarchy-check, manifest; --record adds video and contact sheets of marked transitions)"
mg_job ui-tree host mg_ui_tree \
  "Launch on a route and export the UI hierarchy (and a screenshot): --route <route> [--platform ...] [--device <udid>] [--config ...] [-- launch args]"
mg_job screenshot host mg_screenshot \
  "Screenshot: --route <route> [--platform ...] [--device <udid>] [-- launch args] launches and captures (simulators: with the hierarchy; mac: hidden launch and screencapture -l, needs Screen Recording); --device <udid> alone captures that simulator's screen as it is"
mg_job hierarchy-check host mg_hierarchy_check \
  "Layout and identifier checks over hierarchy exports (tools/scripts/hierarchy-check), in the image-tools container: <hierarchy.json>..."

MG_DRIVE_KINDS="iphone iphone-small ipad ipad-small foldable mac"

# ---------------------------------------------------------------------------
# image-tools container

# mg_container_path <path>: the path inside a container that mounts the
# checkout at /work; fails for a path outside the checkout.
mg_container_path() {
  case "$1" in
    "$MG_ROOT"/*) printf '/work/%s\n' "${1#"$MG_ROOT"/}" ;;
    /*) return 1 ;;
    *) printf '%s\n' "$1" ;;
  esac
}

# mg_image_tools <command...>: runs a command in the image-tools container
# (built first; cached after the first run), labelled with the run id.
mg_image_tools() {
  if ! mg_compose --progress quiet build image-tools >&2; then
    echo "image-tools: building the image failed" >&2
    return 125
  fi
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" image-tools "$@" </dev/null
}

# ---------------------------------------------------------------------------
# Argument helpers

# mg_drive__usage <message>
mg_drive__usage() {
  mg_failure usage "$MG_JOB: $1" "tools/mg --list"
  mg_message "$MG_JOB: $1"
  mg_status error
}

# mg_drive__check_script <path>: a readable drive script with a non-empty
# steps list; the Swift side checks the rest (DriveScript.swift).
mg_drive__check_script() {
  [ -f "$1" ] || { echo "no drive script at $1"; return 1; }
  jq -e 'type == "object" and (.name | type) == "string" and (.steps | type) == "array" and (.steps | length) > 0' "$1" >/dev/null 2>&1 ||
    { echo "$1 is not a drive script (a JSON object with name and steps; tools/drive-scripts/AGENTS.md)"; return 1; }
}

# ---------------------------------------------------------------------------
# The drive runner

# mg_drive__export_attachments <xcresult> <dir>: the result bundle's
# attachments and xcresulttool's manifest.json of them.
mg_drive__export_attachments() {
  xcrun xcresulttool export attachments --path "$1" --output-path "$2"
}

# JXA, no prompt: whether this process may capture the screen.
mg__screen_capture_allowed() {
  [ "$(osascript -l JavaScript -e "ObjC.import('CoreGraphics'); ObjC.bindFunction('CGPreflightScreenCaptureAccess', ['bool', []]); \$.CGPreflightScreenCaptureAccess()" 2>/dev/null)" = true ]
}

# The main display's size in points: {"width": w, "height": h}.
mg__mac_display_points() {
  osascript -l JavaScript -e "ObjC.import('AppKit'); var f = \$.NSScreen.screens.objectAtIndex(0).frame;
    JSON.stringify({width: f.size.width, height: f.size.height})" 2>/dev/null
}

# mg_drive__screen_start <movie>: records the main display with screencapture
# -v (at most an hour), which stops at a character on its stdin: a FIFO this
# job keeps open on fd 7, read and write, so neither side blocks and a write
# after screencapture died raises no SIGPIPE. Sets MG_SCREEN_PID. The movie
# shows the whole screen: it is deleted when the job ends (the sheets keep
# only the app's window). Returns 1 when screencapture did not start.
mg_drive__screen_start() {
  local fifo="$1.stop"
  rm -f "$fifo" "$1"
  mkfifo "$fifo" || return 1
  exec 7<>"$fifo"
  screencapture -v -x -D1 -V3600 "$1" <"$fifo" >"$1.log" 2>&1 7>&- &
  MG_SCREEN_PID=$!
  mg_on_exit "kill -9 $MG_SCREEN_PID 2>/dev/null; rm -f '$1' '$fifo'"
  mg_sleep 1
  kill -0 "$MG_SCREEN_PID" 2>/dev/null
}

# mg_drive__screen_stop: a character on its stdin, then SIGINT after 10 s,
# then SIGKILL after 20 s. Returns 1 when screencapture had stopped already.
mg_drive__screen_stop() {
  local early=0 i
  kill -0 "$MG_SCREEN_PID" 2>/dev/null || early=1
  printf 'q\n' >&7 2>/dev/null
  exec 7>&-
  for i in $(seq 1 100); do
    kill -0 "$MG_SCREEN_PID" 2>/dev/null || break
    [ "$i" != 50 ] || kill -INT "$MG_SCREEN_PID" 2>/dev/null
    sleep 0.2
  done
  kill -9 "$MG_SCREEN_PID" 2>/dev/null
  wait "$MG_SCREEN_PID" 2>/dev/null
  return "$early"
}

# mg_drive__capture_path <out dir> <attachment name>: where an exported
# capture goes, without its drive- prefix: sampled frames to frames/, the
# step captures to steps/.
mg_drive__capture_path() {
  case "$2" in
    *-frame-[0-9]*.png) printf '%s/frames/%s\n' "$1" "${2#drive-}" ;;
    *) printf '%s/steps/%s\n' "$1" "${2#drive-}" ;;
  esac
}

# The contact-sheet window of a marked step: <index, 2 digits>-<transition>.
MG_DRIVE_WINDOW_NAME='def wname: "\(.index | tostring | if length < 2 then "0" + . else . end)-\(.transition)";'

# mg_drive__motion_inputs <capture> <out> <rec start>: writes the contact-sheet
# input for the steps marked with a transition: marks.json (video, screen),
# or frames.json (sampled frames). Returns 1 when it could not.
mg_drive__motion_inputs() {
  local capture="$1" out="$2" t0="$3" display w
  case "$capture" in
    video)
      jq --argjson t0 "$t0" "$MG_DRIVE_WINDOW_NAME"'{windows: [.steps[] | select(.transition != null)
        | {name: wname, start: ([.started - $t0 - 0.5, 0] | max), end: (.ended - $t0 + 1.5)}]}' \
        "$out/drive-log.json" >"$out/marks.json"
      ;;
    screen)
      # The recording may start up to a second after it was launched: a
      # wider lead keeps the action inside the window. Each window's crop is
      # the app's window in that step's first capture.
      display="$(mg__mac_display_points)"
      jq -e 'type == "object" and (.width | type) == "number"' <<<"${display:-null}" >/dev/null 2>&1 || return 1
      jq -c --argjson t0 "$t0" --arg steps "$out/steps" "$MG_DRIVE_WINDOW_NAME"'.steps[] | select(.transition != null)
        | {name: wname, start: ([.started - $t0 - 1.0, 0] | max), end: (.ended - $t0 + 1.5),
           hierarchy: ($steps + "/" + ((.captures | (map(select(.phase == "before")) + .))[0].hierarchy | ltrimstr("drive-")))}' \
        "$out/drive-log.json" >"$out/marks.tmp" || return 1
      : >"$out/marks.lines"
      while IFS= read -r w; do
        jq -ce --argjson w "$w" --argjson display "$display" \
          '$w + {crop: (.window + {display_width: $display.width, display_height: $display.height})} | del(.hierarchy)' \
          "$(jq -r .hierarchy <<<"$w")" >>"$out/marks.lines" || return 1
      done <"$out/marks.tmp"
      jq -s '{windows: .}' "$out/marks.lines" >"$out/marks.json" || return 1
      rm -f "$out/marks.tmp" "$out/marks.lines"
      ;;
    frames)
      jq --arg dir "$(mg_container_path "$out/frames")" "$MG_DRIVE_WINDOW_NAME"'{windows: [.steps[]
        | select(.transition != null and ((.frames // []) | length) > 0)
        | {name: wname, frames: [.frames[] | {path: ($dir + "/" + (.file | ltrimstr("drive-"))), time}]}]}' \
        "$out/drive-log.json" >"$out/frames.json"
      ;;
  esac
}

# mg_drive_run <script> <mac|ios> <udid|""> <config> <record yes|no> <out dir>:
# runs the script (the app and UI tests are built; what mg_sim_take or, for
# the Mac, mg_mac_ui_take takes is held), exports its captures, checks them
# and writes <out>/manifest.json. A drive in the test VM first copies the
# script and the fixtures its launch steps name to the same paths there.
# With record, it also captures motion and makes a contact sheet for every
# step marked with a transition:
#   simulators: simctl io recordVideo around the drive (in the test VM, the
#     video comes back into <out>);
#   the Mac in the test VM: the driver's FrameSampler screenshots (about 10
#     per second); the host cannot record the VM's screen;
#   the Mac with Screen Recording (MEGURI_MAC_UI_ALLOW_HOST=1 runs):
#     screencapture -v of the main display, cut to the app's window in each
#     marked step (the full movie is deleted);
#   the Mac without it: FrameSampler screenshots, with the screen-recording
#     user step.
# Records artifacts and failures. Returns 0 pass, 1 fail (failed steps or
# hierarchy issues), 2 error (the drive, a capture the log lists, the
# hierarchy check or a requested sheet could not be made, or macOS dropped
# the input the drive synthesized). A pass needs every listed capture
# exported and non-empty, and the hierarchy check run over them.
mg_drive_run() {
  local script="$1" platform="$2" udid="$3" config="$4" record="$5" out="$6"
  local dest name abs_script rc capture="" video="" rec_start="" rec_log recorder="" capture_error=0
  mkdir -p "$out"
  abs_script="$(cd "$(dirname "$script")" && pwd -P)/$(basename "$script")"
  name="$(basename "$out")"
  [ "$out" != "$MG_RUN_DIR" ] || name="$MG_JOB"
  case "$platform" in
    mac) dest="platform=macOS,arch=arm64" ;;
    *) dest="id=$udid" ;;
  esac
  if [ "$record" = yes ]; then
    if [ "$platform" != mac ]; then
      capture=video
    elif [ "$MG_MAC_RUNNER" = vm ]; then
      capture=frames
    elif mg__screen_capture_allowed; then
      capture=screen
    else
      capture=frames
      mg_user_step screen-recording missing false "Allow Screen Recording for Mac motion capture" \
        "System Settings > Privacy & Security > Screen Recording: allow the app that runs tools/mg. Until then, tools/mg drive --platform mac --record makes its contact sheets from XCUITest screenshots at about 10 per second instead of a screen recording."
    fi
  fi
  case "$capture" in
    video)
      video="$out/video.mp4"
      rec_log="$out/record.log"
      mg_record__start_recorder "$udid" "$video" "$rec_log"
      recorder="$MG_RECORDER_PID"
      mg_on_exit "kill -9 $recorder 2>/dev/null; mg_record__kill_remote '$video'"
      if ! mg_record__wait_started "$rec_log" "$recorder"; then
        mg_failure record "simctl io recordVideo did not start; see $rec_log"
        return 2
      fi
      rec_start="$MG_RECORD_STARTED"
      ;;
    screen)
      video="$out/screen.mov"
      rec_log="$video.log"
      if ! mg_drive__screen_start "$video"; then
        mg_failure record "screencapture -v did not start; see $rec_log"
        return 2
      fi
      rec_start="$(mg__hires_now)"
      ;;
  esac

  if { [ "$platform" = mac ] && [ "$MG_MAC_RUNNER" = vm ]; } || { [ "$platform" != mac ] && [ "$MG_SIM_RUNNER" = vm ]; }; then
    local inputs=("$abs_script") fixture
    while IFS= read -r fixture; do
      case "$fixture" in /*) inputs[${#inputs[@]}]="$fixture" ;; *) inputs[${#inputs[@]}]="$MG_ROOT/$fixture" ;; esac
    done < <(jq -r '.steps[] | objects | .launch? | objects | .fixture? | strings' "$abs_script")
    mg_mac_vm_copy_in "${inputs[@]}" || return 2
  fi

  echo "drive: running $script on $([ "$platform" = mac ] && echo "the Mac${MG_MAC_RUNNER:+ ($MG_MAC_RUNNER)}" || echo "$udid${MG_SIM_RUNNER:+ ($MG_SIM_RUNNER)}") ($config)${capture:+, capturing motion: $capture}" >&2
  local since refused=0
  since="$(mg_app_log_now)"
  TEST_RUNNER_MEGURI_DRIVE_SCRIPT="$abs_script" TEST_RUNNER_MEGURI_DRIVE_FRAMES="$([ "$capture" = frames ] && echo 1 || echo 0)" \
    mg_app_xcodebuild_test "$out/drive" "$platform" "$dest" "$config" -only-testing:MeguriUITests/AgentDriverTests/testDriveScript
  rc=$?
  # A Mac drive whose input macOS dropped ran, but proves nothing about the
  # app: its captures are kept, and it ends blocked (error), not fail.
  if [ "$platform" = mac ] && mg_app_mac_input_refused "$since"; then
    refused=1
  fi
  case "$capture" in
    video)
      if ! kill -0 "$recorder" 2>/dev/null; then
        mg_failure record "simctl io recordVideo stopped before the drive ended; see $rec_log"
        capture_error=1
      fi
      mg_record__stop_recorder "$recorder" "$video"
      ;;
    screen)
      if ! mg_drive__screen_stop; then
        mg_failure record "screencapture -v stopped before the drive ended; see $rec_log"
        capture_error=1
      fi
      ;;
  esac
  if [ -n "$video" ] && [ ! -s "$video" ]; then
    mg_failure record "the recording is empty; see $rec_log"
    capture_error=1
  fi
  [ "$rc" -ne 2 ] || return 2

  # Attachments: XCTest names each export <name>_<n>_<UUID>.<ext>.
  local raw="$out/attachments" log
  rm -rf "$raw"
  if ! mg_drive__export_attachments "$out/drive.xcresult" "$raw" >/dev/null 2>"$out/export.log"; then
    mg_failure export "xcresulttool could not export the attachments; see $out/export.log"
    return 2
  fi
  local map
  map="$(jq -c '[.[].attachments[] | {(.suggestedHumanReadableName | sub("_[0-9]+_[0-9A-Fa-f-]{36}(?<e>\\.[A-Za-z0-9]+)$"; "\(.e)")): .exportedFileName}] | add // {}' "$raw/manifest.json")"
  log="$(jq -r '.["drive-log.json"] // empty' <<<"$map")"
  if [ -z "$log" ]; then
    mg_failure drive-log "the drive attached no drive-log.json (the test did not run the script); see $out/drive.log"
    return 2
  fi
  mkdir -p "$out/steps"
  local f src to incomplete=0
  for f in $(jq -r 'keys[] | select(startswith("drive-") and . != "drive-log.json")' <<<"$map"); do
    src="$(jq -r --arg f "$f" '.[$f]' <<<"$map")"
    to="$(mg_drive__capture_path "$out" "$f")"
    mkdir -p "$(dirname "$to")"
    if ! mv -f "$raw/$src" "$to" 2>>"$out/export.log"; then
      mg_failure "export/${f#drive-}" "could not move the exported attachment $f ($raw/$src); see $out/export.log"
      incomplete=1
    fi
  done
  if ! mv -f "$raw/$log" "$out/drive-log.json" 2>>"$out/export.log"; then
    mg_failure export "could not move the exported drive-log.json ($raw/$log); see $out/export.log"
    return 2
  fi
  # A failed move leaves the export behind for a look.
  [ "$incomplete" -ne 0 ] || rm -rf "$raw"

  # Every capture the drive log lists (hierarchies, screenshots, sampled
  # frames) must have come out of the result bundle, and not empty: a drive
  # with a capture missing is an error, never a pass (R1965, R1968).
  local cap path
  while IFS= read -r cap; do
    path="$(mg_drive__capture_path "$out" "$cap")"
    [ ! -s "$path" ] || continue
    mg_failure "capture/${cap#drive-}" "the drive log lists $cap, but $([ -e "$path" ] && echo "its export is empty" || echo "the result bundle had no such attachment"); see $out/drive.log" \
      "tools/mg drive $script"
    incomplete=1
  done < <(jq -r '.steps[] | (.captures[] | .hierarchy, .screenshot), (.frames[]?.file) | select(type == "string" and . != "")' "$out/drive-log.json")

  # Hierarchy check over every listed hierarchy, in the image-tools container.
  # A pass needs it: a drive with no hierarchy checked proves nothing.
  local files=() check_rc=0 checked=false
  while IFS= read -r cap; do
    f="$(mg_drive__capture_path "$out" "$cap")"
    [ -s "$f" ] && files[${#files[@]}]="$(mg_container_path "$f")"
  done < <(jq -r '.steps[].captures[].hierarchy | select(type == "string" and . != "")' "$out/drive-log.json")
  if [ "${#files[@]}" -gt 0 ]; then
    mg_image_tools python3 tools/scripts/hierarchy-check/hierarchy_check.py \
      --report "$(mg_container_path "$out/hierarchy-check.json")" "${files[@]}" >&2
    check_rc=$?
    if [ "$check_rc" -le 1 ] && [ -f "$out/hierarchy-check.json" ]; then
      checked=true
    else
      mg_failure hierarchy-check "hierarchy-check could not run (exit $check_rc); see log.txt"
    fi
  fi

  # Motion: a contact sheet for every marked step. Each one asked for must
  # exist; a missing recording, frames or sheet is an error, never a pass.
  # Sampled frames cover only actions on the running app: a marked launch,
  # resize or snapshot step is listed as unsampled instead.
  local expected="" unsampled="" short sheet_args=()
  if [ -n "$capture" ]; then
    expected="$(jq -r --arg c "$capture" "$MG_DRIVE_WINDOW_NAME"'
      .steps[] | select(.transition != null)
      | select($c != "frames" or ([.kind] | inside(["launch", "resize", "snapshot"]) | not)) | wname' "$out/drive-log.json")"
    [ "$capture" != frames ] || unsampled="$(jq -r "$MG_DRIVE_WINDOW_NAME"'
      [.steps[] | select(.transition != null and ([.kind] | inside(["launch", "resize", "snapshot"]))) | wname] | join(" ")' "$out/drive-log.json")"
  fi
  if [ -n "$expected" ] && [ "$capture_error" -eq 0 ]; then
    if [ "$capture" = frames ]; then
      short="$(jq -r "$MG_DRIVE_WINDOW_NAME"'
        .steps[] | select(.transition != null and ([.kind] | inside(["launch", "resize", "snapshot"]) | not))
        | select((.frames // []) | length < 2) | wname' "$out/drive-log.json")"
      for f in $short; do
        mg_failure "frames/$f" "the driver sampled fewer than 2 frames for the marked step $f; see $out/drive.log"
        capture_error=1
      done
      sheet_args=(--frame-list "$(mg_container_path "$out/frames.json")")
    else
      sheet_args=("$(mg_container_path "$video")" --marks "$(mg_container_path "$out/marks.json")")
    fi
    if [ "$capture_error" -eq 0 ]; then
      rm -rf "$out/motion"
      mkdir -p "$out/motion"
      if ! mg_drive__motion_inputs "$capture" "$out" "${rec_start:-0}"; then
        mg_failure motion-input "could not list the marked steps for the contact sheets; see log.txt"
        capture_error=1
      elif ! mg_image_tools python3 tools/scripts/contact-sheet/contact_sheet.py "${sheet_args[@]}" \
        --out "$(mg_container_path "$out/motion")" --fit >&2; then
        mg_failure contact-sheet "contact-sheet failed; see log.txt"
        capture_error=1
      fi
      for f in $expected; do
        if [ ! -s "$out/motion/$f.png" ] || ! jq -e --arg n "$f" 'any(.windows[]; .name == $n)' "$out/motion/report.json" >/dev/null 2>&1; then
          mg_failure "contact-sheet/$f" "no contact sheet for the marked step $f"
          capture_error=1
        fi
      done
    fi
  fi
  # The full-screen movie shows more than the app: only its cuts are kept.
  if [ "$capture" = screen ]; then
    rm -f "$video"
    video=""
  fi

  # Manifest: the drive log with absolute paths and each capture's issues.
  local issues='{}'
  [ "$checked" = false ] || issues="$(jq -c --arg root "/work/" --arg host "$MG_ROOT/" \
    '[.files[] | {key: (.path | if startswith($root) then $host + ltrimstr($root) else . end), value: .issues}] | from_entries' \
    "$out/hierarchy-check.json")"
  local motion=null
  [ ! -f "$out/motion/report.json" ] || motion="$(jq -c --arg root "/work/" --arg host "$MG_ROOT/" \
    'walk(if type == "string" and startswith($root) then $host + ltrimstr($root) else . end)' "$out/motion/report.json")"
  jq --arg dir "$out/steps" --argjson issues "$issues" --arg platform "$platform" --arg udid "$udid" --arg config "$config" \
    --arg video "$video" --argjson motion "$motion" --arg record "$record" --arg capture "$capture" --argjson checked "$checked" \
    --arg frames "$out/frames" --arg unsampled "$unsampled" '
    def file($n): $dir + "/" + ($n | ltrimstr("drive-"));
    {
      script: .script, script_path: .path, platform: $platform, device: (if $udid == "" then null else $udid end),
      config: $config, error: .error,
      steps: [.steps[] | . + {captures: [.captures[] | {phase, hierarchy: file(.hierarchy), screenshot: file(.screenshot)}
        | . + {issues: ($issues[.hierarchy] // [])}]}
        | if .frames == null then . else .frames |= map(.file |= ($frames + "/" + ltrimstr("drive-"))) end],
      hierarchy_checked: $checked,
      issues: ([$issues[] | length] | add // 0),
      record: $record, capture: (if $capture == "" then null else $capture end),
      video: (if $video == "" then null else $video end), motion: $motion,
      unsampled_transitions: ($unsampled | split(" ") | map(select(. != "")))
    }' "$out/drive-log.json" >"$out/manifest.json"
  mg_artifact manifest "$out/manifest.json" "drive manifest"
  local png
  for png in "$out"/steps/*.png; do
    [ -f "$png" ] && mg_artifact png "$png" "$name: $(basename "$png" .png)"
  done
  for f in "$out"/steps/*.json; do
    [ -f "$f" ] && mg_artifact hierarchy "$f" "$name: $(basename "$f" .json)"
  done
  for png in "$out"/motion/*.png; do
    [ -f "$png" ] && mg_artifact contact-sheet "$png" "$name: $(basename "$png" .png)"
  done
  [ -z "$video" ] || [ ! -s "$video" ] || mg_artifact video "$video" "$name: screen recording"

  # Failures: failed steps, then each distinct issue once.
  jq -c '.steps[] | select(.status == "failed") | {id: "step-\(.index)-\(.kind)", message: (.message // "failed")}' \
    "$out/manifest.json" | while IFS= read -r line; do
    mg_failure "$name/$(jq -r .id <<<"$line")" "$(jq -r .message <<<"$line")" "tools/mg drive $script"
  done
  [ -z "$(jq -r '.error // empty' "$out/manifest.json")" ] ||
    mg_failure "$name/script" "$(jq -r .error "$out/manifest.json")" "tools/mg drive $script"
  jq -c '[.steps[].captures[].issues[]] | unique_by([.kind, .identifier // (.identifiers | tostring), .label // (.labels | tostring)])[]' \
    "$out/manifest.json" | while IFS= read -r line; do
    mg_failure "$name/$(jq -r '.kind + "/" + (if (.identifier // "") != "" then .identifier else (.label // (.labels | join("+"))) end)' <<<"$line")" \
      "$(jq -r '.message + " at " + ((.frame // .frames[0]) | "\(.x),\(.y) \(.width)x\(.height)")' <<<"$line")" "tools/mg drive $script"
  done

  [ "$refused" -eq 0 ] || return 2
  # The driver reports failed steps in its log, not as test failures.
  if [ "$rc" -ne 0 ] || [ "$(jq '.issues' "$out/manifest.json")" -gt 0 ] ||
    jq -e '.error != null or any(.steps[]; .status == "failed")' "$out/manifest.json" >/dev/null; then
    return 1
  fi
  if [ "$capture_error" -ne 0 ] || [ "$incomplete" -ne 0 ] || [ "$checked" = false ]; then
    return 2
  fi
  return 0
}

# ---------------------------------------------------------------------------
# Jobs

# mg_drive__job <script> <kind> <udid> <config> <record>: builds, takes the
# test VM (mac-vm.sh: the pool VM's lease and a pool simulator, or for the Mac
# a VM's lease) and runs the script.
mg_drive__job() {
  local script="$1" kind="$2" udid="$3" config="$4" record="$5" platform
  if [ "$kind" = mac ]; then
    platform=mac
    mg_mac_ui_begin || return 0
    mg_app_build mac "$config" || return 0
    mg_mac_ui_take || return 0
  else
    platform=ios
    mg_sim_begin || return 0
    if [ -n "$udid" ]; then
      mg_sim_check_given "$udid" || return 0
    fi
    mg_app_build ios-sim "$config" || return 0
    mg_sim_take || return 0
    if [ -n "$udid" ]; then
      mg_sim_check_given "$udid" || return 0
    else
      mg_sim_lease "$kind" release || return 0
      udid="$MG_SIM_UDID"
      mg_sim_check_runtime "$udid" || return 0
    fi
    mg_sim_prepare "$udid" || return 0
  fi
  mg_drive_run "$script" "$platform" "$udid" "$config" "$record" "$MG_RUN_DIR"
  local rc=$? manifest="$MG_RUN_DIR/manifest.json"
  if [ -f "$manifest" ]; then
    mg_detail drive "$(jq -c '{script, platform, device, config, issues, hierarchy_checked, record, capture, video, unsampled_transitions, steps: [.steps[] | {index, kind, status, transition, message} + (if .frames == null then {} else {frames: (.frames | length)} end)], motion: (if .motion == null then null else {jump_cuts: .motion.jump_cuts, windows: [.motion.windows[] | {name, sheet, jump_cuts, stalls}]} end)}' "$manifest")"
    mg_count total "$(jq '.steps | length' "$manifest")"
    mg_count passed "$(jq '[.steps[] | select(.status == "ok")] | length' "$manifest")"
    mg_count failed "$(jq '[.steps[] | select(.status == "failed")] | length' "$manifest")"
    mg_count skipped "$(jq '[.steps[] | select(.status == "unavailable")] | length' "$manifest")"
    mg_count issues "$(jq '.issues' "$manifest")"
  fi
  case "$rc" in
    0)
      mg_message "$MG_JOB: $(jq -r '"\(.script): \(.steps | length) steps on \(.platform), no issue; manifest: "' "$manifest")$manifest"
      mg_status pass
      ;;
    1)
      mg_message "$MG_JOB: $(jq -r '"\(.script): \([.steps[] | select(.status == "failed")] | length) failed step(s), \(.issues) issue(s); manifest: "' "$manifest" 2>/dev/null)$manifest"
      mg_status fail
      ;;
    *)
      if [ -n "${MG_MAC_INPUT_REFUSED:-}" ]; then
        mg_message "$MG_JOB: blocked: macOS dropped the input the drive synthesized (user step mac-input); its steps did not test the app"
      elif [ -f "$manifest" ]; then
        mg_message "$MG_JOB: the drive ran, but a capture or check it asked for could not be made; see the failures and log.txt"
      else
        mg_message "$MG_JOB: the drive could not run; see log.txt"
      fi
      mg_status error
      ;;
  esac
}

# mg_drive__args: parses the options shared by drive, ui-tree and
# screenshot into MG_D_KIND, MG_D_UDID, MG_D_CONFIG, MG_D_RECORD, MG_D_ROUTE,
# MG_D_REST (launch arguments after --) and MG_D_POSITIONAL. Returns 1 after
# recording a usage error.
mg_drive__args() {
  MG_D_KIND=iphone MG_D_UDID="" MG_D_CONFIG=Debug MG_D_RECORD=no MG_D_ROUTE="" MG_D_REST=() MG_D_POSITIONAL=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "$MG_DRIVE_KINDS" || { mg_drive__usage "--platform takes one of: $MG_DRIVE_KINDS"; return 1; }
        MG_D_KIND="$2" && shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_drive__usage "--device needs a UDID (tools/mg sim acquire <kind>)"; return 1; }
        MG_D_UDID="$2" && shift 2
        ;;
      --config)
        [ "$#" -ge 2 ] && mg_word_in "$2" "Debug Profile" || { mg_drive__usage "--config takes Debug or Profile (the builds with test hooks)"; return 1; }
        MG_D_CONFIG="$2" && shift 2
        ;;
      --route)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_drive__usage "--route needs a route, e.g. sample/sheet"; return 1; }
        MG_D_ROUTE="$2" && shift 2
        ;;
      --record) MG_D_RECORD=yes && shift ;;
      --) shift && MG_D_REST=("$@") && break ;;
      -*) mg_drive__usage "unknown option '$1'"; return 1 ;;
      *) MG_D_POSITIONAL[${#MG_D_POSITIONAL[@]}]="$1" && shift ;;
    esac
  done
  if [ "$MG_D_KIND" = mac ] && [ -n "$MG_D_UDID" ]; then
    mg_drive__usage "--device is for simulators"
    return 1
  fi
  return 0
}

mg_drive() {
  mg_drive__args "$@" || return 0
  [ "${#MG_D_POSITIONAL[@]}" -eq 1 ] || { mg_drive__usage "usage: tools/mg drive <script.json> [options]"; return 0; }
  [ -z "$MG_D_ROUTE" ] && [ "${#MG_D_REST[@]}" -eq 0 ] || { mg_drive__usage "the script sets the launch options; --route and -- are for ui-tree and screenshot"; return 0; }
  local problem
  if ! problem="$(mg_drive__check_script "${MG_D_POSITIONAL[0]}")"; then
    mg_drive__usage "$problem"
    return 0
  fi
  mg_drive__job "${MG_D_POSITIONAL[0]}" "$MG_D_KIND" "$MG_D_UDID" "$MG_D_CONFIG" "$MG_D_RECORD"
}

# mg_drive__generated <route>: a launch-and-snapshot script for the route and
# the raw launch arguments in MG_D_REST.
mg_drive__generated() {
  local file="$MG_RUN_DIR/script.json"
  jq -n --arg route "$1" --arg job "$MG_JOB" --argjson args "$(mg_json_array ${MG_D_REST[0]+"${MG_D_REST[@]}"})" '{
    name: "\($job)-\($route | gsub("/"; "-"))",
    steps: [{launch: {route: $route, animations: false, arguments: $args}}, {snapshot: ($route | gsub("/"; "-"))}]
  }' >"$file"
  printf '%s\n' "$file"
}

mg_ui_tree() {
  mg_drive__args "$@" || return 0
  [ -n "$MG_D_ROUTE" ] && [ "${#MG_D_POSITIONAL[@]}" -eq 0 ] || { mg_drive__usage "usage: tools/mg ui-tree --route <route> [--platform ...] [-- launch args]"; return 0; }
  mg_drive__job "$(mg_drive__generated "$MG_D_ROUTE")" "$MG_D_KIND" "$MG_D_UDID" "$MG_D_CONFIG" no
}

mg_screenshot() {
  mg_drive__args "$@" || return 0
  [ "${#MG_D_POSITIONAL[@]}" -eq 0 ] || { mg_drive__usage "usage: tools/mg screenshot --route <route> [--platform ...] [-- launch args] | --device <udid>"; return 0; }
  if [ -z "$MG_D_ROUTE" ]; then
    [ -n "$MG_D_UDID" ] && [ "${#MG_D_REST[@]}" -eq 0 ] || { mg_drive__usage "give --route (launch and capture) or --device alone (capture the screen as it is)"; return 0; }
    mg_screenshot__device "$MG_D_UDID"
  elif [ "$MG_D_KIND" = mac ]; then
    mg_screenshot__mac "$MG_D_ROUTE" "$MG_D_CONFIG"
  else
    mg_drive__job "$(mg_drive__generated "$MG_D_ROUTE")" "$MG_D_KIND" "$MG_D_UDID" "$MG_D_CONFIG" no
  fi
}

# The screen of a simulator the caller leased, as it is (in the test VM, the
# capture comes back into the run directory).
mg_screenshot__device() {
  local udid="$1" png="$MG_RUN_DIR/screen.png"
  mg_sim_begin || return 0
  mg_sim_check_given "$udid" || return 0
  mg_sim_take --no-copy || return 0
  mg_sim_check_given "$udid" || return 0
  mg_sim_prepare "$udid" || return 0
  if ! mg_sim_mkdir "$MG_RUN_DIR" || ! mg_simctl io "$udid" screenshot "$png" >&2 || ! mg_sim_fetch "$png" || [ ! -s "$png" ]; then
    mg_failure screenshot "simctl io screenshot failed"
    mg_status error
    return 0
  fi
  mg_artifact png "$png" "screen of $udid"
  mg_message "screenshot: $png"
  mg_status pass
}

# mg__mac_window_id <pid>: the id of the process's largest normal window.
mg__mac_window_id() {
  osascript -l JavaScript -e "ObjC.import('CoreGraphics'); ObjC.import('Foundation');
    var l = ObjC.deepUnwrap(ObjC.castRefToObject(\$.CGWindowListCopyWindowInfo(\$.kCGWindowListOptionAll, 0)));
    var w = l.filter(function (x) { return x.kCGWindowOwnerPID == $1 && x.kCGWindowLayer == 0 && x.kCGWindowBounds.Width > 100; })
      .sort(function (a, b) { return b.kCGWindowBounds.Width * b.kCGWindowBounds.Height - a.kCGWindowBounds.Width * a.kCGWindowBounds.Height; });
    w.length ? String(w[0].kCGWindowNumber) : ''" 2>/dev/null
}

# The Mac: a hidden background launch on the route, then screencapture -l of
# its window (root AGENTS.md); never takes over the screen.
mg_screenshot__mac() {
  local route="$1" config="$2" app pid="" wid="" png="$MG_RUN_DIR/mac-$(printf '%s' "$1" | tr '/' '-').png" i
  mg_app_build mac "$config" || return 0
  mg_host_lease || return 0
  mg_app_mac_guard || return 0
  if ! mg__screen_capture_allowed; then
    mg_user_step screen-recording missing true "Allow Screen Recording for Mac screenshots" \
      "System Settings > Privacy & Security > Screen Recording: allow the app that runs tools/mg (the terminal or Claude). Until then, Mac screenshots come from tools/mg drive --platform mac (Mac UI gate)."
    mg_failure blocked-screen-recording "blocked: Screen Recording is not allowed, so screencapture would prompt; the job did not launch the app"
    mg_detail blocked '"screen-recording"'
    mg_message "screenshot: blocked: Screen Recording is not allowed; use tools/mg drive --platform mac, or grant it (user step screen-recording)"
    mg_status error
    return 0
  fi
  app="$(mg_app_products mac "$config")/Meguri.app"
  open -g -j -n "$app" --args -ApplePersistenceIgnoreState YES -MeguriAnimations off -MeguriRoute "$route" ${MG_D_REST[0]+"${MG_D_REST[@]}"} >&2
  mg_on_exit "mg_app_quit_mac '$app'"
  for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    pid="$(mg_app_mac_pids "$app" | head -n 1)"
    [ -z "$pid" ] || wid="$(mg__mac_window_id "$pid")"
    [ -z "$wid" ] || break
    mg_sleep 1
  done
  if [ -z "$wid" ]; then
    mg_failure mac-window "the app showed no window within 15 s of a hidden launch (pid ${pid:-none})"
    mg_status fail
    return 0
  fi
  mg_sleep 1
  if ! screencapture -x -o -l "$wid" "$png" || [ ! -s "$png" ]; then
    mg_failure screencapture "screencapture -l $wid failed"
    mg_status error
    return 0
  fi
  mg_artifact png "$png" "mac $route"
  mg_message "screenshot: $png"
  mg_status pass
}

mg_hierarchy_check() {
  [ "$#" -gt 0 ] || { mg_drive__usage "usage: tools/mg hierarchy-check <hierarchy.json>..."; return 0; }
  local files=() f p rc
  for f in "$@"; do
    p="$(mg_container_path "$(cd "$(dirname "$f")" 2>/dev/null && pwd -P)/$(basename "$f")")" ||
      { mg_drive__usage "$f is outside the checkout; the container sees only the checkout"; return 0; }
    [ -f "$f" ] || { mg_drive__usage "no file $f"; return 0; }
    files[${#files[@]}]="$p"
  done
  mg_image_tools python3 tools/scripts/hierarchy-check/hierarchy_check.py \
    --report "$(mg_container_path "$MG_RUN_DIR/report.json")" "${files[@]}" >&2
  rc=$?
  if [ "$rc" -gt 1 ] || [ ! -f "$MG_RUN_DIR/report.json" ]; then
    mg_failure hierarchy-check "hierarchy-check could not read its input (exit $rc); see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$MG_RUN_DIR/report.json" "hierarchy-check report"
  mg_detail counts "$(jq -c .counts "$MG_RUN_DIR/report.json")"
  jq -c '.files[] | .path as $p | .issues[] | {p: $p, i: .}' "$MG_RUN_DIR/report.json" | while IFS= read -r line; do
    mg_failure "$(jq -r '.i.kind' <<<"$line")" "$(jq -r '"\(.p): \(.i.message)"' <<<"$line")"
  done
  mg_count total "$(jq '.counts.files' "$MG_RUN_DIR/report.json")"
  mg_count issues "$(jq '.counts.issues' "$MG_RUN_DIR/report.json")"
  if [ "$rc" -eq 0 ]; then
    mg_message "hierarchy-check: no issue in $# file(s)"
    mg_status pass
  else
    mg_message "hierarchy-check: $(jq '.counts.issues' "$MG_RUN_DIR/report.json") issue(s)"
    mg_status fail
  fi
}
