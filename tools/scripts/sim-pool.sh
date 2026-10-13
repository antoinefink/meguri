#!/bin/bash
# tools/scripts/sim-pool.sh: a shared pool of iOS simulators, created once,
# booted once, kept booted, and leased to one caller at a time.
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
#       bar), leave it booted, drop the lease.
#   sim-pool.sh status
#       JSON on stdout, a table on stderr.
#   sim-pool.sh gc [--older-than-hours N] [--list]
#       Shut down and delete leftover Meguri-* simulators (never Meguri-Pool-*,
#       never leased ones, never other names) whose device directory is older
#       than N hours (default 6). --list prints the targets and deletes nothing.
#
# Pool devices are Meguri-Pool-<kind>-<n> on the newest installed iOS runtime,
# at most MEGURI_SIM_POOL_MAX (3) per kind. Leases live in
# ~/Library/Caches/meguri/sim-pool/leases/<udid>/lease.json, shared by every
# worktree; each lease is a mkdir lock. Claiming, creating, taking over,
# renewing and dropping leases hold the pool lock (pool.lock, mkdir). A lease
# expires after its TTL (default 60 min) and may then be taken over; the
# device is reset first. The lease's pid is informational.
#
# Exit codes: 0 ok, 1 failed, 2 usage, 3 every device stayed leased until
# the wait ran out. Output other than results goes to stderr.
#
# Overrides (self-tests): MEGURI_SIM_POOL_DIR, MEGURI_SIM_POOL_MAX,
# MEGURI_SIM_POOL_WAIT_SECONDS (1200), MEGURI_SIM_POOL_POLL_SECONDS (10),
# MEGURI_SIM_POOL_SETTLE_SECONDS (30), MEGURI_SIM_DEVICES_DIR.
#
# Runs under macOS /bin/bash 3.2 with BSD tools, and with GNU tools in the
# dev container (self-tests).

set -u

# Kinds: <kind>|<device type name>, one per line. Add a line to add a kind.
SP_KINDS='iphone|iPhone 17 Pro
ipad|iPad Pro 13-inch (M5)'

SP_PREFIX=Meguri-Pool-
SP_DIR="${MEGURI_SIM_POOL_DIR:-$HOME/Library/Caches/meguri/sim-pool}"
SP_LEASES="$SP_DIR/leases"
SP_LOCK="$SP_DIR/pool.lock"
SP_MAX="${MEGURI_SIM_POOL_MAX:-3}"
SP_WAIT="${MEGURI_SIM_POOL_WAIT_SECONDS:-1200}"
SP_POLL="${MEGURI_SIM_POOL_POLL_SECONDS:-10}"
SP_SETTLE="${MEGURI_SIM_POOL_SETTLE_SECONDS:-30}"
SP_DEVICES_DIR="${MEGURI_SIM_DEVICES_DIR:-$HOME/Library/Developer/CoreSimulator/Devices}"
SP_DEFAULT_TTL=60
SP_TAB=$'\t'

SP_LOCKED=""
SP_PENDING="" # a lease this run claimed and has not handed out yet

sp_log() { printf 'sim-pool: %s\n' "$*" >&2; }

sp_usage() {
  sp_log "$1"
  cat >&2 <<EOF
usage: sim-pool.sh acquire <$(sp_kind_names | tr ' ' '|')> [--holder <label>] [--ttl-min N]
       sim-pool.sh renew <udid> [--ttl-min N]
       sim-pool.sh release <udid>
       sim-pool.sh status
       sim-pool.sh gc [--older-than-hours N] [--list]
EOF
  exit 2
}

sp_now() { date -u +%s; }

sp_is_uint() { case "$1" in "" | *[!0-9]*) return 1 ;; esac; }

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
    # A holder that died, or one that has held it for 10 minutes (claims take
    # seconds), left a stale lock.
    if { [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null &&
      [ "$(cat "$SP_LOCK/pid" 2>/dev/null)" = "$pid" ]; } ||
      [ -n "$(find "$SP_LOCK" -maxdepth 0 -mmin +10 2>/dev/null)" ]; then
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
  [ -n "$SP_LOCKED" ] && rm -rf "$SP_LOCK"
  SP_LOCKED=""
}

sp_on_exit() {
  [ -z "$SP_PENDING" ] || rm -rf "$SP_PENDING"
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

# sp_write_lease <udid> <kind> <holder> <ttl minutes>
sp_write_lease() {
  local d="$SP_LEASES/$1" now cwd top
  now="$(sp_now)"
  cwd="$(pwd -P)"
  top="$(git rev-parse --show-toplevel 2>/dev/null)" || top="$cwd"
  jq -n --arg udid "$1" --arg kind "$2" --arg holder "$3" --argjson ttl "$4" \
    --arg cwd "$cwd" --arg wt "$top" --argjson pid "$PPID" --argjson now "$now" '{
      udid: $udid, kind: $kind, holder: $holder, worktree: $wt, cwd: $cwd, pid: $pid,
      ttl_min: $ttl, acquired_at: ($now | todate), expires_at: ($now + $ttl * 60 | todate),
      acquired_epoch: $now, expires_epoch: ($now + $ttl * 60)
    }' >"$d/lease.json.tmp" && mv -f "$d/lease.json.tmp" "$d/lease.json"
}

# sp_take <udid> <kind> <holder> <ttl>: lease a device nobody holds (under the pool lock).
sp_take() {
  mkdir "$SP_LEASES/$1" 2>/dev/null || return 1
  SP_PENDING="$SP_LEASES/$1"
  sp_write_lease "$@"
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

# The newest available iOS runtime: identifier<TAB>version.
sp_runtime() {
  local json
  json="$(xcrun simctl list runtimes --json)" || return 1
  jq -r '[.runtimes[] | select(.isAvailable != false and
      ((.platform // "") == "iOS" or (.identifier | test("SimRuntime\\.iOS-"))))]
    | sort_by(.version | split(".") | map(tonumber? // 0)) | last // empty
    | [.identifier, .version] | @tsv' <<<"$json"
}

sp_devicetype_id() {
  local json
  json="$(xcrun simctl list devicetypes --json)" || return 1
  jq -r --arg n "$1" 'first(.devicetypes[] | select(.name == $n) | .identifier) // empty' <<<"$json"
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

# sp_reset <udid>: back to a clean state, still booted.
sp_reset() {
  local udid="$1" apps bid rc=0
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
  local kind="" holder="" ttl="$SP_DEFAULT_TTL" type_name type rt rt_id deadline rc waiting=""
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
  type_name="$(sp_kind_type "$kind")" || sp_usage "acquire: unknown kind '$kind' (kinds: $(sp_kind_names))"
  [ -n "$holder" ] || holder="$(basename "$(git rev-parse --show-toplevel 2>/dev/null || pwd -P)")"
  rt="$(sp_runtime)" || { sp_log "simctl list runtimes failed"; exit 1; }
  [ -n "$rt" ] || { sp_log "no iOS simulator runtime is installed"; exit 1; }
  rt_id="${rt%%"$SP_TAB"*}"
  type="$(sp_devicetype_id "$type_name")" || { sp_log "simctl list devicetypes failed"; exit 1; }
  [ -n "$type" ] || { sp_log "Xcode has no '$type_name' device type"; exit 1; }

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
  if [ "$SP_HOW" = takeover ]; then
    sp_reset "$SP_UDID" || exit 1
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

sp_release() {
  local udid="${1:-}" rc=0
  [ "$#" -le 1 ] || sp_usage "release: unexpected argument '$2'"
  sp_pool_device "$udid"
  if [ ! -d "$SP_LEASES/$udid" ]; then
    sp_log "$SP_NAME ($udid) is not leased; nothing to release"
    return 0
  fi
  if [ "$SP_STATE" != Booted ]; then
    sp_log "$SP_NAME is $SP_STATE; booting it again"
    sp_boot "$udid" "$SP_STATE" && sp_settle "$SP_NAME" || rc=1
  fi
  [ "$rc" -ne 0 ] || sp_reset "$udid" || rc=1
  sp_lock
  rm -rf "${SP_LEASES:?}/$udid"
  sp_unlock
  if [ "$rc" -eq 0 ]; then
    sp_log "released $SP_NAME ($udid), reset and still booted"
  else
    sp_log "released $SP_NAME ($udid), but it is not fully reset"
  fi
  return "$rc"
}

sp_status() {
  [ "$#" -eq 0 ] || sp_usage "status: unexpected argument '$1'"
  local list rt udid name state rtid avail lease now json orphans="" d
  list="$(sp_devices)" || exit 1
  rt="$(sp_runtime 2>/dev/null)" || rt=""
  now="$(sp_now)"
  json="$(
    while IFS="$SP_TAB" read -r udid name state rtid avail; do
      case "$name" in "$SP_PREFIX"*) ;; *) continue ;; esac
      lease=null
      if [ -d "$SP_LEASES/$udid" ]; then
        lease="$(jq -c . "$SP_LEASES/$udid/lease.json" 2>/dev/null)"
        [ -n "$lease" ] || lease='{}'
      fi
      jq -nc --arg udid "$udid" --arg name "$name" --arg state "$state" --arg rt "$rtid" \
        --arg cur "${rt%%"$SP_TAB"*}" --argjson avail "$avail" --argjson lease "$lease" \
        --arg prefix "$SP_PREFIX" --argjson now "$now" '{
          udid: $udid, name: $name,
          kind: ($name | ltrimstr($prefix) | sub("-[0-9]+$"; "")),
          state: $state, runtime: $rt, current_runtime: ($rt == $cur), available: $avail,
          leased: ($lease != null), lease: $lease,
          lease_expired: (if $lease == null then false else $now >= ($lease.expires_epoch // 0) end)
        }'
    done <<EOF
$list
EOF
  )"
  if [ -d "$SP_LEASES" ]; then
    for d in "$SP_LEASES"/*; do
      [ -d "$d" ] || continue
      printf '%s\n' "$list" | cut -f1 | grep -qx "$(basename "$d")" || orphans="$orphans $(basename "$d")"
    done
  fi
  json="$(printf '%s\n' "$json" | jq -s --arg dir "$SP_DIR" --arg rt "$rt" --argjson max "$SP_MAX" \
    --arg kinds "$SP_KINDS" --arg orphans "$orphans" '{
      pool_dir: $dir,
      runtime: (if $rt == "" then null else ($rt | split("\t") | {identifier: .[0], version: .[1]}) end),
      max_per_kind: $max,
      kinds: [$kinds | split("\n")[] | select(. != "") | split("|") | {kind: .[0], device_type: .[1]}],
      devices: [.[] | select(. != null)] | sort_by(.name),
      orphan_leases: [$orphans | split(" ")[] | select(. != "")]
    }')" || exit 1
  jq -r '"pool \(.pool_dir), runtime \(.runtime.version // "none"), at most \(.max_per_kind) per kind",
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

trap sp_on_exit EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
trap 'exit 129' HUP

if ! command -v jq >/dev/null 2>&1; then
  sp_log "jq is required (macOS ships /usr/bin/jq)"
  exit 1
fi
for v in "$SP_MAX" "$SP_WAIT" "$SP_POLL" "$SP_SETTLE"; do
  sp_is_uint "$v" || sp_usage "MEGURI_SIM_POOL_* overrides must be whole numbers"
done

cmd="${1:-}"
[ "$#" -gt 0 ] && shift
case "$cmd" in
  acquire) sp_acquire "$@" ;;
  renew) sp_renew "$@" ;;
  release) sp_release "$@" ;;
  status) sp_status "$@" ;;
  gc) sp_gc "$@" ;;
  "") sp_usage "missing command" ;;
  *) sp_usage "unknown command '$cmd'" ;;
esac
