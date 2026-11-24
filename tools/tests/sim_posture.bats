#!/usr/bin/env bats
# Focused preparation ordering; full VM dispatch remains in sim_vm.bats.

setup() {
  MG_RUN_DIR="$BATS_TEST_TMPDIR"
  MG_ROOT=/example
  MG_APP_DD=build/app/DerivedData
  local source="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
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
  mg_sim_pool_run() {
    echo pool >>"$MG_RUN_DIR/events"
    cat "$MG_RUN_DIR/pool.json"
  }
  mg_sim_check_given() { :; }
  mg_simctl() {
    if [ "$2" = devices ]; then
      jq '{devices:{runtime:[.devices[0] | {udid,name,state,isAvailable:true,deviceTypeIdentifier:"duo"}]}}' "$MG_RUN_DIR/pool.json"
    else
      echo '{"devicetypes":[{"identifier":"duo","name":"iPhone Duo","productFamily":"iPhone"}]}'
    fi
  }
  # Stop at publication: this fixture never starts a native test or SSH.
  mg_mac_vm_copy_in() { echo copy >>"$MG_RUN_DIR/events"; return 1; }
  udid=00000000-0000-4000-8000-000000000001
  jq -n --arg u "$udid" --argjson expiry "$(($(date +%s) + 600))" '{devices:[{
    udid:$u, kind:"foldable", name:"Meguri-Pool-foldable-1", state:"Booted",
    available:true, current_runtime:true, leased:true,
    runtime:"runtime", lease:{udid:$u,worktree:"/example",vm:"meguri-mac",kind:"foldable",expires_epoch:$expiry}}]}' >"$MG_RUN_DIR/pool.json"
}

@test "sim-posture prepares Mac UI products before reserving the VM and reading the pool" {
  mg_sim_posture "$udid" open
  [ "$(cat "$MG_RUN_DIR/events")" = $'build mac Debug\ntake\npool\ncopy' ]
  [ -z "${failure:-}" ]
}

@test "sim-posture refuses a simulator lease that expired during preparation" {
  expire_during_build=true
  mg_sim_posture "$udid" open
  [ "$failure" = posture-target ]
  [ "$status" = error ]
  [ "$(cat "$MG_RUN_DIR/events")" = $'build mac Debug\ntake\npool' ]
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
  open() { echo "$*" >>"$MG_RUN_DIR/events"; }
  mg_mac_vm_ssh() { eval "${1//\/usr\/libexec\/PlistBuddy/plist_buddy}"; }
  local bundle_id
  for bundle_id in com.apple.dt.DeviceHub com.apple.other.DeviceHub com.apple.dt.Devices.extra; do
    run mg_posture_hub
    [ "$status" -ne 0 ]
    [ ! -e "$MG_RUN_DIR/events" ]
  done
  bundle_id=com.apple.dt.Devices
  run mg_posture_hub
  [ "$status" -eq 0 ]
  [ "$output" = com.apple.dt.Devices ]
  [ "$(cat "$MG_RUN_DIR/events")" = '-g -a /Selected/Xcode.app/Contents/Applications/DeviceHub.app' ]
}
