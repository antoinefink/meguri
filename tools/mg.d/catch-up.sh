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
# again, with the steps it now waits for). An error leaves the entries as
# they were. While a step is missing its entries wait, and the job passes.
# The queue is append-only; the lines of an id, merged in order, are its
# state. The orchestrator's watcher runs `tools/mg catch-up` on its schedule.

mg_job catch-up host mg_catch_up \
  "Re-run what merges queued because of the machine's state (host-session, mac-input, mac-vm), once tools/mg doctor no longer lists the user step: once per job and arguments, on the integration head, in the main checkout; a run that fails records a blocked-red escalation for the unit that queued it (build/mg/catch-up/queue.jsonl)"

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
# the order the entries came.
mg_catch_up__entries() {
  local q
  q="$(mg_catch_up_queue_path)"
  [ -s "$q" ] || return 0
  jq -c -s 'reduce .[] as $l ({order: [], by: {}}; (if .by[$l.id] then . else .order += [$l.id] end) | .by[$l.id] += $l)
    | . as $q | $q.order[] | $q.by[.]' "$q"
}

mg_catch_up__error() {
  mg_failure "$1" "$2" "${3:-$MG_DEFAULT_REPRO}"
  mg_message "catch-up: $2"
  mg_status error
}

# tools/mg catch-up
mg_catch_up() {
  [ "$#" -eq 0 ] || { mg_catch_up__error usage "usage: tools/mg catch-up (no arguments)" "tools/mg --list"; return 0; }
  if ! mg_gate__in_main; then
    mg_catch_up__error not-main "tools/mg catch-up runs in the main checkout ($(mg_gate__main_checkout)), on the integration head"
    return 0
  fi
  if ! git -C "$MG_ROOT" diff --quiet || ! git -C "$MG_ROOT" diff --cached --quiet; then
    mg_catch_up__error main-dirty "the main checkout has uncommitted changes to tracked files: a run would not test the integration head" "git status"
    return 0
  fi
  local pending
  pending="$(mg_catch_up__entries | jq -c 'select(.state == "queued")')"
  if [ -z "$pending" ]; then
    mg_count total 0
    mg_message "catch-up: nothing is queued"
    mg_status pass
    return 0
  fi

  # Which user steps are still missing.
  local doctor missing
  doctor="$("$MG_TOOLS/mg" doctor </dev/null 2>/dev/null | tail -n 1)"
  if [ ! -f "$doctor" ] || [ "$(jq -r .status "$doctor" 2>/dev/null)" = error ] ||
    ! missing="$(jq -c '[.user_steps[] | select(.state == "missing") | .id] | unique' "$doctor" 2>/dev/null)"; then
    mg_catch_up__error doctor "tools/mg doctor gave no usable result, so no user step is known to be clear; nothing ran" "tools/mg doctor"
    return 0
  fi
  mg_artifact doctor "$doctor" "tools/mg doctor"
  local waiting groups
  waiting="$(jq -c -s --argjson m "$missing" '[.[] | select(any(.user_steps[]?; . as $s | $m | index($s) != null))]' <<<"$pending")"
  groups="$(jq -c -s --argjson m "$missing" '[.[] | select(any(.user_steps[]?; . as $s | $m | index($s) != null) | not)]
    | reduce .[] as $e ({order: [], by: {}}; ([$e.job, $e.args] | tojson) as $k
        | (if .by[$k] then . else .order += [$k] end) | .by[$k] += [$e])
    | . as $g | $g.order[] | $g.by[.]' <<<"$pending")"
  mg_detail waiting "$(jq -c 'map({id, unit, job, args, user_steps})' <<<"$waiting")"

  local group job args=() a out result status queued head ids units runs='[]' lines=() id u record
  local passed=0 failed=0 errors=0 requeued=0
  # The groups come on fd 3, so nothing a run reads from stdin can take them.
  while IFS= read -r group <&3; do
    [ -n "$group" ] || continue
    job="$(jq -r '.[0].job' <<<"$group")"
    args=()
    while IFS= read -r -d '' a; do args[${#args[@]}]="$a"; done < <(jq -j '.[0].args[] | . + "\u0000"' <<<"$group")
    ids="$(jq -c 'map(.id)' <<<"$group")"
    units="$(jq -c 'map(.unit) | unique' <<<"$group")"
    echo "catch-up: tools/mg $job${args[0]+ ${args[*]}} (queued by $(jq -r 'join(", ")' <<<"$units"))" >&2
    out="$("$MG_TOOLS/mg" "$job" ${args[0]+"${args[@]}"} </dev/null)"
    result="$(printf '%s\n' "$out" | tail -n 1)"
    [ -f "$result" ] || result=""
    status="$( [ -n "$result" ] && jq -r .status "$result" 2>/dev/null)"
    queued="$( [ -n "$result" ] && jq -r '.details.queued == true' "$result" 2>/dev/null)"
    head="$( [ -n "$result" ] && jq -r '.commit.sha // ""' "$result" 2>/dev/null)"
    [ -n "$head" ] || head="$(git -C "$MG_ROOT" rev-parse -q --verify HEAD)"
    echo "catch-up: ${status:-no result}" >&2
    [ -z "$result" ] || mg_artifact result "$result" "tools/mg $job${args[0]+ ${args[*]}}"
    record="$(jq -nc --arg r "$result" --arg st "${status:-}" --arg head "$head" --arg run "$MG_RUN_ID" --arg at "$(mg_catch_up__now)" \
      '{at: $at, catch_up: $run, head: $head, rerun: (if $r == "" then null else $r end), rerun_status: (if $st == "" then null else $st end)}')"
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
        local escalations='{}'
        while IFS= read -r u; do
          if mg_escalation_add "$MG_ROOT" blocked-red "$u" \
            "tools/mg $job${args[0]+ ${args[*]}} failed on $(git -C "$MG_ROOT" rev-parse --short "$head") when tools/mg catch-up ran it again; the merge of $u had queued it ($(jq -r --arg u "$u" '[.[] | select(.unit == $u) | .user_steps[]] | unique | join(", ")' <<<"$group")): fix it or revert" \
            "$(jq -c --arg u "$u" --arg job "$job" --arg dir "$(dirname "$result")" --arg r "$result" --arg head "$head" \
              '{job: $job, args: .[0].args, run_dir: $dir, result: $r, head: $head, entries: [.[] | select(.unit == $u) | .id],
                units: (map(.unit) | unique)}' <<<"$group")" "tools/mg catch-up $MG_RUN_ID"; then
            escalations="$(jq -c --arg u "$u" --arg e "$MG_ESCALATION_ID" '. + {($u): $e}' <<<"$escalations")"
          else
            mg_failure "escalation:$u" "recording the blocked-red escalation for $u failed: $MG_SESSION_MESSAGE"
          fi
        done < <(jq -r '.[]' <<<"$units")
        mg_failure "rerun:$job${args[0]+ ${args[*]}}" \
          "tools/mg $job${args[0]+ ${args[*]}} failed on $head; queued by $(jq -r 'join(", ")' <<<"$units"); escalations: $(jq -r 'to_entries | map("\(.value) (\(.key))") | join(", ")' <<<"$escalations")" \
          "tools/mg $job${args[0]+ ${args[*]}}"
        while IFS= read -r id; do
          lines[${#lines[@]}]="$(jq -c --arg id "$id" --argjson e "$escalations" --argjson g "$group" \
            '. + {id: $id, state: "failed", escalation: $e[first($g[] | select(.id == $id) | .unit)]}' <<<"$record")"
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
      --arg head "$head" --argjson ids "$ids" --argjson units "$units" \
      '. + [{job: $job, args: $args, status: (if $st == "" then null else $st end), result: (if $r == "" then null else $r end),
        commit: $head, entries: $ids, units: $units}]' <<<"$runs")"
  done 3<<<"$groups"
  if [ "${#lines[@]}" -gt 0 ] && ! mg_catch_up__append "${lines[@]}"; then
    mg_catch_up__error queue "could not append to $(mg_catch_up_queue_path)"
    return 0
  fi
  mg_detail runs "$runs"
  mg_count total "$(jq length <<<"$runs")"
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count errors "$errors"
  mg_count queued "$requeued"
  mg_count waiting "$(jq length <<<"$waiting")"
  mg_message "catch-up: $(jq length <<<"$runs") run(s): $passed passed, $failed failed, $requeued queued again, $errors error(s); waiting: $(jq -r 'if length == 0 then "none" else "\(length) (\([.[].user_steps[]] | unique | join(", ")))" end' <<<"$waiting")"
  if [ "$errors" -gt 0 ]; then
    mg_status error
  elif [ "$failed" -gt 0 ]; then
    mg_status fail
  else
    mg_status pass
  fi
}
