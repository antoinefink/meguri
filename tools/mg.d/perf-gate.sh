# Performance budget gate (U241: R1802, R1803, R1861-R1864, R1866).
# Sourced by tools/mg.
#
# `tools/mg perf-gate` judges the perf results the U12 jobs wrote on this
# commit (perf-ui, bench, bench-mac, bench-icount: the latest per suite and
# runner) against tools/perf/budgets.yaml and tools/perf/baselines/, with
# tools/scripts/gates/perf_gate.py. Its result reports every group of the
# catalog (details.groups, which the milestone gate scopes) and fails on a
# budget breach, a regression past +10% (UI) or +5% (instruction counts), a
# refused result (R1866: not Profile, or with attachments) or a --require'd
# job without results. A case only the regression rule covers and that has
# no baseline is unchecked: the run warns.
#
#   --check                      the catalog against the plan, IntervalNames,
#                                the UI performance tests and milestones.yaml
#   --results <path>             judge these perf results (files or
#                                directories) instead of this commit's
#   --device-run <M> --device <d>  with --results: also write U256's device-run
#                                report (device-run.json), the evidence the
#                                milestone gate checks
#   --rebaseline --reason <text> write tools/perf/baselines/ from the results
#                                (clean tree only); commit them with the reason

mg_job perf-gate host mg_perf_gate \
  "Performance budget gate (U241, R1863, R1864, R1866): [--require <job>]... judges this commit's perf results (perf-ui, bench, bench-mac, bench-icount) against tools/perf/budgets.yaml and tools/perf/baselines/, per-group statuses in details.groups | --results <path>... [--device-run <M> --device iphone|ipad|mac [--build-commit <sha>]] | --rebaseline --reason <text> | --check: the catalog against the plan, IntervalNames, the UI performance tests and milestones.yaml"

MG_PERF_BUDGETS=tools/perf/budgets.yaml
MG_PERF_BASELINES=tools/perf/baselines
MG_PERF_GATE_PY=tools/scripts/gates/perf_gate.py
MG_PERF_GATE_MILESTONES=docs/process/milestones.yaml
# The jobs whose perf results the gate reads (a new perf job joins here).
MG_PERF_GATE_JOBS="perf-ui bench bench-mac bench-icount"

mg_perf_gate__python() { python3 -B -I "$MG_ROOT/$MG_PERF_GATE_PY" "$@"; }

# mg_perf_gate__load_json <out> <errors>: budgets.yaml and milestones.yaml's
# budget groups as JSON (perf_gate.py load). PyYAML is in the dev container; a
# python3 that has it reads the files here. Records nothing; returns non-zero
# when <out> was not written (the errors, as JSON, are in <errors>).
mg_perf_gate__load_json() {
  local out="$1" errors="$2" rel
  if python3 -c 'import yaml' >/dev/null 2>&1; then
    mg_perf_gate__python load --catalog "$MG_ROOT/$MG_PERF_BUDGETS" \
      --milestones "$MG_ROOT/$MG_PERF_GATE_MILESTONES" --out "$out" >"$errors"
  else
    if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
      printf '{"errors": ["Docker is not reachable: tools/perf/budgets.yaml is read in the dev container"]}\n' >"$errors"
      return 1
    fi
    mg_compose --progress quiet build dev >&2 || {
      printf '{"errors": ["building the dev image failed; see log.txt"]}\n' >"$errors"
      return 1
    }
    rel="${out#"$MG_ROOT"/}"
    case "$rel" in /*) printf '{"errors": ["%s is outside the checkout"]}\n' "$out" >"$errors" && return 1 ;; esac
    mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev \
      python3 -B -I "/work/$MG_PERF_GATE_PY" load --catalog "/work/$MG_PERF_BUDGETS" \
      --milestones "/work/$MG_PERF_GATE_MILESTONES" --out "/work/$rel" </dev/null >"$errors"
  fi
  [ -s "$out" ]
}

mg_perf_gate__usage() {
  mg_failure usage "$MG_JOB: $1" "tools/mg --list"
  mg_message "$MG_JOB: $1"
  mg_status error
}

# mg_perf_gate__errors <errors.json> <failure id> <repro>: one failure per error.
mg_perf_gate__errors() {
  jq -c --arg id "$2" --arg r "$3" '.errors[]? | {id: $id, message: ., repro: $r}' "$1" >>"$MG_STATE/failures.jsonl" 2>/dev/null
}

# mg_perf_gate__record <evaluation.json>: the evaluation's failures, details,
# counts and status into this run's result.
mg_perf_gate__record() {
  local ev="$1" repro="$MG_DEFAULT_REPRO"
  jq -c --arg r "$repro" '.failures[] | {id, message, repro: $r}' "$ev" >>"$MG_STATE/failures.jsonl"
  mg_detail groups "$(jq -c .groups "$ev")"
  mg_detail budgets "$(jq -c .budgets "$ev")"
  mg_detail inputs "$(jq -c '[.inputs[] | {job, run_id, suite, kind, runner: .runner.id, placement, refused: (.refused | length > 0)}]' "$ev")"
  mg_detail warnings "$(jq -c .warnings "$ev")"
  mg_detail tracked "$(jq -c .tracked "$ev")"
  mg_count total "$(jq '.evaluations | length' "$ev")"
  mg_count passed "$(jq '[.evaluations[] | select(.status == "pass")] | length' "$ev")"
  mg_count failed "$(jq '[.evaluations[] | select(.status == "fail")] | length' "$ev")"
  mg_count skipped "$(jq '[.evaluations[] | select(.status == "unchecked")] | length' "$ev")"
  mg_count inputs "$(jq '.inputs | length' "$ev")"
  mg_count refused "$(jq '[.inputs[] | select(.refused | length > 0)] | length' "$ev")"
  mg_count untracked "$(jq .untracked "$ev")"
}

mg_perf_gate() {
  local check=false rebaseline=false reason="" milestone="" device="" build="" a
  local results=() require=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --check) check=true && shift ;;
      --require)
        [ "$#" -ge 2 ] && mg_word_in "$2" "$MG_PERF_GATE_JOBS" ||
          { mg_perf_gate__usage "--require takes one of $MG_PERF_GATE_JOBS"; return 0; }
        require[${#require[@]}]="$2" && shift 2
        ;;
      --results)
        [ "$#" -ge 2 ] && [ -e "$2" ] || { mg_perf_gate__usage "--results takes a perf result file or a directory of them"; return 0; }
        a="$2"
        case "$a" in /*) ;; *) a="$MG_ROOT/$a" ;; esac
        results[${#results[@]}]="$a" && shift 2
        ;;
      --rebaseline) rebaseline=true && shift ;;
      --reason)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_perf_gate__usage "--reason takes the one-line reason for the new baselines"; return 0; }
        reason="$2" && shift 2
        ;;
      --device-run)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^M[0-9]$' || { mg_perf_gate__usage "--device-run takes a milestone (M1)"; return 0; }
        milestone="$2" && shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && mg_word_in "$2" "iphone ipad mac" || { mg_perf_gate__usage "--device takes iphone, ipad or mac"; return 0; }
        device="$2" && shift 2
        ;;
      --build-commit)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[0-9a-f]{40}$' || { mg_perf_gate__usage "--build-commit takes a full commit id"; return 0; }
        build="$2" && shift 2
        ;;
      *) mg_perf_gate__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  if [ "$check" = true ] && { [ "$rebaseline" = true ] || [ -n "$milestone$device$reason" ] ||
    [ "${#results[@]}" -gt 0 ] || [ "${#require[@]}" -gt 0 ]; }; then
    mg_perf_gate__usage "--check takes no other argument"
    return 0
  fi
  if [ "$rebaseline" = true ] && [ -z "$reason" ]; then mg_perf_gate__usage "--rebaseline needs --reason <text>"; return 0; fi
  if [ "$rebaseline" = false ] && [ -n "$reason" ]; then mg_perf_gate__usage "--reason goes with --rebaseline"; return 0; fi
  if [ -n "$milestone$device" ] && { [ -z "$milestone" ] || [ -z "$device" ] || [ "${#results[@]}" -eq 0 ]; }; then
    mg_perf_gate__usage "--device-run <M> --device <iphone|ipad|mac> go together, with --results"
    return 0
  fi
  if [ -n "$build" ] && [ -z "$milestone" ]; then mg_perf_gate__usage "--build-commit goes with --device-run"; return 0; fi
  if [ "${#require[@]}" -gt 0 ] && [ "${#results[@]}" -gt 0 ]; then
    mg_perf_gate__usage "--require reads this commit's results; --results names others"
    return 0
  fi

  local catalog="$MG_RUN_DIR/catalog.json" errors="$MG_RUN_DIR/catalog-errors.json" rc
  if ! mg_perf_gate__load_json "$catalog" "$errors"; then
    mg_perf_gate__errors "$errors" catalog "tools/mg perf-gate --check"
    jq -e '.errors | length > 0' "$errors" >/dev/null 2>&1 ||
      mg_failure catalog "could not read $MG_PERF_BUDGETS; see log.txt" "tools/mg perf-gate --check"
    mg_message "perf-gate: could not read $MG_PERF_BUDGETS"
    mg_status error
    return 0
  fi
  mg_artifact catalog "$catalog" "the budget catalog as JSON"
  local report="$MG_RUN_DIR/check.json" n
  mg_perf_gate__python check --catalog "$catalog" --root "$MG_ROOT" >"$report"
  rc=$?
  if [ "$rc" -gt 1 ] || ! jq -e '.errors | type == "array"' "$report" >/dev/null 2>&1; then
    mg_failure check "the catalog check did not run (exit $rc); see log.txt"
    mg_message "perf-gate: the catalog check did not run"
    mg_status error
    return 0
  fi
  n="$(jq '.errors | length' "$report")"
  if [ "$check" = true ]; then
    mg_perf_gate__errors "$report" catalog "tools/mg perf-gate --check"
    mg_count total 1
    mg_count passed $((n == 0 ? 1 : 0))
    mg_count failed $((n == 0 ? 0 : 1))
    mg_count problems "$n"
    if [ "$n" -eq 0 ]; then
      mg_message "perf-gate --check: $MG_PERF_BUDGETS covers every budget and agrees with the plan, IntervalNames, the UI performance tests and $MG_PERF_GATE_MILESTONES"
      mg_status pass
    else
      mg_message "perf-gate --check: $n problems in $MG_PERF_BUDGETS"
      mg_status fail
    fi
    return 0
  fi
  if [ "$n" -gt 0 ]; then
    mg_perf_gate__errors "$report" catalog "tools/mg perf-gate --check"
    mg_message "perf-gate: $MG_PERF_BUDGETS is invalid ($n problems); see tools/mg perf-gate --check"
    mg_status error
    return 0
  fi
  if [ "$rebaseline" = true ] && [ "$MG_COMMIT_DIRTY" != false ]; then
    mg_failure dirty-tree "baselines come from results of a committed state; commit or stash first" "git status"
    mg_message "perf-gate: refused: --rebaseline needs a clean tree"
    mg_status error
    return 0
  fi

  # The inputs: the given results, or this commit's results of the perf jobs.
  local ev="$MG_RUN_DIR/evaluation.json" args=()
  if [ "${#results[@]}" -gt 0 ]; then
    for a in "${results[@]}"; do args[${#args[@]}]="--results" && args[${#args[@]}]="$a"; done
  else
    args=(--discover "$MG_ROOT/build/mg" --jobs "$MG_PERF_GATE_JOBS" --commit "$MG_COMMIT_SHA" --dirty "${MG_COMMIT_DIRTY:-true}")
    for a in ${require[0]+"${require[@]}"}; do args[${#args[@]}]="--require" && args[${#args[@]}]="$a"; done
  fi
  mg_detail require "$(mg_json_array ${require[0]+"${require[@]}"})"
  mg_perf_gate__python evaluate --catalog "$catalog" --baselines "$MG_ROOT/$MG_PERF_BASELINES" "${args[@]}" --out "$ev"
  rc=$?
  if [ "$rc" -eq 2 ] || ! jq -e '.status' "$ev" >/dev/null 2>&1; then
    [ -f "$ev" ] && mg_perf_gate__errors "$ev" evaluate "tools/mg perf-gate"
    jq -e '.errors | length > 0' "$ev" >/dev/null 2>&1 || mg_failure evaluate "the evaluation did not run (exit $rc); see log.txt"
    mg_message "perf-gate: the evaluation did not run (invalid baselines or inputs; see failures)"
    mg_status error
    return 0
  fi
  mg_artifact evaluation "$ev" "every judged case, the inputs and the group statuses"

  if [ "$rebaseline" = true ]; then
    local out="$MG_RUN_DIR/rebaseline.json"
    mg_perf_gate__python rebaseline --catalog "$catalog" --evaluation "$ev" --baselines "$MG_ROOT/$MG_PERF_BASELINES" \
      --reason "$reason" --commit "$MG_COMMIT_SHA" >"$out"
    rc=$?
    if [ "$rc" -ne 0 ] || [ "$(jq '.written | length' "$out" 2>/dev/null || echo 0)" -eq 0 ]; then
      mg_perf_gate__errors "$out" rebaseline "$MG_DEFAULT_REPRO"
      mg_failure rebaseline "no baseline written: no accepted UI or instruction-count result measured a catalog case"
      mg_message "perf-gate --rebaseline: nothing written"
      mg_status fail
      return 0
    fi
    mg_detail baselines_written "$(jq -c --arg root "$MG_ROOT/" '[.written[] | ltrimstr($root)]' "$out")"
    # Judge the results again, against the new baselines.
    mg_perf_gate__python evaluate --catalog "$catalog" --baselines "$MG_ROOT/$MG_PERF_BASELINES" "${args[@]}" --out "$ev"
  fi

  mg_perf_gate__record "$ev"
  local status summary
  status="$(jq -r .status "$ev")"
  summary="$(jq -r '"\([.evaluations[] | select(.status == "pass")] | length) passed, \([.evaluations[] | select(.status == "fail")] | length) failed, \([.evaluations[] | select(.status == "unchecked")] | length) unchecked of \(.evaluations | length) judged cases from \(.inputs | length) perf results"' "$ev")"

  if [ -n "$milestone" ]; then
    local dr="$MG_RUN_DIR/device-run.json" out="$MG_RUN_DIR/device-run-errors.json"
    if ! mg_perf_gate__python device-run --catalog "$catalog" --evaluation "$ev" --milestone "$milestone" \
      --device "$device" --build-commit "${build:-$MG_COMMIT_SHA}" --out "$dr" >"$out"; then
      mg_perf_gate__errors "$out" device-run "$MG_DEFAULT_REPRO"
      mg_message "perf-gate: no device-run report: $(jq -r '.errors | join("; ")' "$out" 2>/dev/null)"
      mg_status fail
      return 0
    fi
    mg_artifact device-run "$dr" "U256 evidence: commit it as docs/process/milestone-records/device-runs/$milestone/$device.json"
    mg_detail device_run "$(jq -c '{milestone, device, build_commit, runner: .runner.id, budgets: [.budgets[] | {id, status}]}' "$dr")"
    case "$(jq -r '[.budgets[].status] | if index("fail") then "fail" elif all(. == "pass") then "pass" else "incomplete" end' "$dr")" in
      fail) status=fail ;;
      incomplete) [ "$status" = fail ] || status=warn ;;
    esac
    summary="$summary; device-run report for $device in $dr (commit it as docs/process/milestone-records/device-runs/$milestone/$device.json)"
  fi
  case "$status" in
    pass) mg_message "perf-gate: pass; $summary$([ "$rebaseline" = false ] || echo "; commit tools/perf/baselines/ with the reason in the message")" ;;
    warn) mg_message "perf-gate: warn ($(jq -r '.warnings[0] // ""' "$ev")); $summary" ;;
    *) mg_message "perf-gate: fail; $summary" ;;
  esac
  mg_status "$status"
}
