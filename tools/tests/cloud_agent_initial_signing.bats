#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
@test "CloudAgent initial simulator signing refuses signer ambiguity and frozen products" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_initial_signing_test.py"
}

@test "CloudAgent initial simulator signing refusal prevents the first freeze" {
  run bash -c '
    mg_job() { :; }
    mg_failure() { printf "failure:%s\n" "$1"; }
    mg_status() { printf "status:%s\n" "$1"; }
    mg__vm_xctestrun() { printf "%s/template.xctestrun\n" "$MG_RUN_DIR"; }
    xcodebuild() { printf "[]\n"; }
    python3() {
      case "$*" in
        *"cloud-agent-initial-signing.py"*) return 1 ;;
        *) printf "unexpected-freeze\n"; return 9 ;;
      esac
    }
    MG_ROOT="$2" MG_RUN_DIR="$2" MG_RUN_ID=20261102T010000Z-101 MG_TOOLS="$1/tools"
    MEGURI_APP_SIGNING=team
    source "$1/tools/mg.d/app.sh"
    mg_app_agent_freeze ios-sim fixture-source
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 1 ]
  [[ "$output" == *"failure:agent-build"* ]]
  [[ "$output" == *"status:fail"* ]]
  [[ "$output" != *"unexpected-freeze"* ]]
  [ ! -e "$BATS_TEST_TMPDIR/agent-build.json" ]
  [ ! -e "$BATS_TEST_TMPDIR/agent-build-reference.json" ]
  [ ! -e "$BATS_TEST_TMPDIR/agent-settings.json" ]
}
