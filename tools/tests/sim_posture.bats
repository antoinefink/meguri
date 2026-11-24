#!/usr/bin/env bats
# Focused preparation ordering; full VM dispatch remains in sim_vm.bats.

setup() {
  MG_RUN_DIR="$BATS_TEST_TMPDIR"
  MG_ROOT="$BATS_TEST_TMPDIR/checkout"
  MG_APP_DD=build/app/DerivedData
  local source="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
  mkdir -p "$MG_ROOT/tools"
  cp "$source/tools/sim-devices.json" "$MG_ROOT/tools/sim-devices.json"
  source "$source/tools/scripts/mac-vm-lib.sh"
  mg_job() { :; }
  source "$source/tools/mg.d/sim-posture.sh"
  mg_mac_vm_probe() {
    MVL_ALL='[{"name":"meguri-mac"}]'
    MG_MAC_VMS="$MVL_ALL"
  }
  mg_detail() { :; }
  mg_failure() { failure="$1"; }
  mg_status() { status="$1"; }
  mg_app_build() {
    echo "build $*" >>"$MG_RUN_DIR/events"
    [ "${build_fails:-false}" != true ] || return 1
    if [ "${expire_during_build:-false}" = true ]; then
      jq '.devices[0].lease.expires_epoch=1' "$MG_RUN_DIR/pool.json" >"$MG_RUN_DIR/expired.json"
      mv "$MG_RUN_DIR/expired.json" "$MG_RUN_DIR/pool.json"
    fi
  }
  mg_mac_vm_take() { echo take >>"$MG_RUN_DIR/events"; }
  mg_sim_pool_run() { echo 'unexpected pool routing' >&2; return 99; }
  mg_mac_vm_ssh() {
    [[ "$1" == *"MEGURI_SIM_POOL_IN_VM=1"* ]] || return 99
    [[ "$1" == *"MEGURI_SIM_DEVICES_JSON=$MG_RUN_DIR/sim-devices.json"* ]] || return 99
    [[ "$1" == *"/bin/bash '$MG_ROOT/tools/scripts/sim-pool.sh' status" ]] || return 99
    echo pool >>"$MG_RUN_DIR/events"
    cat "$MG_RUN_DIR/pool.json"
  }
  mg_sim_check_given() { echo 'unexpected ownership routing' >&2; return 99; }
  mg_sim_check_runtime() {
    [ "$2" = "$MG_RUN_DIR/sim-devices.json" ] && cmp "$2" "$MG_ROOT/tools/sim-devices.json"
  }
  mg_simctl() {
    if [ "$2" = devices ]; then
      jq '{devices:{runtime:[.devices[0] | {udid,name,state,isAvailable:true,deviceTypeIdentifier:"duo"}]}}' "$MG_RUN_DIR/pool.json"
    else
      echo '{"devicetypes":[{"identifier":"duo","name":"iPhone Duo","productFamily":"iPhone"}]}'
    fi
  }
  # Stop at publication: this fixture never starts a native test or SSH.
  mg_mac_vm_copy_in() {
    case "$1" in
      */tools/scripts/sim-pool.sh) echo pin >>"$MG_RUN_DIR/events" ;;
      *) echo copy >>"$MG_RUN_DIR/events"; return 1 ;;
    esac
  }
  udid=00000000-0000-4000-8000-000000000001
  jq -n --arg root "$MG_ROOT" --arg u "$udid" --argjson expiry "$(($(date +%s) + 600))" '{devices:[{
    udid:$u, kind:"foldable", name:"Meguri-Pool-foldable-1", state:"Booted",
    available:true, current_runtime:true, leased:true,
    runtime:"runtime", lease:{udid:$u,worktree:$root,vm:"meguri-mac",kind:"foldable",expires_epoch:$expiry}}]}' >"$MG_RUN_DIR/pool.json"
}

@test "sim-posture prepares Mac UI products before reserving the VM and reading the pool" {
  mg_sim_posture "$udid" open
  [ "$(cat "$MG_RUN_DIR/events")" = $'build mac Debug\ntake\npin\npool\ncopy' ]
  [ -z "${failure:-}" ]
}

@test "sim-posture refuses a simulator lease that expired during preparation" {
  expire_during_build=true
  mg_sim_posture "$udid" open
  [ "$failure" = posture-target ]
  [ "$status" = error ]
  [ "$(cat "$MG_RUN_DIR/events")" = $'build mac Debug\ntake\npin\npool' ]
}

@test "sim-posture never reserves the VM after failed preparation" {
  build_fails=true
  mg_sim_posture "$udid" closed
  [ "$(cat "$MG_RUN_DIR/events")" = 'build mac Debug' ]
}

@test "sim-posture validates the selected Xcode Hub identity before launching" {
  xcode-select() { echo /Selected/Xcode.app/Contents/Developer; }
  plist_buddy() {
    [ "$3" = /Selected/Xcode.app/Contents/Applications/DeviceHub.app/Contents/Info.plist ] || return 1
    printf '%s\n' "$bundle_id"
  }
  codesign() {
    [ "$*" = '--verify --strict /Selected/Xcode.app/Contents/Applications/DeviceHub.app' ] &&
      [ "${signature_valid:-true}" = true ]
  }
  open() { echo "$*" >>"$MG_RUN_DIR/events"; }
  mg_mac_vm_ssh() {
    local command="${1//\/usr\/libexec\/PlistBuddy/plist_buddy}"
    eval "${command//\/usr\/bin\/codesign/codesign}"
  }
  local bundle_id
  for bundle_id in com.apple.dt.DeviceHub com.apple.other.DeviceHub com.apple.dt.Devices.extra; do
    run mg_posture_hub
    [ "$status" -ne 0 ]
    [ ! -e "$MG_RUN_DIR/events" ]
  done
  bundle_id=com.apple.dt.Devices
  signature_valid=false
  run mg_posture_hub
  [ "$status" -ne 0 ]
  [ ! -e "$MG_RUN_DIR/events" ]
  signature_valid=true
  run mg_posture_hub
  [ "$status" -eq 0 ]
  [ "$output" = /Selected/Xcode.app/Contents/Applications/DeviceHub.app ]
  [ "$(cat "$MG_RUN_DIR/events")" = '-a /Selected/Xcode.app/Contents/Applications/DeviceHub.app' ]
}

@test "sim-posture rejects embedded newlines in the entire UUID before build or VM work" {
  local bad
  for bad in "$udid"$'\nforeign' $'foreign\n'"$udid" "$udid"$'\n'; do
    mg_sim_posture "$bad" open
    [ "$failure" = usage ]
    [ "$status" = error ]
    [ ! -e "$MG_RUN_DIR/events" ]
  done
}
