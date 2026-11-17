#!/usr/bin/env bats
# SPDX-License-Identifier: MIT
# Focused synthetic proofs; no image build, service or network.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
setup() {
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/sync-package.sh"
  MG_SYNC_INSTANCE="$BATS_TEST_TMPDIR/instance"
  MG_RUN_DIR="$BATS_TEST_TMPDIR/run"
  MG_RUN_ID=synthetic-package
  mkdir "$MG_RUN_DIR"
  MEGURI_SYNC_ORIGIN=https://localhost:18443
  MEGURI_SYNC_TRUSTED_PEER=127.0.0.1
  receipt="$BATS_TEST_TMPDIR/receipt.json"
  printf '%s\n' '{"details":{"package":{"image_id":"synthetic-image"}}}' >"$receipt"
  mg_sync_package__receipt() { :; }
  mg_on_exit() { :; }
  mg_sync_server__remove_container() { :; }
  mg_sync_package__compose() { printf 'initialized\n' >"$MG_RUN_DIR/helper-called"; return "${INIT_RC:-0}"; }
  docker() {
    case "$1 $2" in
      'volume ls') [ "${LIST_RC:-0}" -eq 0 ] || return 1
        [ ! -f "$MG_RUN_DIR/volume" ] || cat "$MG_RUN_DIR/volume" ;;
      'volume create') [ "${CREATE_RC:-0}" -eq 0 ] || return 1
        printf '%s\n' "${@: -1}" >"$MG_RUN_DIR/volume" ;;
      'volume inspect') jq -n --arg name "$3" --arg owner "$(jq -r .owner "$MG_SYNC_INSTANCE/instance.json")" \
        --arg run "$MG_RUN_ID" '[{Name:$name,Labels:{"meguri.sync.owner":$owner,"meguri.mg.run-id":$run}}]' ;;
      *) return 97 ;;
    esac
  }
}
@test "sync-package: absent failed volume creation permits a fresh init" {
  CREATE_RC=1
  run mg_sync_package__init "$receipt"
  [ "$status" -ne 0 ]
  [ ! -e "$MG_SYNC_INSTANCE/instance.json" ]
  CREATE_RC=0
  run mg_sync_package__init "$receipt"
  [ "$status" -eq 0 ]
  [ -s "$MG_SYNC_INSTANCE/instance.json" ]
  [ -s "$MG_RUN_DIR/helper-called" ]
}
@test "sync-package: failed initialization preserves owned partial volume and record" {
  INIT_RC=1
  run mg_sync_package__init "$receipt"
  [ "$status" -ne 0 ]
  [ -s "$MG_RUN_DIR/volume" ]
  [ -s "$MG_RUN_DIR/helper-called" ]
  [ -s "$MG_SYNC_INSTANCE/instance.json" ]
  run mg_sync_package__volume_check
  [ "$status" -eq 0 ]
}
@test "sync-package: uncertain volume observation preserves candidate record" {
  LIST_RC=1
  run mg_sync_package__init "$receipt"
  [ "$status" -ne 0 ]
  [ -s "$MG_SYNC_INSTANCE/instance.json" ]
}
@test "sync-package: real context assembly rejects synthetic credential inputs" {
  run python3 "$REPO_ROOT/tools/tests/sync_package_context_test.py" "$REPO_ROOT/server/tools/package-context.py"
  [ "$status" -eq 0 ]
}

@test "sync-package: compiled export uses the measured smoke image and checks cleanup before admission" {
  MG_ROOT="$REPO_ROOT"
  smoke="sha256:$(printf '%064d' 1)"
  mg_artifact() { :; }
  docker() {
    printf '%s\n' "$1" >>"$MG_RUN_DIR/asset-sequence"
    case "$1" in
      create)
        [ "${@: -1}" = "$smoke" ] || return 97
        for arg in "$@"; do
          case "$arg" in meguri.sync.owner=*) ASSET_OWNER="${arg#*=}" ;; esac
        done
        printf 'synthetic-created-id\n' ;;
      inspect)
        jq -nc --arg image "$smoke" --arg name "/meguri-package-assets-$MG_RUN_ID" \
          --arg owner "$ASSET_OWNER" --arg run "$MG_RUN_ID" '
          [{Image:$image,Name:$name,State:{Running:false},
            Config:{Labels:{"meguri.sync.owner":$owner,"meguri.mg.run-id":$run}}}]' ;;
      cp)
        [ "$3" = - ] || return 97
        case "$2" in
          "meguri-package-assets-$MG_RUN_ID:/prepared/assets.json"|"meguri-package-assets-$MG_RUN_ID:/prepared/embedded") ;;
          *) return 97 ;;
        esac
        [ "${ASSET_COPY_RC:-0}" -eq 0 ] || return 1
        printf 'synthetic tar transport placeholder, validated in Python proofs\n' ;;
      *) return 97 ;;
    esac
  }
  mg_sync_server__remove_container() {
    printf 'cleanup\n' >>"$MG_RUN_DIR/asset-sequence"
    [ "$1" = "meguri-package-assets-$MG_RUN_ID" ] && [ "$2" = "$ASSET_OWNER" ] || return 97
    return "${ASSET_CLEANUP_RC:-0}"
  }
  python3() {
    if [ "${2:-}" = --export-package-assets ]; then
      [ "$1" = "$REPO_ROOT/server/tools/candidate-output.py" ] && [ "$3" = "$MG_RUN_DIR" ] || return 97
      printf 'validate\n' >>"$MG_RUN_DIR/asset-sequence"
    else command python3 "$@"; fi
  }
  run mg_sync_package__assets "$smoke"
  [ "$status" -eq 0 ]
  [ "$(tr '\n' ' ' <"$MG_RUN_DIR/asset-sequence")" = 'create inspect cp cp cleanup validate ' ]
  [ -s "$MG_RUN_DIR/prepared-manifest.tar" ] && [ -s "$MG_RUN_DIR/prepared-embedded.tar" ]
  for failure in copy cleanup; do
    : >"$MG_RUN_DIR/asset-sequence"
    ASSET_COPY_RC=0 ASSET_CLEANUP_RC=0
    if [ "$failure" = copy ]; then ASSET_COPY_RC=1; else ASSET_CLEANUP_RC=1; fi
    run mg_sync_package__assets "$smoke"
    [ "$status" -ne 0 ]
    [ "$(tail -n 1 "$MG_RUN_DIR/asset-sequence")" = cleanup ]
    ! rg -x validate "$MG_RUN_DIR/asset-sequence"
  done
}

@test "sync-package: phase-suffixed volumes retain the real outer ownership run" {
  MG_SYNC_INSTANCE_SUFFIX=final-rich
  run mg_sync_package__init "$receipt"
  [ "$status" -eq 0 ]
  jq -e --arg run "$MG_RUN_ID" '.run_id==$run and .volume==("meguri-sync-data-"+$run+"-final-rich")' "$MG_SYNC_INSTANCE/instance.json"
  run mg_sync_package__volume_check
  [ "$status" -eq 0 ]
}

@test "sync-package: a failed segment cannot shorten the nine-instance loop or gain credit" {
  MG_ROOT="$REPO_ROOT" MG_JOB=sync-server-interop
  mg_failure() { printf '%s\n' "$1" >>"$MG_RUN_DIR/failures"; }
  mg_artifact() { :; }
  mg_detail() { printf '%s=%s\n' "$1" "$2" >>"$MG_RUN_DIR/details"; }
  mg_sync_server__clean() { :; }
  mg_sync_package__candidate_input() {
    printf '{"server":{"source_commit":"a","source_sha256":"b","lock_sha256":"c","executable_sha256":"d"}}\n'
  }
  mg_sync_server__reference_identity() { printf '{}\n'; }
  mg_compose() { :; }
  mg_sync_server__image() { printf 'sha256:%064d\n' 0; }
  mg_sync_package__final_instance() {
    printf '%s %s\n' "$MG_RUN_ID" "$2" >>"$MG_RUN_DIR/sequence"
    [ "$2" != final-normal ]
  }
  mg_sync_package__final_assemble() { printf 'assemble\n' >>"$MG_RUN_DIR/sequence"; }
  run mg_sync_package__final_owned "$receipt" '["--lane","desktop","--phase","final"]' synthetic-author
  [ "$status" -ne 0 ]
  [ "$(wc -l <"$MG_RUN_DIR/sequence" | tr -d ' ')" -eq 10 ]
  [ "$(cut -d ' ' -f 2 "$MG_RUN_DIR/sequence" | head -n 9 | tr '\n' ' ')" = 'final-full final-normal final-media final-migration final-replacement-upload final-replacement-download final-metadata-only final-fresh-fresh final-rich ' ]
  [ "$(tail -n 1 "$MG_RUN_DIR/sequence")" = assemble ]
  [ "$(head -n 9 "$MG_RUN_DIR/sequence" | cut -d ' ' -f 1 | sort -u)" = "$MG_RUN_ID" ]
  ! rg -F 'candidate_compatibility_credit=true' "$MG_RUN_DIR/details"
}

@test "sync-package: exact-image browser exports only private fixed files and fails closed" {
  # Policy stubs only: no Docker, Node execution, HTTP or browser proof here.
  source "$REPO_ROOT/tools/mg.d/sync-web.sh"
  mkdir "$BATS_TEST_TMPDIR/bin" "$BATS_TEST_TMPDIR/product" "$BATS_TEST_TMPDIR/frontend" "$BATS_TEST_TMPDIR/frontend/prepared"
  printf '[]\n' >"$BATS_TEST_TMPDIR/product/assets.json"
  cp "$BATS_TEST_TMPDIR/product/assets.json" "$BATS_TEST_TMPDIR/frontend/prepared/assets.json"
  printf '{"errors":[],"counts":{"total":9,"passed":9}}\n' >"$BATS_TEST_TMPDIR/frontend/test-counts.json"
  export EXPORT_IMAGE="sha256:$(printf '%064d' 0)" MG_RUN_ID
  export EXPORT_CID="$(printf '%064d' 1)"
  export PATH="$BATS_TEST_TMPDIR/bin:$PATH"
  cat >"$BATS_TEST_TMPDIR/bin/docker" <<'SH'
#!/usr/bin/env bash
set -eu
case "$1" in
  run)
    owner=
    for argument in "$@"; do
      case "$argument" in meguri.sync.owner=*) owner="${argument#meguri.sync.owner=}" ;; esac
    done
    jq -n --arg cid "$EXPORT_CID" --arg name "/meguri-web-package-$MG_RUN_ID" \
      --arg owner "$owner" --arg image "$EXPORT_IMAGE" --arg run "$MG_RUN_ID" '
      [{Id:$cid,Name:$name,Image:$image,State:{Running:true},
        Config:{Image:$image,User:"10001:10001",Cmd:["package-preview"],
          Labels:{"meguri.mg.run-id":$run,"meguri.sync.owner":$owner}}}]' >"$EXPORT_RUN_DIR/proxy.json"
    if [ "$EXPORT_MODE" = directory-mode ]; then chmod 755 "$EXPORT_RUN_DIR/fixture"; fi
    if [ "$EXPORT_MODE" = directory-link ]; then
      rmdir "$EXPORT_RUN_DIR/fixture"
      ln -s "$EXPORT_RUN_DIR/package-artifacts" "$EXPORT_RUN_DIR/fixture"
    fi
    printf '%s\n' "$EXPORT_CID" ;;
  inspect)
    printf 'inspect\n' >>"$EXPORT_RUN_DIR/export-sequence"
    if [ "$2" = synthetic-service ]; then cat "$EXPORT_RUN_DIR/service-container.json"; exit; fi
    [ "$2" = "$EXPORT_CID" ] || exit 97
    case "$EXPORT_MODE" in
      owner) jq '.[0].Config.Labels["meguri.sync.owner"]="foreign"' "$EXPORT_RUN_DIR/proxy.json" ;;
      run) jq '.[0].Config.Labels["meguri.mg.run-id"]="foreign"' "$EXPORT_RUN_DIR/proxy.json" ;;
      image) jq '.[0].Image="foreign"' "$EXPORT_RUN_DIR/proxy.json" ;;
      cid) jq '.[0].Id="foreign"' "$EXPORT_RUN_DIR/proxy.json" ;;
      stopped) jq '.[0].State.Running=false' "$EXPORT_RUN_DIR/proxy.json" ;;
      *) cat "$EXPORT_RUN_DIR/proxy.json" ;;
    esac ;;
  exec)
    if [ "$2" = "meguri-web-package-$MG_RUN_ID" ]; then
      [ "$3 $4" = 'node /opt/sync-web/ready.mjs' ] || exit 97
      exit 0
    fi
    [ "$2" = "$EXPORT_CID" ] && [ "$3 $4 $5" = 'node --input-type=module -e' ] || exit 97
    # Require the in-container ownership/nofollow/link checks in the supplied
    # program. Refusal modes model those checks failing, rather than executing it.
    [[ "$6" == *'constants.O_NOFOLLOW'* && "$6" == *'s.nlink !== 1'* &&
       "$6" == *'s.uid !== 10001'* && "$6" == *'s.gid !== 10001'* &&
       "$6" == *'s.isFile()'* && "$6" == *'s.isDirectory()'* ]] || exit 97
    case "$7" in
      /out/ready) printf 'export-ready\n' >>"$EXPORT_RUN_DIR/export-sequence"
        if [ "$EXPORT_MODE" = missing-ready ]; then exit 1; fi
        printf 'ready\n' ;;
      /out/test/fixture.json)
        printf 'export-descriptor\n' >>"$EXPORT_RUN_DIR/export-sequence"
        case "$EXPORT_MODE" in
          missing|link|hardlink|source-owner) exit 1 ;;
          export-failure) printf 'private-export-sentinel\n'; printf 'private-export-sentinel\n' >&2; exit 1 ;;
          malformed) printf '{private-export-sentinel'; exit ;;
          origin) printf '{"origin":"https://other.invalid","setup_token":"%043d"}' 0; exit ;;
          extra) printf '{"origin":"https://localhost:18447","setup_token":"%043d","extra":true}' 0; exit ;;
          duplicate) printf '{"origin":"https://localhost:18447","origin":"https://localhost:18447","setup_token":"%043d"}' 0; exit ;;
          token) printf '{"origin":"https://localhost:18447","setup_token":"private-export-sentinel"}'; exit ;;
        esac
        printf '{"origin":"https://localhost:18447","setup_token":"%043d"}' 0 ;;
      *) printf 'forbidden-selector\n' >>"$EXPORT_RUN_DIR/export-sequence"; exit 97 ;;
    esac ;;
  *) printf 'forbidden-docker-operation\n' >>"$EXPORT_RUN_DIR/export-sequence"; exit 97 ;;
esac
SH
  chmod 700 "$BATS_TEST_TMPDIR/bin/docker"
  docker() { "$BATS_TEST_TMPDIR/bin/docker" "$@"; }
  mg_artifact() { :; }
  mg_failure() { printf '%s\n' "$1"; }
  mg_status() { :; }
  mg_detail() { :; }
  mg_count() { :; }
  mg_on_exit() { printf '%s\n' "$1" >>"$MG_RUN_DIR/hooks"; }
  mg_sync_package__candidate_input() { printf '{"package":{"assets_sha256":"synthetic-assets"}}\n'; }
  mg_sync_package__init() {
    mkdir "$MG_SYNC_INSTANCE"
    printf '{"volume":"synthetic-volume"}\n' >"$MG_SYNC_INSTANCE/instance.json"
    printf '{"name":"synthetic-service"}\n' >"$MG_SYNC_INSTANCE/container.json"
    cp "$BATS_TEST_TMPDIR/product/assets.json" "$MG_RUN_DIR/package-artifacts/synthetic-assets"
  }
  mg_sync_package__start() {
    printf '[{"Id":"service-id","Image":"service-image","State":{"Running":true,"StartedAt":"same-start"},"Config":{},"Mounts":[],"HostConfig":{}}]\n' >"$MG_RUN_DIR/service-container.json"
  }
  mg_sync_server__remove_container() {
    [ "$1" = "meguri-web-package-$MG_RUN_ID" ] || return 97
    [ "$2" = "$(jq -r '.[0].Config.Labels["meguri.sync.owner"]' "$MG_RUN_DIR/proxy.json")" ] || return 97
    printf 'proxy-cleanup\n' >>"$MG_RUN_DIR/export-sequence"
    [ "$EXPORT_MODE" != cleanup ]
  }
  mg_sync_package__stop() {
    printf 'service-cleanup\n' >>"$MG_RUN_DIR/export-sequence"
    [ "$EXPORT_MODE" != stop ]
  }
  mg_sync_web__browser() {
    [ "$1" = "$MG_RUN_DIR/fixture" ] && [ "$2" = "meguri-web-package-$MG_RUN_ID" ] &&
      [ "$3" = "$EXPORT_IMAGE" ] && [ "$4" = "$(id -u):$(id -g)" ] || return 97
    command python3 - "$1" <<'PY'
import os, stat, sys
root = sys.argv[1]
for path, mode in ((root, 0o700), (root + "/test", 0o700), (root + "/ready", 0o600), (root + "/test/fixture.json", 0o600)):
    s = os.lstat(path)
    assert s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == mode
    assert stat.S_ISDIR(s.st_mode) if mode == 0o700 else stat.S_ISREG(s.st_mode) and s.st_nlink == 1
with open(root + "/test/fixture.json", "rb") as handle:
    assert handle.read() == b'{"origin":"https://localhost:18447","setup_token":"' + b"0" * 43 + b'"}'
PY
    [ "$?" -eq 0 ] || return 97
    printf 'browser\n' >>"$MG_RUN_DIR/export-sequence"
    printf '{"errors":[],"counts":{"total":1,"passed":1}}\n' >"$MG_RUN_DIR/test-counts.json"
    mkdir "$MG_RUN_DIR/browser-artifacts"
    local number
    for number in 1 2 3 4 5 6 7 8 9; do : >"$MG_RUN_DIR/browser-artifacts/$number.png"; done
  }
  local mode
  for mode in valid missing-ready missing link hardlink source-owner malformed origin extra duplicate token export-failure owner run image cid stopped directory-mode directory-link cleanup stop; do
    export EXPORT_MODE="$mode" EXPORT_RUN_DIR="$BATS_TEST_TMPDIR/$mode"
    MG_RUN_DIR="$EXPORT_RUN_DIR"
    mkdir -m 700 "$MG_RUN_DIR"
    run mg_sync_web__package_verify "$receipt" "$EXPORT_IMAGE" "$BATS_TEST_TMPDIR/frontend"
    printf '%s %d\n' "$mode" "$status" >&3
    if [ "$mode" = valid ]; then [ "$status" -eq 0 ]; else [ "$status" -ne 0 ]; fi
    [[ "$output" != *private-export-sentinel* && "$output" != *setup_token* && "$output" != *0000000000000000000000000000000000000000000* ]]
    [ "$(tail -n 2 "$MG_RUN_DIR/export-sequence" | tr '\n' ' ')" = 'proxy-cleanup service-cleanup ' ]
    ! rg 'forbidden-' "$MG_RUN_DIR/export-sequence"
    rg -q 'mg_sync_package__volume_remove' "$MG_RUN_DIR/hooks"
    rg -q 'mg_sync_package__stop' "$MG_RUN_DIR/hooks"
    rg -q 'mg_sync_server__remove_container' "$MG_RUN_DIR/hooks"
    case "$mode" in
      valid|cleanup|stop)
        [ "$(head -n 5 "$MG_RUN_DIR/export-sequence" | tr '\n' ' ')" = 'inspect export-ready inspect export-descriptor inspect ' ]
        rg -qx browser "$MG_RUN_DIR/export-sequence" ;;
      *) ! rg -qx browser "$MG_RUN_DIR/export-sequence" ;;
    esac
  done
}
