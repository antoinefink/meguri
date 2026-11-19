#!/bin/bash
# A stand-in for `xcrun simctl` and devicectl's VoiceOver disable command,
# installed as `xcrun` on PATH by the sim-pool
# self-tests (sim_pool.bats, mg_host_selftest.sh). It never calls the real
# simctl. Devices live in $FAKE_SIM_DB (JSON), device directories under
# $MEGURI_SIM_DEVICES_DIR, and every call except the test-only `__` commands
# is appended to $STUB_LOG as `xcrun <args>`. Runs under macOS /bin/bash 3.2.
#
# Test-only commands:
#   xcrun simctl __add <name> <state> [type id] [runtime id]   prints the UDID
#   xcrun simctl __app <udid> <bundle id> <User|System>
#   xcrun simctl __state <udid>
#   xcrun simctl __apps <udid>                                  bundle ids, one per line
#   xcrun simctl __voiceover <udid> [enabled|disabled]           set or read state
#   xcrun simctl __voiceover_disable_fail <udid> <true|false>
#
# FAKE_SIM_RT27_BUILD sets the pinned iOS 27.2 runtime's build (default 24B8117);
# FAKE_SIM_FOLDABLE=1 adds a foldable iPhone device type. `io <udid>
# recordVideo ... <file>` writes the file and runs until SIGINT (with
# FAKE_SIM_RECORD_EMPTY=1 the file is empty; FAKE_SIM_RECORD_EXIT_AFTER=<s>
# makes it fail after that long); `io <udid> screenshot <file>` writes a
# stand-in PNG. FAKE_SIM_STATUS_BAR_FAIL=1 makes `status_bar <udid> override`
# fail. `spawn <udid> launchctl kickstart|list` stands in for SpringBoard's
# restart, `spawn <udid> log show` for its style arbiter's log: with
# FAKE_SIM_SB=stale new devices' SpringBoard ignores appearance changes until
# FAKE_SIM_SB_RESTARTS (1) restarts, or never. FAKE_SIM_NO_RT27=1 leaves the pinned runtime out
# until `runtime add <image>` adds it.

set -u
tool="${1:-}"
case "$tool" in simctl | devicectl) ;; *) echo "fake xcrun: unexpected tool: $tool" >&2; exit 97 ;; esac
shift
case "$tool ${1:-}" in "simctl __"*) ;; *) printf 'xcrun %s %s\n' "$tool" "$*" >>"$STUB_LOG" ;; esac

DB="$FAKE_SIM_DB"
RT_NEW=com.apple.CoreSimulator.SimRuntime.iOS-27-2
IPHONE=com.apple.CoreSimulator.SimDeviceType.iPhone-18-Pro

# Calls run in parallel (parallel acquires boot different devices), so every
# read-modify-write of the database holds a mkdir lock.
lock() {
  until mkdir "$DB.lock" 2>/dev/null; do sleep 0.05; done
  trap 'rmdir "$DB.lock" 2>/dev/null' EXIT
  [ -f "$DB" ] || echo '{"next": 1, "devices": []}' >"$DB"
}
upd() { jq "$@" "$DB" >"$DB.tmp.$$" && mv -f "$DB.tmp.$$" "$DB"; }
has() { jq -e --arg u "$1" 'any(.devices[]; .udid == $u)' "$DB" >/dev/null; }
state() { jq -r --arg u "$1" '.devices[] | select(.udid == $u) | .state' "$DB"; }
set_state() { upd --arg u "$1" --arg s "$2" '(.devices[] | select(.udid == $u) | .state) = $s'; }
need() { has "$1" || { echo "Invalid device: $1" >&2; exit 148; }; }
need_booted() {
  need "$1"
  [ "$(state "$1")" = Booted ] || { echo "Unable to $2: device is not booted" >&2; exit 149; }
}

add() { # add <name> <state> <type> <runtime>: prints the UDID
  local n udid
  n="$(jq -r .next "$DB")"
  udid="$(printf '00000000-0000-4000-8000-%012d' "$n")"
  upd --arg u "$udid" --arg n "$1" --arg s "$2" --arg t "$3" --arg r "$4" --arg sb "${FAKE_SIM_SB:-ok}" '.next += 1 |
    .devices += [{udid: $u, name: $n, state: $s, type: $t, runtime: $r, appearance: "light", springboard: $sb, restarts: 0, voiceover: false,
      apps: [{id: "com.apple.Preferences", type: "System"}, {id: "com.apple.mobilesafari", type: "System"}]}]'
  mkdir -p "$MEGURI_SIM_DEVICES_DIR/$udid/data"
  printf '%s\n' "$udid"
}

lock
if [ "$tool" = devicectl ]; then
  [ "$#" -eq 6 ] && [ "$1 $2 $3 $4 $6" = "device settings voiceover --device --disable" ] || {
    echo "fake devicectl: unexpected call: $*" >&2; exit 97;
  }
  need_booted "$5" "disable VoiceOver"
  if jq -e --arg u "$5" '.devices[] | select(.udid == $u) | .voiceover_disable_fail == true' "$DB" >/dev/null; then
    echo "fake devicectl: VoiceOver disable failed for $5" >&2
    exit 1
  fi
  upd --arg u "$5" '(.devices[] | select(.udid == $u) | .voiceover) = false'
  exit $?
fi
case "${1:-} ${2:-}" in
  "list runtimes")
    rt27='{"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-27-2", "version": "27.2", "buildversion": "'"${FAKE_SIM_RT27_BUILD:-24B8117}"'", "isAvailable": true, "platform": "iOS"},'
    [ "${FAKE_SIM_NO_RT27:-}" != 1 ] || [ -e "$DB.runtime-added" ] || rt27=""
    cat <<EOF
{"runtimes": [
  {"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-18-2", "version": "18.2", "buildversion": "22C150", "isAvailable": true, "platform": "iOS"},
  $rt27
  {"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-27-2-iPhone19-4", "version": "27.2", "buildversion": "${FAKE_SIM_RT271_BUILD:-24B5}", "isAvailable": ${FAKE_SIM_RT271:-false}, "platform": "iOS"},
  {"identifier": "com.apple.CoreSimulator.SimRuntime.watchOS-28-0", "version": "28.0", "buildversion": "23R1", "isAvailable": true, "platform": "watchOS"}
]}
EOF
    ;;
  "list devicetypes")
    {
      cat <<'EOF'
{"devicetypes": [
  {"name": "iPhone 18 Pro", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-18-Pro", "productFamily": "iPhone"},
  {"name": "iPhone 17 Pro", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro", "productFamily": "iPhone"},
  {"name": "iPhone 17 Pro Max", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro-Max", "productFamily": "iPhone"},
  {"name": "iPhone 17e", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-17e", "productFamily": "iPhone"},
  {"name": "iPad Pro 13-inch (M5) (16GB)", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-16GB", "productFamily": "iPad"},
  {"name": "iPad Pro 13-inch (M5)", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-12GB", "productFamily": "iPad"},
  {"name": "iPad mini (A20 Pro)", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPad-mini-A20-Pro", "productFamily": "iPad"},
  {"name": "iPad mini (A17 Pro)", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPad-mini-A17-Pro", "productFamily": "iPad"},
  {"name": "Apple Watch Ultra 3 (49mm)", "identifier": "com.apple.CoreSimulator.SimDeviceType.Apple-Watch-Ultra-3-49mm", "productFamily": "Apple Watch"}
EOF
      [ "${FAKE_SIM_FOLDABLE:-}" != 1 ] ||
        echo ', {"name": "iPhone Duo", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-Duo", "productFamily": "iPhone"}'
      echo ']}'
    }
    ;;
  "list devices")
    jq '{devices: (reduce .devices[] as $d ({}; .[$d.runtime] += [{
      udid: $d.udid, name: $d.name, state: $d.state, isAvailable: true, deviceTypeIdentifier: $d.type}]))}' "$DB"
    ;;
  create\ *)
    [ "$#" -eq 4 ] || { echo "fake simctl create: want <name> <type> <runtime>" >&2; exit 1; }
    case "$3" in com.apple.CoreSimulator.SimDeviceType.*) ;; *) echo "Invalid device type: $3" >&2; exit 1 ;; esac
    case "$4" in com.apple.CoreSimulator.SimRuntime.iOS-*) ;; *) echo "Invalid runtime: $4" >&2; exit 1 ;; esac
    add "$2" Shutdown "$3" "$4"
    ;;
  boot\ *)
    need "$2"
    [ "$(state "$2")" = Shutdown ] || { echo "Unable to boot device in current state: $(state "$2")" >&2; exit 149; }
    set_state "$2" Booted
    ;;
  bootstatus\ *)
    need "$2"
    echo "Monitoring boot status for $2."
    if [ "$(state "$2")" = Booted ]; then
      echo "Device already booted, nothing to do."
    elif [ "${3:-}" = -b ]; then
      set_state "$2" Booted
      echo "Finished"
    else
      exit 1
    fi
    ;;
  shutdown\ *)
    need "$2"
    [ "$(state "$2")" != Shutdown ] || { echo "Unable to shutdown device in current state: Shutdown" >&2; exit 149; }
    set_state "$2" Shutdown
    ;;
  delete\ *)
    need "$2"
    upd --arg u "$2" 'del(.devices[] | select(.udid == $u))'
    rm -rf "${MEGURI_SIM_DEVICES_DIR:?}/$2"
    ;;
  listapps\ *)
    need_booted "$2" "list apps"
    # The old-style plist simctl prints, with a nested dictionary and array per app.
    jq -r --arg u "$2" '"{", (.devices[] | select(.udid == $u) | .apps[] |
      "    \"\(.id)\" =     {",
      "        ApplicationType = \(.type);",
      "        Bundle = \"file:///Applications/\(.id).app/\";",
      "        CFBundleIdentifier = \"\(.id)\";",
      "        GroupContainers =         {",
      "            \"group.\(.id)\" = \"file:///AppGroup/{x}/\";",
      "        };",
      "        SBAppTags =         (",
      "            \"hidden\"",
      "        );",
      "    };"), "}"' "$DB"
    ;;
  uninstall\ *)
    need_booted "$2" uninstall
    upd --arg u "$2" --arg b "$3" '(.devices[] | select(.udid == $u) | .apps) |= map(select(.id != $b))'
    ;;
  launch\ *)
    # simctl launch [--options] <udid> <bundle id> [args]: prints "<bundle id>: <pid>".
    shift
    while [ "${1#--}" != "${1:-}" ]; do shift; done
    need_booted "$1" launch
    echo "${2:-?}: 4242"
    ;;
  "ui "*)
    need_booted "$2" ui
    [ "${3:-}" != appearance ] || upd --arg u "$2" --arg a "$4" '(.devices[] | select(.udid == $u) | .appearance) = $a'
    ;;
  privacy\ * | keychain\ * | status_bar\ * | install\ *)
    need_booted "$2" "$1"
    if [ "$1 ${3:-}" = "status_bar override" ] && [ "${FAKE_SIM_STATUS_BAR_FAIL:-}" = 1 ]; then
      echo "fake simctl: status_bar override failed" >&2
      exit 1
    fi
    ;;
  io\ *)
    need_booted "$2" io
    for file; do :; done # the last argument
    case "$3" in
      screenshot) printf '\211PNG\r\n\032\nfake' >"$file" ;;
      recordVideo)
        rmdir "$DB.lock" 2>/dev/null
        trap - EXIT
        if [ "${FAKE_SIM_RECORD_EMPTY:-}" = 1 ]; then : >"$file"; else printf 'fake video\n' >"$file"; fi
        trap 'echo "Recording completed" >&2; exit 0' INT
        echo "Recording started" >&2
        if [ -n "${FAKE_SIM_RECORD_EXIT_AFTER:-}" ]; then
          sleep "$FAKE_SIM_RECORD_EXIT_AFTER"
          echo "Recording failed: the device went away" >&2
          exit 1
        fi
        while :; do sleep 0.1; done
        ;;
      *) echo "fake simctl io: unexpected $3" >&2; exit 97 ;;
    esac
    ;;
  "runtime add")
    [ -f "${3:-}" ] || { echo "fake simctl runtime add: no image at ${3:-}" >&2; exit 1; }
    : >"$DB.runtime-added"
    echo "D: 87197EB7-5F23-4323-BCB1-A9E56C24B65F iOS (27.2 - 24B8117) (Ready)"
    ;;
  spawn\ *)
    # Only launchctl in the simulator (restarting SpringBoard, listing it)
    # and its log of the style arbiter, which logs a change to dark while
    # the appearance is dark and SpringBoard applies changes.
    need_booted "$2" spawn
    case "$3 ${4:-}" in
      "launchctl kickstart")
        upd --arg u "$2" --arg need "${FAKE_SIM_SB_RESTARTS:-1}" '(.devices[] | select(.udid == $u)) |=
          (.restarts += 1 | if $need != "never" and .restarts >= ($need | tonumber) then .springboard = "ok" else . end)'
        ;;
      "launchctl list") printf 'PID\tStatus\tLabel\n4242\t0\tcom.apple.SpringBoard\n' ;;
      "log show")
        jq -r --arg u "$2" '.devices[] | select(.udid == $u and .appearance == "dark" and (.springboard // "ok") == "ok")
          | "2026-10-15 15:29:20.971 Df SpringBoard[4242:27643] [com.apple.UIKit:UserInterfaceStyleArbiter] Arbiter changing style from 1 to 2, notifying: 1"' "$DB"
        ;;
      *) echo "fake simctl spawn: unexpected $*" >&2; exit 97 ;;
    esac
    ;;
  erase\ *)
    need "$2"
    ;;
  __add\ *)
    add "$2" "$3" "${4:-$IPHONE}" "${5:-$RT_NEW}"
    ;;
  __app\ *)
    need "$2"
    upd --arg u "$2" --arg b "$3" --arg t "$4" '(.devices[] | select(.udid == $u) | .apps) += [{id: $b, type: $t}]'
    ;;
  __state\ *)
    state "$2"
    ;;
  __apps\ *)
    jq -r --arg u "$2" '.devices[] | select(.udid == $u) | .apps[].id' "$DB"
    ;;
  __voiceover\ *)
    need "$2"
    case "${3:-}" in
      enabled | disabled)
        upd --arg u "$2" --arg s "$3" '(.devices[] | select(.udid == $u) | .voiceover) = ($s == "enabled")' || exit 1 ;;
      "") ;;
      *) echo "fake simctl __voiceover: want enabled or disabled" >&2; exit 97 ;;
    esac
    jq -r --arg u "$2" '.devices[] | select(.udid == $u) | if .voiceover then "enabled" else "disabled" end' "$DB"
    ;;
  __voiceover_disable_fail\ *)
    need "$2"
    case "${3:-}" in true | false) ;; *) exit 97 ;; esac
    upd --arg u "$2" --argjson f "$3" '(.devices[] | select(.udid == $u) | .voiceover_disable_fail) = $f'
    ;;
  *)
    echo "fake simctl: unexpected call: $*" >&2
    exit 97
    ;;
esac
