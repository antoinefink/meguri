# UI lint gate (U9, R1994, KTD20). Sourced by tools/mg.
#
# Host jobs: SwiftLint runs in the `swiftlint` compose service (pinned image,
# checkout read-only, no network) with app/.swiftlint.yml; the files to lint
# and the allowlist come from tools/lint-ui/path-rules.sh on the host. Gates
# that build or verify the app run `lint-ui` before anything else (AE90).

mg_job lint-ui host mg_lint_ui \
  "UI lint gate (R1994): raw colours, hex literals, fixed font sizes, numeric spacing and radii, literal motion, content shadows, glass outside Chrome/, custom bars, non-SF-Symbol icons; [--root <dir>] lints a tree laid out like the repo"
mg_job lint-ui-selftest host mg_lint_ui_selftest \
  "UI lint gate self-test: every case under tools/lint-ui/tests/fixtures/ must yield exactly its expected.tsv findings"

MG_LINT_UI_CONFIG=app/.swiftlint.yml
MG_LINT_UI_RULES=tools/lint-ui/path-rules.sh

# mg__lint_ui_swiftlint <json out> <files relative to the checkout...>: lints
# the files in the container; returns 0 when SwiftLint produced a JSON report.
mg__lint_ui_swiftlint() {
  local out="$1" rc
  shift
  if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    return 1
  fi
  if [ "$#" -eq 0 ]; then
    echo '[]' >"$out"
    return 0
  fi
  # SwiftLint exits 2 when it found error-level violations; the report decides.
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" swiftlint \
    lint --config "$MG_LINT_UI_CONFIG" --reporter json --no-cache --quiet -- "$@" </dev/null >"$out"
  rc=$?
  if ! jq -e 'type == "array"' "$out" >/dev/null 2>&1; then
    mg_failure swiftlint "SwiftLint exited with code $rc without a JSON report; see log.txt"
    return 1
  fi
}

# mg__lint_ui_files <root>: sets LINT_FILES to the root's linted files,
# relative to the checkout.
mg__lint_ui_files() {
  local root="$1" f
  LINT_FILES=()
  while IFS= read -r -d '' f; do
    if [ "$root" = . ]; then
      LINT_FILES[${#LINT_FILES[@]}]="$f"
    else
      LINT_FILES[${#LINT_FILES[@]}]="$root/$f"
    fi
  done < <(/bin/bash "$MG_LINT_UI_RULES" files "$root")
}

# mg__lint_ui_root_ok <root>: a directory inside the checkout, given relative.
mg__lint_ui_root_ok() {
  case "$1" in
    /* | .. | ../* | */.. | */../*) return 1 ;;
  esac
  [ -d "$1" ]
}

# tools/mg lint-ui [--root DIR]
mg_lint_ui() {
  local root=. usage="usage: tools/mg lint-ui [--root <dir inside the checkout>]"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --root)
        if [ "$#" -lt 2 ] || ! mg__lint_ui_root_ok "$2"; then
          mg_failure usage "--root needs a directory inside the checkout, given relative to it; $usage" "tools/mg lint-ui"
          mg_status error
          return 0
        fi
        root="${2%/}"
        shift 2
        ;;
      *)
        mg_failure usage "unknown argument '$1'; $usage" "tools/mg lint-ui"
        mg_status error
        return 0
        ;;
    esac
  done

  mg__lint_ui_files "$root"
  local report="$MG_RUN_DIR/swiftlint.json" found="$MG_RUN_DIR/findings.jsonl" prefix=/work/
  [ "$root" = . ] || prefix="/work/$root/"
  if ! mg__lint_ui_swiftlint "$report" ${LINT_FILES[0]+"${LINT_FILES[@]}"}; then
    mg_status error
    return 0
  fi
  mg_artifact report "$report" "SwiftLint JSON report"
  if ! /bin/bash "$MG_LINT_UI_RULES" findings "$root" "$MG_LINT_UI_CONFIG" "$report" "$prefix" >"$found"; then
    mg_failure path-rules "tools/lint-ui/path-rules.sh failed; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact findings "$found" "findings after the allowlist (JSON lines)"

  local n=0 line
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    n=$((n + 1))
    mg_failure "lint/$(jq -r .rule <<<"$line")" "$(jq -r '"\(.file):\(.line):\(.column): \(.rule): \(.message)"' <<<"$line")"
  done <"$found"
  # Summary: linted files, with and without findings.
  local bad
  bad="$(jq -r .file "$found" | sort -u | grep -cvxF tools/lint-ui/allowlist.tsv)"
  mg_count total "${#LINT_FILES[@]}"
  mg_count failed "$bad"
  mg_count passed $((${#LINT_FILES[@]} > bad ? ${#LINT_FILES[@]} - bad : 0))
  mg_count findings "$n"
  mg_detail findings "$(jq -sc . "$found")"
  if [ "$n" -eq 0 ]; then
    mg_message "lint-ui: ${#LINT_FILES[@]} file(s) clean"
    mg_status pass
  else
    mg_message "lint-ui: $n finding(s) in ${#LINT_FILES[@]} file(s)"
    mg_status fail
  fi
}

# tools/mg lint-ui-selftest: each fixture case is a tree laid out like the
# repo, with expected.tsv listing `rule<TAB>path:line` (empty: clean). One
# SwiftLint run covers every case.
mg_lint_ui_selftest() {
  local fixtures=tools/lint-ui/tests/fixtures case all=() cases=()
  for case in "$fixtures"/*/; do
    case="${case%/}"
    [ -f "$case/expected.tsv" ] || continue
    cases[${#cases[@]}]="$case"
    mg__lint_ui_files "$case"
    all=(${all[0]+"${all[@]}"} ${LINT_FILES[0]+"${LINT_FILES[@]}"})
  done
  if [ "${#cases[@]}" -eq 0 ]; then
    mg_failure no-cases "no fixture case with an expected.tsv under $fixtures"
    mg_status error
    return 0
  fi
  local report="$MG_RUN_DIR/swiftlint.json"
  if ! mg__lint_ui_swiftlint "$report" ${all[0]+"${all[@]}"}; then
    mg_status error
    return 0
  fi
  mg_artifact report "$report" "SwiftLint JSON report"

  local failed=0 name actual expected
  for case in "${cases[@]}"; do
    name="${case##*/}"
    actual="$MG_RUN_DIR/$name.actual.tsv"
    expected="$MG_RUN_DIR/$name.expected.tsv"
    grep -v '^#' "$case/expected.tsv" | sed '/^[[:space:]]*$/d' | sort >"$expected"
    if ! /bin/bash "$MG_LINT_UI_RULES" findings "$case" "$MG_LINT_UI_CONFIG" "$report" "/work/$case/" >"$actual.jsonl"; then
      mg_failure "case/$name" "path-rules.sh failed on $case; see log.txt" "tools/mg lint-ui --root $case"
      failed=$((failed + 1))
      continue
    fi
    jq -r '"\(.rule)\t\(.file):\(.line)"' "$actual.jsonl" | sort >"$actual"
    if cmp -s "$expected" "$actual"; then
      echo "ok $name" >&2
    else
      failed=$((failed + 1))
      mg_failure "case/$name" "$case: findings differ from expected.tsv (- expected, + actual): $(diff "$expected" "$actual" | grep '^[<>]' | sed 's/^</-/; s/^>/+/' | tr '\n' ';' | sed 's/;$//')" \
        "tools/mg lint-ui --root $case"
    fi
  done
  mg_count total "${#cases[@]}"
  mg_count passed $((${#cases[@]} - failed))
  mg_count failed "$failed"
  if [ "$failed" -eq 0 ]; then
    mg_message "lint-ui-selftest: ${#cases[@]} case(s) pass"
    mg_status pass
  else
    mg_message "lint-ui-selftest: $failed of ${#cases[@]} case(s) differ"
    mg_status fail
  fi
}
