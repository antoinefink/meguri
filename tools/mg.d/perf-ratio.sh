# Meguri-versus-Anki timing ratio (U18: R1806, R1865;
# specs/conformance/timed-operations.md, T11). Sourced by tools/mg.
#
#   tools/mg perf-ratio [--sizes 10k,100k] [--platform apple|linux] [--op <name>]... [--repeat <n>] [--now <instant>] [--tz <zone>]
#   tools/mg perf-ratio --private [--op <name>]... [--repeat <n>] [--now <instant>] [--tz <zone>]
#
# Times the catalogue's operations in Anki (tools/mg oracle time) and in
# Meguri (meguri-cli timing run, built with Cargo's release profile, T3.5)
# on the same collection, one side after the other while holding the host
# lease, and appends one row per operation to docs/process/perf-ratio.csv,
# the committed history:
#
#   date,commit,platform,collection,cards,operation,anki_ms,meguri_ms,ratio,status
#
# date is the run's UTC day; commit Meguri's (`meguri.commit`; empty when
# the build has none: core/ differed from HEAD); platform the lane
# (macos-arm64 is the native lane, the primary series; linux-* the Docker
# lane, the secondary one); collection the size (10k, 100k) or `private`;
# cards its card count; anki_ms and meguri_ms the median_ms of each side
# that is ok, with 3 decimals; ratio meguri_ms / anki_ms with 3 decimals,
# only when both sides are ok and their checks are equal (T11). status:
#
#   ok        Meguri faster
#   fail      Meguri not faster: medians equal or Meguri's larger (R1806)
#   pending   Meguri cannot run the operation yet (unsupported); no failure
#   mismatch  both ok but the checks differ: different work, a conformance
#             finding to investigate, never a timing
#   error     either side `error`, or the oracle `unsupported` (a defect)
#
# fail, mismatch and error fail the job, one failure per operation. A pair
# of reports that are not comparable (T11: platform, now, tz, protocol,
# collection; Meguri's profile must be release) or a side without a report
# appends nothing for that collection and makes the job an error.
#
# Nightly (U247 schedules it): the generated collections of --sizes (nested
# tools/mg fixtures), in the native lane, then the Docker lane, where
# meguri-cli runs in the dev container with its copies on its /tmp tmpfs;
# --platform picks one lane. Milestone (--private, U249 runs it): a fresh
# copy of the user's collection from the vault (U15: refused on CI and in
# implementer and fixer worktrees), native lane only, at 12:00 today in this
# Mac's zone unless --now and --tz say otherwise (the copy has no manifest).
# Its Meguri report and scratch stay under build/private/<run>/, the copy
# is deleted at the end, and its rows are scanned against the private token
# set before they are appended (R1940). Every operation of the catalogue
# six times on 100k takes minutes per side; nothing here times out.

mg_job perf-ratio host mg_perf_ratio \
  "Meguri-versus-Anki timing ratio (R1806, R1865; timed-operations.md): [--sizes 10k,100k] [--platform apple|linux] | --private; [--op <name>]... [--repeat <n>] [--now <instant>] [--tz <zone>]; tools/mg oracle time and meguri-cli timing run (release) on the same collections, one row per operation appended to docs/process/perf-ratio.csv; Meguri not faster, differing checks or an error fail"

MG_PERF_RATIO_CSV=docs/process/perf-ratio.csv
MG_PERF_RATIO_HEADER="date,commit,platform,collection,cards,operation,anki_ms,meguri_ms,ratio,status"
MG_PERF_RATIO_SIZES="10k,100k"

mg__perf_ratio_usage() {
  mg_failure usage "$1; usage: tools/mg perf-ratio [--sizes 10k,100k] [--platform apple|linux] | --private; [--op <name>]... [--repeat <n>] [--now <instant>] [--tz <zone>]" "tools/mg --list"
  mg_message "perf-ratio: usage error"
  mg_status error
}

# mg__perf_ratio_platform <apple|linux>: the platform a lane's reports name.
mg__perf_ratio_platform() {
  if [ "$1" = apple ]; then
    echo macos-arm64
  else
    case "$(uname -m)" in
      arm64 | aarch64) echo linux-arm64 ;;
      *) echo linux-x86_64 ;;
    esac
  fi
}

# mg__perf_ratio_commit: HEAD when core/ is as committed (the commit the
# build is made from); nothing otherwise.
mg__perf_ratio_commit() {
  [ -n "${MG_COMMIT_SHA:-}" ] || return 0
  [ -z "$(git -C "$MG_ROOT" status --porcelain -- core 2>/dev/null | head -n 1)" ] || return 0
  printf '%s\n' "$MG_COMMIT_SHA"
}

# mg__perf_ratio_fixtures <size>...: the generated collections, made or
# reused by tools/mg fixtures; prints `<size>\t<dir>` per size. Returns 1
# when that job did not pass.
mg__perf_ratio_fixtures() {
  local result size dir
  result="$("$MG_TOOLS/mg" fixtures "$@" </dev/null)"
  result="${result##*$'\n'}"
  [ -n "$result" ] && [ -f "$result" ] || return 1
  mg_artifact fixtures "$result" "tools/mg fixtures result"
  mg_word_in "$(jq -r .status "$result" 2>/dev/null)" "pass warn" || return 1
  for size in "$@"; do
    dir="$(jq -r --arg s "$size" '[.artifacts[] | select(.kind == "fixture" and .label == $s) | .path][0] // empty' "$result")"
    [ -n "$dir" ] || return 1
    printf '%s\t%s\n' "$size" "$dir"
  done
}

# mg__perf_ratio_build <apple|linux> <commit or "">: meguri-cli with the
# release profile, natively (MG__PR_BIN) or in the dev container's target
# volume. MEGURI_BUILD_COMMIT is the report's meguri.commit.
mg__perf_ratio_build() {
  local lane="$1" commit="$2"
  echo "perf-ratio: building meguri-cli (release) for the $lane lane" >&2
  if [ "$lane" = apple ]; then
    (cd core && MEGURI_BUILD_COMMIT="$commit" cargo build --release -p meguri-cli) || return 1
    MG__PR_BIN="$(cd core && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)/release/meguri-cli"
  else
    mg_compose --progress plain build dev >&2 || return 1
    mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" -e MEGURI_BUILD_COMMIT="$commit" dev \
      bash -c 'cd core && cargo build --release -p meguri-cli' </dev/null >&2
  fi
}

# mg__perf_ratio_meguri <apple|linux> <dir> <out dir> [timing args...]:
# meguri-cli timing run on <dir>, timing.json or timing-error.json into
# <out dir>. Natively its copies go to MG__PR_SCRATCH; in the dev container
# to its /tmp tmpfs (T3.4).
mg__perf_ratio_meguri() {
  local lane="$1" dir="$2" out="$3" cdir cout
  shift 3
  mkdir -p "$out"
  if [ "$lane" = apple ]; then
    "$MG__PR_BIN" timing run "$dir" --out "$out" --scratch "$MG__PR_SCRATCH" ${1+"$@"} </dev/null
  else
    cdir="$(mg_container_path "$dir")" && cout="$(mg_container_path "$out")" || {
      echo "perf-ratio: $dir or $out is outside the checkout, which the dev container cannot see" >&2
      return 1
    }
    mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev \
      /cargo-target/release/meguri-cli timing run "$cdir" --out "$cout" --scratch /tmp ${1+"$@"} </dev/null
  fi
}

# mg__perf_ratio_anki <apple|linux> <label> <dir> [timing args...]: the
# nested tools/mg oracle time; sets MG__PR_REPORT to its timing.json, or
# records the failure and returns 1.
mg__perf_ratio_anki() {
  local lane="$1" label="$2" dir="$3" result id platform=()
  shift 3
  id="anki/$(mg__perf_ratio_platform "$lane")/$label"
  [ "$lane" = linux ] || platform=(--platform apple)
  echo "perf-ratio: Anki on $label ($lane lane)" >&2
  result="$("$MG_TOOLS/mg" oracle time "$dir" ${platform[0]+"${platform[@]}"} ${1+"$@"} </dev/null)"
  result="${result##*$'\n'}"
  if [ -z "$result" ] || [ ! -f "$result" ]; then
    mg_failure "$id" "tools/mg oracle time wrote no result; see log.txt"
    return 1
  fi
  mg_artifact oracle-time "$result" "$lane $label"
  MG__PR_REPORT="$(jq -r '[.artifacts[] | select(.kind == "report") | .path][0] // empty' "$result")"
  if [ -z "$MG__PR_REPORT" ] || [ ! -s "$MG__PR_REPORT" ]; then
    mg_failure "$id" "tools/mg oracle time gave no report: $(jq -r '[.failures[] | .id + ": " + .message] | join("; ")' "$result")"
    return 1
  fi
}

# The T11 comparison of an anki and a meguri report ($a, $m), with $label,
# $date: {incomparable: [members]} or {rows: [...]}, one row per operation
# either report names, in the anki report's order.
MG_PERF_RATIO_JQ='
def fixed: (. * 1000 | round) as $u | "\($u / 1000 | floor).\(("00" + ($u % 1000 | tostring))[-3:])";
def part: if . == null then "" else fixed end;
def said: if . == null then "absent" else .status + (if .error then " (" + .error.kind + " at " + .error.stage + ")" else "" end) end;
[ (if $a.side != "anki" or $m.side != "meguri" then "side" else empty end),
  (("platform", "now", "tz", "protocol", "collection") as $k | select($a[$k] != $m[$k]) | $k),
  (if $m.meguri.profile != "release" then "profile" else empty end) ] as $bad
| if ($bad | length) > 0 then {incomparable: $bad, profile: $m.meguri.profile}
  else
    ($a.operations | map({key: .name, value: .}) | from_entries) as $A
    | ($m.operations | map({key: .name, value: .}) | from_entries) as $M
    | {rows: [
        ([$a.operations[].name] + [$m.operations[].name | select($A[.] == null)])[] as $n
        | $A[$n] as $x | $M[$n] as $y
        | (if $x == null or $y == null or $x.status != "ok" then "error"
           elif $y.status == "unsupported" then "pending"
           elif $y.status != "ok" then "error"
           elif $x.check != $y.check then "mismatch"
           elif $y.statistic.median_ms < $x.statistic.median_ms then "ok"
           else "fail" end) as $status
        | (if $x.status == "ok" then $x.statistic.median_ms else null end) as $am
        | (if $y.status == "ok" then $y.statistic.median_ms else null end) as $mm
        | {date: $date, commit: ($m.meguri.commit // ""), platform: $a.platform, collection: $label,
           cards: $a.collection.cards, operation: $n, status: $status,
           anki_ms: $am, meguri_ms: $mm,
           ratio: (if ($status == "ok" or $status == "fail") and $am > 0 then $mm / $am else null end),
           anki: ($x | said), meguri: ($y | said),
           checks: (if $status == "mismatch" then {anki: $x.check, meguri: $y.check} else null end)}
        | . + {csv: ([.date, .commit, .platform, .collection, (.cards | tostring), .operation,
                      (.anki_ms | part), (.meguri_ms | part), (.ratio | part), .status] | join(","))}
      ]}
  end'

# mg__perf_ratio_compare <anki report> <meguri report> <label> <date>:
# appends the rows to $MG__PR_ROWS (JSON lines), with one failure per row
# that fails; records the failure and returns 1 when the reports are not
# comparable.
mg__perf_ratio_compare() {
  local anki="$1" meguri="$2" label="$3" date="$4" out platform
  out="$(jq -c -n --slurpfile a "$anki" --slurpfile m "$meguri" --arg label "$label" --arg date "$date" \
    '$a[0] as $a | $m[0] as $m | '"$MG_PERF_RATIO_JQ")" || {
    mg_failure "incomparable/$label" "the reports could not be read as T7 reports: $anki, $meguri"
    return 1
  }
  platform="$(jq -r .platform "$anki")"
  if jq -e '.incomparable' >/dev/null <<<"$out"; then
    mg_failure "incomparable/$platform/$label" "the Anki and Meguri reports on $label are not comparable (T11): they differ in $(jq -r '.incomparable | join(", ")' <<<"$out")$(jq -r 'if (.incomparable | index("profile")) then "; Meguri was built with profile \(.profile), not release (T3.5)" else "" end' <<<"$out")"
    return 1
  fi
  jq -c '.rows[]' <<<"$out" >>"$MG__PR_ROWS"
  jq -c '.rows[] | select(.status == "fail" or .status == "mismatch" or .status == "error")' <<<"$out" |
    while IFS= read -r row; do
      mg_failure "$(jq -r '"\(.platform)/\(.collection)/\(.operation)"' <<<"$row")" "$(jq -r '
        "\(.operation) on \(.collection) (\(.platform)): \(.status): "
        + (if .status == "fail" then
             "Meguri \(.meguri_ms) ms, Anki \(.anki_ms) ms, ratio \(.csv | split(",")[8]); Meguri must be faster (R1806)"
           elif .status == "mismatch" then
             "the checks differ (Anki \(.checks.anki | tojson), Meguri \(.checks.meguri | tojson)): no ratio; a conformance difference to investigate (T11)"
           else "Anki \(.anki), Meguri \(.meguri)" end)' <<<"$row")"
    done
}

# mg__perf_ratio_clock <now or ""> <tz or "">: MG__PR_NOW and MG__PR_TZ for
# a collection without a manifest: the values given, else 12:00 today in
# this Mac's zone.
mg__perf_ratio_clock() {
  local now="$1" tz="$2"
  [ -n "$tz" ] || tz="$(mg__anonymize_host_zone)"
  if [ -z "$now" ] && [ -n "$tz" ]; then
    now="$(python3 -I -c 'import datetime, sys, zoneinfo
z = zoneinfo.ZoneInfo(sys.argv[1])
print(datetime.datetime.now(z).replace(hour=12, minute=0, second=0, microsecond=0).isoformat())' "$tz" 2>/dev/null)"
  fi
  if [ -z "$now" ] || [ -z "$tz" ]; then
    mg__perf_ratio_usage "no instant and zone for the private collection, which has no manifest: pass --now and --tz"
    return 1
  fi
  MG__PR_NOW="$now"
  MG__PR_TZ="$tz"
}

mg_perf_ratio() {
  local private=false sizes="" lanes="" now="" tz="" pass=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --private) private=true && shift ;;
      --sizes | --platform | --op | --repeat | --now | --tz)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg__perf_ratio_usage "$1 needs a value"; return 0; }
        case "$1" in
          --sizes) sizes="$(printf '%s' "$2" | tr 'A-Z' 'a-z')" ;;
          --platform)
            case "$2" in apple | linux) lanes="$2" ;; *) mg__perf_ratio_usage "--platform takes apple or linux"; return 0 ;; esac
            ;;
          --op | --repeat) pass+=("$1" "$2") ;;
          --now) now="$2" ;;
          --tz) tz="$2" ;;
        esac
        shift 2
        ;;
      *) mg__perf_ratio_usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  if [ "$private" = true ]; then
    [ -z "$sizes" ] || { mg__perf_ratio_usage "--private times the vault copy, not --sizes"; return 0; }
    [ "$lanes" != linux ] || { mg__perf_ratio_usage "--private runs in the native lane only"; return 0; }
    lanes=apple
  fi
  [ -n "$lanes" ] || lanes="apple linux"
  [ -n "$sizes" ] || sizes="$MG_PERF_RATIO_SIZES"

  local csv="$MG_ROOT/$MG_PERF_RATIO_CSV"
  if [ -f "$csv" ] && [ "$(head -n 1 "$csv")" != "$MG_PERF_RATIO_HEADER" ]; then
    mg_failure history-header "$MG_PERF_RATIO_CSV does not start with the header $MG_PERF_RATIO_HEADER; nothing was run"
    mg_message "perf-ratio: unknown history format"
    mg_status error
    return 0
  fi

  # The collections: label<TAB>dir lines.
  local collections run="" private_dir=""
  if [ "$private" = true ]; then
    mg__private_guard || return 0
    mg__private_source_ok || {
      mg__private_no_source
      return 0
    }
    mg__private_hooks || return 0
    mg__perf_ratio_clock "$now" "$tz" || return 0
    now="$MG__PR_NOW" tz="$MG__PR_TZ"
    mg__private_docker || return 0
    run="$MG_PRIVATE_DIR/$MG_RUN_ID"
    mg_on_exit "rm -rf '$MG_ROOT/$run/copy'"
    mg__private_copy "$run" || return 0
    private_dir="$MG_ROOT/$run"
    collections="$(printf 'private\t%s\n' "$private_dir/copy")"
  else
    # shellcheck disable=SC2046 # one size per word
    if ! collections="$(mg__perf_ratio_fixtures $(printf '%s' "$sizes" | tr ',' ' '))"; then
      mg_failure fixtures "tools/mg fixtures could not make the collections ($sizes); see its result" \
        "tools/mg fixtures $(printf '%s' "$sizes" | tr ',' ' ')"
      mg_status error
      return 0
    fi
  fi
  [ -z "$now" ] || pass+=(--now "$now")
  [ -z "$tz" ] || pass+=(--tz "$tz")

  local commit lane
  commit="$(mg__perf_ratio_commit)"
  for lane in $lanes; do
    if ! mg__perf_ratio_build "$lane" "$commit"; then
      mg_failure "build/$lane" "building meguri-cli with the release profile for the $lane lane failed; see log.txt"
      mg_status error
      return 0
    fi
  done

  # T3.5: one side at a time, and nothing else of ours on the machine.
  mg_host_lease --no-session || return 0

  MG__PR_ROWS="$MG_RUN_DIR/rows.jsonl"
  : >"$MG__PR_ROWS"
  local date label dir out errors=0 platform
  date="$(date -u +%Y-%m-%d)"
  for lane in $lanes; do
    platform="$(mg__perf_ratio_platform "$lane")"
    while IFS="$(printf '\t')" read -r label dir; do
      [ -n "$label" ] || continue
      if ! mg__perf_ratio_anki "$lane" "$label" "$dir" ${pass[0]+"${pass[@]}"}; then
        errors=$((errors + 1))
        continue
      fi
      if [ -n "$private_dir" ]; then
        out="$private_dir/meguri-$lane"
        MG__PR_SCRATCH="$private_dir/scratch"
      else
        out="$MG_RUN_DIR/meguri-$lane-$label"
        MG__PR_SCRATCH="$MG_RUN_DIR/scratch"
      fi
      echo "perf-ratio: Meguri on $label ($lane lane)" >&2
      mg__perf_ratio_meguri "$lane" "$dir" "$out" ${pass[0]+"${pass[@]}"}
      rm -rf "$MG__PR_SCRATCH"
      if [ ! -s "$out/timing.json" ]; then
        mg_failure "meguri/$platform/$label" "meguri-cli timing run gave no report: $(jq -r '.kind + ": " + .detail' "$out/timing-error.json" 2>/dev/null || echo 'see log.txt')"
        errors=$((errors + 1))
        continue
      fi
      mg_artifact report "$out/timing.json" "Meguri, $label, $lane lane"
      mg__perf_ratio_compare "$MG__PR_REPORT" "$out/timing.json" "$label" "$date" || errors=$((errors + 1))
    done <<<"$collections"
  done

  local rows="$MG_RUN_DIR/rows.csv" appended=0
  jq -r '.csv' "$MG__PR_ROWS" >"$rows"
  if [ -s "$rows" ] && [ "$private" = true ]; then
    # R1940: the rows leave build/private/ only once the scan finds no token.
    local scan="$MG_RUN_DIR/rows-scan.json" scan_rc
    mg__private_scan "$scan" "$rows"
    scan_rc=$?
    if [ "$scan_rc" -ne 0 ]; then
      if [ "$scan_rc" -eq 1 ]; then
        mg_failure rows-private-token "the rows hold a private token; nothing was appended (R1940)"
        mg_status fail
      else
        mg_failure scan "scanning the rows failed (exit $scan_rc); nothing was appended"
        mg_status error
      fi
      return 0
    fi
  fi
  if [ -s "$rows" ]; then
    if [ ! -f "$csv" ] && ! printf '%s\n' "$MG_PERF_RATIO_HEADER" >"$csv"; then
      mg_failure history-write "creating the header in $MG_PERF_RATIO_CSV failed"
      mg_status error
      return 0
    fi
    if ! cat "$rows" >>"$csv"; then
      mg_failure history-write "appending rows to $MG_PERF_RATIO_CSV failed"
      mg_status error
      return 0
    fi
    appended="$(wc -l <"$rows" | tr -d ' ')"
  fi

  local n_ok n_fail n_pending n_mismatch n_error
  n_ok="$(jq -s '[.[] | select(.status == "ok")] | length' "$MG__PR_ROWS")"
  n_fail="$(jq -s '[.[] | select(.status == "fail")] | length' "$MG__PR_ROWS")"
  n_pending="$(jq -s '[.[] | select(.status == "pending")] | length' "$MG__PR_ROWS")"
  n_mismatch="$(jq -s '[.[] | select(.status == "mismatch")] | length' "$MG__PR_ROWS")"
  n_error="$(jq -s '[.[] | select(.status == "error")] | length' "$MG__PR_ROWS")"
  mg_detail mode "$(jq -nc --arg m "$([ "$private" = true ] && echo private || echo nightly)" '$m')"
  mg_detail csv "$(jq -nc --arg p "$MG_PERF_RATIO_CSV" --argjson n "$appended" '{path: $p, appended: $n}')"
  mg_detail rows "$(jq -sc '[.[] | del(.csv)]' "$MG__PR_ROWS")"
  mg_count total "$appended"
  mg_count passed "$n_ok"
  mg_count skipped "$n_pending"
  mg_count failed "$((n_fail + n_mismatch + n_error))"
  local tally="$n_ok ok, $n_fail fail, $n_pending pending, $n_mismatch mismatch, $n_error error"
  if [ "$errors" -gt 0 ]; then
    mg_message "perf-ratio: $errors collection run(s) gave no comparison; $appended row(s) appended ($tally)"
    mg_status error
  elif [ $((n_fail + n_mismatch + n_error)) -gt 0 ]; then
    mg_message "perf-ratio: $appended row(s) appended to $MG_PERF_RATIO_CSV: $tally"
    mg_status fail
  else
    mg_message "perf-ratio: $appended row(s) appended to $MG_PERF_RATIO_CSV: $tally"
    mg_status pass
  fi
}
