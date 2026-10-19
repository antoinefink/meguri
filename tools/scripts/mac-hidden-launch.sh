#!/bin/bash
# Hidden Debug launch assertions used by xcode-check in the leased test VM.
# Keep this helper and the product at their checkout paths in the guest.
set -u

# running <executable>: the PIDs of the processes running it.
running() {
  ps -Aww -o pid=,command= | awk -v e="$1" '
    { pid = $1; sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "") }
    $0 == e || index($0, e " ") == 1 { print pid }'
}

# quit_running <executable>: terminates the processes running it (SIGKILL after 10 s).
quit_running() {
  local pids
  pids="$(running "$1")"
  [ -n "$pids" ] || return 0
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    [ -n "$(running "$1")" ] || return 0
    sleep 1
  done
  # shellcheck disable=SC2046
  kill -9 $(running "$1") 2>/dev/null
}

# stays_up <app> [launch args...]: launches the app hidden and in the
# background, as agents must (`open -g -j`, root AGENTS.md), with
# -MeguriBackgroundLaunch YES (U8d), checks that it still runs 3 s later, out
# of the Dock and not the active app (lsappinfo), then quits it. On the Mac an
# alert presented while the app is hidden closes the window, and the app quits
# (app/AGENTS.md).
stays_up() {
  local app="$1" opener rc=0 pids pid shown type
  shift
  open -g -j -n -W "$app" --args -MeguriBackgroundLaunch YES -ApplePersistenceIgnoreState YES "$@" >/dev/null 2>&1 &
  opener=$!
  sleep 3
  if ! kill -0 "$opener" 2>/dev/null; then
    wait "$opener"
    echo "the app quit within 3 s of a hidden launch (open -W exit $?)"
    rc=1
  fi
  pids="$(running "$app/Contents/MacOS/Meguri")"
  if [ -z "$pids" ]; then
    echo "cannot verify the Mac app stays out of sight: no known pid"
    rc=1
  fi
  for pid in $pids; do
    if ! shown="$(lsappinfo info -only ApplicationType "$pid" 2>/dev/null)"; then
      echo "cannot verify the Mac app stays out of sight (pid $pid): lsappinfo failed"
      rc=1
      continue
    fi
    type="$(printf '%s\n' "$shown" | sed -nE 's/.*type="([A-Za-z]+)".*/\1/p' | head -n 1)"
    shown="$(printf '%s\n' "$shown" | tr -s '\n ' '  ')"
    case "$shown" in
      *'type="Foreground"'* | *"(in front)"*)
        echo "the app showed on this Mac (pid $pid; a Dock tile or the active app): lsappinfo:$shown"
        rc=1
        ;;
      *)
        case "$type" in
          UIElement | BackgroundOnly) ;;
          *)
            echo "cannot verify the Mac app stays out of sight (pid $pid; expected UIElement or BackgroundOnly): lsappinfo:${shown:-empty}"
            rc=1
            ;;
        esac
        ;;
    esac
  done
  quit_running "$app/Contents/MacOS/Meguri"
  kill "$opener" 2>/dev/null
  wait "$opener" 2>/dev/null
  return "$rc"
}


app="${1:?app path required}"
shift
# TERM/HUP and a lost SSH session must not leave an app using old products.
trap 'quit_running "$app/Contents/MacOS/Meguri"' EXIT
trap 'exit 130' INT TERM HUP
stays_up "$app" "$@"
