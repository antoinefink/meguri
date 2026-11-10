#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudAgent selected generated coordinator barriers and refusals only" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_selected_test.py"
}

@test "CloudAgent selected scope rejects external inputs and smoke substitution" {
  for arguments in '--selected --header /generated.json' '--selected --smoke' '--selected --results-directory /generated'; do
    run bash -c '
      mg_job() { :; }; mg_failure() { printf "%s\n" "$1"; }; mg_status() { printf "%s\n" "$1"; }
      MG_ROOT="$1" MG_RUN_DIR="$2"; shift 2
      source "$MG_ROOT/tools/mg.d/cloud-agent.sh"
      mg_cloudkit_signed "$@"
    ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR" $arguments
    [ "$status" -eq 0 ]
    [ "$output" = $'usage\nerror' ]
  done
}

@test "CloudAgent selected needs its clean parent without new registered jobs" {
  run bash -c '
    mg_job() { printf "%s\n" "$1"; }; mg_failure() { printf "%s\n" "$1"; }; mg_status() { printf "%s\n" "$1"; }
    MG_ROOT="$1" MG_RUN_DIR="$2" MG_JOB=cloudkit-signed MG_COMMIT_DIRTY=true
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_signed --selected
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = $'cloudkit-signed\ncloudkit-agent-install\nselected-source\nfail' ]
}
