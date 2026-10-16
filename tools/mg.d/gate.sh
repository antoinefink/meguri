# Merge gate and static checks (U19: R1950-R1953, R1991; docs/process/tiers.md).
# Sourced by tools/mg.
#
# tools/gate/tiers.yaml lists every run: a tools/mg job, its tier, the paths
# that trigger it, where it runs and its budget. tools/gate/gate.py turns that
# and the changed paths into a plan, then the job results into a receipt. The
# YAML is read in the dev container (PyYAML); planning and evaluation are
# stdlib Python on the host. Each run is a nested `tools/mg <job>`, which does
# its own placement (R1952). Nothing is ever retried (R1953): a run's result
# is final, and quarantine.yaml decides which test failures do not block.

mg_job gate host mg_gate \
  "Merge gate (R1950, R1951): static checks and canaries first, then T0 for the changed paths and T1 when core/ changed (tools/gate/tiers.yaml); [--local] (every run but the similarity scan and oracle-image jobs) [--base <rev>] [--all] [--only <run>[,<run>]...] [--plan]; receipt in build/mg/gate/<run>/receipt.json"
mg_job static-checks host mg_static_checks \
  "Gate static checks (tools/gate/static-checks/): no AnkiWeb host in tests, scripts or scenarios (R1876), provenance trailers on every commit of the branch (R1958), no test retries (R1953); [--base <rev>] [--rev <rev> (that commit's tree)] [--check ankiweb-hosts|trailers|no-retries]..."

MG_GATE_PY="tools/gate/gate.py"
MG_GATE_TIERS="tools/gate/tiers.yaml"
MG_GATE_QUARANTINE="tools/gate/quarantine.yaml"

mg_gate__usage() {
  mg_failure usage "$MG_JOB: $1" "tools/mg --list"
  mg_message "$MG_JOB: $1"
  mg_status error
}

# mg_gate__main_checkout: the main worktree of this repository.
mg_gate__main_checkout() {
  local main
  main="$(git -C "$MG_ROOT" worktree list --porcelain 2>/dev/null | sed -n '1s/^worktree //p')"
  [ -n "$main" ] && (cd "$main" 2>/dev/null && pwd -P)
}

# mg_gate__in_main: this checkout is the main worktree (it holds refs/ and
# the private vault, so `where: main` runs run here).
mg_gate__in_main() { [ "$(mg_gate__main_checkout)" = "$MG_ROOT" ]; }

# mg_gate__default_base: where this branch left the main checkout's branch.
mg_gate__default_base() {
  local main head
  main="$(mg_gate__main_checkout)"
  head="$(git -C "${main:-$MG_ROOT}" rev-parse -q --verify HEAD 2>/dev/null)"
  git -C "$MG_ROOT" merge-base "${head:-HEAD}" HEAD 2>/dev/null
}

# mg_gate__commit <rev>: the full commit id, or nothing.
mg_gate__commit() { git -C "$MG_ROOT" rev-parse -q --verify "$1^{commit}" 2>/dev/null; }

# mg_gate__changed <base> <out>: NUL list of the paths changed since <base>,
# the working tree's changes and untracked files included.
mg_gate__changed() {
  {
    git -C "$MG_ROOT" diff -z --name-only --no-renames "$1" HEAD
    git -C "$MG_ROOT" diff -z --name-only --no-renames HEAD
    git -C "$MG_ROOT" ls-files -z --others --exclude-standard
  } >"$2"
}

# mg_gate__tree_state: a digest of the working tree's status (tracked and
# untracked), to tell whether the gate's own jobs changed it.
mg_gate__tree_state() {
  git -C "$MG_ROOT" status --porcelain=v1 -z --untracked-files=all 2>/dev/null | cksum
}

# mg_gate__python <args...>: gate.py with the host's python3 (stdlib only).
mg_gate__python() { python3 -B -I "$MG_ROOT/$MG_GATE_PY" "$@"; }

# mg_gate__load <config out>: validates tiers.yaml and quarantine.yaml against
# the job registry. PyYAML is in the dev container; a python3 that has it (the
# dev container itself, where the self-tests run) reads them directly.
# Returns 1 after recording the errors.
mg_gate__load() {
  local out="$1" rel errors="$MG_RUN_DIR/config-errors.json" rc
  "$MG_TOOLS/mg" --list >"$MG_RUN_DIR/registry.tsv" || {
    mg_failure registry "tools/mg --list failed"
    mg_status error
    return 1
  }
  if python3 -c 'import yaml' >/dev/null 2>&1; then
    mg_gate__python load --tiers "$MG_ROOT/$MG_GATE_TIERS" --quarantine "$MG_ROOT/$MG_GATE_QUARANTINE" \
      --registry "$MG_RUN_DIR/registry.tsv" --conformance "$MG_ROOT/conformance" --out "$out" >"$errors"
    rc=$?
  else
    if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
      mg_failure docker-unreachable "Docker is not reachable; the gate reads tools/gate/*.yaml in the dev container" "tools/mg doctor"
      mg_status error
      return 1
    fi
    mg_compose --progress quiet build dev >&2 || {
      mg_failure image-build "building the dev image failed; see log.txt"
      mg_status error
      return 1
    }
    rel="${MG_RUN_DIR#"$MG_ROOT"/}"
    mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev \
      python3 -B -I "/work/$MG_GATE_PY" load --tiers "/work/$MG_GATE_TIERS" --quarantine "/work/$MG_GATE_QUARANTINE" \
      --registry "/work/$rel/registry.tsv" --conformance /work/conformance --out "/work/$rel/$(basename "$out")" \
      </dev/null >"$errors"
    rc=$?
  fi
  if [ "$rc" -ne 0 ] || [ ! -s "$out" ]; then
    if jq -e '.errors | length > 0' "$errors" >/dev/null 2>&1; then
      jq -c '.errors[] | {id: "tiers", message: ., repro: "tools/mg gate --plan"}' "$errors" >>"$MG_STATE/failures.jsonl"
      mg_message "$MG_JOB: tools/gate/tiers.yaml is invalid ($(jq '.errors | length' "$errors") problems)"
    else
      mg_failure tiers "could not read tools/gate/tiers.yaml (exit $rc); see log.txt"
      mg_message "$MG_JOB: could not read tools/gate/tiers.yaml"
    fi
    mg_status error
    return 1
  fi
}

# mg_gate__plan <config> <changed NUL file or ""> <mode> <out> [gate.py plan options...]
mg_gate__plan() {
  local config="$1" changed="$2" mode="$3" out="$4" capable=no errors="$MG_RUN_DIR/plan-errors.json"
  shift 4
  mg_gate__in_main && capable=yes
  if ! mg_gate__python plan --config "$config" --changed "$changed" --mode "$mode" --main-capable "$capable" \
    --out "$out" "$@" >"$errors"; then
    jq -c '.errors[]? | {id: "plan", message: ., repro: "tools/mg gate --plan"}' "$errors" >>"$MG_STATE/failures.jsonl"
    [ -s "$MG_STATE/failures.jsonl" ] || mg_failure plan "planning failed; see log.txt"
    mg_message "$MG_JOB: planning failed"
    mg_status error
    return 1
  fi
  mg_artifact plan "$out" "plan"
}

# mg_gate__now: UTC now; MG_GATE_NOW overrides it (the self-tests' clock for
# quarantine deadlines).
mg_gate__now() { printf '%s\n' "${MG_GATE_NOW:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"; }

# mg_gate__evaluate <config> <plan> <results> <out> [gate.py evaluate options...]
mg_gate__evaluate() {
  local config="$1" plan="$2" results="$3" out="$4"
  shift 4
  mg_gate__python evaluate --config "$config" --plan "$plan" --results "$results" --now "$(mg_gate__now)" \
    --commit "$MG_COMMIT_SHA" --dirty "${MG_COMMIT_DIRTY:-false}" --out "$out" "$@"
}

# mg_gate__run_one <run JSON> <results file> [--merge]: one nested tools/mg
# call; appends {id, result, exit_code} to the results. With --merge, the
# run's merge_args. Sets MG_GATE_LAST_STATUS to the job's status.
mg_gate__run_one() {
  local item="$1" results="$2" key=args id job out rc result a
  [ "${3:-}" = --merge ] && key=merge_args
  id="$(jq -r .id <<<"$item")"
  job="$(jq -r .job <<<"$item")"
  local args=()
  while IFS= read -r -d '' a; do
    args[${#args[@]}]="$a"
  done < <(jq -j --arg k "$key" '.[$k][]? | . + "\u0000"' <<<"$item")
  echo "$MG_JOB: [$id] tools/mg $job${args[0]+ ${args[*]}}" >&2
  out="$("$MG_TOOLS/mg" "$job" ${args[0]+"${args[@]}"} </dev/null)"
  rc=$?
  result="$(printf '%s\n' "$out" | tail -n 1)"
  [ -f "$result" ] || result=""
  MG_GATE_LAST_STATUS="$( [ -n "$result" ] && jq -r .status "$result" 2>/dev/null)"
  echo "$MG_JOB: [$id] ${MG_GATE_LAST_STATUS:-no result} (exit $rc)" >&2
  jq -nc --arg id "$id" --arg r "$result" --argjson rc "$rc" \
    '{id: $id, result: (if $r == "" then null else $r end), exit_code: $rc}' >>"$results"
}

# mg_gate__execute <config> <plan> <results>: runs the plan's runs in order.
# A failing canary stops the run (AE82: a pin bump fails before anything
# else); T1 starts only when T0 has nothing blocking.
mg_gate__execute() {
  local config="$1" plan="$2" results="$3" n i item stage tier id stop="" t1="" status
  : >"$results"
  n="$(jq '.runs | length' "$plan")"
  i=0
  while [ "$i" -lt "$n" ]; do
    item="$(jq -c ".runs[$i]" "$plan")"
    i=$((i + 1))
    [ "$(jq -r .action <<<"$item")" = run ] || continue
    id="$(jq -r .id <<<"$item")"
    stage="$(jq -r .stage <<<"$item")"
    tier="$(jq -r .tier <<<"$item")"
    if [ -n "$stop" ]; then
      jq -nc --arg id "$id" --arg c "$stop" '{id: $id, outcome: "skipped-after-canary", message: ("not run: canary " + $c + " failed")}' >>"$results"
      continue
    fi
    if [ "$tier" = T1 ] && [ -z "$t1" ]; then
      mg_gate__evaluate "$config" "$plan" "$results" "$MG_RUN_DIR/receipt-t0.json" >/dev/null 2>&1
      if jq -e '[.runs[] | select(.tier == "T0" and .blocking and (.outcome | IN("fail", "error", "unresolved", "needs-oracle-tree")))] | length > 0' \
        "$MG_RUN_DIR/receipt-t0.json" >/dev/null 2>&1; then
        t1=skip
      else
        t1=run
      fi
      rm -f "$MG_RUN_DIR/receipt-t0.json"
    fi
    if [ "$t1" = skip ] && [ "$tier" = T1 ]; then
      jq -nc --arg id "$id" '{id: $id, outcome: "skipped-after-t0", message: "not run: T0 failed"}' >>"$results"
      continue
    fi
    mg_gate__run_one "$item" "$results"
    status="$MG_GATE_LAST_STATUS"
    if [ "$stage" = canary ] && [ "$status" != pass ] && [ "$status" != warn ]; then
      stop="$id"
      echo "$MG_JOB: canary $id failed; the remaining runs do not start (AE82 ordering)" >&2
    fi
  done
}

# mg_gate__record <receipt> <mode>: the job's result from a receipt.
mg_gate__record() {
  local receipt="$1" mode="$2" summary
  mg_artifact receipt "$receipt" "receipt"
  jq -c '.blocking[]' "$receipt" >>"$MG_STATE/failures.jsonl"
  summary="$(jq -c '{
      ran: [.runs[] | select(.result != null)] | length,
      pass: [.runs[] | select(.outcome == "pass" or .outcome == "warn")] | length,
      failed: [.runs[] | select(.outcome | IN("fail", "error", "unresolved", "needs-oracle-tree"))] | length,
      outcomes: (reduce .runs[] as $r ({}; .[$r.outcome] += 1))}' "$receipt")"
  mg_count total "$(jq '.runs | length' "$receipt")"
  mg_count passed "$(jq .pass <<<"$summary")"
  mg_count failed "$(jq .failed <<<"$summary")"
  mg_count skipped "$(jq '[.runs[] | select(.result == null and (.outcome | IN("fail", "error", "unresolved", "needs-oracle-tree") | not))] | length' "$receipt")"
  mg_detail receipt "$(jq -c '{mode, commit, dirty, base, values, pass, merge_ready, incomplete,
      runs: [.runs[] | {id, tier, outcome, result, duration_ms, over_budget}],
      quarantine: {active: [.quarantine.active[] | {run, test, class, owner, deadline, failed_here}],
                   expired: [.quarantine.expired[] | {run, test, owner, deadline}],
                   invalid: [.quarantine.invalid[] | .label]}}' "$receipt")"
  local outcomes
  outcomes="$(jq -r '.outcomes | to_entries | map("\(.value) \(.key)") | join(", ")' <<<"$summary")"
  if jq -e '.pass' "$receipt" >/dev/null; then
    if jq -e '[.runs[] | select(.over_budget or (.outcome | IN("queued", "quarantined", "warn")))] | length > 0' "$receipt" >/dev/null ||
      { [ "$mode" != local ] && jq -e '.incomplete | length > 0' "$receipt" >/dev/null; }; then
      mg_message "$MG_JOB: pass with notes ($outcomes)"
      mg_status warn
    else
      mg_message "$MG_JOB: pass ($outcomes)"
      mg_status pass
    fi
  else
    mg_message "$MG_JOB: fail ($outcomes; first: $(jq -r '.blocking[0].message' "$receipt" | cut -c1-200))"
    mg_status fail
  fi
}

# tools/mg gate [--local] [--base <rev>] [--all] [--only <ids>] [--plan]
mg_gate() {
  local mode=full base="" all="" only="" plan_only=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --local) mode=local; shift ;;
      --base) [ "$#" -ge 2 ] || { mg_gate__usage "--base needs a revision"; return 0; }; base="$2"; shift 2 ;;
      --all) all=--all; shift ;;
      --only) [ "$#" -ge 2 ] || { mg_gate__usage "--only needs run ids"; return 0; }; only="$2"; shift 2 ;;
      --plan) plan_only=true; shift ;;
      *) mg_gate__usage "unknown argument '$1'; usage: tools/mg gate [--local] [--base <rev>] [--all] [--only <run>[,<run>]...] [--plan]"; return 0 ;;
    esac
  done
  [ -n "$base" ] || base="$(mg_gate__default_base)"
  local base_sha tip
  base_sha="$(mg_gate__commit "${base:-}")"
  tip="$(mg_gate__commit HEAD)"
  if [ -z "$base_sha" ] || [ -z "$tip" ]; then
    mg_gate__usage "'${base:-<none>}' is not a commit (and HEAD must be one)"
    return 0
  fi
  local config="$MG_RUN_DIR/config.json" plan="$MG_RUN_DIR/plan.json" changed="$MG_RUN_DIR/changed.lst"
  local results="$MG_RUN_DIR/results.jsonl" receipt="$MG_RUN_DIR/receipt.json"
  mg_gate__load "$config" || return 0
  mg_gate__changed "$base_sha" "$changed"
  mg_gate__plan "$config" "$changed" "$mode" "$plan" --var "base=$base_sha" --var "tip=$tip" $all \
    ${only:+--only "$only"} || return 0
  if [ "$plan_only" = true ]; then
    mg_detail plan "$(jq -c '[.runs[] | {id, tier, stage, action, outcome, reason, args}]' "$plan")"
    mg_message "$MG_JOB: plan only: $(jq -r '[.runs[] | select(.action == "run") | .id] | join(", ")' "$plan")"
    mg_status pass
    return 0
  fi
  local before after
  before="$(mg_gate__tree_state)"
  mg_gate__execute "$config" "$plan" "$results"
  after="$(mg_gate__tree_state)"
  mg_artifact results "$results" "job results"
  if ! mg_gate__evaluate "$config" "$plan" "$results" "$receipt" >"$MG_RUN_DIR/evaluate-errors.json"; then
    mg_failure evaluate "evaluating the results failed: $(cat "$MG_RUN_DIR/evaluate-errors.json")"
    mg_status error
    return 0
  fi
  if [ "$before" != "$after" ]; then
    jq '.blocking += [{id: "tree-changed", message: "a job of this gate changed the working tree (git status differs before and after); a receipt is for a commit, so no job may write tracked or untracked files", repro: "git status"}]
      | .pass = false | .merge_ready = false' "$receipt" >"$receipt.tmp" && mv "$receipt.tmp" "$receipt"
  fi
  mg_gate__record "$receipt" "$mode"
}

# tools/mg static-checks [--base <rev>] [--rev <rev>] [--check <name>]...
mg_static_checks() {
  local base="" rev="" checks="" c
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --base) [ "$#" -ge 2 ] || { mg_gate__usage "--base needs a revision"; return 0; }; base="$2"; shift 2 ;;
      --rev) [ "$#" -ge 2 ] || { mg_gate__usage "--rev needs a revision"; return 0; }; rev="$2"; shift 2 ;;
      --check)
        [ "$#" -ge 2 ] && mg_word_in "$2" "ankiweb-hosts trailers no-retries" ||
          { mg_gate__usage "--check takes ankiweb-hosts, trailers or no-retries"; return 0; }
        checks="$checks $2"
        shift 2
        ;;
      *) mg_gate__usage "unknown argument '$1'; usage: tools/mg static-checks [--base <rev>] [--rev <rev>] [--check <name>]..."; return 0 ;;
    esac
  done
  [ -n "$checks" ] || checks="ankiweb-hosts trailers no-retries"
  if [ -n "$rev" ]; then
    rev="$(mg_gate__commit "$rev")" || true
    [ -n "$rev" ] || { mg_gate__usage "--rev is not a commit"; return 0; }
  fi
  [ -n "$base" ] || base="$(mg_gate__default_base)"
  base="$(mg_gate__commit "${base:-}")" || true
  [ -n "$base" ] || { mg_gate__usage "--base is not a commit"; return 0; }
  local findings="$MG_RUN_DIR/findings.jsonl" out rc passed=0 failed=0 errors=0 dir="tools/gate/static-checks"
  : >"$findings"
  for c in $checks; do
    case "$c" in
      ankiweb-hosts) out="$(/bin/bash "$dir/ankiweb-hosts.sh" "$MG_ROOT" ${rev:+"$rev"})" ;;
      trailers) out="$(/bin/bash "$dir/trailers.sh" "$MG_ROOT" "$base" "${rev:-HEAD}")" ;;
      no-retries) out="$(/bin/bash "$dir/no-retries.sh" "$MG_ROOT" ${rev:+"$rev"})" ;;
    esac
    rc=$?
    [ -z "$out" ] || printf '%s\n' "$out" >>"$findings"
    case "$rc" in
      0) passed=$((passed + 1)) ;;
      1)
        failed=$((failed + 1))
        printf '%s\n' "$out" | jq -c --arg repro "tools/mg static-checks --check $c" '
          if .check == "trailers" then
            {id: ("trailers:" + .commit[0:12]), message: (.commit[0:12] + " (" + .subject + "): " + .message), repro: $repro}
          elif .check == "ankiweb-hosts" then
            {id: ("ankiweb-hosts:" + .path + ":" + (.line | tostring)),
             message: (.path + ":" + (.line | tostring) + " names an AnkiWeb host (\(.match)); tests, scripts and scenarios never do (R1876): use the local sync server, or build AnkiWeb cases from meguri-sync'"'"'s ANKIWEB_HOST"),
             repro: $repro}
          else
            {id: ("no-retries:" + .path + ":" + (.line | tostring)), message: (.path + ": " + .message), repro: $repro}
          end' >>"$MG_STATE/failures.jsonl"
        ;;
      *)
        errors=$((errors + 1))
        mg_failure "$c-error" "the $c check could not run (exit $rc); see log.txt" "tools/mg static-checks --check $c"
        ;;
    esac
  done
  mg_artifact findings "$findings" "static-check findings"
  mg_detail scope "$(jq -nc --arg base "$base" --arg rev "$rev" --arg checks "${checks# }" \
    '{base: $base, rev: (if $rev == "" then "working tree" else $rev end), checks: ($checks | split(" "))}')"
  mg_count total $((passed + failed + errors))
  mg_count passed "$passed"
  mg_count failed $((failed + errors))
  if [ "$errors" -gt 0 ]; then
    mg_message "static-checks: $errors check(s) could not run"
    mg_status error
  elif [ "$failed" -gt 0 ]; then
    mg_message "static-checks: $(wc -l <"$findings" | tr -d ' ') finding(s) in$(jq -r -s '[.[].check] | unique | map(" " + .) | join("")' "$findings")"
    mg_status fail
  else
    mg_message "static-checks: ${checks# } clean"
    mg_status pass
  fi
}
