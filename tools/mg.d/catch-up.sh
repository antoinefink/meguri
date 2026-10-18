# Catch-up of runs the machine's state queued (U19b: R1951 as Antoine amended
# it on 2026-10-17; docs/process/merge.md). Sourced by tools/mg.
#
# A gate run that cannot test because of the machine's state is queued, not
# blocking: the host session is locked or its display sleeps (user step
# host-session), macOS drops the input XCUITest synthesizes (mac-input), or
# there is no test VM (mac-vm). After it fast-forwards, `tools/mg merge`
# appends each queued run of the unit's receipt (job, arguments, commit, user
# steps, parts, unit) to the main checkout's build/mg/catch-up/queue.jsonl.
# `tools/mg catch-up`, in the main checkout, asks `tools/mg doctor` which user
# steps are still missing and re-runs once, on the integration head, every
# queued run whose steps doctor no longer lists as missing, one run per job
# and arguments. Each entry then gets a line with its new state: done (the
# run passed), failed (it failed: a blocked-red escalation for each unit whose
# merge queued it; units that do not depend on it go on) or queued (queued
# again, with the steps it now waits for). An entry whose escalation could not
# be recorded is escalation-pending, with the failed run's result: the next
# catch-up records it before anything else. An escalation counts as recorded
# only once committed: one appended to the ledger but not committed (its
# commit failed) is committed by the next catch-up, under its lock and before
# its clean-tree check, and until then its entries stay escalation-pending.
# An error leaves the entries as
# they were. While a step is missing its entries wait, and the job passes.
# The queue is append-only; the lines of an id, merged in order, are its
# state, and a run's lines are appended as soon as it ends. One catch-up at a
# time works the queue (build/mg/catch-up/lock): another one finds it held
# and does nothing. The orchestrator's watcher runs `tools/mg catch-up` on
# its schedule.

mg_job catch-up host mg_catch_up \
  "Re-run what merges queued because of the machine's state (host-session, mac-input, mac-vm), once tools/mg doctor no longer lists the user step: once per job and arguments, on the integration head, in the main checkout, one catch-up at a time; a run that fails records a blocked-red escalation for the unit that queued it, again on the next catch-up when it could not be recorded (build/mg/catch-up/queue.jsonl)"

# mg_catch_up_queue_path [<checkout>]: the queue of that checkout (default:
# this one).
mg_catch_up_queue_path() { printf '%s/build/mg/catch-up/queue.jsonl\n' "${1:-$MG_ROOT}"; }

# mg_catch_up__append <JSON line>...: appends lines to this checkout's queue.
mg_catch_up__append() {
  local q
  q="$(mg_catch_up_queue_path)"
  mkdir -p "$(dirname "$q")" && printf '%s\n' "$@" >>"$q"
}

# mg_catch_up__now: UTC now (MG_GATE_NOW in the self-tests).
mg_catch_up__now() { printf '%s\n' "${MG_GATE_NOW:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"; }

# The latest real run may already prove this exact clean head and argument
# list (for example, a full unit gate just ran a formerly queued check).
# Resolve latest once: another run can replace the link while we work.
mg_catch_up__passing_result() {
  local job="$1" args="$2" dir head dirty
  case "$job" in "" | *[!a-z0-9-]*) return 1 ;; esac
  head="$(git -C "$MG_ROOT" rev-parse --verify HEAD)" || return 1
  dirty="$(git -C "$MG_ROOT" status --porcelain --untracked-files=all)" || return 1
  [ -z "$dirty" ] || return 1
  dir="$(cd "$MG_ROOT/build/mg/$job/latest" 2>/dev/null && pwd -P)" || return 1
  case "$dir" in "$MG_ROOT/build/mg/$job/"*) ;; *) return 1 ;; esac
  jq -e --arg job "$job" --arg root "$MG_ROOT" --arg head "$head" --arg run "$(basename "$dir")" --argjson args "$args" '
    # Standard metadata is part of the proof, not just the passing status.
    def count: type == "number" and . >= 0 and floor == .;
    def stamp: type == "string" and (try (fromdateiso8601 | type == "number") catch false);
    def only($allowed): (keys - $allowed) == [];
    (keys == (["schema_version", "job", "run_id", "status", "exit_code", "placement", "service",
      "dry_run", "args", "root", "started_at", "finished_at", "duration_ms", "commit",
      "message", "summary", "failures", "artifacts", "user_steps", "details"] | sort))
    and .schema_version == 1 and .job == $job and .root == $root and .args == $args
    and (.args | type == "array" and all(.[]; type == "string"))
    and .run_id == $run and (.run_id | test("^[0-9]{8}T[0-9]{6}Z-[0-9]+$"))
    and (.placement == "host" or .placement == "docker" or .placement == "none")
    and (.service | type == "string" or type == "null")
    and (.started_at | stamp) and (.finished_at | stamp) and (.duration_ms | count)
    and (.message | type == "string")
    and (.commit | type == "object" and keys == ["dirty", "sha"])
    and .commit.sha == $head and .commit.dirty == false and .dry_run == false
    and .status == "pass" and .exit_code == 0 and .failures == []
    and (.summary | type == "object" and has("total") and has("passed") and has("failed") and has("skipped")
      and all(.[]; count))
    and .summary.failed == 0 and (.summary.errors // 0) == 0
    and (.artifacts | type == "array" and all(.[];
      type == "object" and only(["kind", "path", "label"])
      and (.kind | type == "string" and length > 0)
      and (.path | type == "string" and startswith("/"))
      and (if has("label") then (.label | type == "string") else true end)))
    and (.details | type == "object")
    and (.details.queued // false) == false and (.details.blocked // false) == false
    and (.details.queued_parts // []) == []
    and (.user_steps | type == "array" and all(.[];
      type == "object" and only(["id", "state", "required", "title", "how", "doc"])
      and (.id | type == "string" and test("^[a-z0-9-]+$"))
      and (.state == "missing" or .state == "unverified") and (.required | type == "boolean")
      and (.title | type == "string") and (.how | type == "string") and (.doc | type == "string")
      and (.required != true or .state != "missing")))
  ' "$dir/result.json" >/dev/null 2>&1 || return 1
  # A commit or edit during validation invalidates the sampled clean HEAD.
  [ "$(git -C "$MG_ROOT" rev-parse --verify HEAD)" = "$head" ] || return 1
  dirty="$(git -C "$MG_ROOT" status --porcelain --untracked-files=all)" || return 1
  [ -z "$dirty" ] || return 1
  printf '%s/result.json\n' "$dir"
}

# mg_catch_up_enqueue <receipt> <branch> <unit> <tip>: after a merge, one
# queue entry per queued run of its receipt, with the arguments its result
# ran with. Returns 1 when the queue cannot be written.
mg_catch_up_enqueue() {
  local receipt="$1" branch="$2" unit="$3" tip="$4" run args result lines=()
  while IFS= read -r run; do
    result="$(jq -r '.result // ""' <<<"$run")"
    args="$( [ -n "$result" ] && jq -c '.args | arrays' "$result" 2>/dev/null)" || args=""
    [ -n "$args" ] || args="$(jq -c .args <<<"$run")"
    lines[${#lines[@]}]="$(jq -c --argjson args "$args" --arg at "$(mg_catch_up__now)" --arg merge "$MG_RUN_ID" \
      --arg tip "$tip" --arg unit "$unit" --arg branch "$branch" '{id: ("Q-" + $merge + "-" + .id), state: "queued",
        at: $at, job, args: $args, run: .id, commit: $tip, unit: $unit, branch: $branch, merge: $merge, result,
        user_steps, parts}' <<<"$run")" || return 1
  done < <(jq -c '.queued[]?' "$receipt")
  [ "${#lines[@]}" -gt 0 ] || return 0
  mg_catch_up__append "${lines[@]}"
}

# mg_catch_up__entries: every entry of the queue with its current state, in
# the order the entries came. Fails (jq's message on stderr) when the queue
# cannot be read or a line is not a JSON object with a string id: a truncated
# or broken line must stop catch-up, never read as an empty queue.
mg_catch_up__entries() {
  local q
  q="$(mg_catch_up_queue_path)"
  [ -e "$q" ] || return 0
  jq -c -s 'reduce .[] as $l ({order: [], by: {}};
      if ($l | type) != "object" or ($l.id | type) != "string" then error("not a queue entry: \($l | tojson | .[0:200])")
      else (if .by[$l.id] then . else .order += [$l.id] end) | .by[$l.id] += $l end)
    | . as $q | $q.order[] | $q.by[.]' "$q"
}

mg_catch_up__error() {
  mg_failure "$1" "$2" "${3:-$MG_DEFAULT_REPRO}"
  mg_message "catch-up: $2"
  mg_status error
}

# mg_catch_up__lock_path: the lock one catch-up holds while it works the queue.
mg_catch_up__lock_path() { printf '%s/lock\n' "$(dirname "$(mg_catch_up_queue_path)")"; }

# mg_catch_up__lock: takes the lock, a lease as mg__lease_try takes it (a
# holder that died leaves a stale lock, which is removed). Its holder.json
# names the run. Returns 1 after recording the result when another catch-up
# holds it (pass: that run works the queue) or it cannot be taken (error).
mg_catch_up__lock() {
  local lock tries=0 rc
  lock="$(mg_catch_up__lock_path)"
  if ! mkdir -p "$(dirname "$lock")"; then
    mg_catch_up__error lock "cannot create $(dirname "$lock")"
    return 1
  fi
  while :; do
    mg__lease_try "$lock" "catch-up lock"
    rc=$?
    case "$rc" in
      0)
        mg_on_exit "mg__host_lease_release '$lock' $$"
        return 0
        ;;
      1)
        mg_detail busy "$(jq -c . <<<"${MG_LEASE_HOLDER:-null}" 2>/dev/null || echo null)"
        mg_message "catch-up: another catch-up ($(jq -r '"\(.run_id // "?"), pid \(.pid // "?"), since \(.started_at // "?")"' <<<"${MG_LEASE_HOLDER:-null}" 2>/dev/null)) is working the queue; this run did nothing"
        mg_status pass
        return 1
        ;;
    esac
    tries=$((tries + 1))
    if [ "$tries" -ge 10 ]; then
      mg_catch_up__error lock "could not take $lock (its mutex $lock.mutex stayed busy)"
      return 1
    fi
    [ "$rc" -eq 2 ] || mg_sleep 1
  done
}

# mg_catch_up__settle: commits what a store appended to the main checkout's
# escalation ledger but could not commit (mg_ledger_settle): such lines would
# keep every later catch-up at its clean-tree check, and an escalation counts
# as recorded only once it is committed. Returns 1 (MG_SESSION_MESSAGE) when
# that fails.
mg_catch_up__settle() {
  mg_ledger_settle "$MG_ROOT" "$MG_ESCALATIONS_FILE" \
    'if length == 1 and .[0].state == "open" then "Record escalation \(.[0].id) (\(.[0].kind), \(.[0].unit))"
     else "Record escalation lines \(map("\(.id) (\(.state))") | join(", "))" end'
}

# mg_catch_up__escalate <unit> <entries> <result> <head> <units>: the
# blocked-red escalation of a re-run that failed, for one unit whose merge
# queued it (<entries>: that unit's entries of the run, each with job, args
# and user_steps; <units>: every unit that queued it). An escalation HEAD's
# ledger already holds for that unit and result is reused; one an earlier
# run appended but could not commit is committed first (mg_catch_up__settle)
# and never counts before. Sets MG_ESCALATION_ID; returns 1
# (MG_SESSION_MESSAGE) when it could not be recorded. Call it as a plain
# command.
mg_catch_up__escalate() {
  local u="$1" entries="$2" result="$3" head="$4" units="$5" cmd existing
  MG_ESCALATION_ID=""
  cmd="tools/mg $(jq -r '.[0] | [.job] + .args | join(" ")' <<<"$entries")"
  mg_catch_up__settle || return 1
  existing="$(git -C "$MG_ROOT" show "HEAD:$MG_ESCALATIONS_FILE" 2>/dev/null | jq -r -s --arg u "$u" --arg r "$result" \
    'map(select(.state == "open" and .kind == "blocked-red" and .unit == $u and .detail.result == $r)) | .[0].id // empty')"
  if [ -n "$existing" ]; then
    MG_ESCALATION_ID="$existing"
    return 0
  fi
  mg_escalation_add "$MG_ROOT" blocked-red "$u" \
    "$cmd failed on $(git -C "$MG_ROOT" rev-parse --short "$head") when tools/mg catch-up ran it again; the merge of $u had queued it ($(jq -r '[.[].user_steps[]?] | unique | join(", ")' <<<"$entries")): fix it or revert" \
    "$(jq -c --arg dir "$(dirname "$result")" --arg r "$result" --arg head "$head" --argjson units "$units" \
      '{job: .[0].job, args: .[0].args, run_dir: $dir, result: $r, head: $head, entries: map(.id), units: $units}' <<<"$entries")" \
    "tools/mg catch-up $MG_RUN_ID"
}

# mg_catch_up__record <JSON line>...: appends a run's state lines at once, so
# that what the run did survives a later failure or a cancel. Returns 1 after
# recording the error.
mg_catch_up__record() {
  [ "$#" -gt 0 ] || return 0
  mg_catch_up__append "$@" && return 0
  mg_failure queue "could not append to $(mg_catch_up_queue_path): $(printf '%s\n' "$@" | jq -r '.id + " " + .state' | paste -sd ',' -)"
  mg_message "catch-up: could not append to $(mg_catch_up_queue_path)"
  mg_status error
  return 1
}

# tools/mg catch-up
mg_catch_up() {
  [ "$#" -eq 0 ] || { mg_catch_up__error usage "usage: tools/mg catch-up (no arguments)" "tools/mg --list"; return 0; }
  if ! mg_gate__in_main; then
    mg_catch_up__error not-main "tools/mg catch-up runs in the main checkout ($(mg_gate__main_checkout)), on the integration head"
    return 0
  fi
  # Selection, runs and state lines under one lock: two catch-ups at once
  # would run the same entries and escalate them twice.
  mg_catch_up__lock || return 0
  mg_catch_up__checked && mg_catch_up__work
  mg__host_lease_release "$(mg_catch_up__lock_path)" $$
}

# mg_catch_up__checked: escalations an earlier run appended but could not
# commit are committed first (mg_catch_up__settle); then the main checkout
# must have no other uncommitted or untracked change. Returns 1 after
# recording the error.
mg_catch_up__checked() {
  if ! mg_catch_up__settle; then
    mg_catch_up__error escalation-ledger "${MG_SESSION_MESSAGE:-the escalation ledger could not be settled}; nothing ran, and escalation-pending entries stay so" \
      "git -C $MG_ROOT diff HEAD -- $MG_ESCALATIONS_FILE"
    return 1
  fi
  local dirty
  if ! dirty="$(git -C "$MG_ROOT" status --porcelain --untracked-files=all)" || [ -n "$dirty" ]; then
    mg_catch_up__error main-dirty "the main checkout has uncommitted or untracked changes: a run would not test the integration head" "git status"
    return 1
  fi
}

mg_catch_up__work() {
  local entries
  if ! entries="$(mg_catch_up__entries 2>"$MG_RUN_DIR/queue-error.txt")"; then
    mg_artifact queue "$(mg_catch_up_queue_path)" "catch-up queue"
    mg_catch_up__error queue-unreadable "$(mg_catch_up_queue_path) cannot be read ($(head -c 400 "$MG_RUN_DIR/queue-error.txt" | tr '\n' ' ' | sed 's/ *$//')); nothing ran: repair or remove the broken line, then run again" \
      "jq -c . $(mg_catch_up_queue_path)"
    return 0
  fi

  local passed=0 failed=0 errors=0 requeued=0 reused_count=0 escalated=0 unrecorded=0 runs='[]' escalations='[]'
  local group u ids lines=() id result head units record
  # Escalations an earlier run could not record: recorded first, without a
  # re-run (the failed run's result is in the entry).
  while IFS= read -r group <&4; do
    [ -n "$group" ] || continue
    u="$(jq -r '.[0].unit' <<<"$group")"
    result="$(jq -r '.[0].rerun' <<<"$group")"
    head="$(jq -r '.[0].head' <<<"$group")"
    units="$(jq -c '.[0].units // [.[0].unit]' <<<"$group")"
    if mg_catch_up__escalate "$u" "$group" "$result" "$head" "$units"; then
      escalated=$((escalated + 1))
      escalations="$(jq -c --arg u "$u" --arg e "$MG_ESCALATION_ID" --argjson g "$group" '. + [{unit: $u, escalation: $e, entries: ($g | map(.id)), late: true}]' <<<"$escalations")"
      mg_failure "rerun:$(jq -r '.[0] | [.job] + .args | join(" ")' <<<"$group")" \
        "tools/mg $(jq -r '.[0] | [.job] + .args | join(" ")' <<<"$group") had failed on $head ($result); the blocked-red escalation for $u that an earlier catch-up could not record is now $MG_ESCALATION_ID" \
        "tools/mg $(jq -r '.[0] | [.job] + .args | join(" ")' <<<"$group")"
      lines=()
      while IFS= read -r id; do
        lines[${#lines[@]}]="$(jq -nc --arg id "$id" --arg e "$MG_ESCALATION_ID" --arg run "$MG_RUN_ID" --arg at "$(mg_catch_up__now)" \
          '{id: $id, state: "failed", escalation: $e, at: $at, catch_up: $run}')"
      done < <(jq -r '.[].id' <<<"$group")
      mg_catch_up__record "${lines[@]}" || return 0
    else
      unrecorded=$((unrecorded + 1))
      mg_failure "escalation:$u" "recording the blocked-red escalation for $u failed again: ${MG_SESSION_MESSAGE:-see log.txt}; its entries stay escalation-pending"
    fi
  done 4< <(jq -c -s '[.[] | select(.state == "escalation-pending")] | group_by([.rerun, .unit]) | .[]' <<<"$entries")

  local pending
  pending="$(jq -c 'select(.state == "queued")' <<<"$entries")"
  local waiting='[]' groups="" missing='[]'
  if [ -n "$pending" ]; then
    # Which user steps are still missing.
    local doctor
    doctor="$("$MG_TOOLS/mg" doctor </dev/null 2>/dev/null | tail -n 1)"
    if [ ! -f "$doctor" ] || [ "$(jq -r .status "$doctor" 2>/dev/null)" = error ] ||
      ! missing="$(jq -c '[.user_steps[] | select(.state == "missing") | .id] | unique' "$doctor" 2>/dev/null)"; then
      mg_catch_up__error doctor "tools/mg doctor gave no usable result, so no user step is known to be clear; nothing ran" "tools/mg doctor"
      return 0
    fi
    mg_artifact doctor "$doctor" "tools/mg doctor"
    waiting="$(jq -c -s --argjson m "$missing" '[.[] | select(any(.user_steps[]?; . as $s | $m | index($s) != null))]' <<<"$pending")"
    groups="$(jq -c -s '[.[]]
      | reduce .[] as $e ({order: [], by: {}}; ([$e.job, $e.args] | tojson) as $k
          | (if .by[$k] then . else .order += [$k] end) | .by[$k] += [$e])
      | . as $g | $g.order[] | $g.by[.]' <<<"$pending")"
  fi

  local job args=() a out status queued failed_units reused
  # The groups come on fd 3, so nothing a run reads from stdin can take them.
  while IFS= read -r group <&3; do
    [ -n "$group" ] || continue
    job="$(jq -r '.[0].job' <<<"$group")"
    reused=false
    if result="$(mg_catch_up__passing_result "$job" "$(jq -c '.[0].args' <<<"$group")")"; then
      reused=true
      reused_count=$((reused_count + 1))
      waiting="$(jq -c --argjson ids "$(jq -c 'map(.id)' <<<"$group")" 'map(select(.id as $id | $ids | index($id) == null))' <<<"$waiting")"
    else
      group="$(jq -c --argjson m "$missing" 'map(select(any(.user_steps[]?; . as $s | $m | index($s) != null) | not))' <<<"$group")"
      [ "$(jq length <<<"$group")" -gt 0 ] || continue
      # Reuse may have been refused because a concurrent edit appeared.
      # Never fall through to a fresh run on that dirty checkout.
      mg_catch_up__checked || return 0
    fi
    args=()
    while IFS= read -r -d '' a; do args[${#args[@]}]="$a"; done < <(jq -j '.[0].args[] | . + "\u0000"' <<<"$group")
    ids="$(jq -c 'map(.id)' <<<"$group")"
    units="$(jq -c 'map(.unit) | unique' <<<"$group")"
    if [ "$reused" = true ]; then
      echo "catch-up: reuse passing tools/mg $job${args[0]+ ${args[*]}} result $result (queued by $(jq -r 'join(", ")' <<<"$units"))" >&2
    else
      echo "catch-up: tools/mg $job${args[0]+ ${args[*]}} (queued by $(jq -r 'join(", ")' <<<"$units"))" >&2
      out="$("$MG_TOOLS/mg" "$job" ${args[0]+"${args[@]}"} </dev/null)"
      result="$(printf '%s\n' "$out" | tail -n 1)"
    fi
    [ -f "$result" ] || result=""
    status="$( [ -n "$result" ] && jq -r .status "$result" 2>/dev/null)"
    queued="$( [ -n "$result" ] && jq -r '.details.queued == true' "$result" 2>/dev/null)"
    head="$( [ -n "$result" ] && jq -r '.commit.sha // ""' "$result" 2>/dev/null)"
    [ -n "$head" ] || head="$(git -C "$MG_ROOT" rev-parse -q --verify HEAD)"
    echo "catch-up: ${status:-no result}" >&2
    [ -z "$result" ] || mg_artifact result "$result" "tools/mg $job${args[0]+ ${args[*]}}"
    record="$(jq -nc --arg r "$result" --arg st "${status:-}" --arg head "$head" --arg run "$MG_RUN_ID" --arg at "$(mg_catch_up__now)" --argjson reused "$reused" \
      '{at: $at, catch_up: $run, head: $head, rerun: (if $r == "" then null else $r end), rerun_status: (if $st == "" then null else $st end), reused: $reused}')"
    lines=()
    case "$status:$queued" in
      pass:* | warn:false)
        passed=$((passed + 1))
        while IFS= read -r id; do
          lines[${#lines[@]}]="$(jq -c --arg id "$id" '. + {id: $id, state: "done"}' <<<"$record")"
        done < <(jq -r '.[]' <<<"$ids")
        ;;
      warn:true)
        # Queued again: the run waits for what keeps it from testing now.
        requeued=$((requeued + 1))
        while IFS= read -r id; do
          lines[${#lines[@]}]="$(jq -c --arg id "$id" --slurpfile r "$result" '. + {id: $id, state: "queued",
            parts: ([$r[0].details.queued_parts[]? | objects | select(.user_step)])}
            | .user_steps = ((.parts | map(.user_step) | unique) as $p
                | if ($p | length) > 0 then $p else [$r[0].user_steps[]? | select(.state == "missing") | .id] end)' <<<"$record")"
        done < <(jq -r '.[]' <<<"$ids")
        ;;
      fail:*)
        failed=$((failed + 1))
        # Per unit: its escalation, or the reason it could not be recorded.
        failed_units='{}'
        while IFS= read -r u <&4; do
          if mg_catch_up__escalate "$u" "$(jq -c --arg u "$u" 'map(select(.unit == $u))' <<<"$group")" "$result" "$head" "$units"; then
            escalated=$((escalated + 1))
            failed_units="$(jq -c --arg u "$u" --arg e "$MG_ESCALATION_ID" '. + {($u): {escalation: $e}}' <<<"$failed_units")"
            escalations="$(jq -c --arg u "$u" --arg e "$MG_ESCALATION_ID" --argjson g "$group" '. + [{unit: $u, escalation: $e, entries: [$g[] | select(.unit == $u) | .id]}]' <<<"$escalations")"
          else
            unrecorded=$((unrecorded + 1))
            failed_units="$(jq -c --arg u "$u" --arg m "${MG_SESSION_MESSAGE:-see log.txt}" '. + {($u): {error: $m}}' <<<"$failed_units")"
            mg_failure "escalation:$u" "recording the blocked-red escalation for $u failed: ${MG_SESSION_MESSAGE:-see log.txt}; its entries are escalation-pending, and the next tools/mg catch-up records it"
          fi
        done 4< <(jq -r '.[]' <<<"$units")
        mg_failure "rerun:$job${args[0]+ ${args[*]}}" \
          "tools/mg $job${args[0]+ ${args[*]}} failed on $head; queued by $(jq -r 'join(", ")' <<<"$units"); escalations: $(jq -r 'to_entries | map("\(.value.escalation // "not recorded") (\(.key))") | join(", ")' <<<"$failed_units")" \
          "tools/mg $job${args[0]+ ${args[*]}}"
        while IFS= read -r id; do
          lines[${#lines[@]}]="$(jq -c --arg id "$id" --argjson f "$failed_units" --argjson g "$group" --argjson units "$units" '
            $f[first($g[] | select(.id == $id) | .unit)] as $u
            | . + {id: $id} + (if $u.escalation then {state: "failed", escalation: $u.escalation}
                else {state: "escalation-pending", escalation_error: $u.error, units: $units} end)' <<<"$record")"
        done < <(jq -r '.[]' <<<"$ids")
        ;;
      *)
        # An error (or no result) says nothing about the unit: the entries
        # stay queued, and this run is an error.
        errors=$((errors + 1))
        mg_failure "rerun:$job${args[0]+ ${args[*]}}" "tools/mg $job${args[0]+ ${args[*]}} ended ${status:-without a result}; its entries stay queued" \
          "tools/mg $job${args[0]+ ${args[*]}}"
        ;;
    esac
    runs="$(jq -c --arg job "$job" --argjson args "$(jq -c '.[0].args' <<<"$group")" --arg st "${status:-}" --arg r "$result" \
      --arg head "$head" --argjson ids "$ids" --argjson units "$units" --argjson reused "$reused" \
      '. + [{job: $job, args: $args, status: (if $st == "" then null else $st end), result: (if $r == "" then null else $r end),
        commit: $head, entries: $ids, units: $units, reused: $reused}]' <<<"$runs")"
    mg_catch_up__record ${lines[0]+"${lines[@]}"} || return 0
  done 3<<<"$groups"

  mg_detail runs "$runs"
  mg_detail waiting "$(jq -c 'map({id, unit, job, args, user_steps})' <<<"$waiting")"
  [ "$escalations" = '[]' ] || mg_detail escalations "$escalations"
  mg_count total "$(jq length <<<"$runs")"
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count errors "$errors"
  mg_count queued "$requeued"
  mg_count reused "$reused_count"
  mg_count waiting "$(jq length <<<"$waiting")"
  mg_count escalations "$escalated"
  mg_count escalation_errors "$unrecorded"
  if [ -z "$pending" ] && [ "$escalated" -eq 0 ] && [ "$unrecorded" -eq 0 ]; then
    mg_message "catch-up: nothing is queued"
  else
    mg_message "catch-up: $(jq length <<<"$runs") run(s): $passed passed ($reused_count reused), $failed failed, $requeued queued again, $errors error(s); waiting: $(jq -r 'if length == 0 then "none" else "\(length) (\([.[].user_steps[]] | unique | join(", ")))" end' <<<"$waiting"); escalations recorded: $escalated$([ "$unrecorded" -eq 0 ] || echo "; not recorded: $unrecorded (escalation-pending)")"
  fi
  # A blocked-red escalation this run recorded fails it, also one an earlier
  # run could not record; one still unrecorded is an error.
  if [ "$errors" -gt 0 ] || [ "$unrecorded" -gt 0 ]; then
    mg_status error
  elif [ "$failed" -gt 0 ] || [ "$escalated" -gt 0 ]; then
    mg_status fail
  else
    mg_status pass
  fi
}
