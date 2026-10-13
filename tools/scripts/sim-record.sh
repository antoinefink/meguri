#!/bin/bash
# Recording worker for `tools/mg record start` (U8, R1976). Runs in its own
# session and outlives that job: it records a simulator's screen with
# `simctl io recordVideo`, keeps the display awake (caffeinate) and holds the
# host lease the job handed to it (tools/mg.d/host.sh) until it stops.
#
# usage: sim-record.sh <udid> <video> <recorder log> <lease dir> <status file> <max seconds>
#
# It stops on SIGTERM or SIGHUP (`tools/mg record stop`; started in the
# background, it may have SIGINT ignored), when the
# recording reaches <max seconds> (a forgotten recording must not hold the
# lease for good) or when the recorder exits on its own. Stopping lets simctl
# finish the file, ends caffeinate, releases the lease slot it holds and
# writes <status file>: {"reason": "stopped"|"limit"|"recorder-exited",
# "recorder_exit": <code>, "ended_epoch": <s>}. Runs under macOS /bin/bash 3.2.

set -u
udid="$1" video="$2" log="$3" lease="$4" status="$5" max="$6"
me=$$
recorder="" awake="" reason=stopped

now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }
holder_pid() { jq -r '.pid // empty' "$1/holder.json" 2>/dev/null; }

# The slot this process holds: the nested one, or the lease. A job of the
# same checkout running nested in the lease inherits it.
release_lease() {
  local inner
  if [ "$(holder_pid "$lease/inner")" = "$me" ]; then
    rm -rf "$lease/inner"
    return 0
  fi
  [ "$(holder_pid "$lease")" = "$me" ] || return 0
  inner="$(holder_pid "$lease/inner")"
  if [ -n "$inner" ] && kill -0 "$inner" 2>/dev/null; then
    jq 'del(.nested)' "$lease/inner/holder.json" >"$lease/holder.json.tmp" &&
      mv -f "$lease/holder.json.tmp" "$lease/holder.json"
    rm -rf "$lease/inner"
  else
    rm -rf "$lease"
  fi
}

finish() {
  trap '' INT TERM HUP
  local code="" i
  if [ -n "$recorder" ]; then
    kill -INT "$recorder" 2>/dev/null
    for i in $(seq 1 150); do
      kill -0 "$recorder" 2>/dev/null || break
      sleep 0.1
    done
    kill -9 "$recorder" 2>/dev/null
    wait "$recorder" 2>/dev/null
    code=$?
  fi
  [ -z "$awake" ] || kill "$awake" 2>/dev/null
  jq -n --arg r "$reason" --arg c "$code" --argjson t "$(now)" \
    '{reason: $r, recorder_exit: ($c | tonumber? // null), ended_epoch: $t}' >"$status.tmp" &&
    mv -f "$status.tmp" "$status"
  release_lease
  exit 0
}

trap 'reason=stopped; finish' INT TERM HUP
if command -v caffeinate >/dev/null 2>&1; then
  caffeinate -d -i -w "$me" >/dev/null 2>&1 &
  awake=$!
fi
# A background command of a script starts with SIGINT ignored; simctl stops
# cleanly on SIGINT, so it gets the default back.
perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die "exec: $!"' \
  xcrun simctl io "$udid" recordVideo --codec h264 --force "$video" </dev/null >"$log" 2>&1 &
recorder=$!
start="$(date +%s)"
while kill -0 "$recorder" 2>/dev/null; do
  if [ $(($(date +%s) - start)) -ge "$max" ]; then
    echo "sim-record: stopping at the $max s limit" >>"$log"
    reason=limit
    finish
  fi
  sleep 1 &
  wait "$!"
done
reason=recorder-exited
finish
