#!/bin/bash
# Host checks for the oracle image, run by `tools/mg oracle test` after the
# driver self-tests; prints TAP. They cover what the container cannot see from
# inside: the content-addressed tag, image rebuilds, compose workers, the
# protocol schema, the network the service runs on, and that no Meguri code
# refers to oracle/.
#
# Environment (set by tools/mg oracle test): MG_ROOT, MG_RUN_DIR, MG_RUN_ID,
# MG_COMPOSE_PROJECT, and MG_ORACLE_TAG, MG_ORACLE_ID and MG_ORACLE_DIR (the
# oracle/ tree) of the image just built.

set -u

REPO="$MG_ROOT"
ORACLE="${MG_ORACLE_DIR:-$REPO/oracle}"
WORK="$MG_RUN_DIR/host-checks"
mkdir -p "$WORK"
# Copies of oracle/ (AGPL) the checks make under this checkout's build/. Each
# check deletes its own; this covers a cancelled run.
EDITED="$WORK/edited-oracle"
SPARSE="$WORK/sparse"
trap 'rm -rf "$EDITED" "$SPARSE" "$WORK/native"' EXIT
trap 'exit 143' TERM INT HUP

# The helpers of tools/mg.d/oracle.sh, without its job registration.
mg_job() { :; }
mg_compose() {
  BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker compose -p "$MG_COMPOSE_PROJECT" -f "$MG_ROOT/docker/compose.yml" "$@"
}
# shellcheck source=SCRIPTDIR/../mg.d/oracle.sh
. "$REPO/tools/mg.d/oracle.sh"

expect() { # <what> <actual> <expected>
  if [ "$2" != "$3" ]; then
    echo "$1: expected '$3', got '$2'"
    return 1
  fi
}

hello_on() { # <tag> <context> <tz> <out>
  mg_oracle__hello "$1" "$2" "$3" "$4" || {
    echo "hello on meguri-oracle:$1 failed:"
    cat "$4"
    return 1
  }
}

t_tag_is_content_addressed_and_stable() {
  local again pin
  again="$(mg_oracle__tag "$ORACLE")" || return 1
  pin="$(sed -n 's/^anki==\([^ ]*\).*/\1/p' "$ORACLE/requirements.lock")"
  expect "tag recomputed" "$again" "$MG_ORACLE_TAG" &&
    expect "tag shape" "$(printf '%s' "$again" | grep -Ec '^[0-9.]+-[0-9a-f]{16}$')" 1 &&
    expect "tag version" "${again%%-*}" "$pin"
}

t_rebuilding_an_unchanged_tree_keeps_tag_and_image() {
  mg_oracle__compose "$MG_ORACLE_TAG" "$ORACLE" --progress quiet build oracle >"$WORK/rebuild.log" 2>&1 || {
    cat "$WORK/rebuild.log"
    return 1
  }
  expect "image id after rebuild" "$(docker image inspect "meguri-oracle:$MG_ORACLE_TAG" --format '{{.Id}}')" "$MG_ORACLE_ID"
}

t_editing_a_driver_file_changes_tag_and_driver_hash() {
  local rc
  editing_a_driver_file "$EDITED"
  rc=$?
  rm -rf "$EDITED"
  return "$rc"
}

editing_a_driver_file() {
  local edited="$1" tag rc
  rm -rf "$edited" && mkdir -p "$edited"
  (cd "$ORACLE" && cp Dockerfile Dockerfile.dockerignore entrypoint.sh worker_holder.py requirements.lock "$edited/" &&
    tar -cf - --exclude __pycache__ --exclude '*.pyc' driver | tar -xf - -C "$edited")
  printf '\n# edited by oracle_host_checks.sh\n' >>"$edited/driver/observe/ids.py"
  tag="$(mg_oracle__tag "$edited")" || return 1
  if [ "$tag" = "$MG_ORACLE_TAG" ]; then
    echo "the tag did not change after editing a driver file"
    return 1
  fi
  mg_oracle__compose "$tag" "$edited" --progress quiet build oracle >"$WORK/edited-build.log" 2>&1 || {
    cat "$WORK/edited-build.log"
    return 1
  }
  hello_on "$MG_ORACLE_TAG" "$ORACLE" UTC "$WORK/hello-original.json" &&
    hello_on "$tag" "$edited" UTC "$WORK/hello-edited.json"
  rc=$?
  docker image rm -f "meguri-oracle:$tag" >/dev/null 2>&1
  [ "$rc" -eq 0 ] || return 1
  local before after
  before="$(jq -r .driver_sha256 "$WORK/hello-original.json")"
  after="$(jq -r .driver_sha256 "$WORK/hello-edited.json")"
  if [ "$before" = "$after" ]; then
    echo "driver_sha256 did not change: $after"
    return 1
  fi
  expect "lock_sha256 of the edited image" "$(jq -r .lock_sha256 "$WORK/hello-edited.json")" \
    "$(jq -r .lock_sha256 "$WORK/hello-original.json")"
}

t_a_compose_worker_says_hello_in_its_zone() {
  local out="$WORK/hello-kathmandu.json" lock pin
  hello_on "$MG_ORACLE_TAG" "$ORACLE" Asia/Kathmandu "$out" || return 1
  lock="$(mg_oracle__sha256 <"$ORACLE/requirements.lock")"
  pin="$(sed -n 's/^anki==\([^ ]*\).*/\1/p' "$ORACLE/requirements.lock")"
  expect "anki_version" "$(jq -r .anki_version "$out")" "$pin" &&
    expect "anki pin" "$pin" "26.9.3" &&
    expect "lock_sha256" "$(jq -r .lock_sha256 "$out")" "$lock" &&
    expect "tz" "$(jq -r .tz "$out")" Asia/Kathmandu &&
    expect "driver_sha256 shape" "$(jq -r '.driver_sha256 | test("^[0-9a-f]{64}$")' "$out")" true
}

# Requests and responses of one worker session, validated against
# conformance/schema/protocol.json with jsonschema in the dev image.
t_protocol_transcript_matches_the_schema() {
  local req="$WORK/protocol-requests.jsonl" resp="$WORK/protocol-responses.jsonl"
  cat >"$req" <<'EOF'
{"cmd":"hello","id":1}
{"cmd":"run","id":"ok","scenario":{"id":"t/schema","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"setup":{"notetypes":[{"name":"QA","fields":["Q","A"],"templates":[{"name":"C1","front":"{{Q}}","back":"{{A}}"}]}],"notes":[{"notetype":"QA","fields":["q","a"]}]},"steps":[{"answer":{"card":"#c1","rating":"good"}},{"observe":["ids","timing","card(#c1)","revlog(#c1)","next_states(#c1)"]}]}}
{"cmd":"run","id":"bad","scenario":{"id":"t/bad","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"frobnicate":1}]}}
{"cmd":"run","id":"ctx","fsrs_context":true,"scenario":{"id":"t/ctx","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"setup":{"config":{"fsrs":true},"notetypes":[{"name":"QA","fields":["Q","A"],"templates":[{"name":"C1","front":"{{Q}}","back":"{{A}}"}]}],"notes":[{"notetype":"QA","fields":["q","a"]}]},"steps":[{"observe":["next_states(#c1)",{"fsrs_next":{"param_sets":{"default":[]},"inputs":[{"id":0,"card_id":1894698000000,"reps":2,"s":2.3065,"d":2.111,"elapsed_days":3,"scheduled_days":2,"dr":0.9,"params":"default","max_ivl":36500}]}}]}]}}
{"cmd":"begin","scenario":{"id":"t/i","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"advance":"1d"}]}}
{"cmd":"step"}
{"cmd":"step","step":{"observe":["timing"]}}
{"cmd":"end"}
{"cmd":"warp"}
EOF
  mg_oracle__run "$MG_ORACLE_TAG" "$ORACLE" oracle <"$req" >"$resp" || return 1
  printf 'not json\n' | mg_oracle__run "$MG_ORACLE_TAG" "$ORACLE" oracle >>"$resp" || return 1
  expect "response lines" "$(wc -l <"$resp" | tr -d ' ')" 10 || return 1
  local rel_req="${req#"$REPO"/}" rel_resp="${resp#"$REPO"/}"
  mg_compose build dev >"$WORK/dev-build.log" 2>&1 || {
    tail -20 "$WORK/dev-build.log"
    return 1
  }
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev python3 - "$rel_req" "$rel_resp" <<'EOF'
import json, sys
from jsonschema import Draft202012Validator
schema = json.load(open("conformance/schema/protocol.json"))
Draft202012Validator.check_schema(schema)
def validator(ref):
    return Draft202012Validator({"$ref": f"#/$defs/{ref}", "$defs": schema["$defs"]})
req, resp = validator("request"), validator("response")
bad = 0
for name, v, path in (("request", req, sys.argv[1]), ("response", resp, sys.argv[2])):
    for n, line in enumerate(open(path), 1):
        msg = json.loads(line)
        if name == "request" and msg.get("cmd") == "warp":
            if v.is_valid(msg):
                print(f"request {n}: an unknown cmd passed the schema"); bad += 1
            continue
        for err in v.iter_errors(msg):
            print(f"{name} {n}: {err.json_path}: {err.message}"); bad += 1
responses = [json.loads(l) for l in open(sys.argv[2])]
hello, ctx = responses[0], next(r for r in responses if r.get("id") == "ctx")
if hello.get("time_control") != "faketime" or not hello.get("platform", "").startswith("linux-"):
    print(f"hello of a container worker: {hello}"); bad += 1
if list(ctx.get("fsrs_context", {})) != ["/steps/0/next_states/#c1"]:
    print(f"fsrs_context: {ctx.get('fsrs_context')}"); bad += 1
kinds = [r.get("error", {}).get("kind") for r in responses]
for kind in ("unsupported_op", "unknown_cmd", "protocol_error"):
    if kind not in kinds:
        print(f"no {kind} response in the transcript"); bad += 1
sys.exit(1 if bad else 0)
EOF
}

# tools/mg oracle check (specs/conformance/collection-check.md) on a copy of a
# committed fixture: a clean report with the zone and clock from the manifest,
# the same with --schedule (K11), a planted problem (fail), and the K7 errors.
# Nested tools/mg runs of this checkout; the outer run rewrites
# build/mg/oracle/latest when it ends.
CHECK_DIR="$WORK/check"

check_run() { # <out var> <args...>: the result.json path of one run
  local var="$1" out
  shift
  out="$(/bin/bash "$REPO/tools/mg" oracle check "$@" 2>>"$WORK/check.log" | tail -n 1)"
  [ -f "$out" ] || {
    echo "tools/mg oracle check $* wrote no result:"
    tail -n 20 "$WORK/check.log"
    return 1
  }
  printf -v "$var" '%s' "$out"
}

t_check_job_reports_problems_and_errors() {
  local col="$CHECK_DIR/col" bad="$CHECK_DIR/bad" garbage="$CHECK_DIR/garbage" bare="$CHECK_DIR/bare" r
  rm -rf "$CHECK_DIR" && mkdir -p "$col" "$bad" "$garbage" "$bare" || return 1
  cp "$REPO/conformance/fixtures/storage/schema-18.anki2" "$col/collection.anki2" &&
    cp "$col/collection.anki2" "$bad/collection.anki2" &&
    cp "$col/collection.anki2" "$bare/collection.anki2" || return 1
  printf '{"now_ms": 1894701600999, "utc_offset_secs": 3600}\n' >"$col/manifest.json"
  cp "$col/manifest.json" "$bad/manifest.json"
  head -c 8192 /dev/zero | tr '\0' x >"$garbage/collection.anki2"
  sqlite3 "$bad/collection.anki2" "insert into revlog values (1, 1, -1, 0, 0, 0, 0, 0, 0)" || return 1
  check_run r "$col" --deck Lang --render-note 1 || return 1
  expect "clean status" "$(jq -r .status "$r")" pass &&
    expect "zone from the manifest" "$(jq -r .details.check.tz "$r")" Etc/GMT-1 &&
    expect "clock from the manifest" "$(jq -r .details.check.now "$r")" 1894701600 &&
    expect "offset in the zone" "$(jq -r .details.check.timing.utc_offset_secs "$r")" 3600 &&
    expect "dir as given" "$(jq -r .details.check.dir "$r")" "$col" &&
    expect "report artifact" "$(jq -r '[.artifacts[] | select(.kind == "report") | .path | endswith("/check.json")] == [true]' "$r")" true &&
    expect "artifact equals details.check" \
      "$(jq -S . "$(dirname "$r")/check.json" | mg_oracle__sha256)" "$(jq -S .details.check "$r" | mg_oracle__sha256)" || return 1
  cp "$(dirname "$r")/check.json" "$CHECK_DIR/clean-report.json"
  check_run r "$col" --schedule || return 1
  expect "schedule status" "$(jq -r .status "$r")" pass &&
    expect "schedule cards" "$(jq -r '.details.check.schedule.cards == .details.check.counts.cards' "$r")" true || return 1
  cp "$(dirname "$r")/check.json" "$CHECK_DIR/schedule-report.json"
  check_run r "$bad" || return 1
  # The repaired row also changes the revlog table.
  expect "planted problem" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["fail",["revlog_properties","changed_revlog"]]' || return 1
  check_run r "$CHECK_DIR/none" || return 1
  expect "missing directory" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["no_collection"]]' || return 1
  check_run r "$garbage" --tz UTC --now 2030-01-15T10:00:00Z || return 1
  expect "not a collection" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["open_failed"]]' || return 1
  check_run r "$bare" || return 1
  expect "no manifest" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["usage"]]' || return 1
  check_run r "$col" --bogus || return 1
  expect "unknown option" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["usage"]]' || return 1
  expect "source left as it was" "$(mg_oracle__sha256 <"$col/collection.anki2")" \
    "$(mg_oracle__sha256 <"$REPO/conformance/fixtures/storage/schema-18.anki2")" &&
    expect "nothing added beside it" "$(ls "$col" | tr '\n' ' ')" "collection.anki2 manifest.json "
}

# The check's worker transcript against protocol.json and the reports of the
# previous test's checks (without and with --schedule) against
# collection-check.schema.json, with jsonschema in the dev image.
t_check_transcript_and_report_match_the_schemas() {
  local req="$WORK/check-requests.jsonl" resp="$WORK/check-responses.jsonl" report="$CHECK_DIR/clean-report.json"
  local schedule_report="$CHECK_DIR/schedule-report.json"
  [ -s "$report" ] && [ -s "$schedule_report" ] || {
    echo "no reports from t_check_job_reports_problems_and_errors"
    return 1
  }
  cat >"$req" <<'EOF'
{"cmd":"hello"}
{"cmd":"check","id":1,"dir":"/in/collection","now":1894701600,"decks":["Lang","Nope"],"render_notes":[1788249600238,1]}
{"cmd":"check","id":2,"dir":"/in/nothing","now":1894701600}
{"cmd":"check","id":3,"dir":"/in/collection"}
{"cmd":"check","id":4,"dir":"/in/collection","now":1894701600,"schedule":true}
EOF
  mg_oracle__run "$MG_ORACLE_TAG" "$ORACLE" -e TZ=Asia/Kathmandu -v "$CHECK_DIR/col:/in/collection:ro" oracle <"$req" >"$resp" || return 1
  expect "response lines" "$(wc -l <"$resp" | tr -d ' ')" 5 &&
    expect "error kinds" "$(jq -sc '[.[] | .error.kind // "ok"]' "$resp")" '["ok","ok","no_collection","protocol_error","ok"]' &&
    expect "rendered cards" "$(jq -sc '.[1].check.render | [keys, (.["1788249600238"] | length > 0), .["1"]]' "$resp")" \
      '[["1","1788249600238"],true,null]' &&
    expect "schedule only when asked" "$(jq -sc '[(.[1].check | has("schedule")), (.[4].check | has("schedule"))]' "$resp")" \
      '[false,true]' || return 1
  local rel_req="${req#"$REPO"/}" rel_resp="${resp#"$REPO"/}" rel_report="${report#"$REPO"/}"
  local rel_schedule="${schedule_report#"$REPO"/}"
  mg_compose build dev >"$WORK/dev-build.log" 2>&1 || {
    tail -20 "$WORK/dev-build.log"
    return 1
  }
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev python3 - "$rel_req" "$rel_resp" "$rel_report" "$rel_schedule" <<'EOF'
import json, sys
from jsonschema import Draft202012Validator
protocol = json.load(open("conformance/schema/protocol.json"))
report_schema = json.load(open("conformance/schema/collection-check.schema.json"))
Draft202012Validator.check_schema(report_schema)
def validator(ref):
    return Draft202012Validator({"$ref": f"#/$defs/{ref}", "$defs": protocol["$defs"]})
bad = 0
for name, v, path in (("request", validator("request"), sys.argv[1]), ("response", validator("response"), sys.argv[2])):
    for n, line in enumerate(open(path), 1):
        msg = json.loads(line)
        if name == "request" and msg.get("cmd") == "check" and "now" not in msg:
            if v.is_valid(msg):
                print(f"request {n}: a check without now passed the schema"); bad += 1
            continue
        for err in v.iter_errors(msg):
            print(f"{name} {n}: {err.json_path}: {err.message}"); bad += 1
report = Draft202012Validator(report_schema)
for path in sys.argv[3:5]:
    for err in report.iter_errors(json.load(open(path))):
        print(f"report {path}: {err.json_path}: {err.message}"); bad += 1
if "schedule" not in json.load(open(sys.argv[4])):
    print("the --schedule report has no schedule"); bad += 1
lines = open(sys.argv[2]).readlines()
for n in (1, 4):
    check = json.loads(lines[n])["check"]
    full = {"report_version": 1, "oracle": {"anki_version": "26.9.3", "lock_sha256": "0" * 64, "driver_sha256": "0" * 64}, "dir": "x", **check}
    for err in report.iter_errors(full):
        print(f"worker check {n + 1} as a report: {err.json_path}: {err.message}"); bad += 1
sys.exit(1 if bad else 0)
EOF
}

# tools/mg oracle time (specs/conformance/timed-operations.md) on a copy of a
# committed fixture: every operation ok with the zone and clock from the
# manifest, an operation error (fail), and the T8 errors. Nested tools/mg
# runs of this checkout, as for check.
TIME_DIR="$WORK/time"

time_run() { # <out var> [env...] -- <args...>: the result.json path of one run
  local var="$1" out
  shift
  local envs=()
  while [ "$#" -gt 0 ] && [ "$1" != -- ]; do
    envs[${#envs[@]}]="$1"
    shift
  done
  shift
  out="$(env ${envs[0]+"${envs[@]}"} /bin/bash "$REPO/tools/mg" oracle time "$@" 2>>"$WORK/time.log" | tail -n 1)"
  [ -f "$out" ] || {
    echo "tools/mg oracle time $* wrote no result:"
    tail -n 20 "$WORK/time.log"
    return 1
  }
  printf -v "$var" '%s' "$out"
}

time_fixture() { # <dir>: schema-18.anki2 with a manifest at 2030-01-15T10:00:00Z, UTC+1
  mkdir -p "$1" && cp "$REPO/conformance/fixtures/storage/schema-18.anki2" "$1/collection.anki2" &&
    printf '{"now_ms": 1894701600999, "utc_offset_secs": 3600}\n' >"$1/manifest.json"
}

t_time_job_reports_operations_and_errors() {
  local col="$TIME_DIR/col" nocards="$TIME_DIR/nocards" garbage="$TIME_DIR/garbage" bare="$TIME_DIR/bare" r args
  rm -rf "$TIME_DIR" && mkdir -p "$garbage" "$bare" || return 1
  time_fixture "$col" && time_fixture "$nocards" && cp "$col/collection.anki2" "$bare/collection.anki2" || return 1
  head -c 8192 /dev/zero | tr '\0' x >"$garbage/collection.anki2"
  sqlite3 "$nocards/collection.anki2" "delete from cards" || return 1
  time_run r -- "$col" --repeat 1 || return 1
  expect "status" "$(jq -r .status "$r")" pass &&
    expect "every operation ok" "$(jq -c '[.details.timing.operations[] | select(.status == "ok")] | length' "$r")" 22 &&
    expect "summary" "$(jq -c '[.summary.total, .summary.passed, .summary.failed]' "$r")" '[22,22,0]' &&
    expect "zone and clock from the manifest" "$(jq -c '.details.timing | [.tz, .now]' "$r")" '["Etc/GMT-1",1894701600]' &&
    expect "side, platform, protocol" "$(jq -c '.details.timing | [.side, (.platform | startswith("linux-")), .protocol]' "$r")" \
      '["anki",true,{"repeat":1,"warmup":1}]' &&
    expect "the Docker lane's lock" "$(jq -r .details.timing.oracle.lock_sha256 "$r")" "$(mg_oracle__sha256 <"$ORACLE/requirements.lock")" &&
    expect "report artifact" "$(jq -r '[.artifacts[] | select(.kind == "report") | .path | endswith("/timing.json")] == [true]' "$r")" true &&
    expect "artifact equals details.timing" \
      "$(jq -S . "$(dirname "$r")/timing.json" | mg_oracle__sha256)" "$(jq -S .details.timing "$r" | mg_oracle__sha256)" || return 1
  cp "$(dirname "$r")/timing.json" "$TIME_DIR/report.json"
  time_run r -- "$nocards" --repeat 1 --op review.answer --op queue.build || return 1
  expect "an operation error fails the job" "$(jq -c '[.status, [.failures[] | [.id, .message]]]' "$r")" \
    '["fail",[["review.answer","error (no_input at setup)"]]]' &&
    expect "operations in catalogue order" "$(jq -c '[.details.timing.operations[].name]' "$r")" '["queue.build","review.answer"]' || return 1
  cp "$(dirname "$r")/timing.json" "$TIME_DIR/error-report.json"
  time_run r -- "$TIME_DIR/none" || return 1
  expect "missing directory" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["no_collection"]]' || return 1
  time_run r -- "$garbage" --tz UTC --now 2030-01-15T10:00:00Z || return 1
  expect "not a collection" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["open_failed"]]' || return 1
  time_run r -- "$bare" || return 1
  expect "no manifest" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["usage"]]' || return 1
  for args in "--bogus" "--op collection.close" "--repeat 0" "--platform windows"; do
    # shellcheck disable=SC2086 # one option and its value per word
    time_run r -- "$col" $args || return 1
    expect "usage: $args" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["usage"]]' || return 1
  done
  expect "source left as it was" "$(mg_oracle__sha256 <"$col/collection.anki2")" \
    "$(mg_oracle__sha256 <"$REPO/conformance/fixtures/storage/schema-18.anki2")" &&
    expect "nothing added beside it" "$(ls "$col" | tr '\n' ' ')" "collection.anki2 manifest.json "
}

# A timing worker's transcript against protocol.json (a worker started with
# ORACLE_CLOCK_CACHE=1 and no clock file is frozen at the entrypoint's
# 2000-01-01), and the previous test's reports against timing.schema.json,
# with jsonschema in the dev image.
t_time_transcript_and_reports_match_the_schemas() {
  local req="$WORK/time-requests.jsonl" resp="$WORK/time-responses.jsonl" report="$TIME_DIR/report.json"
  local error_report="$TIME_DIR/error-report.json"
  [ -s "$report" ] && [ -s "$error_report" ] || {
    echo "no reports from t_time_job_reports_operations_and_errors"
    return 1
  }
  cat >"$req" <<'JSONL'
{"cmd":"hello"}
{"cmd":"time","id":1,"dir":"/in/collection","now":946684800,"ops":["sync.full_upload","collection.open"],"repeat":2}
{"cmd":"time","id":2,"dir":"/in/collection","now":1894701600,"ops":["collection.open"],"repeat":1}
{"cmd":"time","id":3,"dir":"/in/nothing","now":946684800,"ops":["collection.open"],"repeat":1}
{"cmd":"time","id":4,"dir":"/in/collection","now":946684800,"ops":["collection.open"],"repeat":0}
JSONL
  mg_oracle__run "$MG_ORACLE_TAG" "$ORACLE" -e TZ=Asia/Kathmandu -e ORACLE_CLOCK_CACHE=1 \
    -v "$TIME_DIR/col:/in/collection:ro" oracle <"$req" >"$resp" || return 1
  expect "response lines" "$(wc -l <"$resp" | tr -d ' ')" 5 &&
    expect "error kinds" "$(jq -sc '[.[] | .error.kind // "ok"]' "$resp")" '["ok","ok","protocol_error","no_collection","protocol_error"]' &&
    expect "hello of a timing worker" "$(jq -sc '.[0].time_control' "$resp")" '"faketime"' &&
    expect "ops in catalogue order" "$(jq -sc '[.[1].timing.operations[] | [.name, .status, (.samples_ms | length)]]' "$resp")" \
      '[["collection.open","ok",2],["sync.full_upload","ok",2]]' || return 1
  local rel_req="${req#"$REPO"/}" rel_resp="${resp#"$REPO"/}" rel_report="${report#"$REPO"/}" rel_error="${error_report#"$REPO"/}"
  mg_compose build dev >"$WORK/dev-build.log" 2>&1 || {
    tail -20 "$WORK/dev-build.log"
    return 1
  }
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev python3 - "$rel_req" "$rel_resp" "$rel_report" "$rel_error" <<'PY'
import json, sys
from jsonschema import Draft202012Validator
protocol = json.load(open("conformance/schema/protocol.json"))
timing = json.load(open("conformance/schema/timing.schema.json"))
Draft202012Validator.check_schema(protocol)
Draft202012Validator.check_schema(timing)
def validator(ref):
    return Draft202012Validator({"$ref": f"#/$defs/{ref}", "$defs": protocol["$defs"]})
bad = 0
for name, v, path in (("request", validator("request"), sys.argv[1]), ("response", validator("response"), sys.argv[2])):
    for n, line in enumerate(open(path), 1):
        msg = json.loads(line)
        if name == "request" and msg.get("repeat") == 0:
            if v.is_valid(msg):
                print(f"request {n}: repeat 0 passed the schema"); bad += 1
            continue
        for err in v.iter_errors(msg):
            print(f"{name} {n}: {err.json_path}: {err.message}"); bad += 1
report = Draft202012Validator(timing)
for path in sys.argv[3:5]:
    for err in report.iter_errors(json.load(open(path))):
        print(f"report {path}: {err.json_path}: {err.message}"); bad += 1
member = json.loads(open(sys.argv[2]).readlines()[1])["timing"]
full = {"report_version": 1, "side": "anki", "platform": "linux-arm64",
        "oracle": {"anki_version": "26.9.3", "lock_sha256": "0" * 64, "driver_sha256": "0" * 64}, **member}
for err in report.iter_errors(full):
    print(f"worker timing as a report: {err.json_path}: {err.message}"); bad += 1
broken = json.load(open(sys.argv[3]))
broken["operations"][0]["check"] = {"cards": 1}
if report.is_valid(broken):
    print("a collection.open check without notes and revlog passed the schema"); bad += 1
sys.exit(1 if bad else 0)
PY
}

t_the_oracle_service_has_no_route_out() {
  local cfg
  cfg="$(mg_compose config --format json)" || return 1
  expect "oracle networks" "$(jq -c '.services.oracle.networks | keys' <<<"$cfg")" '["oracle-internal"]' &&
    expect "oracle-internal is internal" "$(jq -r '.networks["oracle-internal"].internal' <<<"$cfg")" true
}

# Positive control for the driver's isolation test: the same probe connects
# when the container has a route. Skipped when this Mac is offline.
t_the_network_probe_connects_when_a_route_exists() {
  local out
  out="$(printf '%s\n' '{"cmd":"run","scenario":{"id":"t/net","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"observe":[{"net_probe":{"host":"1.1.1.1","port":443}}]}]}}' |
    docker run --rm -i --init --network bridge --label "meguri.mg.run-id=$MG_RUN_ID" "meguri-oracle:$MG_ORACLE_TAG")"
  if [ "$(jq -r '.observations.steps[0].net_probe.connected' <<<"$out")" != true ]; then
    echo "SKIP no route to 1.1.1.1:443 from the default bridge network ($out)"
    return 2
  fi
}

t_no_meguri_code_refers_to_oracle() {
  local hits rc mode
  local pattern='(\.\./)+oracle/|oracle/(driver|Dockerfile|entrypoint|requirements)'
  # Search shipped and newly added source, not ignored compiler caches.
  # Tracked files remain checked even when an ignore rule matches them.
  # The untracked traversal prunes ignored directories, so scan the index's
  # working-tree files separately to retain forced-added source coverage.
  for mode in tracked untracked; do
    if [ "$mode" = tracked ]; then
      hits="$(git -C "$REPO" grep -I -l -E "$pattern" -- core app 2>/dev/null)"
    else
      hits="$(git -C "$REPO" grep --untracked --exclude-standard -I -l -E "$pattern" -- core app 2>/dev/null)"
    fi
    rc=$?
    if [ "$rc" -gt 1 ]; then
      echo "Meguri source reference scan failed"
      return 1
    fi
    if [ -n "$hits" ]; then
      echo "Meguri source refers to oracle/:"
      printf '%s\n' "$hits" | sed -n '1,5p'
      return 1
    fi
  done
  return 0
}

# An implementer worktree has no oracle/ (U2's sparse checkout). tools/mg there
# builds from the main worktree's oracle/ and compares as usual. Throwaway
# repo: a main checkout with a copy of the tree, and a sparse linked worktree.
# It holds core/ too: compare reads the selected scenarios with meguri-cli to
# find the actor scenarios, built in this checkout's target directory so that
# only the workspace crates compile again.
t_compare_runs_in_a_worktree_without_oracle() {
  local rc
  compare_in_a_sparse_worktree "$SPARSE"
  rc=$?
  rm -rf "$SPARSE"
  return "$rc"
}

compare_in_a_sparse_worktree() {
  local base="$1" main impl out result project rc
  main="$base/main"
  impl="$base/impl"
  rm -rf "$base" && mkdir -p "$main/oracle" || return 1
  (cd "$REPO" && tar -cf - --exclude core/target tools docker conformance core) | tar -xf - -C "$main" &&
    (cd "$ORACLE" && tar -cf - --exclude __pycache__ --exclude '*.pyc' .) | tar -xf - -C "$main/oracle" || return 1
  git -C "$main" init -q && git -C "$main" add -A &&
    git -C "$main" -c user.name=hostcheck -c user.email=hostcheck@localhost -c commit.gpgsign=false commit -qm main &&
    git -C "$main" worktree add -q --no-checkout "$impl" HEAD &&
    git -C "$impl" sparse-checkout set --no-cone '/*' '!/oracle/' &&
    git -C "$impl" checkout -q || return 1
  if [ -e "$impl/oracle" ]; then
    echo "the sparse worktree has oracle/"
    return 1
  fi
  out="$(CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/core/target}" /bin/bash "$impl/tools/mg" oracle compare smoke 2>"$base/compare.log")"
  rc=$?
  result="$(printf '%s\n' "$out" | tail -n 1)"
  project="meguri-impl-$(printf '%s' "$(cd "$impl" && pwd -P)" | cksum | cut -d' ' -f1)"
  # compare starts the profiled sync-faults for actor scenarios; down without
  # its profile would leave it and the network up.
  BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker compose -p "$project" -f "$impl/docker/compose.yml" \
    --profile oracle-sync --profile oracle-record down >/dev/null 2>&1
  if [ "$rc" -ne 0 ] || [ ! -f "$result" ]; then
    echo "tools/mg oracle compare smoke exited $rc in the sparse worktree:"
    tail -n 20 "$base/compare.log"
    [ -f "$result" ] && jq -c '{status, failures}' "$result"
    return 1
  fi
  expect "status" "$(jq -r .status "$result")" pass &&
    expect "image tag" "$(jq -r .details.image.tag "$result")" "meguri-oracle:$MG_ORACLE_TAG" &&
    expect "every scenario passed" "$(jq -r '.summary.passed > 0 and .summary.passed == .summary.total' "$result")" true &&
    expect "oracle/ in the sparse worktree after the run" "$([ -e "$impl/oracle" ] && echo present || echo absent)" absent &&
    expect "containers left by the run" \
      "$(docker ps -aq --filter "label=com.docker.compose.project=$project" | wc -l | tr -d ' ')" 0
}

# --- the macOS native lane (specs/conformance/float-determinism.md F6, F7) ----
# Only on an arm64 Mac: oracle/macos/run-native.sh with its venv under this
# checkout's build/oracle-macos-venv/ (bootstrapped by the first check).
NATIVE="$ORACLE/macos/run-native.sh"
NATIVE_DIR="$WORK/native"

native_or_skip() {
  if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    echo "SKIP the native oracle lane runs only on an arm64 Mac"
    return 2
  fi
}

native_hello_run() { # <out var> [env...]: the result.json of tools/mg oracle hello --platform apple
  local var="$1" out
  shift
  out="$(env "$@" /bin/bash "$REPO/tools/mg" oracle hello --platform apple --tz Asia/Kathmandu 2>>"$WORK/native.log" | tail -n 1)"
  [ -f "$out" ] || {
    echo "tools/mg oracle hello --platform apple wrote no result:"
    tail -n 20 "$WORK/native.log"
    return 1
  }
  printf -v "$var" '%s' "$out"
}

# native_time_control: the time control this Mac's native workers have
# (F6.3), faketime or none, as the hello check found it (or from a hello of
# its own when that check did not run). none means libfaketime cannot be
# injected here, and the checks then expect the real-clock fallback.
native_time_control() {
  local r mode
  if [ ! -s "$WORK/native-time-control" ]; then
    native_hello_run r || return 1
    jq -r '.details.hello.time_control // ""' "$r" >"$WORK/native-time-control"
  fi
  mode="$(cat "$WORK/native-time-control")"
  case "$mode" in
    faketime | none) printf '%s\n' "$mode" ;;
    *)
      echo "hello answered time_control '$mode', neither faketime nor none"
      return 1
      ;;
  esac
}

t_the_native_lane_says_hello_with_its_platform_lock_and_time_control() {
  native_or_skip || return $?
  local r mode container="$WORK/hello-container.json"
  hello_on "$MG_ORACLE_TAG" "$ORACLE" UTC "$container" || return 1
  native_hello_run r || return 1
  mode="$(jq -r '.details.hello.time_control // ""' "$r")"
  printf '%s\n' "$mode" >"$WORK/native-time-control"
  # F6.3: faketime where libfaketime can be injected; otherwise the worker
  # runs on the real clock and hello warns. A passing run says which.
  case "$mode" in
    faketime) expect "status" "$(jq -r .status "$r")" pass ;;
    none)
      expect "status on the real clock" "$(jq -r .status "$r")" warn &&
        expect "failures on the real clock" "$(jq -c '[.failures[].id]' "$r")" '["no-faketime"]' &&
        echo "NOTE time control none on this Mac: the real-clock fallback was checked"
      ;;
    *) expect "time control" "$mode" "faketime or none" ;;
  esac &&
    expect "platform" "$(jq -r .details.hello.platform "$r")" macos-arm64 &&
    expect "tz" "$(jq -r .details.hello.tz "$r")" Asia/Kathmandu &&
    expect "lock: the macOS lock" "$(jq -r .details.hello.lock_sha256 "$r")" \
      "$(mg_oracle__sha256 <"$ORACLE/macos/requirements-macos.lock")" &&
    expect "lock: the pin" "$(jq -r .details.hello.lock_sha256 "$r")" "$(jq -r .macos_lock_sha256 "$REPO/conformance/oracle.json")" &&
    expect "the same driver as the container" "$(jq -r .details.hello.driver_sha256 "$r")" "$(jq -r .driver_sha256 "$container")" &&
    expect "the same anki" "$(jq -r .details.hello.anki_version "$r")" "$(jq -r .anki_version "$container")" || return 1
  # Without libfaketime the worker runs on the real clock (F6.3).
  native_hello_run r ORACLE_NATIVE_FAKETIME=0 || return 1
  expect "status without faketime" "$(jq -r .status "$r")" warn &&
    expect "time control without faketime" "$(jq -r .details.hello.time_control "$r")" none
}

# native_run <scenario file> <out>: one native worker's run response.
native_run() {
  local scratch
  scratch="$(mktemp -d "$NATIVE_DIR/run-XXXXXX")"
  jq -c '{cmd: "run", scenario: .}' "$1" | TZ=UTC ORACLE_SCRATCH="$scratch" "$NATIVE" worker >"$2" 2>>"$WORK/native.log"
  rm -rf "$scratch"
  jq -e .ok "$2" >/dev/null
}

native_harness() { # <record-apple|compare-apple> <report> [args...]
  local cmd="$1" report="$2"
  shift 2
  ORACLE_SCRATCH_ROOT="$NATIVE_DIR/scratch" "$NATIVE" harness "$cmd" --scenarios "$NATIVE_DIR/scenarios" \
    --goldens "$NATIVE_DIR/goldens" --overlays "$NATIVE_DIR/overlays" --pin "$NATIVE_DIR/oracle.json" \
    --jobs 2 --report "$report" "$@" 2>>"$WORK/native.log"
}

# native_golden <id> <jq edit>: a golden for a scenario of the throwaway
# tree, as the Linux oracle would write it (the pin's Linux lock), from a
# native run with the edit applied.
native_golden() {
  local id="$1" resp="$NATIVE_DIR/run.json"
  native_run "$NATIVE_DIR/scenarios/$id.json" "$resp" || {
    cat "$resp"
    return 1
  }
  native_golden_of "$id" "$2" <"$resp"
}

# native_golden_of <id> <jq edit>, a run response on stdin: the golden with
# that response's observations, the edit applied.
native_golden_of() {
  local id="$1" sha
  sha="$("$NATIVE" harness hash "$NATIVE_DIR/scenarios/$id.json" | cut -d' ' -f1)"
  jq -S --arg id "$id" --arg sha "$sha" --slurpfile pin "$NATIVE_DIR/oracle.json" '
    {scenario_id: $id, scenario_sha256: $sha,
     oracle: {anki_version: "26.9.3", lock_sha256: $pin[0].lock_sha256, driver_sha256: ("0" * 64)},
     tz: "UTC", clock: {start: "2030-01-15T10:00:00", mode: "frozen"}, observations: .observations} | '"$2" \
    >"$NATIVE_DIR/goldens/$id.json"
}

t_the_native_harness_records_compares_and_refuses_overlays() {
  native_or_skip || return $?
  local rc
  native_overlays
  rc=$?
  rm -rf "$NATIVE_DIR"
  return "$rc"
}

native_overlays() {
  local dir="$NATIVE_DIR" report="$NATIVE_DIR/report.json" overlay="$NATIVE_DIR/overlays/t/probe.json" before rc rel mode timed
  mode="$(native_time_control)" || {
    echo "$mode"
    return 1
  }
  rm -rf "$dir" && mkdir -p "$dir/scenarios/t" "$dir/goldens/t" "$dir/scratch" || return 1
  jq '{anki_version, lock_sha256, macos_lock_sha256}' "$REPO/conformance/oracle.json" >"$dir/oracle.json"
  cat >"$dir/scenarios/t/probe.json" <<'JSON'
{"id": "t/probe", "tags": ["clock-free"], "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"},
 "steps": [{"observe": [{"fsrs_next": {"param_sets": {"default": []}, "inputs": [
   {"id": 0, "card_id": 1894698000000, "reps": 2, "s": 2.3065, "d": 2.111, "elapsed_days": 3, "scheduled_days": 2, "dr": 0.9, "params": "default", "max_ivl": 36500},
   {"id": 1, "card_id": 1700000000001, "reps": 5, "s": 40.0, "d": 4.0, "elapsed_days": 41, "scheduled_days": 40, "dr": 0.9, "params": "default", "max_ivl": 36500}]}}]}]}
JSON
  cat >"$dir/scenarios/t/timed.json" <<'JSON'
{"id": "t/timed", "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}, "steps": [{"observe": ["timing"]}]}
JSON
  native_golden t/probe . || return 1
  if [ "$mode" = faketime ]; then
    native_golden t/timed . || return 1
    timed=no_difference
  else
    # A real-clock worker does not run t/timed (F6.3): its golden only has
    # to be fresh, so it is written here rather than recorded.
    echo '{"observations": {"steps": [{"timing": {"days_elapsed": 0, "next_day_at": 1894766400, "now": 1894701600, "utc_offset_secs": 0}}]}}' |
      native_golden_of t/timed . || return 1
    timed=clock_unsupported
  fi

  # The Apple oracle reproduces its own output: no overlay. On the real
  # clock only the clock-free t/probe runs.
  native_harness record-apple "$report"
  expect "no difference" "$(jq -c '[.scenarios[] | [.scenario, .status]]' "$report")" \
    "[[\"t/probe\",\"no_difference\"],[\"t/timed\",\"$timed\"]]" &&
    expect "no overlay" "$([ -e "$overlay" ] && echo written || echo none)" none || return 1

  # Good's days 3 apart: an entry outside the class, written and reported (F7.2, F8).
  native_golden t/probe '.observations.steps[0].fsrs_next.rows[1][6] -= 3' || return 1
  native_harness record-apple "$report" --filter t/probe
  rc=$?
  expect "exit" "$rc" 1 &&
    expect "outside the class" "$(jq -c '.scenarios[0] | [.status, [.outside_class[].pointer], .day_entries]' "$report")" \
      '["outside_class",["/steps/0/fsrs_next/rows/1/6"],1]' &&
    expect "overlay entry" "$(jq -c '[.entries[] | [.pointer, .apple - .linux, (.unrounded | keys)]]' "$overlay")" \
      '[["/steps/0/fsrs_next/rows/1/6",3,["apple","linux"]]]' &&
    expect "overlay oracle" "$(jq -r '.oracle | "\(.platform) \(.lock_sha256)"' "$overlay")" \
      "macos-arm64 $(jq -r .macos_lock_sha256 "$dir/oracle.json")" || return 1
  rel="${overlay#"$REPO"/}"
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev python3 - "$rel" <<'PY' || return 1
import json, sys
from jsonschema import Draft202012Validator
schema = json.load(open("conformance/schema/overlay.schema.json"))
errors = list(Draft202012Validator(schema).iter_errors(json.load(open(sys.argv[1]))))
for e in errors:
    print(f"overlay: {e.json_path}: {e.message}")
sys.exit(1 if errors else 0)
PY

  # Golden plus overlay is what the Apple oracle observes (F9 step 2).
  native_harness compare-apple "$report" --filter t/probe
  expect "compare with the overlay" "$(jq -c '.scenarios[0] | [.status, .overlay_applied]' "$report")" '["pass",1]' || return 1

  # A structural difference writes nothing and keeps the overlay.
  before="$(mg_oracle__sha256 <"$overlay")"
  native_golden t/probe '.observations.steps[0].fsrs_next.rows[1][6] -= 3 | .observations.steps[0].fsrs_next.rows += [[1]]' || return 1
  native_harness record-apple "$report" --filter t/probe
  expect "structural" "$(jq -r '.scenarios[0].status' "$report")" structural &&
    expect "overlay kept" "$(mg_oracle__sha256 <"$overlay")" "$before" || return 1

  # The golden matches again: the overlay is deleted.
  native_golden t/probe . || return 1
  native_harness record-apple "$report" --filter t/probe
  expect "deleted" "$(jq -c '.scenarios[0] | [.status, .deleted]' "$report")" '["no_difference",true]' &&
    expect "overlay gone" "$([ -e "$overlay" ] && echo kept || echo gone)" gone || return 1

  # Without faketime only clock-free scenarios run (F6.3).
  ORACLE_NATIVE_FAKETIME=0 native_harness compare-apple "$report"
  expect "clock-free only" "$(jq -c '[.scenarios[] | [.scenario, .status]]' "$report")" \
    '[["t/probe","pass"],["t/timed","clock_unsupported"]]' || return 1

  # A worker from another lock than the pin's is refused.
  jq '.macos_lock_sha256 = ("f" * 64)' "$dir/oracle.json" >"$dir/other.json" && mv "$dir/other.json" "$dir/oracle.json"
  native_harness compare-apple "$report" --filter t/probe
  expect "refused" "$(jq -c '.scenarios[0] | [.status, .error.kind]' "$report")" '["error","oracle_mismatch"]' &&
    expect "scratch left behind" "$(find "$dir/scratch" -mindepth 1 | head -n 3)" ""
}

# tools/mg oracle time --platform apple (timed-operations.md T1, T2.3): the
# primary series' lane times operations with its own lock, and a worker
# without libfaketime ends the run with clock_unsupported. Its scratch is
# gone afterwards.
t_the_native_lane_times_operations() {
  native_or_skip || return $?
  local col="$TIME_DIR/native-col" r mode report
  mode="$(native_time_control)" || {
    echo "$mode"
    return 1
  }
  rm -rf "$col" && time_fixture "$col" || return 1
  time_run r -- "$col" --platform apple --repeat 1 --op collection.open --op sync.full_download --op sync.full_upload || return 1
  if [ "$mode" = none ]; then
    expect "a real-clock worker cannot time" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["clock_unsupported"]]'
    return
  fi
  report="$(dirname "$r")/timing.json"
  expect "status" "$(jq -r .status "$r")" pass &&
    expect "operations" "$(jq -c '[.details.timing.operations[] | [.name, .status]]' "$r")" \
      '[["collection.open","ok"],["sync.full_upload","ok"],["sync.full_download","ok"]]' &&
    expect "platform" "$(jq -r .details.timing.platform "$r")" macos-arm64 &&
    expect "the native lane's lock" "$(jq -r .details.timing.oracle.lock_sha256 "$r")" \
      "$(mg_oracle__sha256 <"$ORACLE/macos/requirements-macos.lock")" &&
    expect "scratch left behind" "$(find "$(dirname "$r")" -mindepth 1 -type d | head -n 3)" "" || return 1
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev python3 - "${report#"$REPO"/}" <<'PY' || return 1
import json, sys
from jsonschema import Draft202012Validator
errors = list(Draft202012Validator(json.load(open("conformance/schema/timing.schema.json"))).iter_errors(json.load(open(sys.argv[1]))))
for e in errors:
    print(f"timing.json: {e.json_path}: {e.message}")
sys.exit(1 if errors else 0)
PY
  time_run r ORACLE_NATIVE_FAKETIME=0 -- "$col" --platform apple --repeat 1 --op collection.open || return 1
  expect "without faketime" "$(jq -c '[.status, [.failures[].id]]' "$r")" '["error",["clock_unsupported"]]'
}

# Runs last. The checks above copy oracle/ into the run directory, under this
# checkout's build/, where U2's deny rules for oracle/ do not reach.
t_no_copy_of_oracle_files_is_left_behind() {
  local left
  left="$(find "$MG_RUN_DIR" \( -name '*.py' -o -name Dockerfile -o -name entrypoint.sh \) | head -n 5)"
  if [ -n "$left" ]; then
    echo "copies of oracle/ files are left in the run directory:"
    echo "$left"
    return 1
  fi
}

TESTS="t_tag_is_content_addressed_and_stable t_rebuilding_an_unchanged_tree_keeps_tag_and_image
t_editing_a_driver_file_changes_tag_and_driver_hash t_a_compose_worker_says_hello_in_its_zone
t_protocol_transcript_matches_the_schema t_check_job_reports_problems_and_errors
t_check_transcript_and_report_match_the_schemas t_time_job_reports_operations_and_errors
t_time_transcript_and_reports_match_the_schemas t_the_oracle_service_has_no_route_out
t_the_network_probe_connects_when_a_route_exists t_no_meguri_code_refers_to_oracle
t_compare_runs_in_a_worktree_without_oracle t_the_native_lane_says_hello_with_its_platform_lock_and_time_control
t_the_native_harness_records_compares_and_refuses_overlays t_the_native_lane_times_operations
t_no_copy_of_oracle_files_is_left_behind"

echo "1..$(echo "$TESTS" | wc -w | tr -d ' ')"
n=0
failed=0
for t in $TESTS; do
  n=$((n + 1))
  out="$("$t" 2>&1)"
  rc=$?
  if [ "$rc" -eq 0 ]; then
    # A passing check may say how it passed with a line "NOTE <text>".
    note="$(printf '%s\n' "$out" | sed -n 's/^NOTE //p' | head -n 1)"
    echo "ok $n $t${note:+ ($note)}"
  elif [ "$rc" -eq 2 ] && printf '%s' "$out" | grep -q '^SKIP '; then
    echo "ok $n $t # SKIP $(printf '%s' "$out" | sed -n 's/^SKIP //p' | head -n 1)"
  else
    failed=$((failed + 1))
    echo "not ok $n $t"
    printf '%s\n' "$out" | sed 's/^/# /'
  fi
done
[ "$failed" -eq 0 ]
