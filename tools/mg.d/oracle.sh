# Anki oracle jobs (oracle/, AGPL quarantine). Sourced by tools/mg.
#
# A host job: it builds the oracle image with a content-addressed tag and runs
# oracle containers through compose (service `oracle`, internal network).
# Implementers use these commands as black boxes and never read oracle/.

mg_job oracle host mg_oracle \
  "Anki oracle: build | hello [--tz Z] | record [--jobs N] [filter...] | compare [--jobs N] [filter...] | bump <version> | lock | test [pattern]"

MG_ORACLE_UV_VERSION=0.12.24
# uv resolves for this platform tag; the lock still hashes every published wheel.
MG_ORACLE_LOCK_PLATFORM=x86_64-manylinux_2_40

mg_oracle__usage() {
  mg_failure usage "usage: tools/mg oracle build | hello [--tz Z] | record [--jobs N] [filter...] | compare [--jobs N] [filter...] | bump <version> | lock | test [pattern]" "tools/mg oracle build"
  mg_message "oracle: $1"
  mg_status error
}

mg_oracle__sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi
}

# mg_oracle__inputs_hash <oracle dir>: sha256 over the image inputs (mode,
# content hash and path of each file the Dockerfile copies), so the tag changes
# exactly when the image can.
mg_oracle__inputs_hash() {
  (
    cd "$1" || exit 1
    {
      printf '%s\n' Dockerfile Dockerfile.dockerignore entrypoint.sh requirements.lock
      find driver -type f ! -path '*/__pycache__/*' ! -name '*.pyc'
    } | LC_ALL=C sort | while IFS= read -r f; do
      mode=644
      [ -x "$f" ] && mode=755
      printf '%s %s %s\n' "$mode" "$(mg_oracle__sha256 <"$f")" "$f"
    done
  ) | mg_oracle__sha256
}

# mg_oracle__tag <oracle dir>: <anki version>-<16 hex of the inputs hash>
mg_oracle__tag() {
  local version hash
  version="$(sed -n 's/^anki==\([^ ]*\).*/\1/p' "$1/requirements.lock" | head -n 1)"
  [ -n "$version" ] || return 1
  hash="$(mg_oracle__inputs_hash "$1")" || return 1
  printf '%s-%s\n' "$version" "$(printf '%s' "$hash" | cut -c1-16)"
}

# mg_oracle__compose <tag> <context dir> compose args...
mg_oracle__compose() {
  local tag="$1" ctx="$2"
  shift 2
  MEGURI_ORACLE_TAG="$tag" MEGURI_ORACLE_CONTEXT="$ctx" mg_compose "$@"
}

# mg_oracle__build <oracle dir>: builds and sets MG_ORACLE_TAG, MG_ORACLE_ID.
mg_oracle__build() {
  local dir="$1" log rc
  log="$MG_RUN_DIR/build-$(basename "$1").log"
  if ! MG_ORACLE_TAG="$(mg_oracle__tag "$dir")"; then
    mg_failure tag "cannot compute the image tag: no anki== pin in $dir/requirements.lock"
    mg_status error
    return 1
  fi
  echo "oracle: building meguri-oracle:$MG_ORACLE_TAG from ${dir#"$MG_ROOT"/}" >&2
  mg_oracle__compose "$MG_ORACLE_TAG" "$dir" --progress plain build oracle 2>&1 | tee "$log" >&2
  rc=${PIPESTATUS[0]}
  mg_artifact log "$log" "oracle image build"
  if [ "$rc" -ne 0 ]; then
    mg_failure image-build "building meguri-oracle:$MG_ORACLE_TAG failed; see $(basename "$log")"
    mg_status error
    return 1
  fi
  MG_ORACLE_ID="$(docker image inspect "meguri-oracle:$MG_ORACLE_TAG" --format '{{.Id}}' 2>/dev/null)"
  if [ -z "$MG_ORACLE_ID" ]; then
    mg_failure image-inspect "meguri-oracle:$MG_ORACLE_TAG is missing after the build"
    mg_status error
    return 1
  fi
}

# mg_oracle__run <tag> <context> [compose run options...] -- <container args...>
# Every container carries the run label, so a cancelled run can remove it.
mg_oracle__run() {
  local tag="$1" ctx="$2"
  shift 2
  mg_oracle__compose "$tag" "$ctx" run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" "$@"
}

mg_oracle__cleanup() {
  local ids
  ids="$(docker ps -aq --filter "label=meguri.mg.run-id=$MG_RUN_ID" 2>/dev/null)"
  # shellcheck disable=SC2086 # one container id per word
  [ -z "$ids" ] || docker rm -f $ids >/dev/null 2>&1
  return 0
}

# mg_oracle__hello <tag> <context> <tz> <out file>: one worker, one hello.
mg_oracle__hello() {
  printf '{"cmd":"hello"}\n' | mg_oracle__run "$1" "$2" -e "TZ=$3" oracle >"$4"
  jq -e '.ok == true' "$4" >/dev/null 2>&1
}

# mg_oracle__lock <dir>: regenerate <dir>/requirements.lock from requirements.in
# (uv in the oracle's base image, default network: it only talks to PyPI).
mg_oracle__lock() {
  local dir="$1" base
  base="$(sed -n 's/^FROM \(python:[^ ]*\).*/\1/p' "$MG_ROOT/oracle/Dockerfile" | head -n 1)"
  docker run --rm --init --label "meguri.mg.run-id=$MG_RUN_ID" -v "$dir:/lock" -w /lock "$base" sh -c "
    pip install --quiet --disable-pip-version-check --root-user-action=ignore uv==$MG_ORACLE_UV_VERSION &&
    uv pip compile --quiet --generate-hashes --python-version 3.12 \
      --python-platform $MG_ORACLE_LOCK_PLATFORM \
      --custom-compile-command 'tools/mg oracle lock' \
      requirements.in -o requirements.lock" >&2
}

# mg_oracle__harness <tag> <context> <goldens dir> harness args...: record or compare
# with conformance/scenarios read-only, goldens and the run directory writable.
mg_oracle__harness() {
  local tag="$1" ctx="$2" goldens="$3"
  shift 3
  mkdir -p "$goldens"
  mg_oracle__run "$tag" "$ctx" -u "$(id -u):$(id -g)" \
    -v "$MG_ROOT/conformance/scenarios:/in/scenarios:ro" \
    -v "$goldens:/in/goldens" \
    -v "$MG_RUN_DIR:/out" \
    oracle harness "$@" </dev/null
}

mg_oracle() {
  local sub="${1:-}"
  [ "$#" -gt 0 ] && shift
  trap 'mg_oracle__cleanup; exit 143' TERM
  case "$sub" in
    build) mg_oracle_build "$@" ;;
    hello) mg_oracle_hello "$@" ;;
    record | compare) mg_oracle_record_compare "$sub" "$@" ;;
    bump) mg_oracle_bump "$@" ;;
    lock) mg_oracle_lock "$@" ;;
    test) mg_oracle_test "$@" ;;
    "") mg_oracle__usage "missing subcommand" ;;
    *) mg_oracle__usage "unknown subcommand '$sub'" ;;
  esac
}

mg_oracle__docker_ok() {
  if ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 1
  fi
}

mg_oracle_build() {
  [ "$#" -eq 0 ] || { mg_oracle__usage "build takes no arguments"; return 0; }
  mg_oracle__docker_ok || return 0
  mg_oracle__build "$MG_ROOT/oracle" || return 0
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  mg_message "oracle: built meguri-oracle:$MG_ORACLE_TAG"
  mg_status pass
}

mg_oracle_hello() {
  local tz=UTC out="$MG_RUN_DIR/hello.json"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --tz) tz="${2:-}"; shift 2 || { mg_oracle__usage "--tz needs a zone"; return 0; } ;;
      *) mg_oracle__usage "hello: unexpected argument '$1'"; return 0 ;;
    esac
  done
  mg_oracle__docker_ok || return 0
  mg_oracle__build "$MG_ROOT/oracle" || return 0
  if ! mg_oracle__hello "$MG_ORACLE_TAG" "$MG_ROOT/oracle" "$tz" "$out"; then
    mg_failure hello "the worker did not answer hello: $(head -c 2000 "$out" 2>/dev/null)"
    mg_status fail
    return 0
  fi
  mg_artifact json "$out" "hello response"
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  mg_detail hello "$(jq -c 'del(.ok)' "$out")"
  mg_message "oracle: $(jq -r '"anki \(.anki_version), tz \(.tz), lock \(.lock_sha256[0:12]), driver \(.driver_sha256[0:12])"' "$out")"
  mg_status pass
}

mg_oracle_record_compare() {
  local cmd="$1" jobs=12 report="$MG_RUN_DIR/report.json" rc
  shift
  local filters=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --jobs) jobs="${2:-}"; shift 2 || { mg_oracle__usage "--jobs needs a number"; return 0; } ;;
      --*) mg_oracle__usage "$cmd: unknown option '$1'"; return 0 ;;
      *) filters[${#filters[@]}]="--filter"; filters[${#filters[@]}]="$1"; shift ;;
    esac
  done
  case "$jobs" in '' | *[!0-9]* | 0) mg_oracle__usage "--jobs needs a positive number"; return 0 ;; esac
  if [ ! -d "$MG_ROOT/conformance/scenarios" ]; then
    mg_failure no-scenarios "conformance/scenarios does not exist"
    mg_status fail
    return 0
  fi
  mg_oracle__docker_ok || return 0
  mg_oracle__build "$MG_ROOT/oracle" || return 0
  mg_oracle__harness "$MG_ORACLE_TAG" "$MG_ROOT/oracle" "$MG_ROOT/conformance/goldens" \
    "$cmd" --scenarios /in/scenarios --goldens /in/goldens --jobs "$jobs" --report /out/report.json \
    ${filters[0]+"${filters[@]}"}
  rc=$?
  mg_oracle__report "$cmd" "$rc" "$report"
}

# mg_oracle__report <record|compare> <harness rc> <report.json>
mg_oracle__report() {
  local cmd="$1" rc="$2" report="$3" ok
  if [ ! -s "$report" ] || [ "$rc" -ge 2 ]; then
    mg_failure harness "the harness exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$report" "$cmd report"
  ok=recorded
  [ "$cmd" = compare ] && ok=pass
  local total passed
  total="$(jq '.scenario_count' "$report")"
  passed="$(jq --arg ok "$ok" '[.scenarios[] | select(.status == $ok)] | length' "$report")"
  mg_count total "$total"
  mg_count passed "$passed"
  mg_count failed $((total - passed))
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" --arg i "$MG_ORACLE_ID" '{tag: $t, id: $i}')"
  mg_detail oracle "$(jq -c '.oracle' "$report")"
  mg_detail totals "$(jq -c '.totals' "$report")"
  mg_detail timing "$(jq -c '{jobs, wall_s, mean_worker_ms}' "$report")"
  jq -c --arg ok "$ok" --arg cmd "$cmd" '.scenarios[] | select(.status != $ok)
    | {id: .scenario,
       message: (.status
         + (if .error then ": " + (.error | tostring) else "" end)
         + (if .diffs then " at " + .diffs[0].pointer + " (expected " + (.diffs[0].expected | tostring) + ", got " + (.diffs[0].actual | tostring) + ")" else "" end)),
       repro: ("tools/mg oracle " + $cmd + " " + (.scenario | @sh))}' "$report" >>"$MG_STATE/failures.jsonl"
  if [ "$total" -eq 0 ]; then
    mg_failure no-scenarios "no scenario matched"
    mg_message "oracle $cmd: no scenario matched"
    mg_status fail
  elif [ "$passed" -eq "$total" ]; then
    mg_message "oracle $cmd: $passed/$total $ok ($(jq -r '"mean \(.mean_worker_ms) ms per scenario in the worker"' "$report"))"
    mg_status pass
  else
    mg_message "oracle $cmd: $passed/$total $ok; $(jq -r '.totals | to_entries | map("\(.key) \(.value)") | join(", ")' "$report")"
    mg_status fail
  fi
}

mg_oracle_lock() {
  [ "$#" -eq 0 ] || { mg_oracle__usage "lock takes no arguments"; return 0; }
  mg_oracle__docker_ok || return 0
  if ! mg_oracle__lock "$MG_ROOT/oracle"; then
    mg_failure lock "uv could not resolve oracle/requirements.in; see log.txt"
    mg_status fail
    return 0
  fi
  mg_artifact lock "$MG_ROOT/oracle/requirements.lock"
  mg_message "oracle: oracle/requirements.lock regenerated (lock $(mg_oracle__sha256 <"$MG_ROOT/oracle/requirements.lock" | cut -c1-12))"
  mg_status pass
}

# bump <version> (R1874): a staging copy of the image inputs with the new pin,
# a fresh lock, a new image, every golden re-recorded into the run directory and
# a behavior-diff report against the committed goldens. Nothing in the
# repository changes; adopting the bump is one separate change.
mg_oracle_bump() {
  local version="${1:-}" jobs=12
  [ "$#" -gt 0 ] && shift
  case "$version" in '' | *[!0-9.]* | .* | *.) mg_oracle__usage "bump needs a version like 26.10.0"; return 0 ;; esac
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --jobs) jobs="${2:-}"; shift 2 || { mg_oracle__usage "--jobs needs a number"; return 0; } ;;
      *) mg_oracle__usage "bump: unexpected argument '$1'"; return 0 ;;
    esac
  done
  mg_oracle__docker_ok || return 0
  local staging="$MG_RUN_DIR/staging" old_tag
  mkdir -p "$staging/oracle"
  (cd "$MG_ROOT/oracle" && cp Dockerfile Dockerfile.dockerignore entrypoint.sh requirements.in "$staging/oracle/" &&
    tar -cf - --exclude __pycache__ --exclude '*.pyc' driver | tar -xf - -C "$staging/oracle")
  sed "s/^anki==.*/anki==$version/" "$MG_ROOT/oracle/requirements.in" >"$staging/oracle/requirements.in"
  echo "oracle: locking anki==$version" >&2
  if ! mg_oracle__lock "$staging/oracle"; then
    mg_failure lock "uv could not resolve anki==$version; see log.txt"
    mg_status fail
    return 0
  fi
  mg_artifact lock "$staging/oracle/requirements.lock" "staged lock for anki $version"
  old_tag="$(mg_oracle__tag "$MG_ROOT/oracle")"
  mg_oracle__build "$staging/oracle" || return 0
  local hello="$MG_RUN_DIR/hello-new.json"
  if ! mg_oracle__hello "$MG_ORACLE_TAG" "$staging/oracle" UTC "$hello"; then
    mg_failure hello "the new oracle did not answer hello: $(head -c 2000 "$hello" 2>/dev/null)"
    mg_status fail
    return 0
  fi
  mg_oracle__harness "$MG_ORACLE_TAG" "$staging/oracle" "$staging/goldens" \
    record --scenarios /in/scenarios --goldens /in/goldens --jobs "$jobs" --report /out/record-report.json
  local rc=$?
  if [ ! -s "$MG_RUN_DIR/record-report.json" ] || [ "$rc" -ge 2 ]; then
    mg_failure harness "re-recording on anki $version exited with code $rc without a report; see log.txt"
    mg_status error
    return 0
  fi
  mg_oracle__run "$MG_ORACLE_TAG" "$staging/oracle" -u "$(id -u):$(id -g)" \
    -v "$MG_ROOT/conformance/goldens:/in/old:ro" -v "$staging/goldens:/in/new:ro" -v "$MG_RUN_DIR:/out" \
    oracle harness diff-report --old /in/old --new /in/new \
    --report /out/bump-report.json --markdown /out/bump-report.md </dev/null
  if [ ! -s "$MG_RUN_DIR/bump-report.json" ]; then
    mg_failure diff-report "the behavior-diff report was not written; see log.txt"
    mg_status error
    return 0
  fi
  mg_artifact report "$MG_RUN_DIR/bump-report.md" "behavior differences (markdown)"
  mg_artifact report "$MG_RUN_DIR/bump-report.json" "behavior differences"
  mg_artifact report "$MG_RUN_DIR/record-report.json" "re-recording report"
  mg_artifact goldens "$staging/goldens" "staged goldens"
  mg_detail bump "$(jq -nc --arg from "$old_tag" --arg to "$MG_ORACLE_TAG" --slurpfile h "$hello" \
    --slurpfile r "$MG_RUN_DIR/bump-report.json" --slurpfile rec "$MG_RUN_DIR/record-report.json" \
    '{from: $from, to: $to, hello: ($h[0] | del(.ok)), goldens: $r[0].totals,
      changed_observations: $r[0].changed_observations, recording: $rec[0].totals}')"
  local failed changed
  failed="$(jq '[.scenarios[] | select(.status != "recorded")] | length' "$MG_RUN_DIR/record-report.json")"
  changed="$(jq '.totals.changed // 0' "$MG_RUN_DIR/bump-report.json")"
  jq -c '.scenarios[] | select(.status != "recorded")
    | {id: .scenario, message: ("not recorded on the new oracle: " + (.error | tostring)),
       repro: ("tools/mg oracle record " + (.scenario | @sh))}' "$MG_RUN_DIR/record-report.json" >>"$MG_STATE/failures.jsonl"
  local total
  total="$(jq '.scenario_count' "$MG_RUN_DIR/record-report.json")"
  mg_count total "$total"
  mg_count passed $((total - failed))
  mg_count failed "$failed"
  mg_count changed "$changed"
  if [ "$failed" -gt 0 ]; then
    mg_message "oracle bump to $version: $failed scenarios could not be recorded, $changed goldens changed; see bump-report.md"
    mg_status fail
  else
    mg_message "oracle bump to $version: $changed goldens changed; review bump-report.md, then adopt staging/oracle/requirements.in, requirements.lock and staging/goldens in one change"
    mg_status pass
  fi
}

# test [pattern]: the driver self-tests inside the container (on the internal
# network), then the host checks in tools/tests/oracle_host_checks.sh. One TAP
# stream, numbered across both.
mg_oracle_test() {
  mg_oracle__docker_ok || return 0
  mg_oracle__build "$MG_ROOT/oracle" || return 0
  local inner="$MG_RUN_DIR/driver.tap" outer="$MG_RUN_DIR/host.tap" tap="$MG_RUN_DIR/oracle-test.tap" rc1 rc2
  mg_oracle__run "$MG_ORACLE_TAG" "$MG_ROOT/oracle" oracle selftest ${1+"$@"} </dev/null | tee "$inner"
  rc1=${PIPESTATUS[0]}
  if [ "$#" -eq 0 ]; then
    MG_ORACLE_ID="$MG_ORACLE_ID" MG_ORACLE_TAG="$MG_ORACLE_TAG" MG_COMPOSE_PROJECT="$(mg_compose_project)" /bin/bash "$MG_ROOT/tools/tests/oracle_host_checks.sh" | tee "$outer"
    rc2=${PIPESTATUS[0]}
  else
    : >"$outer"
    rc2=0
  fi
  awk '
    /^1\.\.[0-9]+$/ { plan += substr($0, 4); next }
    /^(not )?ok [0-9]+/ { n++; sub(/ok [0-9]+/, "ok " n); lines[++m] = $0; next }
    { lines[++m] = $0 }
    END { print "1.." plan; for (i = 1; i <= m; i++) print lines[i] }' "$inner" "$outer" >"$tap"
  mg_artifact tap "$tap"
  mg_tap_report "$tap" $((rc1 > rc2 ? rc1 : rc2)) ""
}
