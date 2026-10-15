#!/bin/bash
# Recording worker for `tools/mg record start` (U8, R1976). Runs in its own
# session and outlives that job: it records a simulator's screen with
# `simctl io recordVideo` and holds the lease the job handed to it
# (tools/mg.d/host.sh: the test VM's, or the host's) until it stops. With
# MEGURI_SIM_RECORD_SSH (the test VM's SSH wrapper, U8c) simctl records in
# the VM, into the same path, and the video comes back when it stops; on
# this Mac the worker also keeps the display awake (caffeinate).
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
vm="${MEGURI_SIM_RECORD_SSH:-}"

# shellcheck source=SCRIPTDIR/host-lease-mutex.sh
. "$(dirname "$0")/host-lease-mutex.sh"
# shellcheck source=SCRIPTDIR/mac-vm-lib.sh
. "$(dirname "$0")/mac-vm-lib.sh"

now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }
holder_pid() { jq -r '.pid // empty' "$1/holder.json" 2>/dev/null; }

# The slot this process holds: the nested one, or the lease. A job of the
# same checkout running nested in the lease inherits it (host_lease_drop).
# Under the lease's mutex: a nested job takes its slot (mkdir, then
# holder.json) under it too, so it never has its slot removed half-taken.
release_lease() {
  local locked=0
  host_lease_lock "$lease" && locked=1
  if [ "$(holder_pid "$lease/inner")" = "$me" ]; then
    rm -rf "$lease/inner"
  elif [ "$(holder_pid "$lease")" = "$me" ]; then
    host_lease_drop "$lease"
  fi
  [ "$locked" = 0 ] || host_lease_unlock
}

# In the VM: a command there (its pid file is <video>.pid there).
remote() { "$vm" "$1" </dev/null; }

finish() {
  trap '' INT TERM HUP
  local code="" i
  if [ -n "$recorder" ]; then
    if [ -n "$vm" ]; then
      remote "kill -INT \$(cat $(mvl_q "$video.pid")) 2>/dev/null" >/dev/null 2>&1
    else
      kill -INT "$recorder" 2>/dev/null
    fi
    for i in $(seq 1 150); do
      kill -0 "$recorder" 2>/dev/null || break
      sleep 0.1
    done
    [ -z "$vm" ] || remote "p=$(mvl_q "$video.pid"); [ ! -f \"\$p\" ] || kill -9 \$(cat \"\$p\") 2>/dev/null; rm -f \"\$p\"" >/dev/null 2>&1
    kill -9 "$recorder" 2>/dev/null
    wait "$recorder" 2>/dev/null
    code=$?
    if [ -n "$vm" ]; then
      rm -f "$video"
      if mvl_copy_out "$vm" "the test VM" "$video" 2>>"$log"; then
        remote "rm -f $(mvl_q "$video")" >/dev/null 2>&1
      else
        echo "sim-record: $MVL_REASON" >>"$log"
      fi
    fi
  fi
  [ -z "$awake" ] || kill "$awake" 2>/dev/null
  jq -n --arg r "$reason" --arg c "$code" --argjson t "$(now)" \
    '{reason: $r, recorder_exit: ($c | tonumber? // null), ended_epoch: $t}' >"$status.tmp" &&
    mv -f "$status.tmp" "$status"
  release_lease
  exit 0
}

trap 'reason=stopped; finish' INT TERM HUP
if [ -z "$vm" ] && command -v caffeinate >/dev/null 2>&1; then
  caffeinate -d -i -w "$me" >/dev/null 2>&1 &
  awake=$!
fi
if [ -n "$vm" ]; then
  # simctl replaces the shell that wrote its pid: SIGINT reaches it there.
  remote "mkdir -p $(mvl_q "$(dirname "$video")") && rm -f $(mvl_q "$video") && echo \$\$ >$(mvl_q "$video.pid") &&
    exec xcrun simctl io $(mvl_q "$udid") recordVideo --codec h264 --force $(mvl_q "$video")" >"$log" 2>&1 &
else
  # A background command of a script starts with SIGINT ignored; simctl stops
  # cleanly on SIGINT, so it gets the default back.
  perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die "exec: $!"' \
    xcrun simctl io "$udid" recordVideo --codec h264 --force "$video" </dev/null >"$log" 2>&1 &
fi
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
