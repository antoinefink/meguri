# The design critic (U10, R1983-R1987). Sourced by tools/mg; a host job.
#
# `tools/mg critic <snapshots run>` builds a packet from the run's images
# (tools/scripts/critic, in image-tools), records a critic session (U2's
# ledger, role critic), and starts a fresh headless Claude session in a copy
# of the packet outside the home directory: restricted mode (file tools
# confined to the packet), safe mode (no CLAUDE.md, hooks, MCP servers or
# skills), only Read, Glob and Grep, and the .claude/settings.critic.json
# deny profile. Its answer is checked against docs/design/critic-issue.schema.json,
# the rubric and the packet; an invalid answer is an error and records
# nothing. A valid one goes into the screen status ledger with the evidence
# of the snapshot run (which must cover every platform the snapshot matrix
# registers each screen on) and of the accessibility runs, one per platform
# (--a11y, repeatable). Every run is a new session (R1984).
# MEGURI_CRITIC_CMD replaces `claude` (self-tests use a stub).

mg_job critic host mg_critic \
  "Design critic (R1983-R1987): <snapshots run id|path|latest> [--screen <id>]... [--a11y <a11y run>]... [--drive <drive run>] [--checks <file>] [--ledger <path>]; a fresh restricted Claude session grades the packet against docs/design/rubric.md; validated issues go into docs/design/screen-status.yaml. --probe-isolation checks that a critic session cannot read outside its packet"

MG_CRITIC_TOOLS="Read,Glob,Grep"

mg_critic__usage() {
  mg_failure usage "critic: $1" "tools/mg --list"
  mg_message "critic: $1"
  mg_status error
}

# mg_critic__run_dir <job> <run id|path|latest>: a finished run's directory.
mg_critic__run_dir() {
  local job="$1" ref="$2" dir
  case "$ref" in
    /*) dir="$ref" ;;
    build/*) dir="$MG_ROOT/$ref" ;;
    *) dir="$MG_ROOT/build/mg/$job/$ref" ;;
  esac
  dir="$(cd "$dir" 2>/dev/null && pwd -P)" || return 1
  [ -f "$dir/result.json" ] && [ "$(jq -r .job "$dir/result.json")" = "$job" ] || return 1
  printf '%s\n' "$dir"
}

# mg_critic__session <prompt> <copy dir> <output file> [extra args]: one
# headless critic session in <copy dir>; returns its exit status.
mg_critic__session() {
  local prompt="$1" copy="$2" out="$3" pid waited=0 limit="${MEGURI_CRITIC_TIMEOUT_SECONDS:-1800}"
  shift 3
  (
    cd "$copy" || exit 2
    exec "${MEGURI_CRITIC_CMD:-claude}" -p "$prompt" --restricted --safe-mode --strict-mcp-config \
      --tools "$MG_CRITIC_TOOLS" --disable-slash-commands --settings "$MG_ROOT/.claude/settings.critic.json" \
      --permission-mode dontAsk --model "${MEGURI_CRITIC_MODEL:-opus}" --no-session-persistence "$@"
  ) </dev/null >"$out" 2>"$out.stderr" &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$waited" -ge "$limit" ]; then
      kill "$pid" 2>/dev/null
      wait "$pid" 2>/dev/null
      echo "critic: the session ran past $limit s and was stopped" >&2
      return 124
    fi
    mg_sleep 1
    waited=$((waited + 1))
  done
  wait "$pid"
}

# mg_critic__copy: a fresh directory for the packet copy, outside the home
# directory (the deny profile refuses everything under it), in
# MG_CRITIC_COPY; removed when the job ends. Call it in the job's shell, not
# in a command substitution (whose exit would remove it at once).
mg_critic__copy() {
  local base="${TMPDIR:-/tmp}"
  base="$(cd "$base" && pwd -P)"
  case "$base/" in "$HOME"/*) base=/tmp ;; esac
  MG_CRITIC_COPY="$(mktemp -d "$base/meguri-critic.XXXXXX")" || return 1
  mg_on_exit "rm -rf '$MG_CRITIC_COPY'"
}

mg_critic() {
  local snapshots="" a11y=() drive="" checks="" ledger="docs/design/screen-status.yaml" screens=() probe=false s
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --screen)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[a-z][A-Za-z0-9]*\.[a-z][A-Za-z0-9]*$' ||
          { mg_critic__usage "--screen takes a screen id (area.screen)"; return 0; }
        screens[${#screens[@]}]="$2" && shift 2
        ;;
      --a11y) [ "$#" -ge 2 ] || { mg_critic__usage "--a11y needs an a11y run"; return 0; }; a11y[${#a11y[@]}]="$2" && shift 2 ;;
      --drive) [ "$#" -ge 2 ] || { mg_critic__usage "--drive needs a drive run"; return 0; }; drive="$2" && shift 2 ;;
      --checks) [ "$#" -ge 2 ] && [ -f "$2" ] || { mg_critic__usage "--checks needs a checks file"; return 0; }; checks="$2" && shift 2 ;;
      --ledger) [ "$#" -ge 2 ] || { mg_critic__usage "--ledger needs a path"; return 0; }; ledger="$2" && shift 2 ;;
      --probe-isolation) probe=true && shift ;;
      -*) mg_critic__usage "unknown option '$1'"; return 0 ;;
      *) [ -z "$snapshots" ] || { mg_critic__usage "one snapshots run at a time"; return 0; }; snapshots="$1" && shift ;;
    esac
  done
  if [ "$probe" = true ]; then
    mg_critic__probe
    return 0
  fi
  [ -n "$snapshots" ] || { mg_critic__usage "usage: tools/mg critic <snapshots run> [--screen <id>]... [--a11y <run>]... [--drive <run>] [--ledger <path>]"; return 0; }
  local snap_dir drive_dir="" a11y_args=() dir
  snap_dir="$(mg_critic__run_dir snapshots "$snapshots")" || { mg_critic__usage "no finished tools/mg snapshots run '$snapshots'"; return 0; }
  for s in ${a11y[0]+"${a11y[@]}"}; do
    dir="$(mg_critic__run_dir a11y "$s")" || { mg_critic__usage "no finished tools/mg a11y run '$s'"; return 0; }
    a11y_args=(${a11y_args[0]+"${a11y_args[@]}"} --a11y "$(mg_container_path "$dir")")
  done
  if [ -n "$drive" ]; then
    drive_dir="$(mg_critic__run_dir drive "$drive")" || { mg_critic__usage "no finished tools/mg drive run '$drive'"; return 0; }
  fi
  case "$ledger" in /*) ;; *) ledger="$MG_ROOT/$ledger" ;; esac
  mg_container_path "$ledger" >/dev/null || { mg_critic__usage "the ledger must be inside the checkout"; return 0; }
  [ -z "$checks" ] || checks="$(cd "$(dirname "$checks")" && pwd -P)/$(basename "$checks")"

  # The packet.
  local packet="$MG_RUN_DIR/packet" args=()
  for s in ${screens[0]+"${screens[@]}"}; do args=(${args[0]+"${args[@]}"} --screen "$s"); done
  [ -z "$drive_dir" ] || args=(${args[0]+"${args[@]}"} --drive "$(mg_container_path "$drive_dir")")
  [ -z "$checks" ] || args=(${args[0]+"${args[@]}"} --checks "$(mg_container_path "$checks")")
  # The a11y runs too: one that audited other app sources than the snapshots
  # rendered is refused before a session starts.
  args=(${args[0]+"${args[@]}"} ${a11y_args[0]+"${a11y_args[@]}"})
  if ! mg_image_tools python3 tools/scripts/critic/critic.py packet --snapshots "$(mg_container_path "$snap_dir")" \
    --out "$(mg_container_path "$packet")" --host-root "$MG_ROOT" --run "$MG_RUN_ID" ${args[0]+"${args[@]}"} >&2; then
    mg_failure packet "could not build the critic packet from $snap_dir; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact packet "$packet" "critic packet"
  mg_detail packet "$(jq -c '{screens: [.screens[].id], images: (.images | length), contact_sheets: (.contact_sheets | length), rubric_version}' "$packet/packet.json")"

  # The session: recorded first (R1958), then run in a fresh copy.
  local record session copy out="$MG_RUN_DIR/critic-output.json" rc
  record="$("$MG_TOOLS/mg" session record --role critic --name "critic-$MG_RUN_ID" </dev/null 2>>"$MG_RUN_DIR/log.txt" | tail -n 1)"
  session="$(jq -r '.details.session.session // empty' "$record" 2>/dev/null)"
  if [ -z "$session" ]; then
    mg_failure session-record "could not record the critic session (tools/mg session record --role critic); the critic did not run" "tools/mg session record --role critic"
    mg_status error
    return 0
  fi
  mg_detail session "$(jq -n --arg s "$session" '$s')"
  mg_critic__copy || { mg_failure packet-copy "could not create the packet copy"; mg_status error; return 0; }
  copy="$MG_CRITIC_COPY"
  cp -R "$packet/." "$copy/"
  echo "critic: session $session grades $(jq -r '[.screens[].id] | join(", ")' "$packet/packet.json") in $copy" >&2
  # The CLI's schema check does not know the 2020-12 meta-schema id; the
  # answer is checked against the full schema afterwards anyway.
  mg_critic__session "$(mg_critic__prompt)" "$copy" "$out" --output-format json \
    --json-schema "$(jq -c 'del(."$schema", ."$id")' "$packet/schema.json")" --append-system-prompt "$(cat "$packet/role.md")"
  rc=$?
  mg_artifact critic-output "$out" "the critic's answer"
  if [ "$rc" -ne 0 ]; then
    mg_failure critic-session "the critic session exited with $rc: $(head -c 400 "$out.stderr" 2>/dev/null)"
    mg_status error
    return 0
  fi

  # The answer: checked, then recorded.
  local report="$MG_RUN_DIR/report.json"
  if ! mg_image_tools python3 tools/scripts/critic/critic.py validate --packet "$(mg_container_path "$packet")" \
    --output "$(mg_container_path "$out")" --report "$(mg_container_path "$report")" >"$MG_RUN_DIR/validate.json" 2>>"$MG_RUN_DIR/log.txt"; then
    local errors
    errors="$(grep -o '{"valid".*' "$MG_RUN_DIR/validate.json" | tail -n 1)"
    if [ -n "$errors" ]; then
      jq -r '.errors[]' <<<"$errors" | while IFS= read -r line; do mg_failure critic-output-invalid "$line"; done
    else
      mg_failure critic-output-invalid "the critic's answer could not be checked; see log.txt"
    fi
    mg_message "critic: the answer was rejected; no issue recorded"
    mg_status error
    return 0
  fi
  mg_artifact report "$report" "validated critic issues"
  if ! mg_image_tools python3 tools/scripts/critic/critic.py apply --report "$(mg_container_path "$report")" \
    --packet "$(mg_container_path "$packet")" ${a11y_args[0]+"${a11y_args[@]}"} \
    --ledger "$(mg_container_path "$ledger")" --session "$session" \
    --commit "$(git -C "$MG_ROOT" rev-parse --verify -q HEAD)" >"$MG_RUN_DIR/apply.json" 2>>"$MG_RUN_DIR/log.txt"; then
    mg_failure ledger "could not record the issues in ${ledger#"$MG_ROOT"/}; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact ledger "$ledger" "screen status ledger"
  local statuses high applied
  applied="$(grep -o '{"statuses".*' "$MG_RUN_DIR/apply.json" | tail -n 1)"
  statuses="$(jq -c .statuses <<<"${applied:-null}")"
  mg_detail statuses "${statuses:-null}"
  mg_detail evidence "$(jq -c '.evidence // null' <<<"${applied:-null}")"
  mg_detail issues "$(jq -c '{high: [.issues[] | select(.severity == "high")] | length, medium: [.issues[] | select(.severity == "medium")] | length, low: [.issues[] | select(.severity == "low")] | length, dropped: (.dropped | length)}' "$report")"
  high="$(jq '[.issues[] | select(.severity == "high")] | length' "$report")"
  mg_count total "$(jq '.issues | length' "$report")"
  mg_count failed "$high"
  jq -c '.issues[] | select(.severity == "high")' "$report" | while IFS= read -r s; do
    mg_failure "high/$(jq -r '"\(.screen)/\(.state)/\(.variant)"' <<<"$s")" "$(jq -r '"\(.rule): \(.summary) (region \(.region | "\(.x),\(.y) \(.width)x\(.height)"))"' <<<"$s")"
  done
  mg_message "critic: $(jq -r 'to_entries | map("\(.key) \(.value)") | join(", ")' <<<"${statuses:-{\}}"); $(jq -r '"\([.issues[] | select(.severity == "high")] | length) high, \([.issues[] | select(.severity == "medium")] | length) medium, \([.issues[] | select(.severity == "low")] | length) low, \(.dropped | length) dropped"' "$report")"
  # High issues keep screens out of verified; they never fail a merge (R1985).
  if [ "$high" -gt 0 ]; then mg_status warn; else mg_status pass; fi
}

mg_critic__prompt() {
  cat <<'EOF'
Grade the screens in this packet as the critic (your role is in the system prompt and in role.md). Read packet.json, rubric.md, brief.md and the calibration/ files, look at every image packet.json lists and at the references of each screen's patterns, then answer with only the JSON object schema.json describes.
EOF
}

# tools/mg critic --probe-isolation: a probe session asked to read a file in
# its packet and one outside it (this checkout's AGENTS.md). Passes when the
# first read works and the second is refused.
mg_critic__probe() {
  local copy out="$MG_RUN_DIR/probe.jsonl" outside="$MG_ROOT/AGENTS.md" inside_ok outside_refused
  mg_critic__copy || { mg_failure packet-copy "could not create the packet copy"; mg_status error; return 0; }
  copy="$MG_CRITIC_COPY"
  echo "inside the packet: PROBE-INSIDE" >"$copy/inside.txt"
  mg_critic__session "Use the Read tool on ./inside.txt, then use the Read tool on $outside. Report each result in one line." \
    "$copy" "$out" --output-format stream-json --verbose
  mg_artifact probe "$out" "isolation probe session (stream-json)"
  inside_ok="$(jq -r 'select(.type == "user") | .message.content[]? | select(.type == "tool_result" and (.is_error | not)) | .content | tostring | select(contains("PROBE-INSIDE"))' "$out" 2>/dev/null | head -n 1)"
  outside_refused="$(jq -r 'select(.type == "user") | .message.content[]? | select(.type == "tool_result" and .is_error == true) | .content | tostring' "$out" 2>/dev/null | head -n 1)"
  local leaked
  # AGENTS.md's own text in any successful tool result means it leaked.
  leaked="$(jq -r 'select(.type == "user") | .message.content[]? | select(.type == "tool_result" and (.is_error | not)) | .content | tostring' "$out" 2>/dev/null |
    grep -c 'Hard rules')"
  mg_detail probe "$(jq -n --arg i "$inside_ok" --arg o "$outside_refused" --argjson l "${leaked:-0}" '{inside_read: ($i != ""), outside_refused: $o, outside_leaked: ($l > 0)}')"
  if [ -n "$inside_ok" ] && [ -n "$outside_refused" ] && [ "${leaked:-0}" = 0 ]; then
    mg_message "critic: a critic session reads its packet and is refused outside it: $(printf '%s' "$outside_refused" | head -c 160)"
    mg_status pass
  else
    mg_failure isolation "the probe session $([ -n "$inside_ok" ] || echo "could not read its packet")$([ -n "$inside_ok" ] && [ -z "$outside_refused" ] && echo "was not refused outside its packet")"
    mg_status fail
  fi
}
