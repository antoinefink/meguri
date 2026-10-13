#!/bin/bash
# A stand-in for `xcrun simctl`, installed as `xcrun` on PATH by the sim-pool
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

set -u
[ "${1:-}" = simctl ] || { echo "fake xcrun: only simctl is faked" >&2; exit 97; }
shift
case "${1:-}" in __*) ;; *) printf 'xcrun simctl %s\n' "$*" >>"$STUB_LOG" ;; esac

DB="$FAKE_SIM_DB"
RT_NEW=com.apple.CoreSimulator.SimRuntime.iOS-27-0
IPHONE=com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro

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
  upd --arg u "$udid" --arg n "$1" --arg s "$2" --arg t "$3" --arg r "$4" '.next += 1 |
    .devices += [{udid: $u, name: $n, state: $s, type: $t, runtime: $r,
      apps: [{id: "com.apple.Preferences", type: "System"}, {id: "com.apple.mobilesafari", type: "System"}]}]'
  mkdir -p "$MEGURI_SIM_DEVICES_DIR/$udid/data"
  printf '%s\n' "$udid"
}

lock
case "${1:-} ${2:-}" in
  "list runtimes")
    cat <<'EOF'
{"runtimes": [
  {"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-18-2", "version": "18.2", "isAvailable": true, "platform": "iOS"},
  {"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-27-0", "version": "27.0", "isAvailable": true, "platform": "iOS"},
  {"identifier": "com.apple.CoreSimulator.SimRuntime.iOS-27-1", "version": "27.1", "isAvailable": false, "platform": "iOS"},
  {"identifier": "com.apple.CoreSimulator.SimRuntime.watchOS-28-0", "version": "28.0", "isAvailable": true, "platform": "watchOS"}
]}
EOF
    ;;
  "list devicetypes")
    cat <<'EOF'
{"devicetypes": [
  {"name": "iPhone 17 Pro", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro"},
  {"name": "iPhone 17 Pro Max", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro-Max"},
  {"name": "iPad Pro 13-inch (M5) (16GB)", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-16GB"},
  {"name": "iPad Pro 13-inch (M5)", "identifier": "com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-12GB"}
]}
EOF
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
  privacy\ * | keychain\ * | ui\ * | status_bar\ *)
    need_booted "$2" "$1"
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
  *)
    echo "fake simctl: unexpected call: $*" >&2
    exit 97
    ;;
esac
