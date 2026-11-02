#!/usr/bin/env bats
bats_require_minimum_version 1.5.0

setup() {
  export REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
  export MG_ROOT="$BATS_TEST_TMPDIR/repo with spaces"
  export MG_RUN_DIR="$MG_ROOT/build/mg/swift-test/run"
  mkdir -p "$MG_ROOT/tools" "$MG_RUN_DIR"
  cat >"$MG_ROOT/tools/mg" <<'EOF'
#!/bin/bash
set -eu
[ "$*" = 'core-test -- --test cloud_initial_app_fixture -E package(meguri-ffi) & test(=export_initial_app_adoption_replies_from_real_publication)' ]
[ "${FX_PRODUCER_FAIL:-0}" = 0 ] || exit 1
output="$MG_ROOT/${MEGURI_INITIAL_ADOPTION_FIXTURE#/work/}"
[ "$output" = "$MG_RUN_DIR/initial-adoption-fixture.json" ]
if [ "${FX_OUTPUT_MISSING:-0}" = 0 ]; then
  if [ "${FX_OUTPUT_DIRECTORY:-0}" = 1 ]; then
    mkdir "$output"
  else
    echo '{"records":[],"sourceRows":[]}' >"$output"
  fi
fi
result="$MG_RUN_DIR/producer.json"
jq -nc --arg status "${FX_PRODUCER_STATUS:-pass}" --argjson skipped "${FX_PRODUCER_SKIPPED:-0}" --argjson dry "${FX_PRODUCER_DRY:-false}" \
  '{job:"core-test",status:$status,dry_run:$dry,summary:{total:1,passed:1,failed:0,skipped:$skipped}}' >"$result"
echo "$result"
EOF
  chmod +x "$MG_ROOT/tools/mg"
}

generate() {
  run bash -c '
    mg_job() { :; }
    mg_failure() { echo "$*" >&2; }
    mg_status() { echo "status=$1" >&2; }
    mg_artifact() { :; }
    mg_detail() { printf "%s\n" "$2" >"$MG_RUN_DIR/detail.json"; }
    source "$REPO_ROOT/tools/mg.d/bridge.sh"
    mg__swift_initial_adoption_fixture || exit $?
    test "$MEGURI_INITIAL_ADOPTION_FIXTURE" = "$MG_RUN_DIR/initial-adoption-fixture.json"
  '
}

@test "Swift fixture generation keeps a passing producer receipt and native output path" {
  generate
  [ "$status" -eq 0 ]
  jq -e --arg path "$MG_RUN_DIR/initial-adoption-fixture.json" \
    '.path == $path and (.sha256 | test("^[a-f0-9]{64}$"))' "$MG_RUN_DIR/detail.json"
}

@test "Swift fixture generation stops on a failed producer" {
  export FX_PRODUCER_FAIL=1
  generate
  [ "$status" -ne 0 ]
  [[ "$output" == *"Rust fixture generation failed"* ]]
  [ ! -e "$MG_RUN_DIR/detail.json" ]
}

@test "Swift fixture generation refuses a dry-run receipt" {
  export FX_PRODUCER_DRY=true
  generate
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/detail.json" ]
}

@test "Swift fixture generation refuses a nonregular output" {
  export FX_OUTPUT_DIRECTORY=1
  generate
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/detail.json" ]
}

@test "Swift fixture generation refuses a skipped producer or missing output" {
  export FX_PRODUCER_SKIPPED=1
  generate
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/detail.json" ]
  unset FX_PRODUCER_SKIPPED
  export FX_OUTPUT_MISSING=1
  rm -f "$MG_RUN_DIR/initial-adoption-fixture.json"
  generate
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/detail.json" ]
}

@test "Swift fixture generation never adopts a preexisting publication or ambient path" {
  export MEGURI_INITIAL_ADOPTION_FIXTURE=/ambient/unverified
  echo stale >"$MG_RUN_DIR/initial-adoption-fixture.json"
  generate
  [ "$status" -ne 0 ]
  [[ "$output" == *'Rust fixture publication must be fresh'* ]]
  [ "$(cat "$MG_RUN_DIR/initial-adoption-fixture.json")" = stale ]
  [ ! -e "$MG_RUN_DIR/producer.json" ]
}
