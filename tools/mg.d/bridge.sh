# Rust core bridge jobs: the XCFramework and the Swift package tests. Sourced
# by tools/mg; they run on the host (Apple SDK, xcodebuild, swift).

mg_job core-xcframework host mg_core_xcframework \
  "Build the core XCFramework and Swift bindings for MeguriCore; args go to tools/scripts/build-core-xcframework.sh (--configuration, --slices)"
mg_job swift-test host mg_swift_test \
  "swift test on a local package (--package, default MeguriCore) after lint-ui and core-xcframework (Debug); any compiler warning fails; args after -- go to swift test"

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

# tools/mg swift-test [--package NAME] [-- swift test args]
mg_swift_test() {
  local package=MeguriCore usage="usage: tools/mg swift-test [--package NAME] [-- swift test args]"
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
      --) shift && break ;;
      *)
        mg_failure usage "unknown argument '$1'; $usage" "tools/mg swift-test"
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
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_status fail
    return 0
  fi

  # Zero diagnostics (U6): every warning in the package, the generated
  # bindings included, fails the run.
  local build="$MG_RUN_DIR/swift-build.log" rc
  swift build --package-path "$dir" --build-tests 2>&1 | tee "$build"
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

  local xunit="$MG_RUN_DIR/xunit.xml" tests="$MG_RUN_DIR/xunit-swift-testing.xml"
  swift test --package-path "$dir" --skip-build --xunit-output "$xunit" ${1+"$@"}
  rc=$?
  local total=0 failed=0 skipped=0
  if [ -f "$tests" ]; then
    mg_artifact xunit "$tests"
    total="$(xmllint --xpath 'count(//testcase)' "$tests")"
    failed="$(xmllint --xpath 'count(//testcase[failure or error])' "$tests")"
    skipped="$(xmllint --xpath 'count(//testcase[skipped])' "$tests")"
    xmllint --xpath '//testcase[failure or error]/@name' "$tests" 2>/dev/null |
      sed -n 's/.*name="\([^"]*\)".*/\1/p' | while IFS= read -r name; do
      mg_failure "test/$name" "$name failed; see log.txt" "tools/mg swift-test --package $package -- --filter '$name'"
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
