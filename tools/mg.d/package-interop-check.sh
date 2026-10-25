# Generated-only RF4 package proof. Inputs must be frozen owned build/ artifacts;
# callers are responsible for synthetic content. See the Python helper docstring
# for the deliberately bounded UTC collection-clock scope. Never repairs a DB.
mg_job package-interop-check host mg_package_interop_check \
  "Generated native RF4 package proof: --package <build/*.colpkg> --expected <build/frozen-dir> --now <UTC instant, 2000..2037> --tz UTC"

mg_package_interop_check() {
  local package="" expected="" now="" tz="" flag value helper="$MG_ROOT/tools/scripts/package-interop-check.py"
  local extra=() stage="$MG_RUN_DIR/interop" rc
  # selftest dry-runs every registered job without arguments.
  if [ "${MG_DRY_RUN:-0}" = 1 ] && [ "$#" -eq 0 ]; then
    mg_message "package-interop-check: explicit generated inputs required for execution"
    mg_status pass
    return 0
  fi
  while [ "$#" -gt 0 ]; do
    flag="$1"
    case "$flag" in --package|--expected|--now|--tz) ;; *)
      mg_failure usage "package-interop-check requires --package, --expected, --now and --tz UTC"
      mg_status error; return 0 ;;
    esac
    if [ "$#" -lt 2 ]; then
      mg_failure usage "package-interop-check option requires a value"
      mg_status error; return 0
    fi
    value="$2"
    case "$flag" in
      --package) [ -z "$package" ] || break; package="$value" ;;
      --expected) [ -z "$expected" ] || break; expected="$value" ;;
      --now) [ -z "$now" ] || break; now="$value" ;;
      --tz) [ -z "$tz" ] || break; tz="$value" ;;
    esac
    shift 2
  done
  if [ "$#" -ne 0 ] || [ -z "$package" ] || [ -z "$expected" ] || [ -z "$now" ] || [ -z "$tz" ]; then
    mg_failure usage "package-interop-check requires each explicit option exactly once"
    mg_status error; return 0
  fi
  if [ "${MG_DRY_RUN:-0}" = 1 ]; then extra=(--validate-only); fi
  if ! python3 "$helper" stage --root "$MG_ROOT" --run "$MG_RUN_DIR" \
      --package "$package" --expected "$expected" --now "$now" --tz "$tz" \
      ${extra[0]+"${extra[@]}"}; then
    mg_failure input "generated frozen input validation or safe staging failed"
    mg_status error; return 0
  fi
  if [ "${MG_DRY_RUN:-0}" = 1 ]; then
    mg_message "package-interop-check: validated generated inputs; oracle skipped"
    mg_status pass; return 0
  fi
  # No oracle/library open until the independently decoded DB and complete media archive
  # equal the frozen source inventory. Service has no network or checkout mount; only owned inputs.
  if ! mg_compose --progress plain build package-interop-decoder >&2 </dev/null; then
    mg_failure decoder-image "generated proof decoder image build failed"
    mg_status error; return 0
  fi
  if ! mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
      -u "$(id -u):$(id -g)" \
      -v "$helper:/decoder.py:ro" -v "$stage/fixtures:/in:ro" \
      -v "$stage/media-inventory.json:/expected-media.json:ro" \
      -v "$stage/decoded:/out" package-interop-decoder \
      python3 /decoder.py decode --package /in/package.colpkg \
      --expected /in/collection.anki2 --output /out \
      --media-inventory /expected-media.json --now "$now" </dev/null; then
    mg_failure pre-client-database "independent bounded package DB and media archive proof failed"
    mg_status fail; return 0
  fi
  python3 "$helper" ready --root "$MG_ROOT" --run "$MG_RUN_DIR" || {
    mg_failure staging "staged input validation failed"; mg_status error; return 0;
  }
  mg_oracle__tree || return 0
  mg_oracle__build "$MG_ORACLE_DIR" || return 0
  python3 "$helper" ready --root "$MG_ROOT" --run "$MG_RUN_DIR" || {
    mg_failure staging "staged input changed before oracle execution"; mg_status error; return 0;
  }
  # All mounts are operation-owned, no inherited ORACLE_FIXTURES, no expected
  # media mount/copy, no general-purpose harness helper's fixed fixture mounts.
  (
  set -o noclobber
  mg_oracle__run "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" -u "$(id -u):$(id -g)" \
    -e TZ=UTC -e ORACLE_FIXTURES=/in/fixtures \
    -v "$stage/scenarios:/in/scenarios:ro" \
    -v "$stage/fixtures:/in/fixtures:ro" \
    -v "$stage/goldens:/in/goldens" -v "$stage/out:/out" \
    oracle harness record --scenarios /in/scenarios --goldens /in/goldens \
    --jobs 1 --report /out/report.json >"$stage/out/oracle.log" 2>&1 </dev/null
  )
  rc=$?
  mg_artifact report "$stage/out/report.json" "local raw oracle report"
  mg_artifact report "$stage/goldens/rf4/package-interop.json" "local raw oracle golden"
  mg_artifact log "$stage/out/oracle.log" "local oracle output"
  if [ "$rc" -ne 0 ]; then
    mg_failure oracle "package oracle recording failed; raw evidence retained locally"
    mg_status fail; return 0
  fi
  if ! python3 "$helper" check --root "$MG_ROOT" --run "$MG_RUN_DIR"; then
    mg_failure compatibility "exact package compatibility proof failed; raw evidence retained locally"
    mg_status fail; return 0
  fi
  mg_artifact report "$stage/evidence.json" "bounded exact comparison evidence"
  mg_detail package_interop "$(cat "$stage/evidence.json")"
  mg_count total 1
  mg_count passed 1
  mg_status pass
}
