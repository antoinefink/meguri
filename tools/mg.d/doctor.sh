# Host readiness check. Sourced by tools/mg.
# doctor only reads state. It never changes a system setting, accepts a
# license, downloads a runtime, installs a toolchain or signs in (R1969); it
# lists what is missing in user_steps (docs/process/one-time-setup.md).

mg_job doctor host mg_doctor \
  "Check Xcode, iOS runtime, Docker, Rust targets and disk; list missing one-time user steps"

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
  local info="" cpus=0 mem=0 server="" compose=""
  info="$(docker info --format '{{json .}}' 2>/dev/null)"
  if [ -n "$info" ] && jq -e '.ServerVersion' >/dev/null 2>&1 <<<"$info"; then
    server="$(jq -r '.ServerVersion' <<<"$info")"
    cpus="$(jq -r '.NCPU // 0' <<<"$info")"
    mem="$(jq -r '.MemTotal // 0' <<<"$info")"
    compose="$(docker compose version --short 2>/dev/null)"
    if [ -n "$compose" ]; then
      mg__doctor_check docker pass "Docker $server, Compose $compose"
    else
      mg__doctor_check docker fail "Docker $server is reachable but 'docker compose' is missing"
    fi
    local mem_gib=$((mem / 1073741824))
    if [ "$cpus" -ge "$MG_DOCTOR_MIN_DOCKER_CPUS" ] && [ "$mem_gib" -ge "$MG_DOCTOR_MIN_DOCKER_MEM_GIB" ]; then
      mg__doctor_check docker-resources pass "$cpus CPUs, $mem_gib GiB memory"
    else
      mg__doctor_check docker-resources warn "$cpus CPUs, $mem_gib GiB memory; want at least $MG_DOCTOR_MIN_DOCKER_CPUS CPUs and $MG_DOCTOR_MIN_DOCKER_MEM_GIB GiB"
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
