#!/bin/bash
# tools/scripts/sim-pool.sh: a shared pool of iOS simulators, created once,
# booted once, kept booted, and leased to one caller at a time.
#
# The pool lives in the macOS test VM (U8c, docs/process/mac-ui-vm.md): the
# first VM of the per-host config (tools/vm/AGENTS.md) that can run. Called
# on this Mac, the script starts that VM headless when it is stopped, waits
# for its logged-in session and runs itself there over SSH, with the caller's
# checkout, directory and pid recorded in the lease. Without a VM it runs on
# this Mac only with MEGURI_SIM_ALLOW_HOST=1 (a run Antoine starts himself),
# and otherwise exits 5 (queued).
#
# The VM runs one job at a time (its lease, tools/mg.d/mac-vm.sh). acquire
# and gc (but gc --list) create, boot and delete simulators there, so they
# run under that lease: directly when the caller runs under the job that
# holds it (a job's own leases and scripts), otherwise through `tools/mg sim
# <command>`, which waits for the lease and runs this script again under it.
#
# A fresh simulator's first boot crashes system apps (PosterBoard, about 15 s
# in) and a shutdown crashes simulator daemons; macOS shows each crash as a
# dialog on Antoine's screen. So pool devices are never shut down, erased or
# deleted, by any command here.
#
#   sim-pool.sh acquire <kind> [--holder <label>] [--ttl-min N]
#       Lease a booted pool device; prints its UDID, the only stdout line.
#   sim-pool.sh renew <udid> [--ttl-min N]
#   sim-pool.sh release <udid>
#       Reset the device (user apps, privacy, keychain, UI options, status
#       bar, VoiceOver), leave it booted, drop the lease.
#   sim-pool.sh reset <udid>
#       Reset a leased device the same way and keep the lease.
#   sim-pool.sh status
#       JSON on stdout, a table on stderr.
#   sim-pool.sh gc [--older-than-hours N] [--list]
#       Shut down and delete leftover Meguri-* simulators (never Meguri-Pool-*,
#       never leased ones, never other names) whose device directory is older
#       than N hours (default 6). --list prints the targets and deletes nothing.
#
# Pool devices are Meguri-Pool-<kind>-<n> on their kind's pinned iOS runtime,
# at most MEGURI_SIM_POOL_MAX (3) per kind. Kinds and their device types come
# from tools/sim-devices.json; kind `foldable` exists once Xcode has an iPhone
# device type matching its pattern (until then acquire exits 4). Leases live in
# ~/Library/Caches/meguri/sim-pool/leases/<udid>/lease.json, shared by every
# worktree; each lease is a mkdir lock. Claiming, creating, taking over,
# renewing and dropping leases hold the pool lock (pool.lock, mkdir). A lease
# expires after its TTL (default 60 min) and may then be taken over; the
# device is reset first. The lease's pid is informational.
#
# Exit codes: 0 ok, 1 failed, 2 usage, 3 every device stayed leased until
# the wait ran out, 4 the kind's device type is unavailable in this toolchain,
# 5 no test VM can run the pool now (queued): none is there
# (MEGURI_SIM_ALLOW_HOST=1 runs it here), or it cannot be reached (U19c: no
# logged-in session over SSH in time, ssh failed with 255, or the command
# could not start or fork there: mvl_unreachable; the last "sim-pool:
# queued: " line on stderr says why), 6 the test VM could not run it (the
# config is not valid, tart could not start the VM, the VM's lease).
# Output other than results goes to stderr.
#
# Overrides (self-tests): MEGURI_SIM_POOL_DIR, MEGURI_SIM_POOL_MAX,
# MEGURI_SIM_POOL_WAIT_SECONDS (1200), MEGURI_SIM_POOL_POLL_SECONDS (10),
# MEGURI_SIM_POOL_SETTLE_SECONDS (30), MEGURI_SIM_POOL_SPRINGBOARD_SECONDS
# (30), MEGURI_SIM_DEVICES_DIR,
# MEGURI_SIM_DEVICES_JSON (tools/sim-devices.json); MEGURI_MAC_VM_CONFIG and
# the other overrides of tools/scripts/mac-vm-lib.sh. The MEGURI_SIM_POOL_*
# and MEGURI_SIM_DEVICES_DIR overrides set here go to the VM along.
# Internal: MEGURI_SIM_POOL_IN_VM=1 (this run is the VM's), with
# MEGURI_SIM_POOL_CALLER naming the caller's description (vm, worktree, cwd,
# pid), which sets MEGURI_SIM_POOL_VM, _WORKTREE, _CWD and _PID;
# MEGURI_SIM_POOL_VIA_MG=1 (this run asked tools/mg sim to take the VM's
# lease: the run it starts must find it held).
#
# Runs under macOS /bin/bash 3.2 with BSD tools, and with GNU tools in the
# dev container (self-tests).

set -u

SP_SCRIPTS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
SP_ROOT="$(cd "$SP_SCRIPTS/../.." && pwd -P)"
SP_DEVICES_JSON="${MEGURI_SIM_DEVICES_JSON:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)/sim-devices.json}"
SP_KINDS="" # set from SP_DEVICES_JSON before any command runs

SP_PREFIX=Meguri-Pool-
SP_DIR="${MEGURI_SIM_POOL_DIR:-$HOME/Library/Caches/meguri/sim-pool}"
SP_LEASES="$SP_DIR/leases"
SP_LOCK="$SP_DIR/pool.lock"
SP_READY="$SP_DIR/springboard" # devices whose SpringBoard was checked since their boot
SP_MAX="${MEGURI_SIM_POOL_MAX:-3}"
SP_WAIT="${MEGURI_SIM_POOL_WAIT_SECONDS:-1200}"
SP_POLL="${MEGURI_SIM_POOL_POLL_SECONDS:-10}"
SP_SETTLE="${MEGURI_SIM_POOL_SETTLE_SECONDS:-30}"
SP_SB_SETTLE="${MEGURI_SIM_POOL_SPRINGBOARD_SECONDS:-30}"
SP_DEVICES_DIR="${MEGURI_SIM_DEVICES_DIR:-$HOME/Library/Developer/CoreSimulator/Devices}"
SP_DEFAULT_TTL=60
SP_TAB=$'\t'

SP_LOCKED=""
SP_PENDING_ID=""
SP_PENDING="" # a lease this run claimed and has not handed out yet

sp_log() { printf 'sim-pool: %s\n' "$*" >&2; }

sp_usage() {
  sp_log "$1"
  cat >&2 <<EOF
usage: sim-pool.sh acquire <$(sp_kind_names | tr ' ' '|')> [--holder <label>] [--ttl-min N]
       sim-pool.sh renew <udid> [--ttl-min N]
       sim-pool.sh release <udid>
       sim-pool.sh reset <udid>
       sim-pool.sh status
       sim-pool.sh gc [--older-than-hours N] [--list]
EOF
  exit 2
}

sp_now() { date -u +%s; }

sp_is_uint() { case "$1" in "" | *[!0-9]*) return 1 ;; esac; }

# Kinds, one per line: <kind>|<device type id>, from tools/sim-devices.json.
# The foldable kind has no fixed type (sp_foldable_type finds it).
sp_kinds() {
  jq -r '(.devices[] | "\(.kind)|\(.device_type)"), "\(.foldable.kind)|"' "$SP_DEVICES_JSON"
}

# sp_kind_type <kind>: the kind's device type id (empty for the foldable kind).
sp_kind_type() {
  local line
  while IFS= read -r line; do
    if [ "${line%%|*}" = "$1" ]; then
      printf '%s\n' "${line#*|}"
      return 0
    fi
  done <<EOF
$SP_KINDS
EOF
  return 1
}

sp_kind_names() { printf '%s\n' "$SP_KINDS" | cut -d'|' -f1 | tr '\n' ' ' | sed 's/ $//'; }

# ---------------------------------------------------------------------------
# Pool lock and leases

sp_lock() {
  local tries=0 pid
  mkdir -p "$SP_LEASES" || { sp_log "cannot create $SP_LEASES"; exit 1; }
  until mkdir "$SP_LOCK" 2>/dev/null; do
    pid="$(cat "$SP_LOCK/pid" 2>/dev/null)"
    # Never evict a live transaction. An old lock without a pid, or a
    # holder that died, is stale.
    if { [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null &&
      [ "$(cat "$SP_LOCK/pid" 2>/dev/null)" = "$pid" ]; } ||
      { [ -z "$pid" ] && [ -n "$(find "$SP_LOCK" -maxdepth 0 -mmin +10 2>/dev/null)" ]; }; then
      sp_log "removing a stale pool lock (pid ${pid:-unknown})"
      rm -rf "$SP_LOCK"
      continue
    fi
    tries=$((tries + 1))
    if [ "$tries" -ge 1500 ]; then
      sp_log "the pool lock $SP_LOCK stayed held for 5 minutes (pid ${pid:-unknown})"
      exit 1
    fi
    sleep 0.2
  done
  printf '%s\n' "$$" >"$SP_LOCK/pid"
  SP_LOCKED=1
}

sp_unlock() {
  [ -z "$SP_LOCKED" ] || [ "$(cat "$SP_LOCK/pid" 2>/dev/null)" != "$$" ] || rm -rf "$SP_LOCK"
  SP_LOCKED=""
}

sp_on_exit() {
  if [ -n "$SP_PENDING" ]; then
    [ -n "$SP_LOCKED" ] || sp_lock
    if [ "$(jq -r '.acquisition_id // empty' "$SP_PENDING/lease.json" 2>/dev/null)" = "$SP_PENDING_ID" ] &&
      [ -n "$SP_PENDING_ID" ]; then
      rm -rf "$SP_PENDING"
    fi
  fi
  sp_unlock
}

# sp_lease_state <udid>: free, held or expired
sp_lease_state() {
  local d="$SP_LEASES/$1" exp
  if [ ! -d "$d" ]; then
    echo free
    return 0
  fi
  exp="$(jq -r '.expires_epoch // empty' "$d/lease.json" 2>/dev/null)"
  if [ -z "$exp" ]; then
    # A run that died between mkdir and writing lease.json.
    if [ -n "$(find "$d" -maxdepth 0 -mmin +2 2>/dev/null)" ]; then echo expired; else echo held; fi
  elif [ "$(sp_now)" -ge "$exp" ]; then
    echo expired
  else
    echo held
  fi
}

# sp_checkout: the caller's checkout (in the VM, the one on the host).
sp_checkout() {
  if [ -n "${MEGURI_SIM_POOL_WORKTREE:-}" ]; then
    printf '%s\n' "$MEGURI_SIM_POOL_WORKTREE"
  else
    git rev-parse --show-toplevel 2>/dev/null || pwd -P
  fi
}

# sp_write_lease <udid> <kind> <holder> <ttl minutes>
sp_write_lease() {
  local d="$SP_LEASES/$1" now cwd top acquisition
  acquisition="$(mktemp "$d/acquisition.XXXXXXXX")" || return 1
  rm -f "$acquisition"
  now="$(sp_now)"
  cwd="${MEGURI_SIM_POOL_CWD:-$(pwd -P)}"
  top="$(sp_checkout)"
  jq -n --arg udid "$1" --arg kind "$2" --arg holder "$3" --argjson ttl "$4" \
    --arg acquisition "${acquisition##*/}" --arg cwd "$cwd" --arg wt "$top" --argjson pid "${MEGURI_SIM_POOL_PID:-$PPID}" --argjson now "$now" \
    --arg vm "${MEGURI_SIM_POOL_VM:-}" '{
      udid: $udid, kind: $kind, holder: $holder, worktree: $wt, cwd: $cwd, pid: $pid,
      acquisition_id: $acquisition,
      ttl_min: $ttl, acquired_at: ($now | todate), expires_at: ($now + $ttl * 60 | todate),
      acquired_epoch: $now, expires_epoch: ($now + $ttl * 60)
    } + (if $vm == "" then {} else {vm: $vm} end)' >"$d/lease.json.tmp" && mv -f "$d/lease.json.tmp" "$d/lease.json"
}

# sp_take <udid> <kind> <holder> <ttl>: lease a device nobody holds (under the pool lock).
sp_take() {
  mkdir "$SP_LEASES/$1" 2>/dev/null || return 1
  SP_PENDING="$SP_LEASES/$1"
  if ! sp_write_lease "$@"; then
    rm -rf "$SP_PENDING"
    SP_PENDING=""
    return 1
  fi
  SP_PENDING_ID="$(jq -r .acquisition_id "$SP_PENDING/lease.json")"
}

# ---------------------------------------------------------------------------
# simctl queries

# One line per device: udid, name, state, runtime id, available (true|false).
sp_devices() {
  local json
  json="$(xcrun simctl list devices --json)" || { sp_log "simctl list devices failed"; return 1; }
  jq -r '.devices | to_entries[] | .key as $rt | .value[] |
    [.udid, .name, .state, $rt, (if .isAvailable == false then "false" else "true" end)] | @tsv' <<<"$json"
}

# sp_runtime [<kind>]: the kind's pinned iOS runtime, when it is installed and
# available: identifier<TAB>version. The foldable kind has its own, which a
# newer runtime in the pool never changes for the other kinds.
sp_runtime() {
  local json pin
  pin="$(jq -r --arg k "${1:-}" 'if $k == .foldable.kind then .foldable.runtime else .runtime end | .identifier' "$SP_DEVICES_JSON")" || return 1
  json="$(xcrun simctl list runtimes --json)" || return 1
  jq -r --arg p "$pin" 'first(.runtimes[] | select(.identifier == $p and .isAvailable != false))
    | [.identifier, .version] | @tsv' <<<"$json"
}

# sp_devicetype_known <id>: prints the id when Xcode has that device type.
sp_devicetype_known() {
  local json
  json="$(xcrun simctl list devicetypes --json)" || return 1
  jq -r --arg i "$1" 'first(.devicetypes[] | select(.identifier == $i) | .identifier) // empty' <<<"$json"
}

# sp_foldable_type: the first iPhone device type whose name matches the
# foldable pattern of tools/sim-devices.json, if any.
sp_foldable_type() {
  local json pattern
  pattern="$(jq -r '.foldable.device_type_name_pattern' "$SP_DEVICES_JSON")" || return 1
  json="$(xcrun simctl list devicetypes --json)" || return 1
  jq -r --arg p "$pattern" 'first(.devicetypes[] | select((.productFamily // "") == "iPhone" and (.name | test($p; "i")))
    | .identifier) // empty' <<<"$json"
}

# sp_device <udid>: that device's line from sp_devices, if any.
sp_device() {
  local list
  list="$(sp_devices)" || return 1
  printf '%s\n' "$list" | awk -F'\t' -v u="$1" '$1 == u { print; exit }'
}

# Bundle ids of user-installed apps, from `simctl listapps` (an old-style
# plist: one dictionary per app, keyed by bundle id) on stdin.
sp_user_apps() {
  awk '
    function update(line,   i, c, n) {
      n = length(line)
      for (i = 1; i <= n; i++) {
        c = substr(line, i, 1)
        if (esc) { esc = 0; continue }
        if (instr) {
          if (c == "\\") esc = 1
          else if (c == "\"") instr = 0
          continue
        }
        if (c == "\"") instr = 1
        else if (c == "{") depth++
        else if (c == "}") depth--
      }
    }
    function value(line) {
      sub(/^[^=]*=[ \t]*/, "", line)
      sub(/[ \t]*;[ \t]*$/, "", line)
      gsub(/"/, "", line)
      return line
    }
    {
      d = depth
      if (d == 2 && !instr && $0 ~ /^[ \t]*ApplicationType[ \t]*=/) user = (value($0) == "User")
      if (d == 2 && !instr && $0 ~ /^[ \t]*CFBundleIdentifier[ \t]*=/) id = value($0)
      update($0)
      if (d == 1 && depth == 2) { user = 0; id = "" }
      if (d == 2 && depth == 1) {
        if (user && id != "") print id
        user = 0; id = ""
      }
    }'
}

# ---------------------------------------------------------------------------
# Device operations (pool devices only)

# sp_boot <udid> <state>: boot unless booted, wait for the boot to finish.
sp_boot() {
  rm -f "$SP_READY/$1"
  if [ "$2" = Shutdown ]; then
    xcrun simctl boot "$1" >&2 || sp_log "simctl boot $1 failed; waiting for its boot status"
  fi
  xcrun simctl bootstatus "$1" -b >&2
}

# A background sleep, so that a TERM, INT or HUP trap runs at once.
sp_sleep() {
  sleep "$1" &
  wait "$!"
}

sp_settle() {
  if [ "$SP_SETTLE" -gt 0 ]; then
    sp_log "letting $1 settle for $SP_SETTLE s after its boot"
    sp_sleep "$SP_SETTLE"
  fi
}

# sp_appearance_applies <udid>: whether the simulator's SpringBoard applies
# an appearance change: dark, seen by its style arbiter (the unified log
# inside the simulator) within 10 s, then light again.
sp_appearance_applies() {
  local since _ seen=1
  xcrun simctl ui "$1" appearance light >&2 || return 1
  since="$(date '+%Y-%m-%d %H:%M:%S')"
  xcrun simctl ui "$1" appearance dark >&2 || return 1
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    sp_sleep 1
    if xcrun simctl spawn "$1" log show --start "$since" --style compact \
      --predicate 'category == "UserInterfaceStyleArbiter"' 2>/dev/null | grep -q 'changing style from [0-9]* to 2'; then
      seen=0
      break
    fi
  done
  xcrun simctl ui "$1" appearance light >&2 || return 1
  return "$seen"
}

# sp_prepare_springboard <udid> <name>: once after each boot, checks that
# SpringBoard applies appearance changes (simctl ui appearance,
# XCUIDevice.appearance), restarting it (inside the simulator; the device
# stays booted) until it does, at most 3 times. In the test VM on 2026-10-15
# the SpringBoard of a fresh boot, and one restarted soon after it, accepted
# the change and never applied it, so the accessibility audit's dark variants
# rendered light. Devices booted before this check (no mark in $SP_READY)
# get it on their next acquire.
sp_prepare_springboard() {
  local n=0 _
  [ ! -e "$SP_READY/$1" ] || return 0
  while :; do
    if sp_appearance_applies "$1"; then
      mkdir -p "$SP_READY" && : >"$SP_READY/$1"
      [ "$n" -eq 0 ] || sp_log "SpringBoard on $2 applies appearance changes after $n restart(s)"
      return 0
    fi
    if [ "$n" -ge 3 ]; then
      sp_log "SpringBoard on $2 still ignores appearance changes after $n restarts"
      return 1
    fi
    n=$((n + 1))
    sp_log "SpringBoard on $2 ignores appearance changes; restarting it ($n of 3)"
    xcrun simctl spawn "$1" launchctl kickstart -k user/foreground/com.apple.SpringBoard >&2 || {
      sp_log "restarting SpringBoard on $2 failed"
      return 1
    }
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      xcrun simctl spawn "$1" launchctl list 2>/dev/null | awk '$3 == "com.apple.SpringBoard" && $1 ~ /^[0-9]+$/ { found = 1 } END { exit !found }' && break
      sp_sleep 1
    done
    # A SpringBoard just started drops appearance changes for a while.
    sp_sleep "$SP_SB_SETTLE"
  done
}

sp_disable_voiceover() {
  xcrun devicectl device settings voiceover --device "$1" --disable >&2 || {
    sp_log "disabling VoiceOver on $1 failed"
    return 1
  }
}

# sp_reset <udid>: back to a clean state, still booted.
sp_reset() {
  local udid="$1" apps bid rc=0
  sp_disable_voiceover "$udid" || rc=1
  if apps="$(xcrun simctl listapps "$udid")"; then
    apps="$(printf '%s\n' "$apps" | sp_user_apps)"
    while IFS= read -r bid; do
      [ -n "$bid" ] || continue
      sp_log "uninstalling $bid"
      xcrun simctl uninstall "$udid" "$bid" >&2 || rc=1
    done <<EOF
$apps
EOF
  else
    sp_log "simctl listapps failed"
    rc=1
  fi
  xcrun simctl privacy "$udid" reset all >&2 || rc=1
  xcrun simctl keychain "$udid" reset >&2 || rc=1
  xcrun simctl ui "$udid" appearance light >&2 || rc=1
  xcrun simctl ui "$udid" content_size large >&2 || rc=1
  xcrun simctl ui "$udid" increase_contrast disabled >&2 || rc=1
  xcrun simctl status_bar "$udid" clear >&2 || rc=1
  [ "$rc" -eq 0 ] || sp_log "resetting $udid failed in part; see the output above"
  return "$rc"
}

# ---------------------------------------------------------------------------
# Commands

# sp_claim <kind> <runtime id> <device type id> <holder> <ttl>, under the pool
# lock: lease a free pool device (booted ones first), else create one below
# the cap, else take over an expired lease. Sets SP_UDID, SP_NAME, SP_STATE and
# SP_HOW (reused, created or takeover). Returns 0, 1 on error, 3 when all are held.
sp_claim() {
  local kind="$1" rt="$2" type="$3" holder="$4" ttl="$5"
  local list cands udid name state n count=0 old="" old_name="" old_state=""
  list="$(sp_devices)" || return 1
  cands="$(printf '%s\n' "$list" | awk -F'\t' -v p="$SP_PREFIX$kind-" -v rt="$rt" '
    index($2, p) == 1 && substr($2, length(p) + 1) ~ /^[0-9]+$/ && $4 == rt && $5 == "true" {
      printf "%d\t%d\t%s\n", ($3 == "Booted" ? 0 : 1), substr($2, length(p) + 1), $0
    }' | sort -t "$SP_TAB" -k1,1n -k2,2n | cut -f3-)"
  while IFS="$SP_TAB" read -r udid name state _; do
    [ -n "$udid" ] || continue
    count=$((count + 1))
    case "$(sp_lease_state "$udid")" in
      free)
        if sp_take "$udid" "$kind" "$holder" "$ttl"; then
          SP_UDID="$udid" SP_NAME="$name" SP_STATE="$state" SP_HOW=reused
          return 0
        fi
        ;;
      expired)
        [ -n "$old" ] || { old="$udid" old_name="$name" old_state="$state"; }
        ;;
    esac
  done <<EOF
$cands
EOF
  if [ "$count" -lt "$SP_MAX" ]; then
    n=1
    while printf '%s\n' "$list" | cut -f2 | grep -qx "$SP_PREFIX$kind-$n"; do n=$((n + 1)); done
    name="$SP_PREFIX$kind-$n"
    sp_log "creating $name ($type, $rt)"
    udid="$(xcrun simctl create "$name" "$type" "$rt")" || { sp_log "simctl create failed"; return 1; }
    udid="$(printf '%s' "$udid" | tr -d '[:space:]')"
    [ -n "$udid" ] || { sp_log "simctl create printed no UDID"; return 1; }
    sp_take "$udid" "$kind" "$holder" "$ttl" || return 1
    SP_UDID="$udid" SP_NAME="$name" SP_STATE=Shutdown SP_HOW=created
    return 0
  fi
  if [ -n "$old" ]; then
    sp_log "taking over the expired lease on $old_name ($old) held by" \
      "$(jq -r '"\(.holder) in \(.worktree), expired \(.expires_at)"' "$SP_LEASES/$old/lease.json" 2>/dev/null)"
    rm -rf "${SP_LEASES:?}/$old"
    sp_take "$old" "$kind" "$holder" "$ttl" || return 1
    SP_UDID="$old" SP_NAME="$old_name" SP_STATE="$old_state" SP_HOW=takeover
    return 0
  fi
  return 3
}

sp_acquire() {
  local kind="" holder="" ttl="$SP_DEFAULT_TTL" pinned type rt rt_id deadline rc waiting=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --holder)
        [ "$#" -ge 2 ] && [ -n "$2" ] || sp_usage "--holder needs a label"
        holder="$2"
        shift 2
        ;;
      --ttl-min)
        [ "$#" -ge 2 ] && sp_is_uint "$2" && [ "$2" -gt 0 ] || sp_usage "--ttl-min needs a positive number of minutes"
        ttl="$2"
        shift 2
        ;;
      -*) sp_usage "acquire: unknown option '$1'" ;;
      *)
        [ -z "$kind" ] || sp_usage "acquire: unexpected argument '$1'"
        kind="$1"
        shift
        ;;
    esac
  done
  [ -n "$kind" ] || sp_usage "acquire: missing kind"
  pinned="$(sp_kind_type "$kind")" || sp_usage "acquire: unknown kind '$kind' (kinds: $(sp_kind_names))"
  [ -n "$holder" ] || holder="$(basename "$(sp_checkout)")"
  rt="$(sp_runtime "$kind")" || { sp_log "simctl list runtimes failed"; exit 1; }
  if [ -z "$rt" ]; then
    [ -n "$pinned" ] || { sp_log "unavailable: toolchain: the foldable iPhone's iOS runtime is not installed"; exit 4; }
    sp_log "the pinned iOS simulator runtime is not installed"
    exit 1
  fi
  rt_id="${rt%%"$SP_TAB"*}"
  if [ -n "$pinned" ]; then
    type="$(sp_devicetype_known "$pinned")" || { sp_log "simctl list devicetypes failed"; exit 1; }
    [ -n "$type" ] || { sp_log "Xcode has no device type $pinned (tools/sim-devices.json)"; exit 1; }
  else
    type="$(sp_foldable_type)" || { sp_log "simctl list devicetypes failed"; exit 1; }
    [ -n "$type" ] || { sp_log "unavailable: toolchain: Xcode has no foldable iPhone device type"; exit 4; }
  fi

  deadline=$(($(sp_now) + SP_WAIT))
  while :; do
    sp_lock
    sp_claim "$kind" "$rt_id" "$type" "$holder" "$ttl"
    rc=$?
    sp_unlock
    [ "$rc" -eq 0 ] && break
    [ "$rc" -eq 3 ] || exit 1
    if [ "$(sp_now)" -ge "$deadline" ]; then
      sp_log "all $SP_MAX $kind pool devices stayed leased for $SP_WAIT s; see: sim-pool.sh status"
      exit 3
    fi
    if [ -z "$waiting" ]; then
      sp_log "all $SP_MAX $kind pool devices are leased; waiting up to $SP_WAIT s for one"
      waiting=1
    fi
    sp_sleep "$SP_POLL"
  done

  if [ "$SP_STATE" != Booted ]; then
    sp_log "booting $SP_NAME ($SP_UDID)"
    sp_boot "$SP_UDID" "$SP_STATE" || { sp_log "booting $SP_NAME failed"; exit 1; }
    sp_settle "$SP_NAME"
  else
    xcrun simctl bootstatus "$SP_UDID" -b >&2 || { sp_log "$SP_NAME did not report booted"; exit 1; }
  fi
  sp_prepare_springboard "$SP_UDID" "$SP_NAME" || exit 1
  if [ "$SP_HOW" = takeover ]; then
    sp_reset "$SP_UDID" || exit 1
  else
    sp_disable_voiceover "$SP_UDID" || exit 1
  fi
  SP_PENDING=""
  sp_log "leased $SP_NAME ($SP_UDID, $SP_HOW) to $holder for $ttl min; release it with: sim-pool.sh release $SP_UDID"
  printf '%s\n' "$SP_UDID"
}

# sp_pool_device <udid>: sets SP_NAME and SP_STATE, or exits when the UDID is
# not a pool device (release and renew never touch other simulators).
sp_pool_device() {
  local line
  [ -n "$1" ] || sp_usage "missing UDID"
  line="$(sp_device "$1")" || exit 1
  [ -n "$line" ] || { sp_log "no simulator has UDID $1"; exit 1; }
  SP_NAME="$(printf '%s\n' "$line" | cut -f2)"
  SP_STATE="$(printf '%s\n' "$line" | cut -f3)"
  case "$SP_NAME" in
    "$SP_PREFIX"*) ;;
    *)
      sp_log "$1 is '$SP_NAME', not a pool device ($SP_PREFIX<kind>-<n>); refusing to touch it"
      exit 1
      ;;
  esac
}

sp_renew() {
  local udid="${1:-}" ttl="$SP_DEFAULT_TTL" now
  [ "$#" -gt 0 ] && shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --ttl-min)
        [ "$#" -ge 2 ] && sp_is_uint "$2" && [ "$2" -gt 0 ] || sp_usage "--ttl-min needs a positive number of minutes"
        ttl="$2"
        shift 2
        ;;
      *) sp_usage "renew: unexpected argument '$1'" ;;
    esac
  done
  sp_pool_device "$udid"
  sp_lock
  if [ ! -f "$SP_LEASES/$udid/lease.json" ]; then
    sp_unlock
    sp_log "$SP_NAME ($udid) is not leased"
    exit 1
  fi
  now="$(sp_now)"
  if ! jq --argjson now "$now" --argjson ttl "$ttl" \
    '.ttl_min = $ttl | .expires_epoch = ($now + $ttl * 60) | .expires_at = (.expires_epoch | todate) | .renewed_at = ($now | todate)' \
    "$SP_LEASES/$udid/lease.json" >"$SP_LEASES/$udid/lease.json.tmp" ||
    ! mv -f "$SP_LEASES/$udid/lease.json.tmp" "$SP_LEASES/$udid/lease.json"; then
    sp_unlock
    sp_log "could not update the lease on $udid"
    exit 1
  fi
  sp_unlock
  sp_log "renewed $SP_NAME ($udid) for $ttl min"
}

sp_reset_leased() {
  local udid="${1:-}"
  [ "$#" -le 1 ] || sp_usage "reset: unexpected argument '$2'"
  sp_lock
  sp_pool_device "$udid"
  [ -d "$SP_LEASES/$udid" ] || { sp_log "$SP_NAME ($udid) is not leased; lease it before resetting it"; exit 1; }
  [ "$SP_STATE" = Booted ] || { sp_log "$SP_NAME is $SP_STATE, not booted"; exit 1; }
  sp_reset "$udid" || exit 1
  sp_unlock
  sp_log "reset $SP_NAME ($udid); still booted and leased"
}

sp_release() {
  local udid="${1:-}" rc=0
  [ "$#" -le 1 ] || sp_usage "release: unexpected argument '$2'"
  sp_lock
  sp_pool_device "$udid"
  if [ ! -d "$SP_LEASES/$udid" ]; then
    sp_unlock
    sp_log "$SP_NAME ($udid) is not leased; nothing to release"
    return 0
  fi
  if [ "$SP_STATE" != Booted ]; then
    sp_log "$SP_NAME is $SP_STATE; booting it again"
    sp_boot "$udid" "$SP_STATE" && sp_settle "$SP_NAME" && sp_prepare_springboard "$udid" "$SP_NAME" || rc=1
  fi
  [ "$rc" -ne 0 ] || sp_reset "$udid" || rc=1
  rm -rf "${SP_LEASES:?}/$udid"
  sp_unlock
  if [ "$rc" -eq 0 ]; then
    sp_log "released $SP_NAME ($udid), reset and still booted"
  else
    sp_log "released $SP_NAME ($udid), but it is not fully reset"
  fi
  return "$rc"
}

# sp_status_devices <device list> <pinned runtime id> <now> <foldable runtime id>: one JSON line
# per pool device. (A function: macOS bash 3.2 cannot parse a case statement
# inside $(...).)
sp_status_devices() {
  local udid name state rtid avail lease
  while IFS="$SP_TAB" read -r udid name state rtid avail; do
    case "$name" in "$SP_PREFIX"*) ;; *) continue ;; esac
    lease=null
    if [ -d "$SP_LEASES/$udid" ]; then
      lease="$(jq -c . "$SP_LEASES/$udid/lease.json" 2>/dev/null)"
      [ -n "$lease" ] || lease='{}'
    fi
    jq -nc --arg udid "$udid" --arg name "$name" --arg state "$state" --arg rt "$rtid" \
      --arg cur "$2" --arg fold "$4" --argjson avail "$avail" --argjson lease "$lease" \
      --arg prefix "$SP_PREFIX" --argjson now "$3" '($name | ltrimstr($prefix) | sub("-[0-9]+$"; "")) as $kind | {
        udid: $udid, name: $name,
        kind: $kind,
        state: $state, runtime: $rt,
        current_runtime: ($rt == (if $kind == "foldable" then $fold else $cur end)), available: $avail,
        leased: ($lease != null), lease: $lease,
        lease_expired: (if $lease == null then false else $now >= ($lease.expires_epoch // 0) end)
      }'
  done <<EOF
$1
EOF
}

sp_status() {
  [ "$#" -eq 0 ] || sp_usage "status: unexpected argument '$1'"
  local list rt json orphans="" d
  list="$(sp_devices)" || exit 1
  rt="$(sp_runtime 2>/dev/null)" || rt=""
  json="$(sp_status_devices "$list" "${rt%%"$SP_TAB"*}" "$(sp_now)" "$(jq -r .foldable.runtime.identifier "$SP_DEVICES_JSON")")"
  if [ -d "$SP_LEASES" ]; then
    for d in "$SP_LEASES"/*; do
      [ -d "$d" ] || continue
      printf '%s\n' "$list" | cut -f1 | grep -qx "$(basename "$d")" || orphans="$orphans $(basename "$d")"
    done
  fi
  json="$(printf '%s\n' "$json" | jq -s --arg dir "$SP_DIR" --arg rt "$rt" --argjson max "$SP_MAX" \
    --arg kinds "$SP_KINDS" --arg orphans "$orphans" --arg vm "${MEGURI_SIM_POOL_VM:-}" '{
      pool_dir: $dir, vm: (if $vm == "" then null else $vm end),
      runtime: (if $rt == "" then null else ($rt | split("\t") | {identifier: .[0], version: .[1]}) end),
      max_per_kind: $max,
      kinds: [$kinds | split("\n")[] | select(. != "") | split("|")
        | {kind: .[0], device_type: (if .[1] == "" then null else .[1] end)}],
      devices: [.[] | select(. != null)] | sort_by(.name),
      orphan_leases: [$orphans | split(" ")[] | select(. != "")]
    }')" || exit 1
  jq -r '"pool \(.pool_dir)\(if .vm then " in the test VM \(.vm)" else "" end), runtime \(.runtime.version // "none"), at most \(.max_per_kind) per kind",
    (.devices[] | "  \(.name)  \(.udid)  \(.state)\(if .current_runtime then "" else "  (old runtime)" end)  " +
      (if .lease == null then "free"
       else "leased to \(.lease.holder // "?") (\(.lease.worktree // "?")) until \(.lease.expires_at // "?")" +
         (if .lease_expired then ", expired" else "" end) end)),
    (if (.orphan_leases | length) > 0 then "  leases without a device: \(.orphan_leases | join(" "))" else empty end)' \
    <<<"$json" >&2
  printf '%s\n' "$json"
}

sp_gc() {
  local hours=6 list_only=false list udid name state _rt _avail dir targets="" t deleted="" failed="" rc=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --older-than-hours)
        [ "$#" -ge 2 ] && sp_is_uint "$2" || sp_usage "--older-than-hours needs a whole number of hours"
        hours="$2"
        shift 2
        ;;
      --list)
        list_only=true
        shift
        ;;
      *) sp_usage "gc: unexpected argument '$1'" ;;
    esac
  done
  list="$(sp_devices)" || exit 1
  while IFS="$SP_TAB" read -r udid name state _rt _avail; do
    [ -n "$udid" ] || continue
    case "$name" in
      Meguri-Pool*) continue ;;
      Meguri-*) ;;
      *) continue ;;
    esac
    [ ! -e "$SP_LEASES/$udid" ] || continue
    dir="$SP_DEVICES_DIR/$udid"
    [ -d "$dir" ] || continue
    [ -n "$(find "$dir" -maxdepth 0 -mmin +$((hours * 60)) 2>/dev/null)" ] || continue
    targets="$targets$udid$SP_TAB$name$SP_TAB$state"$'\n'
  done <<EOF
$list
EOF
  while IFS="$SP_TAB" read -r udid name state; do
    [ -n "$udid" ] || continue
    if [ "$list_only" = true ]; then
      sp_log "would delete $name ($udid, $state)"
      continue
    fi
    sp_log "deleting $name ($udid, $state)"
    [ "$state" = Shutdown ] || xcrun simctl shutdown "$udid" >&2 || true
    if xcrun simctl delete "$udid" >&2; then
      deleted="$deleted $udid"
    else
      failed="$failed $udid"
      rc=1
    fi
  done <<EOF
$targets
EOF
  t="$(printf '%s' "$targets" | jq -R 'split("\t") | {udid: .[0], name: .[1], state: .[2]}' | jq -sc .)"
  jq -n --argjson hours "$hours" --argjson list_only "$list_only" --argjson targets "$t" \
    --arg deleted "$deleted" --arg failed "$failed" '{
      older_than_hours: $hours, list_only: $list_only, targets: $targets,
      deleted: [$deleted | split(" ")[] | select(. != "")],
      failed: [$failed | split(" ")[] | select(. != "")]
    }'
  return "$rc"
}

# ---------------------------------------------------------------------------
# Where the pool lives

# sp_route <command> [args]: the pool is the test VM's (U8c). Runs this
# script there over SSH and exits with its code, or returns to run it here
# (no VM and MEGURI_SIM_ALLOW_HOST=1); exits 5 when no VM can run it now (none,
# or it does not answer) and 6 when the VM could not.
sp_route() {
  local rc vm logs
  # shellcheck source=mac-vm-lib.sh
  . "$SP_SCRIPTS/mac-vm-lib.sh"
  MVL_REASON=""
  mvl_probe
  rc=$?
  case "$rc" in
    0) ;;
    2)
      sp_log "$MVL_REASON"
      exit 6
      ;;
    *)
      [ "${MEGURI_SIM_ALLOW_HOST:-}" != 1 ] || return 0
      sp_log "queued: $MVL_REASON; simulators run only in the macOS test VM (docs/process/mac-ui-vm.md), never on this Mac (MEGURI_SIM_ALLOW_HOST=1 uses this Mac's simulators, for a run Antoine starts himself)"
      exit 5
      ;;
  esac
  vm="$(jq -c '.[0]' <<<"$MVL_VMS")"
  if sp_changes_pool "$@" && ! mvl_lease_held "$(mvl_lease_dir "$SP_ROOT" "$(jq -r .name <<<"$vm")")"; then
    sp_via_mg "$@"
  fi
  # The tart log of a VM this starts stays: tart keeps writing to it.
  logs="${TMPDIR:-/tmp}/meguri-sim-pool-vm"
  mkdir -p "$logs" || { sp_log "cannot create $logs"; exit 6; }
  if ! mvl_ensure_ready "$vm" "$logs"; then
    if [ "$MVL_FAIL" = ssh ]; then
      sp_log "queued: $MVL_REASON"
      exit 5
    fi
    sp_log "the test VM could not run the pool: $MVL_REASON"
    exit 6
  fi
  sp_forward "$vm" "$@"
}

# sp_changes_pool <command> [args]: whether the command creates, boots or
# deletes simulators: acquire, and gc but gc --list.
sp_changes_pool() {
  local a
  case "${1:-}" in
    acquire) return 0 ;;
    gc)
      for a in "$@"; do [ "$a" != --list ] || return 1; done
      return 0
      ;;
  esac
  return 1
}

# sp_via_mg <command> [args]: the command under the VM's lease, through
# `tools/mg sim <command>` (which takes it and runs this script again under
# it). Prints what that run printed and exits with its code: 5 when the job
# was queued, 6 when it did not run the pool (the VM's lease stayed busy, the
# VM did not start or answer).
sp_via_mg() {
  local result rc
  if [ -n "${MEGURI_SIM_POOL_VIA_MG:-}" ]; then
    sp_log "tools/mg sim ran the pool without holding the test VM's lease"
    exit 6
  fi
  sp_log "$1 changes the pool in the test VM: running it under the VM's lease (tools/mg sim $1)"
  result="$(MEGURI_SIM_POOL_VIA_MG=1 "$SP_ROOT/tools/mg" sim "$@" </dev/null | tail -n 1)"
  rc="$(jq -r '.details.pool.exit // empty' "$result" 2>/dev/null)"
  if [ -z "$rc" ]; then
    if [ "$(jq -r '.details.queued // false' "$result" 2>/dev/null)" = true ]; then
      sp_log "queued: $(jq -r '.details.queued_parts[0].reason // .message' "$result" 2>/dev/null)"
      exit 5
    fi
    sp_log "the test VM could not run the pool under its lease: $(jq -r '.message // "no result"' "$result" 2>/dev/null || echo "tools/mg sim $1 left no result")"
    exit 6
  fi
  jq -r '.details.pool.output | select(. != "")' "$result"
  exit "$rc"
}

# sp_forward <vm JSON> <command> [args]: this script, the kinds file and the
# caller's description go to a temporary directory in the VM (one per call:
# a script being replaced while bash reads it breaks), and the script runs
# there for the caller (the lease names this Mac's checkout, not a directory
# of the VM). Exits with the VM's exit code; 5 when the VM could not be
# reached or could not start what the script runs there (mvl_unreachable on
# its stderr, which still reaches this one's).
sp_forward() {
  local vm="$1" ssh name envs=(MEGURI_SIM_POOL_IN_VM=1) v json tmp rc
  shift
  ssh="$(jq -r .ssh <<<"$vm")"
  name="$(jq -r .name <<<"$vm")"
  for v in MEGURI_SIM_POOL_DIR MEGURI_SIM_POOL_MAX MEGURI_SIM_POOL_WAIT_SECONDS MEGURI_SIM_POOL_POLL_SECONDS \
    MEGURI_SIM_POOL_SETTLE_SECONDS MEGURI_SIM_POOL_SPRINGBOARD_SECONDS MEGURI_SIM_DEVICES_DIR; do
    [ -z "${!v:-}" ] || envs[${#envs[@]}]="$v=${!v}"
  done
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/meguri-sim-pool.XXXXXX")" || { sp_log "cannot create a temporary directory"; exit 1; }
  jq -n --arg vm "$name" --arg wt "$(sp_checkout)" --arg cwd "$(pwd -P)" --argjson pid "$PPID" \
    '{vm: $vm, worktree: $wt, cwd: $cwd, pid: $pid}' >"$tmp/caller.json" || { rm -rf "$tmp"; sp_log "cannot describe the caller"; exit 1; }
  json="$(basename "$SP_DEVICES_JSON")"
  # Its stderr also goes to a file, read when it fails.
  {
    COPYFILE_DISABLE=1 tar -cf - -C "$SP_SCRIPTS" sim-pool.sh -C "$tmp" caller.json -C "$(dirname "$SP_DEVICES_JSON")" "$json" |
      "$ssh" "d=\$(mktemp -d /tmp/meguri-sim-pool.XXXXXX) || exit 1
tar -C \"\$d\" -xf - || { rm -rf \"\$d\"; exit 1; }
env $(mvl_q "${envs[@]}") MEGURI_SIM_POOL_CALLER=\"\$d/caller.json\" MEGURI_SIM_DEVICES_JSON=\"\$d\"/$(mvl_q "$json") \\
  /bin/bash \"\$d/sim-pool.sh\" $(mvl_q "$@")
rc=\$?
rm -rf \"\$d\"
exit \$rc" 2>&1 1>&3 3>&- | tee "$tmp/stderr" >&2 3>&-
    rc="${PIPESTATUS[1]}"
  } 3>&1
  if [ "$rc" -ne 0 ] && mvl_unreachable "$rc" "$(cat "$tmp/stderr" 2>/dev/null)"; then
    sp_log "queued: the test VM $name could not be reached or could not start the pool's commands (exit $rc$(mvl_last "$tmp/stderr"))"
    rm -rf "$tmp"
    exit 5
  fi
  rm -rf "$tmp"
  exit "$rc"
}

# ---------------------------------------------------------------------------

# Guest posture transaction shares these functions, without public dispatch.
[ "${BASH_SOURCE[0]}" = "$0" ] || return 0

trap sp_on_exit EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
trap 'exit 129' HUP

if ! command -v jq >/dev/null 2>&1; then
  sp_log "jq is required (macOS ships /usr/bin/jq)"
  exit 1
fi
for v in "$SP_MAX" "$SP_WAIT" "$SP_POLL" "$SP_SETTLE" "$SP_SB_SETTLE"; do
  sp_is_uint "$v" || sp_usage "MEGURI_SIM_POOL_* overrides must be whole numbers"
done
SP_KINDS="$(sp_kinds)" && [ -n "$SP_KINDS" ] || { sp_log "cannot read the kinds from $SP_DEVICES_JSON"; exit 1; }

if [ -n "${MEGURI_SIM_POOL_IN_VM:-}" ]; then
  if [ -n "${MEGURI_SIM_POOL_CALLER:-}" ]; then
    MEGURI_SIM_POOL_VM="$(jq -r '.vm // empty' "$MEGURI_SIM_POOL_CALLER")"
    MEGURI_SIM_POOL_WORKTREE="$(jq -r '.worktree // empty' "$MEGURI_SIM_POOL_CALLER")"
    MEGURI_SIM_POOL_CWD="$(jq -r '.cwd // empty' "$MEGURI_SIM_POOL_CALLER")"
    MEGURI_SIM_POOL_PID="$(jq -r '.pid // empty' "$MEGURI_SIM_POOL_CALLER")"
  fi
else
  sp_route "$@"
fi

cmd="${1:-}"
[ "$#" -gt 0 ] && shift
case "$cmd" in
  acquire) sp_acquire "$@" ;;
  renew) sp_renew "$@" ;;
  release) sp_release "$@" ;;
  reset) sp_reset_leased "$@" ;;
  status) sp_status "$@" ;;
  gc) sp_gc "$@" ;;
  "") sp_usage "missing command" ;;
  *) sp_usage "unknown command '$cmd'" ;;
esac
