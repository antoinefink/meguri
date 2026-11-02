#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudAgent smoke parent generated transport ordering and refusals only" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_smoke_test.py"
}

@test "CloudAgent legacy header remains admission only and smoke rejects external inputs" {
  run bash -c '
    mg_job() { :; }
    mg_failure() { printf "%s\n" "$1"; }
    mg_status() { printf "%s\n" "$1"; }
    mg_cloudkit_signed_smoke() { exit 99; }
    MG_ROOT="$1" MG_RUN_DIR="$2"
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_signed --smoke --header /generated.json
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = $'usage\nerror' ]
}

@test "CloudAgent smoke needs its clean parent and adds no registered job" {
  run bash -c '
    mg_job() { printf "%s\n" "$1"; }
    mg_failure() { printf "%s\n" "$1"; }
    mg_status() { printf "%s\n" "$1"; }
    MG_ROOT="$1" MG_RUN_DIR="$2" MG_JOB=cloudkit-signed MG_COMMIT_DIRTY=true
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_signed --smoke
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = $'cloudkit-signed\ncloudkit-agent-install\nsmoke-source\nfail' ]
}
