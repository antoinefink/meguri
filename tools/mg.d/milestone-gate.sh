# Milestone gate (U240; docs/process/milestones.yaml, docs/process/tiers.md).
# Sourced by tools/mg.
#
# `tools/mg milestone-gate <M0..M5>` evaluates one milestone with
# tools/scripts/gates/milestone_gate.py: the results that `tools/mg milestone
# <M>` (U19) leaves in build/mg, all on one gate commit; unit merges and open
# escalations; the user-step evidence committed at HEAD (setup steps read a
# fresh `tools/mg doctor`); deferrals; the global conditions. Pass, fail, or
# warn with details.outcome awaiting_user when only Antoine's evidence is
# missing. It fails outside the main checkout. A pass writes
# docs/process/milestone-records/<M>.json: commit it.
# `--check` validates the manifest against tools/gate/tiers.yaml and the
# plan's Delivery Milestones table and Unit Index (a T0 run).

mg_job milestone-gate host mg_milestone_gate \
  "Milestone gate (U240): <M0..M5> evaluates docs/process/milestones.yaml (suite results on one gate commit, unit merges and escalations, user-step evidence at HEAD, deferrals, global conditions): pass, fail, or warn when awaiting Antoine; a pass writes docs/process/milestone-records/<M>.json | --check: the manifest against tiers.yaml and the plan"

MG_MILESTONES_FILE="docs/process/milestones.yaml"
MG_MILESTONE_GATE_PY="tools/scripts/gates/milestone_gate.py"

mg_milestone_gate__python() { python3 -B -I "$MG_ROOT/$MG_MILESTONE_GATE_PY" "$@"; }

# mg_milestone_gate__load <out>: milestones.yaml as JSON. PyYAML is in the dev
# container; a python3 that has it reads the file here. Call it after
# mg_gate__load, which checks Docker and builds the dev image when needed.
# Returns 1 after recording the errors.
mg_milestone_gate__load() {
  local out="$1" errors="$MG_RUN_DIR/manifest-errors.json" rel rc
  if python3 -c 'import yaml' >/dev/null 2>&1; then
    mg_milestone_gate__python load --manifest "$MG_ROOT/$MG_MILESTONES_FILE" --out "$out" >"$errors"
    rc=$?
  else
    rel="${MG_RUN_DIR#"$MG_ROOT"/}"
    mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev \
      python3 -B -I "/work/$MG_MILESTONE_GATE_PY" load --manifest "/work/$MG_MILESTONES_FILE" \
      --out "/work/$rel/$(basename "$out")" </dev/null >"$errors"
    rc=$?
  fi
  if [ "$rc" -ne 0 ] || [ ! -s "$out" ]; then
    if jq -e '.errors | length > 0' "$errors" >/dev/null 2>&1; then
      jq -c --arg r "tools/mg milestone-gate --check" '.errors[] | {id: "manifest", message: ., repro: $r}' \
        "$errors" >>"$MG_STATE/failures.jsonl"
    else
      mg_failure manifest "could not read $MG_MILESTONES_FILE (exit $rc); see log.txt" "tools/mg milestone-gate --check"
    fi
    mg_message "milestone-gate: could not read $MG_MILESTONES_FILE"
    mg_status error
    return 1
  fi
}

mg_milestone_gate__usage() {
  mg_failure usage "usage: tools/mg milestone-gate <M0..M5> | --check; $1" "tools/mg --list"
  mg_message "milestone-gate: $1"
  mg_status error
}

mg_milestone_gate() {
  local which="${1:-}" config="$MG_RUN_DIR/config.json" manifest="$MG_RUN_DIR/manifest.json" rc
  [ "$#" -eq 1 ] || { mg_milestone_gate__usage "one argument is required"; return 0; }
  case "$which" in
    M[0-5] | --check) ;;
    *) mg_milestone_gate__usage "unknown argument '$which'"; return 0 ;;
  esac
  mg_gate__load "$config" || return 0
  mg_milestone_gate__load "$manifest" || return 0
  mg_artifact manifest "$manifest" "the manifest as JSON"

  if [ "$which" = --check ]; then
    local report="$MG_RUN_DIR/check.json" n
    mg_milestone_gate__python check --manifest "$manifest" --config "$config" --root "$MG_ROOT" >"$report"
    rc=$?
    if [ "$rc" -gt 1 ] || ! jq -e '.errors | type == "array"' "$report" >/dev/null 2>&1; then
      mg_failure check "the manifest check did not run (exit $rc); see log.txt"
      mg_message "milestone-gate --check: the check did not run"
      mg_status error
      return 0
    fi
    jq -c '.errors[] | {id: "manifest", message: ., repro: "tools/mg milestone-gate --check"}' \
      "$report" >>"$MG_STATE/failures.jsonl"
    n="$(jq '.errors | length' "$report")"
    mg_count total 1
    mg_count passed $((n == 0 ? 1 : 0))
    mg_count failed $((n == 0 ? 0 : 1))
    mg_count problems "$n"
    if [ "$n" -eq 0 ]; then
      mg_message "milestone-gate --check: $MG_MILESTONES_FILE is valid and mirrors the plan's Delivery Milestones"
      mg_status pass
    else
      mg_message "milestone-gate --check: $n problems in $MG_MILESTONES_FILE"
      mg_status fail
    fi
    return 0
  fi

  # Setup steps read the host as doctor sees it now.
  local doctor=""
  if jq -e --arg m "$which" \
    '[.milestones[]? | select(.id == $m) | .user_steps[]? | select(.validator.type? == "setup")] | length > 0' \
    "$manifest" >/dev/null 2>&1; then
    doctor="$("$MG_TOOLS/mg" doctor 2>>"$MG_RUN_DIR/doctor.log" | tail -n 1)"
    if [ -f "$doctor" ]; then
      mg_artifact doctor "$doctor" "tools/mg doctor"
    else
      doctor=""
    fi
  fi

  # Only the integration branch closes a milestone: elsewhere a unit
  # branch's own commits count as merged, so the evaluation fails there.
  local out="$MG_RUN_DIR/evaluation.json" errors="$MG_RUN_DIR/evaluate-errors.json" where=""
  local record="$MG_ROOT/docs/process/milestone-records/$which.json"
  mg_gate__in_main || where=--outside-main
  mg_milestone_gate__python evaluate --manifest "$manifest" --config "$config" --milestone "$which" \
    --root "$MG_ROOT" --results "$MG_ROOT/build/mg" --now "$(mg_gate__now)" ${doctor:+--doctor "$doctor"} \
    --record "$record" --out "$out" $where >"$errors"
  rc=$?
  if [ "$rc" -eq 2 ] || [ ! -s "$out" ]; then
    if jq -e '.errors | length > 0' "$errors" >/dev/null 2>&1; then
      jq -c '.errors[] | {id: "manifest", message: ., repro: "tools/mg milestone-gate --check"}' \
        "$errors" >>"$MG_STATE/failures.jsonl"
      mg_message "milestone-gate $which: the manifest is invalid ($(jq '.errors | length' "$errors") problems)"
    else
      mg_failure evaluate "the evaluation did not run (exit $rc); see log.txt"
      mg_message "milestone-gate $which: the evaluation did not run"
    fi
    mg_status error
    return 0
  fi
  mg_artifact evaluation "$out" "milestone evaluation"
  jq -c --arg r "tools/mg milestone-gate $which" '.failures[] | {id, message, repro: $r}' "$out" >>"$MG_STATE/failures.jsonl"
  mg_detail outcome "$(jq -c .outcome "$out")"
  mg_detail milestone "$(jq -nc --arg m "$which" '$m')"
  mg_detail gate_commit "$(jq -c .gate_commit "$out")"
  mg_detail awaiting_user "$(jq -c .awaiting "$out")"
  mg_detail suites "$(jq -c '[.suites[] | {id, outcome, run_id, commit}]' "$out")"
  mg_detail user_steps "$(jq -c '[.user_steps[] | {id, type, outcome, message}]' "$out")"
  mg_detail deferred "$(jq -c '[.deferred[] | {id, outcome}]' "$out")"
  mg_detail record "$(jq -c .record "$out")"
  mg_count total "$(jq '.suites | length' "$out")"
  mg_count passed "$(jq '[.suites[] | select(.outcome == "pass")] | length' "$out")"
  mg_count failed "$(jq '[.suites[] | select(.outcome != "pass" and .outcome != "awaiting_user")] | length' "$out")"
  mg_count skipped 0
  mg_count pending "$(jq '[.suites[] | select(.outcome == "pending")] | length' "$out")"
  mg_count missing "$(jq '[.suites[] | select(.outcome == "missing")] | length' "$out")"
  mg_count user_steps "$(jq '.user_steps | length' "$out")"
  mg_count awaiting_user "$(jq '.awaiting | length' "$out")"
  mg_count units_unmerged "$(jq '[.units[] | select(.merged | not)] | length' "$out")"
  local summary
  summary="$(jq -r '"\([.suites[] | select(.outcome == "pass")] | length) of \(.suites | length) suites pass, \([.failures[]] | length) failures, \(.awaiting | length) awaiting Antoine"' "$out")"
  case "$(jq -r .outcome "$out")" in
    pass)
      mg_message "milestone-gate $which: pass on $(jq -r '.gate_commit[0:12]' "$out"); $summary$(jq -r 'if .record.written then "; commit \(.record.path | sub(".*/docs/"; "docs/"))" else "" end' "$out")"
      mg_status pass
      ;;
    awaiting_user)
      mg_message "milestone-gate $which: awaiting Antoine: $(jq -r '[.awaiting[].id] | join(", ")' "$out"); $summary"
      mg_status warn
      ;;
    *)
      mg_message "milestone-gate $which: fail; $summary"
      mg_status fail
      ;;
  esac
}
