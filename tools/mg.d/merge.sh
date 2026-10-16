# Merge and escalations (U19: R1951, R1957, R1991; docs/process/merge.md).
# Sourced by tools/mg.
#
# `tools/mg merge <branch>` is the only path to the integration branch (the
# branch checked out in the main checkout). There is no git remote, so this
# job, not a push hook, enforces the receipt. It runs `tools/mg gate` in the
# branch's worktree at its tip (the worktree-safe T0 and T1 runs, with that
# branch's own tiers and tools), then the runs the worktree left, in the
# plan's order: `where: main` runs from the main checkout against the
# branch's tree (the similarity scan, which needs refs/, and the static checks
# over the whole tree), and the others (`where: tip` runs, oracle-image runs
# that need the branch's own oracle/) in a full checkout of the tip at
# build/mg/merge-tip, removed afterwards. It evaluates everything with the
# branch's gate, the worktree receipt's own blockers included, checks the
# worktree again, and fast-forwards only when that receipt passes and is
# complete.
#
# docs/process/provenance/escalations.jsonl is append-only: an `add` line per
# escalation and a `resolve` line when Antoine settles it; the last line of an
# id is its state. A unit with an open escalation is never merged: the merge
# reads the main checkout's ledger joined with the branch tip's, since an
# escalation can be added or resolved in the unit's worktree.

mg_job merge host mg_merge \
  "Merge a unit branch into the integration branch (the main checkout's branch), only path to it (R1951): <branch> [--worktree <dir>] [--unit <id>]; gate in the branch's worktree, similarity scan and static checks from the main checkout against the branch's tree, where: tip and oracle runs in a full checkout of the tip, fast-forward only on a passing complete receipt"
mg_job escalation host mg_escalation \
  "Escalations for Antoine (docs/process/provenance/escalations.jsonl): add --kind behavior-gap|approval|blocked-red|stop-condition --unit <id> --summary <text> [--detail <JSON>] | resolve <id> --by <name> --resolution <text> | list [--open] [--unit <id>]"

MG_ESCALATIONS_FILE="docs/process/provenance/escalations.jsonl"
MG_ESCALATION_KINDS="behavior-gap approval blocked-red stop-condition"

# mg_escalation__state [<checkout> [<commit>...]]: one JSON object per
# escalation (its current state), from the ledger of that checkout's working
# tree joined with the ledger at each <commit>. A line both hold counts once,
# and every add line is read before any resolve line, so an escalation either
# side opened stays open until either side resolves it. Returns 1 when a
# ledger cannot be read or does not parse: a caller that gates on it must
# refuse, never read it as empty.
mg_escalation__state() {
  local root="${1:-$MG_ROOT}" rev ledger="" part
  [ "$#" -eq 0 ] || shift
  if [ -f "$root/$MG_ESCALATIONS_FILE" ]; then
    ledger="$(cat "$root/$MG_ESCALATIONS_FILE")" || return 1
  fi
  for rev in ${1+"$@"}; do
    git -C "$root" cat-file -e "$rev:$MG_ESCALATIONS_FILE" 2>/dev/null || continue
    part="$(git -C "$root" show "$rev:$MG_ESCALATIONS_FILE")" || return 1
    ledger="$ledger
$part"
  done
  jq -c -s 'reduce .[] as $l ({seen: {}, lines: []}; ($l | tojson) as $k
      | if .seen[$k] then . else .seen[$k] = true | .lines += [$l] end)
    | .lines | map(select(.state == "open")) + map(select(.state != "open"))
    | reduce .[] as $l ({}; if $l.state == "open" and (.[$l.id] | not) then .[$l.id] = $l
      elif .[$l.id] then .[$l.id] += ($l | del(.id)) else . end) | .[]' <<<"$ledger"
}

# mg_escalation_add <checkout> <kind> <unit> <summary> <detail JSON> <source>:
# appends and commits an escalation on the checkout's current branch and sets
# MG_ESCALATION_ID. Returns 1 (MG_SESSION_FAILURE, MG_SESSION_MESSAGE) on
# failure. Call it as a plain command: those variables do not leave a $(...).
mg_escalation_add() {
  local dir="$1" kind="$2" unit="$3" summary="$4" detail="${5:-}" source="$6" id record
  [ -n "$detail" ] || detail=null
  id="E-$(date -u +%Y%m%dT%H%M%SZ)-$(od -An -N2 -tx1 /dev/urandom | tr -d ' \n')"
  MG_ESCALATION_ID=""
  record="$(jq -nc --arg id "$id" --arg kind "$kind" --arg unit "$unit" --arg summary "$summary" \
    --argjson detail "$detail" --arg source "$source" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{id: $id, kind: $kind, state: "open", unit: $unit, summary: $summary, created_at: $at, source: $source}
      + (if $detail == null then {} else {detail: $detail} end)')" || return 1
  mg_ledger_store "$dir" "$MG_ESCALATIONS_FILE" "$record" "Record escalation $id ($kind, $unit)

$summary

Role: infra" || return 1
  MG_ESCALATION_ID="$id"
}

mg_escalation__usage() {
  mg_failure usage "usage: tools/mg escalation add --kind <$(printf '%s' "$MG_ESCALATION_KINDS" | tr ' ' '|')> --unit <id> --summary <text> [--detail <JSON>] | resolve <id> --by <name> --resolution <text> | list [--open] [--unit <id>]; $1"
  mg_message "escalation: $1"
  mg_status error
}

mg_escalation() {
  local sub="${1:-}"
  [ "$#" -gt 0 ] && shift
  local kind="" unit="" summary="" detail="" by="" resolution="" id="" open=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --kind) kind="${2:-}"; shift 2 || shift ;;
      --unit) unit="${2:-}"; shift 2 || shift ;;
      --summary) summary="${2:-}"; shift 2 || shift ;;
      --detail) detail="${2:-}"; shift 2 || shift ;;
      --by) by="${2:-}"; shift 2 || shift ;;
      --resolution) resolution="${2:-}"; shift 2 || shift ;;
      --open) open=true; shift ;;
      -*) mg_escalation__usage "unknown option '$1'"; return 0 ;;
      *) [ -z "$id" ] || { mg_escalation__usage "unexpected argument '$1'"; return 0; }; id="$1"; shift ;;
    esac
  done
  case "$sub" in
    add)
      mg_word_in "$kind" "$MG_ESCALATION_KINDS" || { mg_escalation__usage "--kind must be one of $MG_ESCALATION_KINDS"; return 0; }
      [ -n "$unit" ] && [ -n "$summary" ] || { mg_escalation__usage "--unit and --summary are required"; return 0; }
      if [ -n "$detail" ] && ! jq -e . >/dev/null 2>&1 <<<"$detail"; then
        mg_escalation__usage "--detail must be JSON"
        return 0
      fi
      mg_escalation_add "$MG_ROOT" "$kind" "$unit" "$summary" "$detail" "tools/mg escalation add" || {
        mg_failure "$MG_SESSION_FAILURE" "$MG_SESSION_MESSAGE"
        mg_status error
        return 0
      }
      id="$MG_ESCALATION_ID"
      mg_detail escalation "$(mg_escalation__state | jq -c --arg id "$id" 'select(.id == $id)')"
      mg_message "escalation $id recorded ($kind, $unit): $summary"
      mg_status pass
      ;;
    resolve)
      [ -n "$id" ] && [ -n "$by" ] && [ -n "$resolution" ] || { mg_escalation__usage "resolve needs <id>, --by and --resolution"; return 0; }
      if ! mg_escalation__state | jq -e --arg id "$id" 'select(.id == $id and .state == "open")' >/dev/null; then
        mg_escalation__usage "no open escalation $id"
        return 0
      fi
      local record
      record="$(jq -nc --arg id "$id" --arg by "$by" --arg r "$resolution" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        '{id: $id, state: "resolved", resolved_at: $at, resolved_by: $by, resolution: $r}')"
      if ! mg_ledger_store "$MG_ROOT" "$MG_ESCALATIONS_FILE" "$record" "Resolve escalation $id

$resolution

Role: infra"; then
        mg_failure "$MG_SESSION_FAILURE" "$MG_SESSION_MESSAGE"
        mg_status error
        return 0
      fi
      mg_detail escalation "$(mg_escalation__state | jq -c --arg id "$id" 'select(.id == $id)')"
      mg_message "escalation $id resolved by $by"
      mg_status pass
      ;;
    list)
      local entries
      entries="$(mg_escalation__state | jq -c -s --argjson open "$open" --arg unit "$unit" \
        'map(select((($open | not) or .state == "open") and ($unit == "" or .unit == $unit)))')"
      mg_detail escalations "$entries"
      mg_count total "$(jq length <<<"$entries")"
      mg_count open "$(jq '[.[] | select(.state == "open")] | length' <<<"$entries")"
      mg_message "escalation: $(jq -r '[.[] | select(.state == "open")] | length' <<<"$entries") open of $(jq length <<<"$entries")"
      mg_status pass
      ;;
    *) mg_escalation__usage "a command is required" ;;
  esac
}

mg_merge__fail() {
  mg_failure "$1" "$2" "${3:-$MG_DEFAULT_REPRO}"
  mg_message "merge: $2"
  mg_status "${4:-fail}"
}

# mg_merge__worktree <branch>: the checkout that has <branch> checked out.
mg_merge__worktree() {
  git -C "$MG_ROOT" worktree list --porcelain | awk -v ref="refs/heads/$1" '
    /^worktree / { wt = substr($0, 10) }
    $0 == "branch " ref { print wt; exit }'
}

# The tip checkout: a full checkout of the branch tip (its code and its
# oracle/) for the runs its worktree could not run. One merge at a time: an
# existing one is refused, never reused, and it is removed when the merge
# ends. Its fixed path keeps one compose project for every merge.
MG_MERGE_TIP_DIR=""

# mg_merge__tip_open <tip>: creates it at build/mg/merge-tip and sets
# MG_MERGE_TIP_DIR; returns 1 with the reason in MG_MERGE_TIP_ERROR.
mg_merge__tip_open() {
  local dir="$MG_ROOT/build/mg/merge-tip"
  MG_MERGE_TIP_ERROR=""
  if [ -e "$dir" ]; then
    MG_MERGE_TIP_ERROR="$dir exists: another merge is using it, or one was killed; when no merge runs, remove it with git worktree remove --force $dir"
    return 1
  fi
  mkdir -p "$(dirname "$dir")"
  if ! git -C "$MG_ROOT" worktree add -q --detach "$dir" "$1" >&2; then
    MG_MERGE_TIP_ERROR="git worktree add --detach $dir $1 failed; see log.txt"
    return 1
  fi
  MG_MERGE_TIP_DIR="$(cd "$dir" && pwd -P)"
  mg_on_exit mg_merge__tip_close
}

mg_merge__tip_close() {
  [ -n "$MG_MERGE_TIP_DIR" ] || return 0
  git -C "$MG_ROOT" worktree remove --force "$MG_MERGE_TIP_DIR" >&2 ||
    echo "merge: could not remove $MG_MERGE_TIP_DIR; remove it with git worktree remove --force" >&2
  MG_MERGE_TIP_DIR=""
}

# mg_merge__keep_tip_results <results>: copies each run directory the tip
# checkout wrote under this run's directory, and points the results and the
# copied result's artifacts there, since the checkout goes away.
mg_merge__keep_tip_results() {
  local results="$1" out="$1.tmp" line id result dir dest
  : >"$out"
  while IFS= read -r line; do
    result="$(jq -r '.result // empty' <<<"$line")"
    case "$result" in
      "$MG_MERGE_TIP_DIR"/*)
        id="$(jq -r .id <<<"$line")"
        dir="$(dirname "$result")"
        dest="$MG_RUN_DIR/tip/$id"
        mkdir -p "$MG_RUN_DIR/tip"
        rm -rf "$dest"
        cp -R "$dir" "$dest"
        jq --arg from "$dir/" --arg to "$dest/" '.artifacts |= (. // [] | map(if (.path // "") | startswith($from)
            then .path = $to + (.path | ltrimstr($from)) else . end))' "$dest/result.json" >"$dest/result.json.tmp" &&
          mv "$dest/result.json.tmp" "$dest/result.json"
        line="$(jq -c --arg r "$dest/result.json" '.result = $r' <<<"$line")"
        ;;
    esac
    printf '%s\n' "$line" >>"$out"
  done <"$results"
  mv "$out" "$results"
}

# mg_merge__evaluate <worktree> <gate run dir> <results> <tip> <out>
# [gate.py evaluate options...]: one receipt for the tip by the branch's own
# gate, from its worktree receipt and the merge's runs.
mg_merge__evaluate() {
  local wt="$1" dir="$2" results="$3" tip="$4" out="$5"
  shift 5
  python3 -B -I "$wt/$MG_GATE_PY" evaluate --config "$dir/config.json" --plan "$dir/plan.json" \
    --receipt "$dir/receipt.json" --results "$results" --now "$(mg_gate__now)" --commit "$tip" --dirty false \
    --out "$out" "$@"
}

# tools/mg merge <branch> [--worktree <dir>] [--unit <id>]
mg_merge() {
  local branch="" wt="" unit=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --worktree) wt="${2:-}"; shift 2 || shift ;;
      --unit) unit="${2:-}"; shift 2 || shift ;;
      -*) mg_merge__fail usage "unknown option '$1'" "" error; return 0 ;;
      *) [ -z "$branch" ] || { mg_merge__fail usage "one branch per merge" "" error; return 0; }; branch="$1"; shift ;;
    esac
  done
  [ -n "$branch" ] || { mg_merge__fail usage "usage: tools/mg merge <branch> [--worktree <dir>] [--unit <id>]" "" error; return 0; }
  if ! mg_gate__in_main; then
    mg_merge__fail not-main "tools/mg merge runs in the main checkout ($(mg_gate__main_checkout)), which holds refs/ and the integration branch" "" error
    return 0
  fi
  local target target_head tip
  target="$(git -C "$MG_ROOT" symbolic-ref -q --short HEAD)" || {
    mg_merge__fail detached "the main checkout's HEAD is detached; check out the integration branch" "" error
    return 0
  }
  if ! git -C "$MG_ROOT" diff --quiet || ! git -C "$MG_ROOT" diff --cached --quiet; then
    mg_merge__fail main-dirty "the main checkout has uncommitted changes to tracked files; commit or stash them first" "git status" error
    return 0
  fi
  target_head="$(mg_gate__commit HEAD)"
  tip="$(mg_gate__commit "refs/heads/$branch")"
  [ -n "$tip" ] || { mg_merge__fail no-branch "no branch '$branch'" "" error; return 0; }
  if [ -z "$unit" ]; then
    case "$branch" in unit/*) unit="${branch#unit/}" ;; *) unit="$branch" ;; esac
  fi
  local run_dir="$MG_RUN_DIR" summary
  mg_detail merge "$(jq -nc --arg b "$branch" --arg t "$tip" --arg into "$target" --arg h "$target_head" --arg u "$unit" \
    '{branch: $b, tip: $t, into: $into, into_head: $h, unit: $u}')"

  # A unit with an open escalation stays unmerged until Antoine resolves it.
  # The tip's ledger is what the fast-forward would bring in: an escalation
  # added or resolved in the unit's worktree counts as much as the main
  # checkout's.
  local state open
  if ! state="$(mg_escalation__state "$MG_ROOT" "$tip")"; then
    mg_merge__fail escalation-ledger "$MG_ESCALATIONS_FILE does not parse in the main checkout or at $branch's tip ($(git -C "$MG_ROOT" rev-parse --short "$tip")); fix it there (a merge conflict left in it?)" \
      "jq -s . $MG_ESCALATIONS_FILE; git show $tip:$MG_ESCALATIONS_FILE | jq -s ." error
    return 0
  fi
  open="$(jq -c --arg u "$unit" 'select(.unit == $u and .state == "open")' <<<"$state")"
  if [ -n "$open" ]; then
    jq -c --arg r "tools/mg escalation list --open --unit $unit, in the main checkout and in $branch's worktree" \
      '{id: ("escalation:" + .id), message: ("open \(.kind) escalation \(.id) for \(.unit): \(.summary)"), repro: $r}' \
      <<<"$open" >>"$MG_STATE/failures.jsonl"
    mg_message "merge: $unit has $(printf '%s\n' "$open" | wc -l | tr -d ' ') open escalation(s); $target not moved"
    mg_status fail
    return 0
  fi
  if [ "$tip" = "$target_head" ]; then
    mg_merge__fail nothing-to-merge "$branch is at $target's head already" "" error
    return 0
  fi
  if ! git -C "$MG_ROOT" merge-base --is-ancestor "$target_head" "$tip"; then
    mg_merge__fail not-fast-forward "$branch does not contain $target's head ($(git -C "$MG_ROOT" rev-parse --short "$target_head")): merge $target into $branch in its worktree first" \
      "git -C <worktree> merge $target" error
    return 0
  fi
  [ -n "$wt" ] || wt="$(mg_merge__worktree "$branch")"
  if [ -z "$wt" ] || [ ! -x "$wt/tools/mg" ]; then
    mg_merge__fail no-worktree "no worktree has $branch checked out; give it with --worktree" "" error
    return 0
  fi
  wt="$(cd "$wt" && pwd -P)"
  if [ "$(git -C "$wt" rev-parse HEAD)" != "$tip" ] || [ -n "$(git -C "$wt" status --porcelain 2>/dev/null | head -n 1)" ]; then
    mg_merge__fail worktree-not-at-tip "$wt must be clean and at $branch's tip ($(git -C "$MG_ROOT" rev-parse --short "$tip"))" "git -C $wt status" error
    return 0
  fi

  # 1. The worktree's gate at the tip, measured from the integration head.
  echo "merge: tools/mg gate in $wt (base $target_head)" >&2
  local gate_result gate_receipt gate_dir
  gate_result="$("$wt/tools/mg" gate --base "$target_head" </dev/null | tail -n 1)"
  gate_receipt="$( [ -f "$gate_result" ] && jq -r '.artifacts[] | select(.kind == "receipt") | .path' "$gate_result" | head -n 1)"
  if [ -z "$gate_receipt" ] || [ ! -f "$gate_receipt" ]; then
    mg_merge__fail worktree-gate "the worktree's gate wrote no receipt ($(jq -r '.message // "no result"' "$gate_result" 2>/dev/null))" \
      "cd $wt && tools/mg gate --base $target_head" fail
    [ -f "$gate_result" ] && mg_artifact gate-result "$gate_result" "worktree gate"
    return 0
  fi
  mg_artifact gate-result "$gate_result" "worktree gate"
  gate_dir="$(dirname "$gate_receipt")"
  if [ "$(jq -r .commit "$gate_receipt")" != "$tip" ] || [ "$(jq -r .dirty "$gate_receipt")" != false ]; then
    mg_merge__fail receipt-commit "the worktree's receipt is not for $tip on a clean tree" "" fail
    return 0
  fi

  # 2. The runs the worktree left, in the plan's order and under the gate's
  # rules: nothing starts after a failed canary, T1 only when T0 has nothing
  # blocking (AE82). `where: main` runs run here with their merge_args
  # against the branch's tree; the others (`where: tip` runs, oracle runs
  # that need the branch's own oracle/) run with their args in the tip
  # checkout, with this checkout's similarity corpus.
  local results="$run_dir/results.jsonl" partial="$run_dir/receipt-partial.json" blockers=()
  local ids id item outcome tier stopped t1="" tip_state="" tip_before="" main_before refs=""
  : >"$results"
  [ ! -d "$MG_ROOT/refs/anki" ] || refs="$MG_ROOT/refs"
  stopped="$(jq -r 'any(.runs[]; .outcome == "skipped-after-canary")' "$gate_receipt")"
  ids="$(jq -r '.runs[] | select(.outcome | IN("deferred-main", "deferred-tip", "needs-oracle-tree")) | .id' "$gate_receipt")"
  main_before="$(mg_gate__tree_state)"
  for id in $ids; do
    item="$(jq -c --arg id "$id" '.runs[] | select(.id == $id)' "$gate_dir/plan.json")"
    outcome="$(jq -r --arg id "$id" '.runs[] | select(.id == $id) | .outcome' "$gate_receipt")"
    tier="$(jq -r .tier <<<"$item")"
    if [ "$stopped" = true ]; then
      jq -nc --arg id "$id" '{id: $id, outcome: "skipped-after-canary", message: "not run: a canary failed in the worktree gate"}' >>"$results"
      continue
    fi
    if [ "$tier" = T1 ] && [ -z "$t1" ]; then
      t1=skip
      if mg_merge__evaluate "$wt" "$gate_dir" "$results" "$tip" "$partial" >/dev/null 2>&1 &&
        ! mg_gate__receipt_blocks "$partial" tier T0; then
        t1=run
      fi
      rm -f "$partial"
    fi
    if [ "$tier" = T1 ] && [ "$t1" = skip ]; then
      jq -nc --arg id "$id" '{id: $id, outcome: "skipped-after-t0", message: "not run: T0 failed"}' >>"$results"
      continue
    fi
    if [ "$outcome" = deferred-main ]; then
      mg_gate__run_one "$item" "$results" merge_args
      continue
    fi
    if [ -z "$tip_state" ]; then
      if mg_merge__tip_open "$tip"; then
        tip_state=open
        tip_before="$(mg_gate__tree_state "$MG_MERGE_TIP_DIR")"
      else
        tip_state=failed
        mg_failure tip-checkout "$MG_MERGE_TIP_ERROR" "git worktree list"
      fi
    fi
    if [ "$tip_state" = failed ]; then
      jq -nc --arg id "$id" --arg m "$MG_MERGE_TIP_ERROR" '{id: $id, outcome: "error", message: ("not run: no checkout of the tip: " + $m)}' >>"$results"
      continue
    fi
    MG_SIMILARITY_REFS="$refs" mg_gate__run_one "$item" "$results" args "$MG_MERGE_TIP_DIR/tools/mg"
  done
  # A receipt is for a commit: no run may change a checkout it ran in.
  [ "$(mg_gate__tree_state)" = "$main_before" ] ||
    blockers[${#blockers[@]}]="$(mg_gate__tree_blocker tree-changed:main "a run of this merge")"
  if [ "$tip_state" = open ]; then
    [ "$(mg_gate__tree_state "$MG_MERGE_TIP_DIR")" = "$tip_before" ] ||
      blockers[${#blockers[@]}]="$(mg_gate__tree_blocker tree-changed:tip "a run of this merge" "$MG_MERGE_TIP_DIR")"
    mg_merge__keep_tip_results "$results"
    mg_merge__tip_close
  fi
  mg_artifact results "$results" "runs of the main checkout and the tip checkout"
  # The worktree's gate tested its tree at the tip: it must still be that.
  if [ "$(git -C "$wt" rev-parse HEAD 2>/dev/null)" != "$tip" ] ||
    [ -n "$(git -C "$wt" status --porcelain --untracked-files=all 2>/dev/null | head -n 1)" ]; then
    blockers[${#blockers[@]}]="$(jq -nc --arg wt "$wt" --arg t "$tip" '{id: "worktree-changed",
      message: ("after its gate, " + $wt + " is no longer clean at " + $t + ": a job wrote to the tested tree, or something changed it during the merge"),
      repro: ("git -C " + $wt + " status")}')"
  fi

  # 3. One receipt for the tip, evaluated by the branch's own gate.
  local receipt="$run_dir/receipt.json" args=() b
  for b in ${blockers[0]+"${blockers[@]}"}; do
    args[${#args[@]}]=--blocker
    args[${#args[@]}]="$b"
  done
  if ! mg_merge__evaluate "$wt" "$gate_dir" "$results" "$tip" "$receipt" ${args[0]+"${args[@]}"} \
    >"$run_dir/evaluate-errors.json"; then
    mg_merge__fail evaluate "evaluating the receipt failed: $(cat "$run_dir/evaluate-errors.json")" "" error
    return 0
  fi
  jq --arg b "$branch" --arg into "$target" --arg h "$target_head" --arg wt "$wt" --arg g "$gate_result" \
    '. + {branch: $b, into: $into, into_head: $h, worktree: $wt, worktree_gate: $g}' "$receipt" >"$receipt.tmp" &&
    mv "$receipt.tmp" "$receipt"

  # 4. What only a person settles (R1991, R1957): a similarity flag sends the
  # implementer view (never the reviewer view) to the branch's worktree and
  # records an escalation; so does a changed approved signature baseline.
  local feedback="" sim_result flags ids
  sim_result="$(jq -r '.runs[] | select(.job == "similarity" and .outcome == "fail") | .result // empty' "$receipt" | head -n 1)"
  if [ -n "$sim_result" ] && [ -f "$sim_result" ]; then
    feedback="$wt/build/mg/merge-feedback/$MG_RUN_ID"
    mkdir -p "$feedback"
    jq -r '.artifacts[] | select(.kind == "similarity-implementer" or .kind == "similarity-implementer-json") | .path' "$sim_result" |
      while IFS= read -r f; do
        [ -f "$f" ] && cp "$f" "$feedback/"
      done
    flags="$(jq -c '[.failures[] | select(.id | startswith("similarity:")) | .id]' "$sim_result")"
    mg_detail implementer_feedback "$(jq -nc --arg d "$feedback" '$d')"
    if ! mg_escalation_add "$MG_ROOT" approval "$unit" \
      "similarity flags on $branch at $(git -C "$MG_ROOT" rev-parse --short "$tip"): rewrite from the spec, or Antoine approves an allowlist entry (R1957)" \
      "$(jq -nc --argjson f "$flags" --arg v "$feedback" '{flags: $f, implementer_view: $v}')" "tools/mg merge $MG_RUN_ID"; then
      mg_failure "$MG_SESSION_FAILURE" "recording the similarity escalation failed: $MG_SESSION_MESSAGE"
    fi
  fi
  ids="$(jq -c '[.runs[] | select(any(.failures[]?; .id == "needs-human-review")) | .id]' "$receipt")"
  if [ "$ids" != "[]" ]; then
    mg_escalation_add "$MG_ROOT" approval "$unit" \
      "approved signature baselines changed on $branch: Antoine reviews them in the gallery (R1991)" \
      "$(jq -nc --argjson r "$ids" '{runs: $r}')" "tools/mg merge $MG_RUN_ID" ||
      mg_failure "$MG_SESSION_FAILURE" "recording the baseline escalation failed: $MG_SESSION_MESSAGE"
  fi

  mg_gate__record "$receipt" full
  summary="$(cat "$MG_STATE/message")"
  if ! jq -e '.merge_ready' "$receipt" >/dev/null; then
    if jq -e '.pass' "$receipt" >/dev/null; then
      mg_failure incomplete "the receipt is incomplete: $(jq -r '.incomplete | join(", ")' "$receipt") did not run"
    fi
    mg_message "merge: $target not moved; ${summary#merge: }"
    mg_status fail
    return 0
  fi

  # 5. Fast-forward, only if nothing moved meanwhile.
  if [ "$(mg_gate__commit "refs/heads/$branch")" != "$tip" ] || [ "$(mg_gate__commit HEAD)" != "$target_head" ]; then
    mg_merge__fail moved "$branch or $target moved during the merge run; run it again" "" fail
    return 0
  fi
  if ! git -C "$MG_ROOT" merge --ff-only -q "$tip" >&2; then
    mg_merge__fail fast-forward "git merge --ff-only $tip failed in the main checkout; see log.txt" "" error
    return 0
  fi
  mg_detail merged true
  mg_message "merge: $target fast-forwarded to $branch ($(git -C "$MG_ROOT" rev-parse --short "$tip")); ${summary#merge: }"
}
