# Floating-point determinism probe (specs/conformance/float-determinism.md
# F10). Sourced by tools/mg.
#
# A host job on an arm64 Mac: the Apple oracle side is confirmed with a
# nested `tools/mg oracle compare --platform apple fp-probe/corpus`, Meguri's
# side runs as `meguri-cli fp-probe eval` (release build) natively here and
# in the Linux dev container, and `meguri-cli fp-probe report` compares the
# four sides. The report is build/mg/fp-probe/<run id>/report.json.

mg_job fp-probe host mg_fp_probe \
  "Floating-point determinism probe (float-determinism.md F10), arm64 Mac only: tools/mg oracle compare --platform apple fp-probe/corpus confirms the Apple oracle side, meguri-cli fp-probe eval (release) runs natively and in the Linux dev container, then the four-side report; [--jobs N] (native oracle workers, default 8)"

mg_fp_probe__usage() {
  mg_failure usage "usage: tools/mg fp-probe [--jobs N]" "tools/mg fp-probe --help"
  mg_message "fp-probe: $1"
  mg_status error
}

# tools/mg fp-probe [--jobs N]
mg_fp_probe() {
  local jobs=8 dir="$MG_RUN_DIR" compare status message rc commit
  local unavailable=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --jobs) jobs="${2:-}"; shift 2 || { mg_fp_probe__usage "--jobs needs a number"; return 0; } ;;
      --help) mg_fp_probe__usage "help"; return 0 ;;
      *) mg_fp_probe__usage "unexpected argument '$1'"; return 0 ;;
    esac
  done
  case "$jobs" in '' | *[!0-9]* | 0) mg_fp_probe__usage "--jobs needs a positive number"; return 0 ;; esac
  if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    mg_failure platform "the probe needs an arm64 Mac: the Apple oracle and Meguri-macOS sides run natively" \
      "tools/mg fp-probe (on the Mac)"
    mg_message "fp-probe: refused, not an arm64 Mac"
    mg_status error
    return 0
  fi
  mg_conf__docker_ok || return 0
  trap 'mg_conf__cleanup; exit 143' TERM

  # F10.1 apple_oracle: the overlays must still hold every Apple value.
  echo "fp-probe: tools/mg oracle compare --platform apple fp-probe/corpus" >&2
  compare="$("$MG_TOOLS/mg" oracle compare --platform apple --jobs "$jobs" fp-probe/corpus </dev/null)"
  compare="${compare##*$'\n'}"
  if [ -z "$compare" ] || [ ! -f "$compare" ]; then
    unavailable+=(--unavailable "apple_oracle=tools/mg oracle compare --platform apple wrote no result")
  else
    mg_artifact oracle-compare "$compare" "tools/mg oracle compare --platform apple fp-probe/corpus"
    status="$(jq -r .status "$compare")"
    if [ "$status" != pass ]; then
      message="$(jq -r '.message + ([.failures[:3][] | "; " + .id + ": " + .message] | add // "")' "$compare")"
      unavailable+=(--unavailable "apple_oracle=Apple side not reproduced ($status): $message")
    fi
  fi

  # F10.1 meguri_macos: release build, natively.
  echo "fp-probe: meguri-cli fp-probe eval on this Mac" >&2
  if (cd "$MG_ROOT/core" && cargo build --release -q -p meguri-cli) &&
    "$MG_ROOT/core/target/release/meguri-cli" fp-probe eval --conformance "$MG_ROOT/conformance" \
      --out "$dir/meguri-macos.json"; then
    :
  else
    unavailable+=(--unavailable "meguri_macos=meguri-cli fp-probe eval failed on this Mac; see log.txt")
  fi

  # F10.1 meguri_linux: release build in the dev container.
  echo "fp-probe: meguri-cli fp-probe eval in the Linux dev container" >&2
  if mg_conf__dev -- "cd core && cargo build --release -q -p meguri-cli && \
      /cargo-target/release/meguri-cli fp-probe eval --conformance /work/conformance \
      --out /work/$(mg_conf__rel "$dir")/meguri-linux.json"; then
    :
  else
    unavailable+=(--unavailable "meguri_linux=meguri-cli fp-probe eval failed in the dev container; see log.txt")
  fi

  commit="$(git -C "$MG_ROOT" rev-parse HEAD 2>/dev/null)"
  "$MG_ROOT/core/target/release/meguri-cli" fp-probe report --conformance "$MG_ROOT/conformance" \
    --meguri-macos "$dir/meguri-macos.json" --meguri-linux "$dir/meguri-linux.json" \
    --commit "${commit:-unknown}" ${unavailable[0]+"${unavailable[@]}"} --out "$dir/report.json"
  rc=$?
  if [ ! -s "$dir/report.json" ] || [ "$rc" -ge 2 ]; then
    mg_failure no-report "meguri-cli fp-probe report exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$dir/report.json" "fp-probe report (float-determinism.md F10.3)"
  mg_detail sides "$(jq -c '.sides' "$dir/report.json")"
  mg_detail pairs "$(jq -c '[.pairs[] | del(.differences)]' "$dir/report.json")"
  mg_detail fuzz "$(jq -c '.fuzz | .mismatches |= length' "$dir/report.json")"
  jq -c '(.sides | to_entries[] | select(.value.error) | {id: ("side:" + .key), message: .value.error, repro: "tools/mg fp-probe"}),
    (.pairs[] | select(.gate and ((.differences // []) | length > 0))
      | {id: ("pair:" + .left + "-" + .right),
         message: ("\(.differing_inputs) inputs differ, first input \(.differences[0].id) \(.differences[0].name): \(.differences[0].left) vs \(.differences[0].right)"),
         repro: "tools/mg fp-probe"}),
    (.fuzz | select((.mismatches | length) > 0 or .error)
      | {id: "fuzz", message: (.error // "\(.mismatches | length) fuzz table cells differ, first card \(.mismatches[0].card_id) reps \(.mismatches[0].reps) interval \(.mismatches[0].interval)"),
         repro: "tools/mg fp-probe"})' "$dir/report.json" >>"$MG_STATE/failures.jsonl"
  message="$(jq -r '[.pairs[] | select(.error | not) | "\(.left) vs \(.right): \(.differing_inputs) inputs, \(.differing_days) days, \(.differing_memory) memory"] | join("; ")' "$dir/report.json")"
  if [ "$(jq -r .status "$dir/report.json")" = pass ]; then
    mg_message "fp-probe: pass; $message; fuzz table agrees on every side"
    mg_status pass
  else
    mg_message "fp-probe: fail; $message"
    mg_status fail
  fi
}
