bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$SB/specs/render"
  cp -R "$REPO_ROOT/tools" "$SB/tools"
  cp -R "$REPO_ROOT/docker" "$SB/docker"
  printf '{}\n' >"$SB/specs/render/card-runtime-color-inputs.json"
  git -C "$SB" init -q
  cat >"$SB/tools/mg.d/zz-css-fixture.sh" <<'EOF'
mg_session_role() { printf '%s\n' "${CSS_ROLE:-spec-writer}"; }
mg_compose() { return 0; }
docker() {
  mg_json_array "$@" >"$MG_RUN_DIR/container-args.json"
  [ "${CSS_CRASH:-0}" != 1 ] || return 8
  jq -n --argjson failed "${CSS_MISMATCH:-0}" '
    {schema_version:1,compiler:{version:"1.102.0"},cases:[range(17)|{}],
     totals:{cases:17,pass:(17-$failed),mismatch:$failed}}' >"$MG_RUN_DIR/colors.json"
  [ "${CSS_MISMATCH:-0}" = 0 ]
}
EOF
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

@test "css-reference mounts only neutral facts, runner and output" {
  mg css-reference card-runtime
  [ "$status" -eq 0 ]
  jq -e '.status=="pass" and .summary.passed==17' "$result"
  local args="$(dirname "$result")/container-args.json"
  jq -e '[.[]|select(endswith(":ro"))]|length==2' "$args"
  jq -e '[.[]|select(.=="-v")]|length==3' "$args"
  jq -e 'index("--read-only")!=null and index("--cap-drop")!=null and index("ALL")!=null' "$args"
  ! grep -Eq '/work|/oracle|/refs|/local-data|docker.sock' "$args"
  grep -q '/facts.json:ro' "$args"
  grep -q '/runner.py:ro' "$args"
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "css-reference rejects unapproved inputs and source-restricted roles" {
  mg css-reference ../card-runtime
  [ "$status" -eq 2 ]
  [ ! -f "$(dirname "$result")/container-args.json" ]
  mg css-reference card-runtime --out /elsewhere
  [ "$status" -eq 2 ]
  export CSS_ROLE=implementer
  mg css-reference card-runtime
  [ "$status" -eq 2 ]
  jq -e '.failures[0].id=="role"' "$result"
  [ ! -f "$(dirname "$result")/container-args.json" ]
}

@test "css-reference distinguishes comparison failure from compiler crash" {
  export CSS_MISMATCH=1
  mg css-reference card-runtime
  [ "$status" -eq 1 ]
  jq -e '.status=="fail" and .summary.failed==1' "$result"
  export CSS_CRASH=1
  mg css-reference card-runtime
  [ "$status" -eq 2 ]
  jq -e '.status=="error" and .failures[0].id=="compiler"' "$result"
}
