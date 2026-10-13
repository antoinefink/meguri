# Rust core jobs. Sourced by tools/mg; they run inside the `dev` container.

mg_job core-test docker:dev mg_core_test \
  "Rust tests: cargo nextest on core/ (no retries), canary_* test binaries first; args after -- go to nextest"
mg_job core-lint docker:dev mg_core_lint \
  "Rust lint: cargo fmt --check, clippy -D warnings on all targets and features, layering-check (edges.toml)"
mg_job core-deny docker:dev mg_core_deny \
  "Rust dependency gate: cargo deny check on core/ (licenses, advisories, bans, sources); args after -- name checks"

# Canary test binaries (tests/canary_*.rs) run first and alone in a bare
# core-test; when one fails, the other tests do not run (AE82 ordering).
# The canary phase builds only the packages that hold a canary, so a build
# error elsewhere (say, code broken by a pin bump) cannot hide the pin canary.
# nextest rejects a binary() filter that matches no binary, so the canary
# phase runs only when a canary exists.
MG_CORE_CANARIES='binary(/^canary_/)'

# mg__core_canary_packages: workspace packages with a canary_* test target, one per line.
mg__core_canary_packages() {
  (cd core && cargo metadata --no-deps --format-version 1) |
    jq -r '.packages[] | select(any(.targets[]; any(.kind[]; . == "test") and (.name | startswith("canary_")))) | .name' |
    sort -u
}

mg__core_environment() {
  mg_detail environment "$(
    cd core 2>/dev/null || cd .
    jq -nc \
      --arg container "${MG_CONTAINER:-}" \
      --arg os "$(uname -sm)" \
      --arg rustc "$(rustc -V 2>&1)" \
      --arg nextest "$(cargo nextest --version 2>&1 | head -n 1)" \
      --arg deny "$(cargo deny --version 2>&1 | head -n 1)" \
      '{container: $container, os: $os, rustc: $rustc, nextest: $nextest, cargo_deny: $deny}'
  )"
}

# mg__nextest <jsonl> <package scope and nextest args>: one nextest run in
# core/. The caller passes the scope: --workspace or -p <package>...
mg__nextest() {
  local out="$1"
  shift
  (
    cd core &&
      NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run \
        --retries 0 --no-fail-fast \
        --message-format libtest-json-plus --message-format-version 0.1 \
        ${1+"$@"} >"$out"
  )
}

# tools/mg core-test [-- nextest args, e.g. -E 'package(meguri-text)']
mg_core_test() {
  [ "${1:-}" = "--" ] && shift
  mg__core_environment

  # Only a bare run may match no tests (no or empty workspace). With nextest
  # arguments, an empty match must fail, or a stale repro command would pass.
  if [ ! -f core/Cargo.toml ]; then
    if [ "$#" -eq 0 ]; then
      mg_message "core-test: no Cargo workspace in core/ yet; 0 tests"
      mg_status pass
    else
      mg_failure no-tests-matched "core/ has no Cargo workspace yet, so no test matches the nextest arguments"
      mg_message "core-test: no Cargo workspace in core/ yet; no test matches the nextest arguments"
      mg_status fail
    fi
    return 0
  fi

  local main="$MG_RUN_DIR/nextest.jsonl" canary="$MG_RUN_DIR/nextest-canary.jsonl"
  local rc stopped=false pkg
  local files=() canaries=()
  if [ "$#" -eq 0 ]; then
    while IFS= read -r pkg; do
      if [ -n "$pkg" ]; then canaries+=(-p "$pkg"); fi
    done < <(mg__core_canary_packages)
  fi
  if [ "${#canaries[@]}" -gt 0 ]; then
    mg__nextest "$canary" "${canaries[@]}" --no-tests=pass -E "$MG_CORE_CANARIES"
    rc=$?
    mg_artifact nextest-canary-jsonl "$canary"
    files=("$canary")
    if [ "$rc" -ne 0 ]; then
      stopped=true
    else
      mg__nextest "$main" --workspace --no-tests=pass -E "not ($MG_CORE_CANARIES)"
      rc=$?
      mg_artifact nextest-jsonl "$main"
      files=("$canary" "$main")
    fi
  else
    [ "$#" -gt 0 ] || set -- --no-tests=pass
    mg__nextest "$main" --workspace "$@"
    rc=$?
    mg_artifact nextest-jsonl "$main"
    files=("$main")
  fi

  # Test names look like `<crate>::<binary>$<test path>`.
  local counts
  counts="$(cat "${files[@]}" | jq -sc '[.[] | select(.type == "test" and .event != "started")] |
    {passed: map(select(.event == "ok")) | length,
     skipped: map(select(.event == "ignored")) | length,
     failed: map(select(.event != "ok" and .event != "ignored")) | length}')"
  local passed failed skipped
  passed="$(jq -r .passed <<<"$counts")"
  failed="$(jq -r .failed <<<"$counts")"
  skipped="$(jq -r .skipped <<<"$counts")"
  mg_count total $((passed + failed + skipped))
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count skipped "$skipped"

  cat "${files[@]}" | jq -c 'select(.type == "test" and .event != "started" and .event != "ok" and .event != "ignored")
    | (.name | split("$")) as $p
    | {id: .name,
       message: ((.stdout // .event) | .[0:2000]),
       repro: ("tools/mg core-test -- -E " + ("package(" + ($p[0] | split("::")[0]) + ") & test(=" + ($p[1] // $p[0]) + ")" | @sh))}' \
    >>"$MG_STATE/failures.jsonl"

  if [ "$rc" -eq 4 ] && [ $((passed + failed + skipped)) -eq 0 ]; then
    mg_failure no-tests-matched "no test matched the nextest arguments; check the filter (a renamed test?)"
  elif [ "$rc" -ne 0 ] && [ "$failed" -eq 0 ]; then
    mg_failure "nextest-exit-$rc" "cargo nextest exited with code $rc before reporting a test failure (build or setup error); see log.txt"
  fi
  if [ "$rc" -eq 0 ] && [ "$failed" -eq 0 ]; then
    mg_message "core-test: $passed passed, $skipped skipped"
    mg_status pass
  elif [ "$stopped" = true ] && [ "$failed" -gt 0 ]; then
    mg_message "core-test: canary failed ($failed failed, $passed passed); the other tests did not run (nextest exit $rc)"
    mg_status fail
  else
    mg_message "core-test: $failed failed, $passed passed, $skipped skipped (nextest exit $rc)"
    mg_status fail
  fi
}

# tools/mg core-lint: rustfmt, clippy and layering-check. All three run, so one
# run reports every kind of finding.
mg_core_lint() {
  [ "${1:-}" = "--" ] && shift
  if [ "$#" -gt 0 ]; then
    mg_failure usage "core-lint takes no arguments" "tools/mg core-lint"
    mg_status error
    return 0
  fi
  mg__core_environment
  if [ ! -f core/Cargo.toml ]; then
    mg_message "core-lint: no Cargo workspace in core/ yet; nothing to lint"
    mg_status pass
    return 0
  fi

  local repro="tools/mg core-lint" failed_steps="" passed=0 failed=0 rc n f

  # rustfmt: one failure per file it would change.
  local fmt="$MG_RUN_DIR/rustfmt-files.txt"
  (cd core && cargo fmt --all --check)
  rc=$?
  if [ "$rc" -ne 0 ]; then
    (cd core && cargo fmt --all --check -- --files-with-diff) >"$fmt" 2>/dev/null
    n=0
    while IFS= read -r f; do
      [ -n "$f" ] || continue
      f="${f#"$MG_ROOT"/core/}"
      mg_failure "fmt/$f" "rustfmt would reformat core/$f; run cargo fmt --all in core/" "$repro"
      n=$((n + 1))
    done <"$fmt"
    [ "$n" -gt 0 ] || mg_failure "fmt-exit-$rc" "cargo fmt --check exited with code $rc; see log.txt" "$repro"
    failed_steps="$failed_steps fmt"
  fi

  # clippy: one failure per distinct diagnostic.
  local clippy="$MG_RUN_DIR/clippy.jsonl"
  (
    cd core &&
      cargo clippy --workspace --all-targets --all-features --keep-going --message-format json -- -D warnings
  ) >"$clippy"
  rc=$?
  mg_artifact clippy-jsonl "$clippy"
  local diags
  diags="$(jq -c 'select(.reason == "compiler-message") | .message
      | select((.level == "error" or .level == "warning") and (.spans | length) > 0)' "$clippy" |
    jq -sc 'unique_by(.rendered) | .[]')"
  if [ -n "$diags" ]; then
    jq -r '.rendered' <<<"$diags" >&2
    jq -c --arg repro "$repro" '([.spans[] | select(.is_primary)][0] // .spans[0]) as $s
      | {id: ("clippy/" + (.code.code // .level) + "/" + $s.file_name + ":" + ($s.line_start | tostring)),
         message: (.rendered | .[0:2000]), repro: $repro}' <<<"$diags" >>"$MG_STATE/failures.jsonl"
  fi
  if [ "$rc" -ne 0 ] || [ -n "$diags" ]; then
    [ -n "$diags" ] || mg_failure "clippy-exit-$rc" "cargo clippy exited with code $rc without a diagnostic; see log.txt" "$repro"
    failed_steps="$failed_steps clippy"
  fi

  # layering-check: the KTD2 table in core/tools/layering-check/edges.toml.
  local layering="$MG_RUN_DIR/layering.jsonl"
  if [ ! -f core/tools/layering-check/Cargo.toml ]; then
    mg_failure layering-check-missing "core/tools/layering-check is missing, so the crate layering is unchecked" "$repro"
    failed_steps="$failed_steps layering"
  else
    (cd core && cargo run -q -p layering-check -- --json) >"$layering"
    rc=$?
    mg_artifact layering-jsonl "$layering"
    jq -r '"[\(.rule)] \(.message)"' "$layering" >&2
    jq -c --arg repro "$repro" '{id: ("layering/" + .rule + "/" + .subject), message: .message, repro: $repro}' \
      "$layering" >>"$MG_STATE/failures.jsonl"
    if [ "$rc" -ne 0 ]; then
      [ -s "$layering" ] || mg_failure "layering-check-exit-$rc" "layering-check exited with code $rc; see log.txt" "$repro"
      failed_steps="$failed_steps layering"
    fi
  fi

  for n in fmt clippy layering; do
    case " $failed_steps " in
      *" $n "*) failed=$((failed + 1)) ;;
      *) passed=$((passed + 1)) ;;
    esac
  done
  mg_count total 3
  mg_count passed "$passed"
  mg_count failed "$failed"
  if [ "$failed" -eq 0 ]; then
    mg_message "core-lint: rustfmt, clippy and layering-check clean"
    mg_status pass
  else
    mg_message "core-lint: failed:$failed_steps"
    mg_status fail
  fi
}

# tools/mg core-deny [-- checks, e.g. licenses bans]: cargo deny with core/deny.toml.
mg_core_deny() {
  [ "${1:-}" = "--" ] && shift
  mg__core_environment
  if [ ! -f core/Cargo.toml ]; then
    mg_message "core-deny: no Cargo workspace in core/ yet; no dependencies to check"
    mg_status pass
    return 0
  fi
  if [ ! -f core/deny.toml ]; then
    mg_failure deny-config-missing "core/deny.toml is missing, so cargo deny would run with its permissive defaults"
    mg_status fail
    return 0
  fi

  local out="$MG_RUN_DIR/cargo-deny.jsonl" rc
  (cd core && cargo deny --format json --color never check ${1+"$@"}) 2>"$out.raw"
  rc=$?
  grep '^{' "$out.raw" >"$out"
  grep -v '^{' "$out.raw" >&2
  rm -f "$out.raw"
  mg_artifact cargo-deny-jsonl "$out"

  local errors
  errors="$(jq -c --arg repro "$MG_DEFAULT_REPRO" 'select(.type == "diagnostic" and .fields.severity == "error")
    | .fields as $f
    | ($f.graphs[0].Krate // null) as $k
    | (if $k then $k.name + "@" + $k.version else null end) as $crate
    | ([$f.labels[]?.span] | unique | join(", ")) as $spans
    | ([$f.labels[]?.message | select(. != "")] | unique | join("; ")) as $why
    | {id: ("deny/" + ($f.code // "error") + (if $crate then "/" + $crate else "" end)),
       message: ((if $crate then $crate + ": " else "" end) + $f.message
         + (if $spans != "" then ": " + $spans else "" end)
         + (if $why != "" then " (" + $why + ")" else "" end)
         + (if $f.advisory then " [" + $f.advisory.id + "]" else "" end)),
       repro: $repro}' "$out")"
  if [ -n "$errors" ]; then
    printf '%s\n' "$errors" >>"$MG_STATE/failures.jsonl"
    jq -r '.message' <<<"$errors" >&2
  fi

  local summary
  summary="$(jq -c 'select(.type == "summary") | .fields' "$out" | tail -n 1)"
  [ -n "$summary" ] || summary='{}'
  mg_detail checks "$summary"
  mg_count total "$(jq 'length' <<<"$summary")"
  mg_count passed "$(jq '[.[] | select(.errors == 0)] | length' <<<"$summary")"
  mg_count failed "$(jq '[.[] | select(.errors > 0)] | length' <<<"$summary")"
  if [ "$rc" -ne 0 ] && [ -z "$errors" ]; then
    mg_failure "cargo-deny-exit-$rc" "cargo deny exited with code $rc without an error diagnostic; see log.txt"
  fi
  if [ "$rc" -eq 0 ] && [ -z "$errors" ]; then
    mg_message "core-deny: $(jq -r 'keys | join(", ")' <<<"$summary") clean"
    mg_status pass
  else
    mg_message "core-deny: $(jq -r '[to_entries[] | select(.value.errors > 0) | "\(.value.errors) error(s) in \(.key)"] | join(", ")' <<<"$summary") (cargo deny exit $rc)"
    mg_status fail
  fi
}
