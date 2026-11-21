# Run-output pruning. Sourced by tools/mg.
# The rule and the deletion live in tools/scripts/prune-run-output.py; this
# wrapper hands it every checkout's Docker Rust target (mg_cargo_target_for)
# and turns its summary into the result.

mg_job prune host mg_prune \
  "Report what old run folders under build/mg would free (by default deletes nothing); --apply deletes them in this checkout; --cleanroom archives cleanroom records per month, then removes source copies and codex-home; --root <checkout> or --all-worktrees report on other checkouts; --no-docker skips volume sizes"

mg_prune() {
  local list="$MG_RUN_DIR/worktrees.z" map="$MG_RUN_DIR/checkouts.tsv" out="$MG_RUN_DIR/prune.json"
  local field checkout key
  : >"$map"
  git -C "$MG_ROOT" worktree list --porcelain -z >"$list" 2>/dev/null
  while IFS= read -r -d '' field; do
    case "$field" in worktree\ *) checkout="${field#worktree }" ;; *) continue ;; esac
    [ -d "$checkout" ] || continue
    checkout="$(cd "$checkout" && pwd -P)" || continue
    printf '%s\t%s\t%s\n' "$checkout" "$(mg_compose_project_for "$checkout")" \
      "$(mg_cargo_target_for "$checkout" "${MG_CARGO_TARGET_REQUESTED:-}")" >>"$map"
  done <"$list"
  rm -f "$list"
  if ! python3 "$MG_TOOLS/scripts/prune-run-output.py" --self "$MG_ROOT" --run-dir "$MG_RUN_DIR" \
    --checkouts "$map" --out "$out" -- ${1+"$@"} || [ ! -s "$out" ]; then
    mg_failure prune "prune-run-output.py stopped before it wrote its summary; see log.txt"
    mg_status error
    return 0
  fi
  [ ! -s "$MG_RUN_DIR/plan.json" ] || mg_artifact json "$MG_RUN_DIR/plan.json" "every run considered: action and keep reasons"
  jq -c '.counts | to_entries[]' "$out" | while IFS= read -r field; do
    mg_count "$(jq -r .key <<<"$field")" "$(jq -r .value <<<"$field")"
  done
  jq -c '.failures[]' "$out" | while IFS= read -r field; do
    mg_failure "$(jq -r .id <<<"$field")" "$(jq -r .message <<<"$field")"
  done
  for key in $(jq -r '.details | keys[]' "$out"); do
    mg_detail "$key" "$(jq -c --arg k "$key" '.details[$k]' "$out")"
  done
  mg_message "$(jq -r .message "$out")"
  mg_status "$(jq -r .status "$out")"
}
