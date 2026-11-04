#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "vm-xcode-recover synthetic causal admission stop proof retention and whole archive" {
  run python3 -B "$REPO_ROOT/tools/tests/vm_xcode_recover_test.py"
  [ "$status" -eq 0 ]
}

@test "vm-xcode-recover rejects every unsupported selector before reading configuration" {
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/vm-xcode-recover.sh"
    mg_failure() { :; }
    mg_status() { [ "$1" = error ]; }
    mvl_read_config() { echo forbidden-read; exit 96; }
    MG_RUN_DIR="$2"
    for args in "" "--stage-run other --inspect" "--stage-run 20261102T200056Z-94582 --restart" \
      "--stage-run 20261102T200056Z-94582 --stop-and-archive --force" \
      "--stage-run 20261102T200056Z-94582 --inspect --vm other"; do
      mg_vm_xcode_recover $args && exit 95
    done
    exit 0
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

@test "vm-xcode-recover final publication error cannot become pass after archival" {
  mkdir -m 700 "$BATS_TEST_TMPDIR/temp" "$BATS_TEST_TMPDIR/run"
  run bash -c '
    mg_job() { :; }
    source "$1/tools/mg.d/vm-xcode-recover.sh"
    MG_TOOLS="$1/tools" MG_ROOT="$2" MG_RUN_DIR="$2/run"
    MG_RUN_ID=20261103T010203Z-12345 TMPDIR="$2/temp"
    mvl_read_config() { MVL_ALL="[{\"name\":\"Generated\",\"ssh\":\"/generated/ssh\",\"tart\":\"/generated/tart\"}]"; }
    mvl_config_path() { echo /generated/config; }
    mvl_lease_base() { echo "$MG_ROOT/build/mg"; }
    python3() {
      [ "$#" -eq 6 ] && [ "$1" = -B ] && [ "$2" = -I ] &&
        [ "$3" = "$MG_TOOLS/scripts/vm-xcode-recover.py" ] &&
        [ "$4" = --stage-run ] && [ "$5" = 20261102T200056Z-94582 ] &&
        [ "$6" = --stop-and-archive ] || return 97
      echo '\''{"scope":"VM-XCODE-MAINTENANCE-ONLY","outcome":"archived","checks":{"whole_lease_archived":true}}'\''
    }
    mg_artifact() { return 1; }
    mg_failure() { echo "$1"; }
    mg_status() { [ "$1" = error ] || { echo forbidden-pass; return 1; }; }
    mg_message() { echo forbidden-pass-message; }
    mg_vm_xcode_recover --stage-run 20261102T200056Z-94582 --stop-and-archive && exit 98
    exit 0
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = vm-xcode-recover ]
}
