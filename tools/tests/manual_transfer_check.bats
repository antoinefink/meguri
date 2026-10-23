#!/usr/bin/env bats
# Controller contract doubles; real native execution and peer proof are separate.
bats_require_minimum_version 1.5.0

setup() {
  export FX_REPO="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
  export FX_ROOT="$BATS_TEST_TMPDIR/checkout" FX_VM="$BATS_TEST_TMPDIR/guest"
  export FX_EVENTS="$BATS_TEST_TMPDIR/events" FX_WRAPPER="$BATS_TEST_TMPDIR/vm-ssh"
  export MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH=1
  mkdir -p "$FX_ROOT/docker"
  cp "$FX_REPO/docker/syncserver.env.example" "$FX_ROOT/docker/"
  : >"$FX_EVENTS"
  cat >"$FX_WRAPPER" <<'SH'
#!/bin/bash
set -eu
forward=""
while [ "$#" -gt 1 ]; do
  case "$1" in
    -o)
      case "$2" in
        BatchMode=yes|ConnectTimeout=10|ExitOnForwardFailure=yes|ServerAliveInterval=10|ServerAliveCountMax=2) ;;
        *) exit 97 ;;
      esac
      shift 2
      ;;
    -R) forward="$2"; shift 2 ;;
    *) exit 97 ;;
  esac
done
[ "$forward" = '127.0.0.1:18765:127.0.0.1:19090' ] || exit 98
printf 'reverse-forward-loopback-only\n' >>"$FX_EVENTS"
# This parser asserts requested options without opening a network connection.
exec /bin/bash -c "${1//"$FX_ROOT"/"$FX_VM"}"
SH
  chmod +x "$FX_WRAPPER"
}

invoke_fake() { /bin/bash "$FX_REPO/tools/tests/manual_transfer_fakes.sh" "$@"; }
recorded_status() {
  cat "$FX_ROOT/build/mg/manual-transfer-check/fake-run/.mg/status"
}
guest_manifest() { printf '%s/app/Fixtures/ManualTransfer/controlled-rig.json' "$FX_VM"; }

@test "manual transfer: Mac evidence requires peer proof followed by cleanup" {
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = pass ]
  [ ! -e "$(guest_manifest)" ]
  [ ! -e "$FX_ROOT/app/Fixtures/ManualTransfer/controlled-rig.json" ]
  [ ! -e "$FX_ROOT/build/manual-transfer-check.lock" ]
  grep -qx 'selector -only-testing:MeguriUITests/ManualTransferJourneyTests/testDistinctAccountRoundTripPreservesImportCooldownAfterRestart' "$FX_EVENTS"
  python3 - "$FX_EVENTS" <<'PY'
from pathlib import Path
import sys
events = Path(sys.argv[1]).read_text().splitlines()
sequence = (
    "seed", "build mac Debug", "lease", "native-container", "stage-media", "stage-reader", "start-wire",
    "reverse-forward-loopback-only", "loopback-probe", "stage-manifest", "test mac", "wire-proof", "capture", "peer",
    "native_transfer_cleanup true", "release",
)
positions = [events.index(event) for event in sequence]
assert all(left < right for left, right in zip(positions, positions[1:]))
assert "native_transfer_complete true" in events
PY
}

@test "manual transfer: iPad work holds one VM lease for the selected method" {
  run invoke_fake check --platform ipad
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = pass ]
  [ "$(grep -cx lease "$FX_EVENTS")" -eq 1 ]
  for event in 'build ios-sim Debug' 'sim-lease ipad release' 'test ipad' \
      'selector -only-testing:MeguriUITests/ManualTransferJourneyTests/testDistinctAccountRoundTripPreservesImportCooldownAfterRestart'; do
    grep -qx "$event" "$FX_EVENTS"
  done
}

@test "manual transfer: no attestation means no synthetic account or VM work" {
  unset MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = warn ]
  ! grep -Eq '^(seed|lease|test|peer)$' "$FX_EVENTS"
}

@test "manual transfer: unavailable VM leaves setup pending before build" {
  export FX_MISSING_VM=1
  run invoke_fake check --platform iphone
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = warn ]
  ! grep -Eq '^(seed|build|lease)' "$FX_EVENTS"
}

@test "manual transfer: a route without a held lease is rejected" {
  export FX_NO_LEASE=1
  run invoke_fake forward
  [ "$status" -ne 0 ]
  [ "$(recorded_status)" = error ]
  ! grep -q reverse-forward "$FX_EVENTS"
}

@test "manual transfer: failed listener proof cleans up without running native work" {
  export FX_BAD_ROUTE=1
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = warn ]
  ! grep -q '^test ' "$FX_EVENTS"
  [ ! -e "$(guest_manifest)" ]
}

@test "manual transfer: unavailable manifest prevents execution and peer proof" {
  rm "$FX_ROOT/docker/syncserver.env.example"
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = error ]
  ! grep -q '^test ' "$FX_EVENTS"
  ! grep -qx peer "$FX_EVENTS"
  [ ! -e "$(guest_manifest)" ]
}

@test "manual transfer: one skipped case cannot supply a passing receipt" {
  export FX_SKIP=1
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = fail ]
  for forbidden in peer 'native_transfer_complete true'; do
    ! grep -qx "$forbidden" "$FX_EVENTS"
  done
  [ ! -e "$(guest_manifest)" ]
}

@test "manual transfer: zero native passes leave peer work unstarted" {
  export FX_PASSED=0
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = fail ]
  if grep -qx peer "$FX_EVENTS" || grep -qx 'native_transfer_complete true' "$FX_EVENTS"; then
    return 1
  fi
}

@test "manual transfer: an unrelated passing case fails identity validation" {
  export FX_TEST_ID=OtherTests/testOther
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = fail ]
  [ ! -e "$(guest_manifest)" ]
  if grep -qx peer "$FX_EVENTS" || grep -qx 'native_transfer_complete true' "$FX_EVENTS"; then
    return 1
  fi
}

@test "manual transfer: the exact qualified XCTest identifier is accepted" {
  export FX_TEST_ID='MeguriUITests/ManualTransferJourneyTests/testDistinctAccountRoundTripPreservesImportCooldownAfterRestart()'
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = pass ]
  grep -qx peer "$FX_EVENTS"
}

@test "manual transfer: input loss does not excuse an independent native failure" {
  export FX_INPUT_DROP=1 FX_INPUT_RESULT=1
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = fail ]
  ! grep -qx peer "$FX_EVENTS"
}

@test "manual transfer: differing logical cells fail the controller's receipt" {
  export FX_PEER_MATCH=false
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = fail ]
  grep -qx 'failure manual-transfer/peer-proof' "$FX_EVENTS"
  ! grep -qx 'native_transfer_complete true' "$FX_EVENTS"
  [ ! -e "$(guest_manifest)" ]
}

@test "manual transfer: collection-only peer success cannot supply media evidence" {
  export FX_MEDIA_MATCH=false
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = fail ]
  grep -qx 'failure manual-transfer/peer-proof' "$FX_EVENTS"
  ! grep -qx 'native_transfer_complete true' "$FX_EVENTS"
  [ ! -e "$FX_VM/build/mg/manual-transfer-check/fake-run/device-fixture" ]
  [ ! -e "$FX_VM/build/mg/manual-transfer-check/fake-run/media-proof-config.py" ]
}

@test "manual transfer: failed device fixture copy prevents native work and cleans inputs" {
  export FX_MEDIA_STAGE_FAIL=1
  run invoke_fake check --platform ipad
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = error ]
  ! grep -q '^test ' "$FX_EVENTS"
  ! grep -qx peer "$FX_EVENTS"
  [ ! -e "$(guest_manifest)" ]
  [ ! -e "$FX_ROOT/build/mg/manual-transfer-check/fake-run/device-fixture" ]
}

@test "manual transfer: manifest names the generated fixture without installing a collection" {
  run bash -c 'printf '\''{"port":19090,"token":"0000000000000000000000000000000000000000000000000000000000000000","nativeContext":"1111111111111111111111111111111111111111111111111111111111111111"}'\'' | python3 "$1/tools/scripts/manual-transfer-config.py" manifest "$2/docker/syncserver.env.example" 18765 "$2/build/mg/manual-transfer-check/fake-run/device-fixture" "$3/native-support"' _ "$FX_REPO" "$FX_ROOT" "$FX_VM"
  [ "$status" -eq 0 ]
  # Do not echo the manifest or assert on its credential values.
  python3 -c 'import json,sys; assert json.load(sys.stdin)["fixturePath"].endswith("/fake-run/device-fixture")' <<<"$output"
}

@test "manual transfer: wire evidence failure cannot pass on native text or peer bytes" {
  export FX_WIRE_FAIL=1
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = fail ]
  ! grep -qx peer "$FX_EVENTS"
  ! grep -qx 'native_transfer_complete true' "$FX_EVENTS"
  [ ! -e "$(guest_manifest)" ]
}

@test "manual transfer: real socket observer enforces consent counts gate and bounded teardown" {
  run python3 "$FX_REPO/tools/tests/manual_transfer_wire_test.py"
  [ "$status" -eq 0 ]
}

@test "manual transfer: missing native media evidence fails without exposing input" {
  mkdir -p "$FX_ROOT/native/ManualTransfers"
  printf '%s' '{"private":"never-print-this"}' >"$FX_ROOT/native/ManualTransfers/manual-transfers.json"
  run python3 "$FX_REPO/tools/scripts/manual-transfer-config.py" native-proof "$FX_ROOT/native"
  [ "$status" -ne 0 ]
  [[ "$output" != *never-print-this* ]]
}

@test "manual transfer: native reducer emits only media aggregates and fixed logical IDs" {
  python3 - "$FX_ROOT/native" <<'PY'
import json, sys
from pathlib import Path
root = Path(sys.argv[1])
(root / "ManualTransfers").mkdir(parents=True)
(root / "Collection/collection.media").mkdir(parents=True)
for name in ("rf4-server.bin", "rf4-device.bin", "rf4-zero.bin"):
    (root / "Collection/collection.media" / name).write_bytes(b"" if name.endswith("zero.bin") else b"synthetic")
counters = dict(uploaded_files=1, uploaded_deletions=0, downloaded_files=1,
                downloaded_deletions=0, checked=3, bytes_downloaded=9,
                bytes_uploaded=9, awaiting=0, resynced=False,
                empty=["rf4-zero.bin"], oversized=[])
operations = {}
for direction in ("Import", "Export"):
    operations[direction] = dict(direction=direction, prepared_at=10, revision=5,
        account="opaque-account-do-not-export", configured_endpoint="http://reserved.invalid/",
        actual_generation=2, phase="Complete", scope="CollectionAndMedia",
        collection_complete=True, receipt=dict(completed_at=20, next_eligible_at=3600020),
        media=dict(stage="Complete", exclusions_verified=True, remote_outcome_unknown=False,
            counters=counters, consent=dict(generation=2, categories=1,
                account="opaque-account-do-not-export", configured_endpoint="http://reserved.invalid/")))
(root / "ManualTransfers/manual-transfers.json").write_text(json.dumps(dict(manualTransfers=dict(operations=operations))))
PY
  run python3 "$FX_REPO/tools/scripts/manual-transfer-config.py" native-proof "$FX_ROOT/native"
  [ "$status" -eq 0 ]
  [[ "$output" != *opaque-account-do-not-export* ]]
  [[ "$output" != *reserved.invalid* ]]
  python3 -c 'import json,sys; p=json.load(sys.stdin); assert set(p)=={"stages","files"}; assert all(s["consent_bound"] for s in p["stages"].values()); assert p["files"]["rf4-zero.bin"]["bytes"]==0' <<<"$output"
  rm "$FX_ROOT/native/Collection/collection.media/rf4-device.bin"
  run python3 "$FX_REPO/tools/scripts/manual-transfer-config.py" native-proof "$FX_ROOT/native"
  [ "$status" -ne 0 ]
  [[ "$output" != *opaque-account-do-not-export* ]]
}

@test "manual transfer: cancellation cleans VM inputs before returning its lease" {
  run invoke_fake terminate
  [ "$status" -eq 143 ]
  [ ! -e "$(guest_manifest)" ]
  [ ! -e "$FX_ROOT/build/mg/manual-transfer-check/fake-run/route.fifo" ]
  [ "$(tail -n 1 "$FX_EVENTS")" = release ]
}

@test "manual transfer: refused account configuration never echoes its supplied secret" {
  run bash -c 'printf '\''[{"Config":{"Env":["SYNC_USER3=unknown:never-print-this"]}}]'\'' | python3 "$FX_REPO/tools/scripts/manual-transfer-config.py" check-users "$FX_ROOT/docker/syncserver.env.example"'
  [ "$status" -ne 0 ]
  [[ "$output" != *never-print-this* ]]
}

@test "manual transfer: refused reader staging prevents observer and native work" {
  export FX_READER_STAGE_FAIL=1
  run invoke_fake check --platform mac
  [ "$status" -eq 0 ]
  [ "$(recorded_status)" = error ]
  ! grep -qx start-wire "$FX_EVENTS"
  ! grep -q '^test ' "$FX_EVENTS"
  [ ! -e "$FX_ROOT/build/mg/manual-transfer-check/fake-run/native-reader-command.json" ]
  [ ! -e "$FX_VM/build/mg/manual-transfer-check/fake-run/native-reader-input.json" ]
  [ "$(tail -n 1 "$FX_EVENTS")" = release ]
}
