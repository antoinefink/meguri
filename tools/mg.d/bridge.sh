# Rust core bridge jobs: the XCFramework and the Swift package tests. Sourced
# by tools/mg; they run on the host (Apple SDK, xcodebuild, swift).

mg_job core-xcframework host mg_core_xcframework \
  "Build the core XCFramework and Swift bindings for MeguriCore; args go to tools/scripts/build-core-xcframework.sh (--configuration, --slices)"
mg_job swift-test host mg_swift_test \
  "swift test on a local package (--package, default MeguriCore), --config Debug|Profile (default Debug); matched core/SPM products, Profile tests under the host lease; any compiler warning fails; args after -- select tests"

# mg__xcframework <report> [script args...]: run the build script, record its
# report, and return its exit code.
mg__xcframework() {
  local report="$1" rc
  shift
  /bin/bash tools/scripts/build-core-xcframework.sh ${1+"$@"} --report "$report"
  rc=$?
  [ ! -s "$report" ] || mg_detail xcframework "$(cat "$report")"
  return "$rc"
}

# tools/mg core-xcframework [--configuration C] [--slices mac,sim,device]
mg_core_xcframework() {
  [ "${1:-}" = "--" ] && shift
  local report="$MG_RUN_DIR/xcframework.json" rc
  mg__xcframework "$report" ${1+"$@"}
  rc=$?
  if [ "$rc" -eq 0 ] && [ -s "$report" ]; then
    mg_artifact xcframework app/Packages/MeguriCore/Binaries/MeguriCoreFFI.xcframework
    mg_artifact swift app/Packages/MeguriCore/Sources/MeguriCoreBindings/MeguriCoreBindings.swift "generated bindings"
    mg_message "core-xcframework: $(jq -r '"\(.status), \(.configuration) (\(.slices)), \(.replaced | length) file(s) replaced"' "$report")"
    mg_status pass
  elif [ "$rc" -eq 2 ]; then
    mg_failure usage "build-core-xcframework.sh rejected its arguments; see log.txt"
    mg_status error
  else
    mg_failure xcframework "build-core-xcframework.sh failed (exit $rc); see log.txt"
    mg_status fail
  fi
}

# The fixture is emitted by real Rust publication into this run's directory.
# Keep its input and receipt with the Swift results; a missing or skipped
# producer stops verification before Apple compilation.
mg__swift_initial_adoption_fixture() {
  local fixture="$MG_RUN_DIR/initial-adoption-fixture.json" result
  unset MEGURI_INITIAL_ADOPTION_FIXTURE MEGURI_INITIAL_ADOPTION_RESULT
  if [ -e "$fixture" ] || [ -L "$fixture" ]; then
    mg_failure initial-adoption-fixture "Rust fixture publication must be fresh"
    mg_status fail
    return 1
  fi
  if ! result="$(MEGURI_INITIAL_ADOPTION_FIXTURE="/work/${fixture#"$MG_ROOT"/}" \
    "$MG_ROOT/tools/mg" core-test -- --test ffi_it \
      -E 'package(meguri-ffi) & test(=cloud_initial_app_fixture::export_initial_app_adoption_replies_from_real_publication)')"; then
    mg_failure initial-adoption-fixture "Rust fixture generation failed; Swift tests did not start"
    mg_status fail
    return 1
  fi
  if [ ! -f "$result" ] || [ -L "$result" ] ||
    ! jq -e '.job == "core-test" and .status == "pass" and .dry_run == false and .summary.total == 1 and .summary.passed == 1 and .summary.failed == 0 and .summary.skipped == 0' "$result" >/dev/null ||
    [ ! -f "$fixture" ] || [ ! -s "$fixture" ] || [ -L "$fixture" ]; then
    mg_failure initial-adoption-fixture "Rust fixture generation lacks a passing receipt and owned output"
    mg_status fail
    return 1
  fi
  mg_artifact json "$result" "initial adoption fixture producer"
  mg_artifact json "$fixture" "public initial adoption fixture"
  mg_detail initial_adoption_fixture "$(jq -nc --arg result "$result" --arg path "$fixture" \
    --arg sha256 "$(shasum -a 256 "$fixture" | awk '{print $1}')" \
    '{result: $result, path: $path, sha256: $sha256}')"
  export MEGURI_INITIAL_ADOPTION_FIXTURE="$fixture"
  export MEGURI_INITIAL_ADOPTION_RESULT="$result"
}

# Host-only synthetic Service helper for headless MeguriKit tests. Cargo owns
# its normal native cache; tests execute an exclusive copy owned by this run.
mg__swift_cloud_service_helper() {
  local build="$MG_RUN_DIR/cloud-service-build.jsonl" executable published
  unset MEGURI_CLOUD_SERVICE_CLI
  if ! (cd "$MG_ROOT/core" && cargo build -q -p meguri-cli --message-format=json) >"$build"; then
    mg_artifact log "$build" "cloud Service native Cargo build"
    mg_failure cloud-service-helper "native helper build failed; Swift tests did not start"
    mg_status fail
    return 1
  fi
  mg_artifact log "$build" "cloud Service native Cargo build"
  # Validate the complete JSON stream before publication; Cargo can succeed
  # and produce an executable while still reporting compiler warnings.
  if ! jq -es 'all(.[]; .reason != "compiler-message" or
    (.message.level != "warning" and .message.level != "error"))' "$build" >/dev/null 2>&1; then
    mg_failure cloud-service-helper "native helper build diagnostics rejected; Swift tests did not start"
    mg_status fail
    return 1
  fi
  if ! executable="$(jq -ers '[.[] | select(.reason=="compiler-artifact" and .target.name=="meguri-cli" and .executable!=null) | .executable]
    | if length==1 then .[0] else error("expected one meguri-cli executable") end' "$build")" ||
    ! published="$(python3 "$MG_ROOT/tools/scripts/swift-cloud-service-helper.py" "$MG_RUN_DIR" "$executable")"; then
    mg_failure cloud-service-helper "native helper publication failed; Swift tests did not start"
    mg_status fail
    return 1
  fi
  if [ "$published" != "$MG_RUN_DIR/cloud-service-code/meguri-cli" ] || [ ! -x "$published" ] || [ -L "$published" ]; then
    mg_failure cloud-service-helper "native helper publication lacks its owned executable"
    mg_status fail
    return 1
  fi
  mg_artifact executable "$published" "headless synthetic cloud Service helper"
  mg_artifact json "$MG_RUN_DIR/cloud-service-code.json" "native helper publication receipt"
  mg_detail cloud_service_helper "$(cat "$MG_RUN_DIR/cloud-service-code.json")"
  export MEGURI_CLOUD_SERVICE_CLI="$published"
}

# tools/mg swift-test [--package NAME] [--config Debug|Profile] [-- swift test args]
mg_swift_test() {
  local package=MeguriCore config=Debug spm_config=debug
  local selection_args=()
  local usage="usage: tools/mg swift-test [--package NAME] [--config Debug|Profile] [-- swift test args]"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --package)
        if [ "$#" -lt 2 ] || [ -z "$2" ]; then
          mg_failure usage "--package needs a package name; $usage" "tools/mg swift-test"
          mg_status error
          return 0
        fi
        package="$2"
        shift 2
        ;;
      --config)
        if [ "$#" -lt 2 ]; then
          mg_failure usage "--config needs Debug or Profile; $usage"
          mg_status error
          return 0
        fi
        config="$2"
        shift 2
        ;;
      --) shift && break ;;
      *)
        mg_failure usage "unknown argument '$1'; $usage" "tools/mg swift-test"
        mg_status error
        return 0
        ;;
    esac
  done
  case "$config" in
    Debug) ;;
    Profile) spm_config=release ;;
    *)
      mg_failure usage "--config needs Debug or Profile; $usage"
      mg_status error
      return 0
      ;;
  esac
  case "$package" in
    '' | *[!a-zA-Z0-9_-]*)
      mg_failure usage "--package needs a package directory name; $usage"
      mg_status error
      return 0
      ;;
  esac
  # The existing callers only select tests. Keep product/cache flags out of
  # the prebuilt invocation, including compiler and instrumentation options.
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --filter | --skip)
        if [ "$#" -lt 2 ] || [ -z "$2" ] || [[ "$2" = -* ]]; then
          mg_failure usage "$1 needs a test pattern; $usage"
          mg_status error
          return 0
        fi
        selection_args+=("$1" "$2")
        shift 2
        ;;
      --filter=?* | --skip=?*)
        selection_args+=("$1")
        shift
        ;;
      *)
        mg_failure usage "swift-test accepts only --filter/--skip test selection after --; use --config Debug|Profile for the build"
        mg_status error
        return 0
        ;;
    esac
  done
  local dir="app/Packages/$package"
  if [ ! -f "$dir/Package.swift" ]; then
    mg_failure no-package "$dir/Package.swift does not exist" "tools/mg swift-test"
    mg_status error
    return 0
  fi

  mg_lint_ui_first || return 0
  if [ "$package" = MeguriKit ]; then
    mg__swift_initial_adoption_fixture || return 0
    mg__swift_cloud_service_helper || return 0
  fi
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json" --configuration "$config"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_status fail
    return 0
  fi

  # Zero diagnostics (U6): every warning in the package, the generated
  # bindings included, fails the run.
  # Profile keeps optimized SPM products separate from ordinary Debug runs;
  # build and --skip-build test always name the exact same cache and mode.
  local product_args=(--configuration "$spm_config")
  if [ "$config" = Profile ]; then
    # swiftbuild's release --build-tests leaves source modules without
    # @testable access. Enable access without changing release optimization.
    product_args+=(--scratch-path "$MG_ROOT/build/swift-test/$package/Profile" -Xswiftc -enable-testing)
  fi
  mg_detail configuration "$(jq -nc --arg core "$config" --arg swift "$spm_config" \
    '{core: $core, swift: $swift}')"
  if [ "$config" = Profile ]; then
    mg_detail environment "$(jq -nc \
      --arg cpu "$(sysctl -n machdep.cpu.brand_string 2>/dev/null)" \
      --arg os "macOS $(sw_vers -productVersion 2>/dev/null)" \
      --arg os_build "$(sw_vers -buildVersion 2>/dev/null)" --arg arch "$(uname -m)" \
      '{cpu: $cpu, os: $os, os_build: $os_build, arch: $arch}')"
  fi
  local build="$MG_RUN_DIR/swift-build.log" rc
  swift build --package-path "$dir" "${product_args[@]}" --build-tests 2>&1 | tee "$build"
  rc=${PIPESTATUS[0]}
  mg_artifact log "$build" "swift build"
  local warnings
  warnings="$(perl -pe 's/\e\[[0-9;]*m//g' "$build" | grep -E '(^/.*: warning: |^warning: )' | sort -u)"
  if [ -n "$warnings" ]; then
    while IFS= read -r w; do
      mg_failure "warning/${w%%: warning: *}" "$w"
    done <<<"$warnings"
  fi
  if [ "$rc" -ne 0 ]; then
    mg_failure swift-build "swift build --build-tests failed (exit $rc); see swift-build.log"
    mg_status fail
    return 0
  fi
  if [ "$config" = Profile ]; then
    mg_host_lease --no-session || return 0
  fi

  local xunit="$MG_RUN_DIR/xunit.xml" tests="$MG_RUN_DIR/xunit-swift-testing.xml"
  swift test --package-path "$dir" "${product_args[@]}" --skip-build --xunit-output "$xunit" ${selection_args[0]+"${selection_args[@]}"}
  rc=$?
  local total=0 failed=0 skipped=0
  if [ -f "$tests" ]; then
    mg_artifact xunit "$tests"
    total="$(xmllint --xpath 'count(//testcase)' "$tests")"
    failed="$(xmllint --xpath 'count(//testcase[failure or error])' "$tests")"
    skipped="$(xmllint --xpath 'count(//testcase[skipped])' "$tests")"
    xmllint --xpath '//testcase[failure or error]/@name' "$tests" 2>/dev/null |
      sed -n 's/.*name="\([^"]*\)".*/\1/p' | while IFS= read -r name; do
      mg_failure "test/$name" "$name failed; see log.txt" "tools/mg swift-test --package $package --config $config -- --filter '$name'"
    done
  fi
  mg_count total "$total"
  mg_count passed $((total - failed - skipped))
  mg_count failed "$failed"
  mg_count skipped "$skipped"
  if [ "$rc" -ne 0 ] && [ "$failed" -eq 0 ]; then
    mg_failure swift-test-exit "swift test exited with code $rc without a failing test (crash or filter matching nothing); see log.txt"
  elif [ "$total" -eq 0 ]; then
    mg_failure no-tests "swift test ran no tests"
  fi
  if [ "$rc" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$total" -gt 0 ] && [ -z "$warnings" ]; then
    mg_message "swift-test $package: $total passed"
    mg_status pass
  else
    mg_message "swift-test $package: $failed failed of $total$([ -z "$warnings" ] || printf '; %s warning(s)' "$(grep -c . <<<"$warnings")") (swift test exit $rc)"
    mg_status fail
  fi
}
