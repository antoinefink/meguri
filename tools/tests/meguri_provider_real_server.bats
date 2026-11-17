#!/usr/bin/env bats
# SPDX-License-Identifier: MIT
# Pure resource mocks: no Docker, server, compilation or network in this file.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
setup() {
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync.sh"
  source "$REPO_ROOT/tools/mg.d/sync-package.sh"
  source "$REPO_ROOT/tools/mg.d/meguri-provider-test.sh"
  MG_ROOT="$BATS_TEST_TMPDIR/source"; MG_RUN_DIR="$BATS_TEST_TMPDIR/run"; MG_RUN_ID=provider-test
  MG_TOOLS="$BATS_TEST_TMPDIR/tools"
  mkdir -p "$MG_ROOT/build/mg/sync-server-package" "$MG_RUN_DIR" "$MG_TOOLS"
  trace="$BATS_TEST_TMPDIR/trace"; hooks="$BATS_TEST_TMPDIR/hooks"
  touch "$trace" "$hooks"
  provider_commit=current
  receipt="$MG_ROOT/build/mg/sync-server-package/current/result.json"
  mkdir -p "$(dirname "$receipt")"
  jq -n '{job:"sync-server-package",status:"pass",commit:{sha:"current",dirty:false},details:{package:{image_id:"runtime",client_image_id:"tool"}}}' >"$receipt"
  export PROVIDER_TEST_TRACE="$trace" PROVIDER_TEST_RECEIPT="$receipt"
  cat >"$MG_TOOLS/mg" <<'SH'
#!/usr/bin/env bash
printf 'build-package\n' >>"$PROVIDER_TEST_TRACE"
printf '%s\n' "$PROVIDER_TEST_RECEIPT"
SH
  chmod +x "$MG_TOOLS/mg"
  mg_status() { printf 'status:%s\n' "$1" >>"$trace"; }
  mg_failure() { printf 'failure:%s\n' "$1" >>"$trace"; }
  mg_detail() { printf 'detail:%s:%s\n' "$1" "$2" >>"$trace"; }
  mg_artifact() { :; }
  mg_count() { printf 'count:%s:%s\n' "$1" "$2" >>"$trace"; }
  mg_on_exit() { printf '%s\n' "$1" >>"$hooks"; }
  mg_meguri_provider__commit() { printf '%s\n' current; }
  mg_sync_server__clean() { [ "${DIRTY:-0}" = 0 ]; }
  mg_sync_package__receipt() {
    printf 'validate-receipt\n' >>"$trace"
    jq -e '.job=="sync-server-package" and .status=="pass" and .commit=={sha:"current",dirty:false}' "$1" >/dev/null
  }
  mg_meguri_provider__nonce() { printf 'nonce-%s\n' "$RANDOM"; }
}
latest() { ln -s "$(dirname "$1")" "$MG_ROOT/build/mg/sync-server-package/latest"; }
finish_hooks() {
  local stack=() line i rc=0
  while IFS= read -r line; do stack+=("$line"); done <"$hooks"
  for ((i=${#stack[@]}-1;i>=0;i--)); do eval "${stack[$i]}" || rc=1; done
  return "$rc"
}
result_file() {
  local phase="$1" out="$2"
  jq -n --arg phase "$phase" '{format:1,phase:$phase,status:"pass",
    counts:{notes:2,cards:2,media:2,revlogs:0,reps:0},checks:
    (if $phase=="upload" then ["fresh-upload-client","explicit-collection-and-media","retained-safety-artifacts","all-media-accounted","ready-automatic","exact-fixture","closed"]
    elif $phase=="download" then ["initial-no-collection","explicit-collection-and-media","initial-no-backup","all-media-accounted","ready-automatic","exact-fixture","closed"]
    else ["cold-ready","persisted-binding-epoch","exact-fixture","closed"] end)}' >"$out/result.json"
}
mock_run_resources() {
  mg_sync_package__init() {
    printf 'init\n' >>"$trace"
    mkdir -p "$MG_SYNC_INSTANCE"
    mg_on_exit "$(printf '%q ' mg_meguri_provider__remove init init-nonce)"
    [ "${FAIL_AT:-}" != init ]
  }
  mg_sync_package__start() {
    printf 'start\n' >>"$trace"
    mg_on_exit "$(printf '%q ' mg_meguri_provider__remove service service-nonce)"
    printf '[{"Id":"actual-runtime-container"}]\n' >"$MG_RUN_DIR/service-container.json"
    [ "${FAIL_AT:-}" != start ]
  }
  mg_sync_package__stop() { printf 'stop-runtime\n' >>"$trace"; }
  mg_sync_package__volume_remove() { printf 'remove-volume\n' >>"$trace"; }
  mg_meguri_provider__build() {
    printf 'build-client\n' >>"$trace"
    provider_dev_image=dev-image; provider_binary_sha=binary-hash
    mg_on_exit "$(printf '%q ' mg_meguri_provider__remove build build-nonce)"
    [ "${FAIL_AT:-}" != build ]
  }
  mg_meguri_provider__fixture() {
    printf 'fixture\n' >>"$trace"
    provider_fixture_name=fixture; provider_fixture_owner=fixture-nonce
    mg_on_exit "$(printf '%q ' mg_meguri_provider__remove fixture fixture-nonce)"
    [ "${FAIL_AT:-}" != fixture ]
  }
  mg_meguri_provider__owned() {
    printf 'owned:%s:%s:%s:%s\n' "$1" "$2" "$3" "$4" >>"$trace"
    [ "${FAIL_AT:-}" != "owned-$1" ]
  }
  mg_sync_server__remove_container() {
    # This mock refuses missing nonces. The real helper's run/nonce negatives
    # below use its actual implementation with a Docker inspection mock.
    [ -n "$1" ] && [ -n "$2" ] || return 1
    printf 'remove:%s:%s\n' "$1" "$2" >>"$trace"
    [ "${FAIL_AT:-}" != cleanup ]
  }
  mg_meguri_provider__wait_client() {
    printf 'wait:%s\n' "$1" >>"$trace"
    [ "${FAIL_AT:-}" != "wait-${1##*-}" ]
  }
  docker() {
    local arg out= phase= image= previous= name=
    case "$1" in
      create)
        for arg in "$@"; do
          case "$arg" in
            type=bind,src=*,dst=/out) out="${arg#type=bind,src=}"; out="${out%,dst=/out}" ;;
            upload|download|reopen) phase="$arg" ;;
          esac
          [ "$previous" != --name ] || name="$arg"
          previous="$arg"
        done
        printf 'client:%s\n' "$phase" >>"$trace"
        [ "${FAIL_AT:-}" != "create-$phase" ] || return 1
        mkdir -p "$out"
        result_file "$phase" "$out"
        [ "$phase" != upload ] || printf '{"format":1}\n' >"$out/expected-facts.json"
        if [ "${BAD_PHASE:-}" = "$phase" ]; then printf '{}\n' >"$out/result.json"; fi
        printf 'client-id\n' ;;
      logs) : ;;
      stop) printf 'stop-fixture\n' >>"$trace" ;;
      *) return 99 ;;
    esac
  }
}
invoke() { mg_meguri_provider_real_server "$@"; finish_hooks; }
@test "meguri-provider-real-server: invalid arguments fail before resource creation" {
  for operands in '--stage upload' '--package-receipt' '--unknown' '--package-receipt x extra'; do
    run mg_meguri_provider_real_server $operands
    [ "$status" = 0 ]
    grep -Eq 'status:error' "$trace"
    [ ! -s "$hooks" ]
  done
}
@test "meguri-provider-real-server: dirty source and invalid explicit receipt fail before resources" {
  DIRTY=1
  mg_meguri_provider_real_server
  [ ! -s "$hooks" ]
  DIRTY=0
  mg_meguri_provider_real_server --package-receipt "$BATS_TEST_TMPDIR/missing.json"
  [ ! -s "$hooks" ]
  ! grep -Eq build-package "$trace"
  grep -Eq 'status:fail' "$trace"
}
@test "meguri-provider-real-server: exact latest clean pass is reused once" {
  latest "$receipt"
  mg_meguri_provider__receipt ''
  [ "$provider_receipt" = "$receipt" ]
  [ "$(grep -Ec '^validate-receipt$' "$trace")" = 1 ]
  ! grep -Eq build-package "$trace"
}
@test "meguri-provider-real-server: failed latest at this commit is never searched past or rebuilt" {
  latest "$receipt"
  jq '.status="fail"' "$receipt" >"$receipt.tmp"; mv "$receipt.tmp" "$receipt"
  run mg_meguri_provider__receipt ''
  [ "$status" -ne 0 ]
  ! grep -Eq build-package "$trace"
  [ "$(grep -Ec '^validate-receipt$' "$trace")" = 1 ]
}
@test "meguri-provider-real-server: absent or stale commit permits exactly one package build" {
  mg_meguri_provider__receipt ''
  [ "$(grep -Ec '^build-package$' "$trace")" = 1 ]
  mkdir "$BATS_TEST_TMPDIR/stale"
  jq '.commit.sha="old" | .status="fail"' "$receipt" >"$BATS_TEST_TMPDIR/stale/result.json"
  latest "$BATS_TEST_TMPDIR/stale/result.json"
  : >"$trace"
  mg_meguri_provider__receipt ''
  [ "$(grep -Ec '^build-package$' "$trace")" = 1 ]
}
@test "meguri-provider-real-server: a newly built failing receipt is not retried" {
  jq '.status="error"' "$receipt" >"$receipt.tmp"; mv "$receipt.tmp" "$receipt"
  run mg_meguri_provider__receipt ''
  [ "$status" -ne 0 ]
  [ "$(grep -Ec '^build-package$' "$trace")" = 1 ]
}
@test "meguri-provider-real-server: strict phase schema rejects absent wrong malformed zero-check and failed results" {
  mkdir "$BATS_TEST_TMPDIR/output"
  local output_dir="$BATS_TEST_TMPDIR/output" value
  run mg_meguri_provider__result upload "$output_dir/result.json"
  [ "$status" -ne 0 ]
  result_file download "$output_dir"
  run mg_meguri_provider__result upload "$output_dir/result.json"
  [ "$status" -ne 0 ]
  printf '{broken' >"$output_dir/result.json"
  run mg_meguri_provider__result upload "$output_dir/result.json"
  [ "$status" -ne 0 ]
  for value in '.checks=[]' '.status="fail"' '.counts.media=0' '.extra="unapproved"'; do
    result_file upload "$output_dir"
    jq "$value" "$output_dir/result.json" >"$output_dir/tmp"; mv "$output_dir/tmp" "$output_dir/result.json"
    run mg_meguri_provider__result upload "$output_dir/result.json"
    [ "$status" -ne 0 ]
  done
}
@test "meguri-provider-real-server: actual fixture runs as UID10001 with private hardened mounts" {
  MG_SYNC_INSTANCE="$MG_RUN_DIR/instance"
  provider_private="$MG_RUN_DIR/private"
  provider_service=synthetic-runtime-container; provider_tool_image=synthetic-tool-image
  local run_args="$BATS_TEST_TMPDIR/fixture-run-args"
  mkdir -m 0700 "$MG_SYNC_INSTANCE" "$provider_private" "$provider_private/fixture"
  printf '{"volume":"synthetic-runtime-volume"}\n' >"$MG_SYNC_INSTANCE/instance.json"
  mg_meguri_provider__fixture_wait() {
    [ "$#" -eq 1 ] && [ "$1" = "$provider_fixture_name" ] || return 1
    printf 'fixture-ready\n' >>"$trace"
  }
  mg_meguri_provider__owned() {
    [ "$#" -eq 4 ] && [ "$1" = "$provider_fixture_name" ] &&
      [ "$2" = "$provider_fixture_owner" ] && [ "$3" = "$provider_tool_image" ] && [ "$4" = running ] || return 1
    printf 'fixture-owned\n' >>"$trace"
  }
  docker() {
    case "$1" in
      run) printf '%s\0' "$@" >"$run_args"; printf 'synthetic-fixture-container\n' ;;
      cp)
        [ "$#" -eq 3 ] || return 1
        case "$2" in
          "$provider_fixture_name:/tmp/provider-fixture/input.json")
            [ "$3" = "$provider_private/fixture/input.json" ] || return 1
            printf '{}\n' >"$3" ;;
          "$provider_fixture_name:/tmp/provider-fixture/ca.der")
            [ "$3" = "$provider_private/fixture/ca.der" ] || return 1
            printf 'synthetic-ca\n' >"$3" ;;
          *) return 99 ;;
        esac ;;
      *) return 99 ;;
    esac
  }
  mg_meguri_provider__fixture
  jq -Rse --arg network "container:$provider_service" --arg image "$provider_tool_image" \
    --arg name "$provider_fixture_name" --arg run "$MG_RUN_ID" --arg owner "$provider_fixture_owner" '
    split("\u0000") | .[:-1] |
    def values_after($flag): [range(0; length - 1) as $i | select(.[$i]==$flag) | .[$i+1]];
    .[0:2]==["run","-d"] and values_after("--user")==["10001:10001"] and
    values_after("--tmpfs")==["/tmp:uid=10001,gid=10001,mode=0700,size=128m"] and
    index("--read-only")!=null and values_after("--cap-drop")==["ALL"] and
    values_after("--security-opt")==["no-new-privileges"] and
    (values_after("--mount") | index("type=volume,src=synthetic-runtime-volume,dst=/data,readonly"))!=null and
    values_after("--network")==[$network] and values_after("--name")==[$name] and
    values_after("--label")==["meguri.mg.run-id="+$run,"meguri.sync.owner="+$owner] and
    .[-3:]==[$image,"/package-smoke.mjs","provider-fixture"]' "$run_args"
  [ "$(cat "$trace")" = $'fixture-ready\nfixture-owned' ]
  grep -Fx "$(printf '%q ' mg_meguri_provider__remove "$provider_fixture_name" "$provider_fixture_owner")" "$hooks"
  [ -f "$provider_private/fixture/input.json" ] && [ -f "$provider_private/fixture/ca.der" ]
}
@test "meguri-provider-real-server: exactly once upload download reopen and successful owned cleanup" {
  mock_run_resources
  invoke --package-receipt "$receipt"
  [ "$(grep '^client:' "$trace" | paste -sd ' ' -)" = 'client:upload client:download client:reopen' ]
  grep -Eq 'status:pass' "$trace"
  grep -Eq 'count:total:3' "$trace"
  [ "$(grep -Ec '^remove-volume$' "$trace")" = 1 ]
  for name in init service build fixture meguri-provider-provider-test-upload meguri-provider-provider-test-download meguri-provider-provider-test-reopen; do
    grep -Eq "^remove:$name:.*nonce" "$trace"
  done
  local stop_line volume_line
  stop_line="$(grep -En '^stop-runtime$' "$trace" | head -1 | cut -d: -f1)"
  volume_line="$(grep -En '^remove-volume$' "$trace" | cut -d: -f1)"
  [ "$stop_line" -lt "$volume_line" ]
}
@test "meguri-provider-real-server: earlier client failure suppresses every later client without retry" {
  mock_run_resources
  FAIL_AT=wait-upload
  invoke --package-receipt "$receipt"
  [ "$(grep '^client:' "$trace")" = client:upload ]
  grep -Eq 'status:fail' "$trace"
  ! grep -Eq 'count:total:3' "$trace"
  grep -Eq '^remove:meguri-provider-provider-test-upload:nonce-' "$trace"
}
@test "meguri-provider-real-server: malformed download suppresses reopen" {
  mock_run_resources
  BAD_PHASE=download
  invoke --package-receipt "$receipt"
  [ "$(grep '^client:' "$trace" | paste -sd ' ' -)" = 'client:upload client:download' ]
  grep -Eq 'status:fail' "$trace"
}
@test "meguri-provider-real-server: every partial start retains run nonce cleanup and stops before volume removal" {
  mock_run_resources
  local point last
  for point in init start build fixture create-upload wait-upload create-download wait-download create-reopen wait-reopen; do
    rm -rf "$MG_RUN_DIR"; mkdir "$MG_RUN_DIR"; : >"$trace"; : >"$hooks"
    FAIL_AT="$point"
    invoke --package-receipt "$receipt"
    grep -Eq 'status:fail' "$trace"
    grep -Eq '^remove:init:init-nonce$' "$trace"
    [ "$(grep -Ec '^remove-volume$' "$trace")" = 1 ]
    last="${point#*-}"
    if [[ "$point" = create-* || "$point" = wait-* ]]; then
      grep -Eq "^remove:meguri-provider-provider-test-$last:nonce-" "$trace"
    fi
  done
}
@test "meguri-provider-real-server: cleanup uncertainty prevents success" {
  mock_run_resources
  FAIL_AT=cleanup
  run invoke --package-receipt "$receipt"
  [ "$status" -ne 0 ]
  ! grep -Eq 'status:pass' "$trace"
  grep -Eq 'status:fail' "$trace"
}
@test "meguri-provider-real-server: actual remover rejects another run nonce and uncertain inspection" {
  # Preserve the real ownership helper loaded in setup. Only Docker is mocked.
  docker() {
    case "$1 $2" in
      'container ls') [ "${REMOVED:-0}" = 1 ] || printf '%064d\n' 1 ;;
      'inspect '*)
        [ "${INSPECT_FAIL:-0}" = 0 ] || return 1
        jq -n --arg run "${OBSERVED_RUN:-$MG_RUN_ID}" --arg owner "${OBSERVED_OWNER:-nonce}" \
          '[{Id:("0"*63+"1"),Name:"/owned",Config:{Labels:{"meguri.mg.run-id":$run,"meguri.sync.owner":$owner}}}]' ;;
      'rm -f') REMOVED=1; printf 'removed\n' >>"$trace" ;;
      *) return 98 ;;
    esac
  }
  OBSERVED_RUN=other
  run mg_meguri_provider__remove owned nonce
  [ "$status" -ne 0 ]; ! grep -Eq '^removed$' "$trace"
  OBSERVED_RUN="$MG_RUN_ID"; OBSERVED_OWNER=other
  run mg_meguri_provider__remove owned nonce
  [ "$status" -ne 0 ]; ! grep -Eq '^removed$' "$trace"
  OBSERVED_OWNER=nonce; INSPECT_FAIL=1
  run mg_meguri_provider__remove owned nonce
  [ "$status" -ne 0 ]; ! grep -Eq '^removed$' "$trace"
  INSPECT_FAIL=0
  mg_meguri_provider__remove owned nonce
  grep -Eq '^removed$' "$trace"
}
@test "meguri-provider-real-server: fixture branch is separate from existing package smoke paths" {
  # Guard the exact mode and existing native/browser branches without executing JS.
  grep -Eq "else if \(browserMode\)" "$REPO_ROOT/server/tools/package-smoke.mjs"
  grep -Eq "execFileSync\('/smoke/native', \[phase\]" "$REPO_ROOT/server/tools/package-smoke.mjs"
  grep -Eq "assert\(\['seed', 'check', 'provider-fixture'\]" "$REPO_ROOT/server/tools/package-smoke.mjs"
  grep -Eq "MEGURI_PROVIDER_FIXTURE_READY" "$REPO_ROOT/server/tools/package-smoke.mjs"
  grep -Eq "delete headers\['x-forwarded-host'\]" "$REPO_ROOT/server/tools/package-smoke.mjs"
}
