# Post-process only the synthetic replay UI case. Keep the real test result
# counts and check the actual exports, including every action boundary.
mg_mac_replay_native_report() {
  local prefix="$1" result="$2" out="$1-proof" raw summary map stage name file count rc action phase screenshot
  local files=() names=()
  mkdir -p "$out"
  summary="$(set -o pipefail
    xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
      jq -ce '{passed: .passedTests, failed: .failedTests, skipped: .skippedTests, outcome: .result}
        | if all(.passed, .failed, .skipped; type == "number" and . >= 0 and . == floor)
          then . else error("invalid test counts") end')" || summary=""
  if [ -z "$summary" ]; then
    mg_failure replay-native-results "native replay test summary is missing or malformed"
    return 2
  fi
  printf '%s\n' "$summary" >"$out/test-summary.json"
  mg_detail replay_native_tests "$summary"
  mg_artifact report "$out/test-summary.json" "Actual native replay test counts and outcome"
  mg_artifact xcresult "$prefix.xcresult" "Native replay UI result bundle"
  if ! xcrun xcresulttool get test-results tests --path "$prefix.xcresult" >"$out/test-cases.json"; then
    mg_failure replay-native-results "native replay test case results could not be exported"
    return 2
  fi
  mg_artifact report "$out/test-cases.json" "Actual native replay executed cases"
  if [ "$result" -eq 0 ] && ! jq -e '.outcome == "Passed" and .passed == 1 and .failed == 0 and .skipped == 0' <<<"$summary" >/dev/null; then
    mg_failure replay-native-results "native replay proof requires one executed passing test, zero failures and zero skips"
    result=1
  fi
  raw="$out/attachments"
  if ! mg_drive__export_attachments "$prefix.xcresult" "$raw" >"$out/export.log" 2>&1; then
    mg_failure replay-native-export "native replay attachments could not be exported"
    return 2
  fi
  map="$(jq -ce '[.[].attachments[]
    | {name: (.suggestedHumanReadableName | sub("_[0-9]+_[0-9A-Fa-f-]{36}(?<e>\\.[A-Za-z0-9]+)$"; "\(.e)")), file: .exportedFileName}]' "$raw/manifest.json")" || return 2
  for stage in 0 1 2 3; do names[${#names[@]}]="replay-native-$stage"; done
  # 20 replay presses, 20 checkpoints and three side/card changes.
  for ((action=0; action<43; action++)); do
    for phase in before after; do names[${#names[@]}]="replay-native-action-$action-$phase"; done
  done
  for name in "${names[@]}"; do
    screenshot="$name.png"
    name="$name.json"
    count="$(jq --arg name "$name" '[.[] | select(.name == $name)] | length' <<<"$map")"
    if [ "$count" -ne 1 ]; then
      mg_failure replay-native-export "expected one native hierarchy for $name; found $count"
      result=1
      continue
    fi
    file="$(jq -r --arg name "$name" '.[] | select(.name == $name) | .file' <<<"$map")"
    case "$file" in ""|.*|*/*|*\\*) mg_failure replay-native-export "invalid exported attachment basename"; return 2 ;; esac
    if ! cp "$raw/$file" "$out/$name"; then
      mg_failure replay-native-export "native hierarchy for $name is unreadable"
      return 2
    fi
    files[${#files[@]}]="$(mg_container_path "$out/$name")"
    mg_artifact hierarchy "$out/$name" "Actual native replay hierarchy, $name"
    count="$(jq --arg name "$screenshot" '[.[] | select(.name == $name)] | length' <<<"$map")"
    if [ "$count" -ne 1 ]; then
      mg_failure replay-native-export "expected one screenshot for $name; found $count"
      result=1
      continue
    fi
    file="$(jq -r --arg name "$screenshot" '.[] | select(.name == $name) | .file' <<<"$map")"
    case "$file" in ""|.*|*/*|*\\*) mg_failure replay-native-export "invalid screenshot attachment basename"; return 2 ;; esac
    cp "$raw/$file" "$out/$screenshot" || return 2
    mg_artifact png "$out/$screenshot" "Native replay screenshot, $screenshot"
  done
  [ "${#files[@]}" -gt 0 ] || return 1
  mg_image_tools python3 tools/scripts/hierarchy-check/hierarchy_check.py \
    --report "$(mg_container_path "$out/hierarchy-check.json")" "${files[@]}" >&2
  rc=$?
  if [ "$rc" -gt 1 ] || [ ! -s "$out/hierarchy-check.json" ]; then
    mg_failure replay-native-hierarchy "native hierarchy checker could not run"
    return 2
  fi
  mg_artifact report "$out/hierarchy-check.json" "Hierarchy findings with the named D4 rule; all other findings retained"
  mg_detail replay_native_identity_issues "$(jq '[.files[].issues[] | select(.kind == "native-web-identity")] | length' "$out/hierarchy-check.json")"
  mg_detail replay_native_other_issues "$(jq '[.files[].issues[] | select(.kind != "native-web-identity")] | length' "$out/hierarchy-check.json")"
  mg_detail replay_native_hierarchies "${#files[@]}"
  if [ "$rc" -ne 0 ]; then
    mg_failure replay-native-hierarchy "native replay hierarchy has findings; see the checker report"
    result=1
  fi
  return "$result"
}
