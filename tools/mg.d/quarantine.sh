# Test quarantine (U19, R1953; docs/process/tiers.md). Sourced by tools/mg.
#
# tools/gate/quarantine.yaml lists flaky tests that run and report but do not
# block the gate until their deadline. An entry names a tools/gate/tiers.yaml
# run and the failure id that run's job reports. `add` is the only writer: it
# checks the gate-recorded evidence (two receipts of one commit, the test
# failing in one and its run passing in the other) and Antoine's approval,
# then appends the entry and validates the whole file again.

mg_job quarantine host mg_quarantine \
  "Quarantined tests (R1953): check | add --run <tiers run id> --test <failure id> --class <class> --owner <owner> --reason <text> --deadline YYYY-MM-DD --failed <gate run id|receipt.json> --passed <gate run id|receipt.json> --approved-by antoine [--escalation <id>]"

mg_quarantine__usage() {
  mg_failure usage "usage: tools/mg quarantine check | add --run <run id> --test <id> --class <class> --owner <owner> --reason <text> --deadline YYYY-MM-DD --failed <gate run|receipt> --passed <gate run|receipt> --approved-by antoine [--escalation <id>]; $1"
  mg_message "quarantine: $1"
  mg_status error
}

# mg_quarantine__receipt <gate run id or receipt path>: the receipt's path.
mg_quarantine__receipt() {
  if [ -f "$1" ]; then
    (cd "$(dirname "$1")" && printf '%s/%s\n' "$(pwd -P)" "$(basename "$1")")
  elif [ -f "$MG_ROOT/build/mg/gate/$1/receipt.json" ]; then
    printf '%s\n' "$MG_ROOT/build/mg/gate/$1/receipt.json"
  else
    return 1
  fi
}

mg_quarantine__report() {
  local config="$1"
  mg_detail quarantine "$(jq -c --arg day "$(mg_gate__now | cut -c1-10)" '.quarantine | {
      active: [.entries[] | select(.deadline >= $day) | {run, test, class, owner, deadline}],
      expired: [.entries[] | select(.deadline < $day) | {run, test, class, owner, deadline}],
      invalid: .invalid}' "$config")"
}

mg_quarantine() {
  local sub="${1:-}"
  [ "$#" -gt 0 ] && shift
  local config="$MG_RUN_DIR/config.json"
  case "$sub" in
    check)
      [ "$#" -eq 0 ] || { mg_quarantine__usage "check takes no arguments"; return 0; }
      mg_gate__load "$config" || return 0
      mg_quarantine__report "$config"
      local day expired invalid total
      day="$(mg_gate__now | cut -c1-10)"
      total="$(jq '.quarantine.entries | length' "$config")"
      expired="$(jq --arg day "$day" '[.quarantine.entries[] | select(.deadline < $day)] | length' "$config")"
      invalid="$(jq '.quarantine.invalid | length' "$config")"
      jq -c --arg day "$day" '.quarantine.entries[] | select(.deadline < $day)
        | {id: ("expired:" + .run + ":" + .test), message: ("deadline " + .deadline + " passed (owner " + .owner + ")"), repro: "tools/mg quarantine check"}' \
        "$config" >>"$MG_STATE/failures.jsonl"
      jq -c '.quarantine.invalid[] | {id: ("invalid:" + .label), message: (.errors | join("; ")), repro: "tools/mg quarantine check"}' \
        "$config" >>"$MG_STATE/failures.jsonl"
      mg_count total $((total + invalid))
      mg_count failed $((expired + invalid))
      mg_count passed $((total - expired))
      if [ $((expired + invalid)) -gt 0 ]; then
        mg_message "quarantine: $expired expired, $invalid invalid of $((total + invalid)) entries"
        mg_status fail
      else
        mg_message "quarantine: $total active entries"
        mg_status pass
      fi
      ;;
    add)
      local run="" test="" cls="" owner="" reason="" deadline="" failed="" passed="" approved="" escalation=""
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --run | --test | --class | --owner | --reason | --deadline | --failed | --passed | --approved-by | --escalation)
            [ "$#" -ge 2 ] || { mg_quarantine__usage "$1 needs a value"; return 0; }
            case "$1" in
              --run) run="$2" ;; --test) test="$2" ;; --class) cls="$2" ;; --owner) owner="$2" ;;
              --reason) reason="$2" ;; --deadline) deadline="$2" ;; --failed) failed="$2" ;; --passed) passed="$2" ;;
              --approved-by) approved="$2" ;; --escalation) escalation="$2" ;;
            esac
            shift 2
            ;;
          *) mg_quarantine__usage "unknown argument '$1'"; return 0 ;;
        esac
      done
      local fr pr
      fr="$(mg_quarantine__receipt "$failed")" || { mg_quarantine__usage "--failed: no gate receipt '$failed' (a gate run id under build/mg/gate/, or a receipt.json)"; return 0; }
      pr="$(mg_quarantine__receipt "$passed")" || { mg_quarantine__usage "--passed: no gate receipt '$passed'"; return 0; }
      mg_gate__load "$config" || return 0
      local block="$MG_RUN_DIR/entry.yaml" errors="$MG_RUN_DIR/entry-errors.json"
      if ! mg_gate__python quarantine-entry --config "$config" --run "$run" --test "$test" --class "$cls" --owner "$owner" \
        --reason "$reason" --deadline "$deadline" --approved-by "$approved" --escalation "$escalation" \
        --failed-run "$(basename "$(dirname "$fr")")" --passed-run "$(basename "$(dirname "$pr")")" \
        --failed-receipt "$fr" --passed-receipt "$pr" --now "$(mg_gate__now)" --out "$block" >"$errors"; then
        jq -c '.errors[]? | {id: "rejected", message: ., repro: "tools/mg quarantine add ..."}' "$errors" >>"$MG_STATE/failures.jsonl"
        [ -s "$MG_STATE/failures.jsonl" ] || mg_failure rejected "the entry was rejected; see log.txt"
        mg_message "quarantine: $run:$test rejected ($(jq -r '.errors[0] // "see log.txt"' "$errors" 2>/dev/null))"
        mg_status fail
        return 0
      fi
      local file="$MG_ROOT/$MG_GATE_QUARANTINE"
      cp "$file" "$MG_RUN_DIR/quarantine.before.yaml"
      cat "$block" >>"$file"
      if ! mg_gate__load "$MG_RUN_DIR/config-after.json" ||
        [ "$(jq '.quarantine.invalid | length' "$MG_RUN_DIR/config-after.json")" -gt "$(jq '.quarantine.invalid | length' "$config")" ] ||
        [ "$(jq '.quarantine.entries | length' "$MG_RUN_DIR/config-after.json")" -ne $(($(jq '.quarantine.entries | length' "$config") + 1)) ]; then
        cp "$MG_RUN_DIR/quarantine.before.yaml" "$file"
        mg_failure rejected "quarantine.yaml would not validate with the entry; it was left unchanged"
        mg_status fail
        return 0
      fi
      mg_quarantine__report "$MG_RUN_DIR/config-after.json"
      mg_message "quarantine: $run:$test added until $deadline (owner $owner); commit tools/gate/quarantine.yaml"
      mg_status pass
      ;;
    *) mg_quarantine__usage "a command is required" ;;
  esac
}
