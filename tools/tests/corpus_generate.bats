#!/usr/bin/env bats
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$SB/conformance/corpora/type-answer" "$SB/conformance/scenarios/render/type-answer/corpus"
  cp -R "$REPO_ROOT/tools" "$SB/tools"
  cp -R "$REPO_ROOT/docker" "$SB/docker"
  printf 'generator fixture\n' >"$SB/conformance/corpora/type-answer/generate.py"
  printf 'keep\n' >"$SB/conformance/scenarios/render/type-answer/corpus/existing.json"
  git -C "$SB" init -q
  git -C "$SB" add -A
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  cat >"$SB/tools/mg.d/zz-corpus-fixture.sh" <<'EOF'
mg_oracle__tree() { MG_ORACLE_DIR="$MG_ROOT/build/oracle-fixture"; }
mg_oracle__build() { MG_ORACLE_TAG=fixture; }
mg_oracle__run() {
  mg_json_array "$@" >"$MG_RUN_DIR/container-args.json"
  [ "${CORPUS_FAIL:-0}" != 1 ] || return 9
  local i
  for i in 01 02 03 04 05 06 07 08 09 10; do
    printf '{"id":"fixture-%s"}\n' "$i" >"$MG_RUN_DIR/generated/compare-$i.json"
  done
  if [ "${CORPUS_BAD_RUNTIME:-0}" = 1 ]; then
    printf '{}\n'
  else
    printf '{"python":"3.12.0","unicode":"15.0.0"}\n'
  fi
}
EOF
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

@test "corpus-generate keeps generated evidence under the run directory" {
  mg corpus-generate type-answer
  [ "$status" -eq 0 ]
  jq -e '.status=="pass" and .summary.passed==10' "$result"
  [ "$(cat "$SB/conformance/scenarios/render/type-answer/corpus/existing.json")" = keep ]
  [ ! -f "$SB/conformance/scenarios/render/type-answer/corpus/compare-01.json" ]
  local dir="$(dirname "$result")"
  [ -f "$dir/generated/compare-10.json" ]
  jq -e 'index("--entrypoint") as $i | .[$i+1]=="python3"' "$dir/container-args.json"
  jq -e 'index("/generator.py") != null and index("/out") != null' "$dir/container-args.json"
  grep -q '3, 12' "$dir/container-args.json"
  jq -e '.details.runtime=={python:"3.12.0",unicode:"15.0.0"}' "$result"
  grep -q '/generator.py:ro' "$dir/container-args.json"
  python3 "$SB/tools/tests/validate_result.py" "$result"
}

@test "corpus-generate rejects paths and extra arguments before starting a container" {
  mg corpus-generate ../type-answer
  [ "$status" -eq 2 ]
  jq -e '.failures[0].id=="usage"' "$result"
  [ ! -f "$(dirname "$result")/container-args.json" ]
  mg corpus-generate type-answer --out /elsewhere
  [ "$status" -eq 2 ]
  [ ! -f "$(dirname "$result")/container-args.json" ]
}

@test "corpus-generate reports a failed generator without publishing output" {
  export CORPUS_FAIL=1
  mg corpus-generate type-answer
  [ "$status" -eq 2 ]
  jq -e '.status=="error" and .failures[0].id=="corpus-generate"' "$result"
  [ "$(cat "$SB/conformance/scenarios/render/type-answer/corpus/existing.json")" = keep ]
}

@test "corpus-generate requires concrete runtime metadata" {
  export CORPUS_BAD_RUNTIME=1
  mg corpus-generate type-answer
  [ "$status" -eq 2 ]
  jq -e '.status=="error" and .failures[0].id=="corpus-runtime"' "$result"
  [ "$(cat "$SB/conformance/scenarios/render/type-answer/corpus/existing.json")" = keep ]
}
