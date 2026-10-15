# Host readiness check. Sourced by tools/mg.
# doctor only reads state. It never changes a system setting, accepts a
# license, downloads a runtime, installs a toolchain or signs in (R1969); it
# lists what is missing in user_steps (docs/process/one-time-setup.md).

mg_job doctor host mg_doctor \
  "Check Xcode, iOS runtime, Docker, Rust targets, disk and the macOS test VM (Mac UI suites and simulators); list missing one-time user steps"

MG_DOCTOR_TEAM=DNQ34W43XX
MG_DOCTOR_MIN_XCODE_MAJOR=27
MG_DOCTOR_MIN_DOCKER_CPUS=12
MG_DOCTOR_MIN_DOCKER_MEM_GIB=16
MG_DOCTOR_WARN_DISK_GIB=40
MG_DOCTOR_FAIL_DISK_GIB=10

# mg__doctor_check <id> <pass|warn|fail> <message>
mg__doctor_check() {
  jq -nc --arg id "$1" --arg s "$2" --arg m "$3" '{id: $id, status: $s, message: $m}' >>"$MG_RUN_DIR/checks.jsonl"
  printf '  %-4s  %-16s %s\n' "$2" "$1" "$3"
  if [ "$2" = fail ]; then
    mg_failure "$1" "$3" "tools/mg doctor"
  fi
}

# mg__doctor_env <key> <string value>
mg__doctor_env() {
  jq -nc --arg k "$1" --arg v "$2" '{($k): (if $v == "" then null else $v end)}' >>"$MG_RUN_DIR/environment.jsonl"
}

# Reads `key = [ "a", "b" ]` or `key = "a"` from core/rust-toolchain.toml, one value per line.
mg__doctor_toolchain_values() {
  sed -n "s/^$1 *= *//p" core/rust-toolchain.toml 2>/dev/null | tr -d '[]" ' | tr ',' '\n' | sed '/^$/d'
}

# mg__doctor_mac_vm <host Xcode version> <host Xcode build>: the macOS test
# VM (mac-vm.sh, docs/process/mac-ui-vm.md): configured, its drive
# connected, present, running, answering over SSH, with the host's Xcode and
# the pinned iOS simulator runtime (tools/sim-devices.json; U8c). It reads
# only: a stopped VM is not started (jobs start it). Problems warn; Mac UI
# suites and simulator jobs queue without the VM.
mg__doctor_mac_vm() {
  local host_xcode="$1 ($2)" rc n i=0 vm name tart ssh state out vxv vxb status msg vms='[]' missing=0 pin rt
  mg__mac_vm_read_config
  rc=$?
  if [ "$rc" -ne 0 ]; then
    mg__doctor_check mac-vm warn "$MG_MAC_VM_REASON; Mac UI suites and simulator jobs queue until there is one"
    mg_mac_vm_user_step
    mg_detail mac_vm "$(jq -nc --arg c "$(mg_mac_vm_config_path)" --argjson ok "$([ "$rc" -eq 1 ] && echo false || echo null)" '{config: $c, configured: $ok, vms: []}')"
    return 0
  fi
  pin="$(jq -c .runtime tools/sim-devices.json)"
  n="$(jq length <<<"$MG_MAC_VM_ALL")"
  while [ "$i" -lt "$n" ]; do
    vm="$(jq -c --argjson i "$i" '.[$i]' <<<"$MG_MAC_VM_ALL")"
    i=$((i + 1))
    name="$(jq -r .name <<<"$vm")"
    tart="$(jq -r .tart <<<"$vm")"
    ssh="$(jq -r .ssh <<<"$vm")"
    state="" vxv="" vxb="" rt=""
    if [ ! -x "$tart" ] || [ ! -x "$ssh" ]; then
      status=warn missing=1
      msg="$name: $([ -x "$tart" ] && echo "its SSH wrapper $ssh" || echo "the tart wrapper $tart") is not there; connect the drive that holds the VM"
    else
      state="$(mg__mac_vm_state "$tart" "$name")"
      case "$state" in
        "") status=warn msg="$name: '$tart list' failed" ;;
        missing) status=warn missing=1 msg="$name: tart lists no such VM" ;;
        running)
          out="$("$ssh" "xcodebuild -version" </dev/null 2>/dev/null)"
          vxv="$(printf '%s\n' "$out" | sed -n 's/^Xcode \([0-9.]*\).*/\1/p' | head -n 1)"
          vxb="$(printf '%s\n' "$out" | sed -n 's/^Build version \(.*\)/\1/p' | head -n 1)"
          if [ -z "$vxv" ]; then
            status=warn msg="$name: running, but SSH does not answer ($ssh)"
          elif [ "$vxv ($vxb)" != "$host_xcode" ]; then
            status=warn msg="$name: Xcode $vxv ($vxb) in the VM, $host_xcode on this Mac; Mac UI results may differ"
          else
            status=pass msg="$name: running, SSH answers, Xcode $vxv ($vxb) as on this Mac"
          fi
          if [ -n "$vxv" ]; then
            # The simulators' runtime (U8c): the pool runs there on it.
            rt="$("$ssh" "xcrun simctl list runtimes --json" </dev/null 2>/dev/null | jq -r --argjson p "$pin" \
              'if any(.runtimes[]?; .identifier == $p.identifier and .buildversion == $p.build and .isAvailable != false) then "yes" else "no" end' 2>/dev/null)"
            if [ "$rt" = yes ]; then
              mg__doctor_check "mac-vm/$name/simulators" pass "$name: iOS $(jq -r '"\(.version) (\(.build))"' <<<"$pin") simulator runtime; the simulator pool runs there"
            else
              rt=no
              mg__doctor_check "mac-vm/$name/simulators" warn "$name: no iOS $(jq -r '"\(.version) (\(.build))"' <<<"$pin") simulator runtime; add it with tools/vm/add-sim-runtime.sh (docs/process/mac-ui-vm.md), or simulator jobs fail there"
            fi
          fi
          ;;
        *) status=pass msg="$name: $state; Mac UI and simulator jobs start it headless when they need it" ;;
      esac
    fi
    mg__doctor_check "mac-vm/$name" "$status" "$msg"
    vms="$(jq -c --arg n "$name" --argjson d "$([ -x "$tart" ] && [ -x "$ssh" ] && echo true || echo false)" --arg s "$state" \
      --arg x "$vxv" --arg b "$vxb" --arg hx "$host_xcode" --arg rt "$rt" '. + [{name: $n, drive_connected: $d,
        present: (if $s == "" then null else $s != "missing" end), state: (if $s == "" or $s == "missing" then null else $s end),
        ssh: (if $s != "running" then null else $x != "" end),
        xcode: (if $x == "" then null else "\($x) (\($b))" end), xcode_matches_host: (if $x == "" then null else "\($x) (\($b))" == $hx end),
        sim_runtime: (if $rt == "" then null else $rt == "yes" end)}]' <<<"$vms")"
  done
  [ "$missing" -eq 0 ] || mg_mac_vm_user_step
  mg_detail mac_vm "$(jq -nc --arg c "$(mg_mac_vm_config_path)" --argjson v "$vms" '{config: $c, configured: true, vms: $v}')"
}

mg_doctor() {
  : >"$MG_RUN_DIR/checks.jsonl"
  : >"$MG_RUN_DIR/environment.jsonl"
  echo "doctor: checking this host (read-only)"

  # Host
  local os arch macos=""
  os="$(uname -s)"
  arch="$(uname -m)"
  [ "$os" = Darwin ] && macos="$(sw_vers -productVersion 2>/dev/null)"
  mg__doctor_env macos "$macos"
  mg__doctor_env arch "$arch"
  if [ "$os" = Darwin ] && [ "$arch" = arm64 ]; then
    mg__doctor_check host pass "macOS $macos on arm64"
  else
    mg__doctor_check host fail "expected an Apple-silicon Mac, found $os $arch"
  fi

  # Active developer directory
  local devdir
  devdir="$(xcode-select -p 2>/dev/null)"
  mg__doctor_env developer_dir "$devdir"
  case "$devdir" in
    */*.app/Contents/Developer) mg__doctor_check xcode-select pass "$devdir" ;;
    *)
      mg__doctor_check xcode-select fail "active developer directory is '${devdir:-none}', not an Xcode app"
      mg_user_step xcode-select missing true "Select Xcode as the active developer directory" \
        "Run 'sudo xcode-select -s /Applications/Xcode.app/Contents/Developer' yourself."
      ;;
  esac

  # Xcode version and build
  local xv xversion="" xbuild="" xmajor=0 xminor=0
  xv="$(xcodebuild -version 2>/dev/null)"
  xversion="$(printf '%s\n' "$xv" | sed -n 's/^Xcode \([0-9.]*\).*/\1/p' | head -n 1)"
  xbuild="$(printf '%s\n' "$xv" | sed -n 's/^Build version \(.*\)/\1/p' | head -n 1)"
  mg__doctor_env xcode_version "$xversion"
  mg__doctor_env xcode_build "$xbuild"
  if [ -n "$xversion" ]; then
    xmajor="${xversion%%.*}"
    xminor="$(printf '%s' "$xversion" | cut -s -d. -f2)"
    xminor="${xminor:-0}"
  fi
  if [ -z "$xversion" ]; then
    mg__doctor_check xcode fail "xcodebuild -version failed; install Xcode $MG_DOCTOR_MIN_XCODE_MAJOR"
  elif [ "$xmajor" -lt "$MG_DOCTOR_MIN_XCODE_MAJOR" ]; then
    mg__doctor_check xcode fail "Xcode $xversion ($xbuild); Meguri needs Xcode $MG_DOCTOR_MIN_XCODE_MAJOR"
  else
    mg__doctor_check xcode pass "Xcode $xversion ($xbuild)"
  fi
  if [ -n "$xversion" ] && { [ "$xmajor" -lt 27 ] || { [ "$xmajor" -eq 27 ] && [ "$xminor" -lt 1 ]; }; }; then
    mg_user_step xcode-27-1 missing false "Install Xcode 27.1 when it ships" \
      "Install Xcode 27.1 once released; it adds the foldable iPhone simulator (R1962). Until then foldable layouts are approximated."
  fi

  # Xcode license
  if xcodebuild -license check >/dev/null 2>&1; then
    mg__doctor_check xcode-license pass "license accepted"
  else
    mg__doctor_check xcode-license fail "the Xcode license is not accepted"
    mg_user_step xcode-license missing true "Accept the Xcode license" \
      "Open Xcode once and accept the license, or run 'sudo xcodebuild -license accept' yourself."
  fi

  # iOS 27 simulator runtime (iPhone and iPad simulators share it)
  local runtimes ios27
  runtimes="$(xcrun simctl list runtimes --json 2>/dev/null |
    jq -c '[.runtimes[]? | select(((.platform // "") == "iOS") or ((.identifier // "") | test("SimRuntime\\.iOS-")))
            | {identifier, version, build: .buildversion, available: .isAvailable}]' 2>/dev/null)"
  [ -n "$runtimes" ] || runtimes="[]"
  mg_detail ios_runtimes "$runtimes"
  ios27="$(jq -r '[.[] | select(.available == true and ((.version // "") | startswith("27.")))] | first
                 | if . == null then "" else "iOS \(.version) (\(.build))" end' <<<"$runtimes")"
  if [ -n "$ios27" ]; then
    mg__doctor_check ios-runtime pass "$ios27 simulator runtime"
  else
    mg__doctor_check ios-runtime fail "no available iOS 27 simulator runtime"
    mg_user_step ios-runtime missing true "Install the iOS 27 simulator runtime" \
      "Xcode > Settings > Components: get the iOS 27 simulator runtime (or run 'xcodebuild -downloadPlatform iOS' yourself)."
  fi

  # Xcode account for the team (read-only preference lookup)
  if { defaults read com.apple.dt.Xcode IDEProvisioningTeamByIdentifier 2>/dev/null
    defaults read com.apple.dt.Xcode IDEProvisioningTeams 2>/dev/null; } | grep -q "$MG_DOCTOR_TEAM"; then
    mg__doctor_check xcode-account pass "team $MG_DOCTOR_TEAM is in Xcode's accounts"
  else
    mg__doctor_check xcode-account warn "no Xcode account for team $MG_DOCTOR_TEAM: Mac builds are ad-hoc signed, device builds are blocked"
    mg_user_step xcode-account missing true "Add the Apple account for team $MG_DOCTOR_TEAM to Xcode" \
      "Xcode > Settings > Accounts > + > Apple Account: sign in with the account of team $MG_DOCTOR_TEAM."
  fi

  # Docker
  # With the daemon unreachable, `docker info` still prints the client-side
  # JSON (ServerVersion "", NCPU 0) and exits 1, so check both.
  local info="" cpus=0 mem=0 server="" compose=""
  if info="$(docker info --format '{{json .}}' 2>/dev/null)" &&
    jq -e '(.ServerVersion // "") != ""' >/dev/null 2>&1 <<<"$info"; then
    server="$(jq -r '.ServerVersion' <<<"$info")"
    cpus="$(jq -r '.NCPU // 0' <<<"$info")"
    mem="$(jq -r '.MemTotal // 0' <<<"$info")"
    compose="$(docker compose version --short 2>/dev/null)"
    if [ -n "$compose" ]; then
      mg__doctor_check docker pass "Docker $server, Compose $compose"
    else
      mg__doctor_check docker fail "Docker $server is reachable but 'docker compose' is missing"
    fi
    # MemTotal is the Linux VM's total after the kernel's reservation, up to
    # about 1 GiB below what Docker was given (OrbStack: 44 GiB given, 43.1
    # reported), so accept 90% of the documented setting.
    local mem_text
    mem_text="$(jq -n --argjson m "$mem" '$m / 1073741824 * 10 | floor / 10')"
    if [ "$cpus" -ge "$MG_DOCTOR_MIN_DOCKER_CPUS" ] &&
      [ "$mem" -ge $((MG_DOCTOR_MIN_DOCKER_MEM_GIB * 1073741824 * 9 / 10)) ]; then
      mg__doctor_check docker-resources pass "$cpus CPUs, $mem_text GiB memory in the Docker VM"
    else
      mg__doctor_check docker-resources warn "$cpus CPUs, $mem_text GiB memory in the Docker VM; give Docker at least $MG_DOCTOR_MIN_DOCKER_CPUS CPUs and $MG_DOCTOR_MIN_DOCKER_MEM_GIB GB"
      mg_user_step docker-resources missing true "Give Docker enough CPUs and memory" \
        "Docker Desktop > Settings > Resources: at least $MG_DOCTOR_MIN_DOCKER_CPUS CPUs and $MG_DOCTOR_MIN_DOCKER_MEM_GIB GB memory (12 oracle workers)."
    fi
  else
    mg__doctor_check docker fail "Docker is not reachable; start Docker Desktop"
  fi
  mg__doctor_env docker_server "$server"
  mg__doctor_env docker_compose "$compose"
  mg_detail docker_resources "$(jq -nc --argjson c "${cpus:-0}" --argjson m "${mem:-0}" '{cpus: $c, memory_bytes: $m}')"

  # Rust toolchain from core/rust-toolchain.toml (agents may install it; not a user step)
  local channel missing="" want have
  channel="$(mg__doctor_toolchain_values channel | head -n 1)"
  mg__doctor_env rust_channel "$channel"
  if [ -z "$channel" ]; then
    mg__doctor_check rust fail "core/rust-toolchain.toml has no channel"
  elif ! command -v rustup >/dev/null 2>&1; then
    mg__doctor_check rust fail "rustup not found; install it from https://rustup.rs"
  elif ! RUSTUP_AUTO_INSTALL=0 rustup toolchain list 2>/dev/null | grep -q "^$channel-"; then
    mg__doctor_check rust warn "Rust $channel is not installed; run: (cd core && rustup toolchain install)"
  else
    have="$(RUSTUP_AUTO_INSTALL=0 rustup "+$channel" target list --installed 2>/dev/null)"
    for want in $(mg__doctor_toolchain_values targets); do
      printf '%s\n' "$have" | grep -qx "$want" || missing="$missing $want"
    done
    have="$(RUSTUP_AUTO_INSTALL=0 rustup "+$channel" component list --installed 2>/dev/null)"
    for want in $(mg__doctor_toolchain_values components); do
      printf '%s\n' "$have" | grep -Eq "^$want(-|$)" || missing="$missing $want"
    done
    if [ -z "$missing" ]; then
      mg__doctor_check rust pass "Rust $channel with $(mg__doctor_toolchain_values targets | tr '\n' ' ')and $(mg__doctor_toolchain_values components | tr '\n' ' ' | sed 's/ $//')"
    else
      mg__doctor_check rust warn "Rust $channel lacks:$missing; run: (cd core && rustup toolchain install)"
    fi
  fi

  # Free disk space where the checkout lives
  local avail_kb gib
  avail_kb="$(df -Pk "$MG_ROOT" 2>/dev/null | awk 'NR == 2 { print $4 }')"
  gib=$((${avail_kb:-0} / 1048576))
  mg_detail free_disk_gib "$gib"
  if [ "$gib" -ge "$MG_DOCTOR_WARN_DISK_GIB" ]; then
    mg__doctor_check disk pass "$gib GiB free"
  elif [ "$gib" -ge "$MG_DOCTOR_FAIL_DISK_GIB" ]; then
    mg__doctor_check disk warn "$gib GiB free; simulators, DerivedData and images want at least $MG_DOCTOR_WARN_DISK_GIB GiB"
  else
    mg__doctor_check disk fail "$gib GiB free; need at least $MG_DOCTOR_FAIL_DISK_GIB GiB"
  fi

  # The Mac UI test VM (U8b)
  mg__doctor_mac_vm "$xversion" "$xbuild"

  # Steps doctor cannot verify from the command line
  mg_user_step uitest-automation unverified true "Let the Xcode UI-test runner control the Mac" \
    "On the first macOS UI-test run, allow the test runner in System Settings > Privacy & Security > Accessibility and Automation."
  mg_user_step device-developer-mode unverified false "Enable Developer Mode on test devices" \
    "On each iPhone and iPad used for milestone runs: Settings > Privacy & Security > Developer Mode."
  mg_user_step screen-recording unverified false "Allow Screen Recording for macOS video capture" \
    "Only if macOS motion capture is wanted: System Settings > Privacy & Security > Screen Recording."
  mg_user_step xcode-external-agents unverified false "Allow external agents to use Xcode tools" \
    "Optional: enable 'Allow external agents to use Xcode tools' in Xcode's settings."
  mg_user_step nightly-scheduler unverified false "Approve the scheduler entry for nightly jobs" \
    "When tools/mg nightly (U19) asks for its scheduler entry, approve it."

  # Result
  local checks env npass nwarn nfail status
  checks="$(jq -sc . "$MG_RUN_DIR/checks.jsonl")"
  env="$(jq -sc 'reduce .[] as $e ({}; . + $e)' "$MG_RUN_DIR/environment.jsonl")"
  rm -f "$MG_RUN_DIR/checks.jsonl" "$MG_RUN_DIR/environment.jsonl"
  mg_detail checks "$checks"
  mg_detail environment "$env"
  npass="$(jq '[.[] | select(.status == "pass")] | length' <<<"$checks")"
  nwarn="$(jq '[.[] | select(.status == "warn")] | length' <<<"$checks")"
  nfail="$(jq '[.[] | select(.status == "fail")] | length' <<<"$checks")"
  mg_count total $((npass + nwarn + nfail))
  mg_count passed "$npass"
  mg_count warned "$nwarn"
  mg_count failed "$nfail"
  if [ "$nfail" -gt 0 ]; then
    status=fail
  elif [ "$nwarn" -gt 0 ]; then
    status=warn
  else
    status=pass
  fi
  jq -r '"  [\(.state)] \(.id): \(.title)\n      \(.how)"' "$MG_STATE/user_steps.jsonl" |
    { echo "one-time user steps (perform them yourself; see docs/process/one-time-setup.md):"; cat; }
  mg_message "doctor: $npass pass, $nwarn warn, $nfail fail; user steps: $(jq -rs '
    "\(map(select(.state == "missing" and .required)) | length) required missing, " +
    "\(map(select(.state == "missing" and (.required | not))) | length) optional missing, " +
    "\(map(select(.state == "unverified")) | length) unverified"' "$MG_STATE/user_steps.jsonl")"
  mg_status "$status"
}
