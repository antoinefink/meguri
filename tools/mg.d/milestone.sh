# Milestone and release runs (U19: R1950 T3/T4, R1880, R1953;
# docs/process/tiers.md). Sourced by tools/mg.
#
# `tools/mg milestone <M0..M5|release>` runs every T0 and T1 run in full (no
# path triggers), then T3, and lists T4 (device runs, U256) as user steps. It
# writes one summary result. On top of the runs' own results it fails while
# any conformance, sync or data-safety test is quarantined (R1953), and its
# harvest-coverage run fails on any unclassified row, and at release on any
# in-scope or preservation-only scenario that does not pass (R1880). U240's
# `milestone-gate` evaluates a milestone from these results and the manifest.
# It runs in the main checkout on a clean tree: the private suite and the
# similarity scan need it, and a result from a dirty tree proves nothing.

mg_job milestone host mg_milestone \
  "Milestone and release runs (R1950 T3/T4, R1880, R1953): <M0..M5|release>; every T0 and T1 run in full, then T3, T4 as user steps; fails while a conformance, sync or data-safety test is quarantined; main checkout, clean tree; one summary result"

mg_milestone() {
  local which="${1:-}" mode=milestone
  [ "$#" -le 1 ] || { mg_gate__usage "usage: tools/mg milestone <M0..M5|release>"; return 0; }
  case "$which" in
    M[0-5]) ;;
    release) mode=release ;;
    *) mg_gate__usage "usage: tools/mg milestone <M0..M5|release>"; return 0 ;;
  esac
  if ! mg_gate__in_main; then
    mg_gate__usage "tools/mg milestone runs in the main checkout ($(mg_gate__main_checkout)): the private suite and the similarity scan need it"
    return 0
  fi
  if [ "${MG_COMMIT_DIRTY:-false}" != false ]; then
    mg_gate__usage "the tree has uncommitted or untracked files; a milestone result must be for a commit"
    return 0
  fi
  mg_gate__scheduled "$mode" --var "milestone=$which" || return 0
  # T4 runs are Antoine's (U256): awaiting him, never passed by this run.
  local awaiting
  awaiting="$(jq -c '[.runs[] | select(.outcome == "user-step") | {id, message}]' "$MG_RUN_DIR/receipt.json")"
  mg_detail awaiting_user "$awaiting"
  mg_detail milestone "$(jq -nc --arg m "$which" '$m')"
  mg_message "milestone $which: $(sed 's/^milestone: //' "$MG_STATE/message")$([ "$awaiting" = "[]" ] || echo "; awaiting Antoine: $(jq -r 'map(.id) | join(", ")' <<<"$awaiting")")"
  if [ "$(head -n 1 "$MG_STATE/status")" = pass ] && [ "$awaiting" != "[]" ]; then
    mg_status warn
  fi
}
