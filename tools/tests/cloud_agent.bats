#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "signed-agent input admission rejects generated receipt and product substitutions" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_test.py"
}

@test "signed-agent MG job reports missing implementation without signed pass cells" {
  run bash -c '
    mg_job() { :; }
    mg_failure() { printf "failure:%s\n" "$1"; }
    mg_detail() { printf "detail:%s:%s\n" "$1" "$2"; }
    mg_status() { printf "status:%s\n" "$1"; }
    MG_ROOT="$1" MG_RUN_DIR="$2"
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_signed
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [[ "$output" == *"failure:implementation_missing"* ]]
  [[ "$output" == *"status:fail"* ]]
  [[ "$output" != *"awaiting_user"* ]]
  [[ "$output" != *"scenarios"* ]]
  [[ "$output" != *"status:pass"* ]]
}

@test "CloudAgent adhoc compile does not emit signed build input proof" {
  run bash -c '
    mg_job() { :; }
    mg_detail() { printf "detail:%s:%s\n" "$1" "$2"; }
    python3() { printf "unexpected verifier\n"; return 9; }
    xcodebuild() { printf "unexpected build\n"; return 9; }
    MG_RUN_DIR="$2" MEGURI_APP_SIGNING=adhoc
    source "$1/tools/mg.d/app.sh"
    mg_app_agent_freeze mac unused
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = "detail:agent_compile_only:true" ]
  [ ! -e "$BATS_TEST_TMPDIR/agent-build.json" ]
}

@test "CloudAgent team mode with source edits keeps compile-only output" {
  run bash -c '
    mg_job() { :; }
    mg_word_in() { local item; for item in $2; do [ "$item" != "$1" ] || return 0; done; return 1; }
    mg_failure() { printf "failure:%s\n" "$1"; }
    mg_status() { printf "status:%s\n" "$1"; }
    mg_artifact() { printf "artifact:%s\n" "$1"; }
    mg_detail() { printf "detail:%s:%s\n" "$1" "$2"; }
    mg_message() { :; }
    git() {
      case "$*" in
        *"rev-parse HEAD") printf "%040d\n" 1;;
        *"status --porcelain=v1 --untracked-files=all") printf " M tools/fixture.sh\n";;
        *) return 8;;
      esac
    }
    python3() { printf "unexpected verifier\n"; return 9; }
    xcodebuild() { printf "unexpected settings query\n"; return 9; }
    MG_ROOT="$2" MG_RUN_DIR="$2" MEGURI_APP_SIGNING=team
    source "$1/tools/mg.d/app.sh"
    mg_app_build() { printf "compile:%s\n" "$*"; }
    mg_build_app --platform mac --test-plan CloudAgent
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [[ "$output" == *"compile:mac Debug CloudAgent"* ]]
  [[ "$output" == *"artifact:app"* ]]
  [[ "$output" == *"detail:agent_compile_only:true"* ]]
  [[ "$output" == *"status:pass"* ]]
  [[ "$output" != *"failure:"* ]]
  [[ "$output" != *"unexpected"* ]]
  [[ "$output" != *"detail:agent_build:"* ]]
  [ ! -e "$BATS_TEST_TMPDIR/agent-build.json" ]
  [ ! -e "$BATS_TEST_TMPDIR/agent-build-reference.json" ]
}

@test "CloudAgent producer declares frozen inputs and exact manifest reference" {
  run bash -c '
    mg_job() { :; }
    mg_failure() { printf "failure:%s\n" "$1"; }
    mg_status() { printf "status:%s\n" "$1"; }
    mg_artifact() { printf "artifact:%s:%s\n" "$1" "${2##*/}"; }
    mg_detail() { printf "detail:%s:%s\n" "$1" "$2"; }
    mg__vm_xctestrun() {
      [ "$*" = "CloudAgent iphonesimulator Debug" ] || return 1
      printf "%s/template.xctestrun\n" "$MG_RUN_DIR"
    }
    xcodebuild() { printf "[{\"target\":\"Meguri\",\"buildSettings\":{\"CONFIGURATION\":\"Debug\"}}]\n"; }
    python3() {
      case "$*" in
        *"cloud-agent-initial-signing.py"*"--source-commit fixture-source"*"--xctestrun"*"--settings"*)
          printf "initial-signing-before-freeze\n" >&2
          return 0
          ;;
        *"--freeze-build"*"--source-commit fixture-source"*"--platform ios-sim"*"--xctestrun"*"--manifest"*) ;;
        *) return 8 ;;
      esac
      printf "{\"path\":\"fixture-manifest\",\"sha256\":\"fixture-digest\",\"bytes\":42}\n"
    }
    MG_ROOT="$2" MG_RUN_DIR="$2" MG_RUN_ID=20261102T010000Z-101 MG_TOOLS="$1/tools"
    MEGURI_APP_SIGNING=team
    source "$1/tools/mg.d/app.sh"
    mg_app_agent_freeze ios-sim fixture-source
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [[ "$output" == *"initial-signing-before-freeze"* ]]
  [[ "$output" == *"artifact:directory:Products"* ]]
  [[ "$output" == *"artifact:json:agent-build.json"* ]]
  [[ "$output" == *"artifact:json:agent-cloud-settings.json"* ]]
  [[ "$output" == *'detail:agent_build:{"path":"fixture-manifest","sha256":"fixture-digest","bytes":42}'* ]]
  [[ "$output" != *"scenarios"* ]]
  [[ "$output" != *"status:pass"* ]]
}

# bats test_tags=gate
@test "signed-agent T3 registration leaves the original soak pending" {
  run python3 -B -c '
import pathlib, sys, yaml
model = yaml.safe_load(pathlib.Path(sys.argv[1]).read_text())
rows = {row["id"]: row for row in model["runs"]}
row = rows["u248-soak"]
assert row["job"] == "cloudkit-signed" and row["tier"] == "T3"
assert row["placement"] == ["host", "vm"] and "vm" in row["needs"]
assert "simulator" in row["needs"] and row["class"] == "sync"
assert not {"pending", "disabled", "blocking", "release_only"}.intersection(row)
assert rows["soak"]["pending"] == "U248"
' "$REPO_ROOT/tools/gate/tiers.yaml"
  [ "$status" -eq 0 ]
}
