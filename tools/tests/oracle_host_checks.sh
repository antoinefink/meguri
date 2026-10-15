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
trap 'rm -rf "$EDITED" "$SPARSE"' EXIT
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
  (cd "$ORACLE" && cp Dockerfile Dockerfile.dockerignore entrypoint.sh requirements.lock "$edited/" &&
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
{"cmd":"begin","scenario":{"id":"t/i","clock":{"tz":"UTC","start":"2030-01-15T10:00:00"},"steps":[{"advance":"1d"}]}}
{"cmd":"step"}
{"cmd":"step","step":{"observe":["timing"]}}
{"cmd":"end"}
{"cmd":"warp"}
EOF
  mg_oracle__run "$MG_ORACLE_TAG" "$ORACLE" oracle <"$req" >"$resp" || return 1
  printf 'not json\n' | mg_oracle__run "$MG_ORACLE_TAG" "$ORACLE" oracle >>"$resp" || return 1
  expect "response lines" "$(wc -l <"$resp" | tr -d ' ')" 9 || return 1
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
kinds = [json.loads(l).get("error", {}).get("kind") for l in open(sys.argv[2])]
for kind in ("unsupported_op", "unknown_cmd", "protocol_error"):
    if kind not in kinds:
        print(f"no {kind} response in the transcript"); bad += 1
sys.exit(1 if bad else 0)
EOF
}

# tools/mg oracle check (specs/conformance/collection-check.md) on a copy of a
# committed fixture: a clean report with the zone and clock from the manifest,
# a planted problem (fail), and the K7 errors. Nested tools/mg runs of this
# checkout; the outer run rewrites build/mg/oracle/latest when it ends.
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

# The check's worker transcript against protocol.json and the report of the
# previous check against collection-check.schema.json, with jsonschema in the
# dev image.
t_check_transcript_and_report_match_the_schemas() {
  local req="$WORK/check-requests.jsonl" resp="$WORK/check-responses.jsonl" report="$CHECK_DIR/clean-report.json"
  [ -s "$report" ] || {
    echo "no report from t_check_job_reports_problems_and_errors"
    return 1
  }
  cat >"$req" <<'EOF'
{"cmd":"hello"}
{"cmd":"check","id":1,"dir":"/in/collection","now":1894701600,"decks":["Lang","Nope"],"render_notes":[1788249600238,1]}
{"cmd":"check","id":2,"dir":"/in/nothing","now":1894701600}
{"cmd":"check","id":3,"dir":"/in/collection"}
EOF
  mg_oracle__run "$MG_ORACLE_TAG" "$ORACLE" -e TZ=Asia/Kathmandu -v "$CHECK_DIR/col:/in/collection:ro" oracle <"$req" >"$resp" || return 1
  expect "response lines" "$(wc -l <"$resp" | tr -d ' ')" 4 &&
    expect "error kinds" "$(jq -sc '[.[] | .error.kind // "ok"]' "$resp")" '["ok","ok","no_collection","protocol_error"]' &&
    expect "rendered cards" "$(jq -sc '.[1].check.render | [keys, (.["1788249600238"] | length > 0), .["1"]]' "$resp")" \
      '[["1","1788249600238"],true,null]' || return 1
  local rel_req="${req#"$REPO"/}" rel_resp="${resp#"$REPO"/}" rel_report="${report#"$REPO"/}"
  mg_compose build dev >"$WORK/dev-build.log" 2>&1 || {
    tail -20 "$WORK/dev-build.log"
    return 1
  }
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev python3 - "$rel_req" "$rel_resp" "$rel_report" <<'EOF'
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
for err in report.iter_errors(json.load(open(sys.argv[3]))):
    print(f"report: {err.json_path}: {err.message}"); bad += 1
check = json.loads(open(sys.argv[2]).readlines()[1])["check"]
full = {"report_version": 1, "oracle": {"anki_version": "26.9.3", "lock_sha256": "0" * 64, "driver_sha256": "0" * 64}, "dir": "x", **check}
for err in report.iter_errors(full):
    print(f"worker check as a report: {err.json_path}: {err.message}"); bad += 1
sys.exit(1 if bad else 0)
EOF
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
  local hits d
  for d in core app; do
    [ -d "$REPO/$d" ] || continue
    hits="$(grep -rIEn '(\.\./)+oracle/|oracle/(driver|Dockerfile|entrypoint|requirements)' "$REPO/$d" 2>/dev/null | grep -v '/target/' | head -n 5)"
    if [ -n "$hits" ]; then
      echo "$d refers to oracle/:"
      echo "$hits"
      return 1
    fi
  done
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
t_check_transcript_and_report_match_the_schemas t_the_oracle_service_has_no_route_out
t_the_network_probe_connects_when_a_route_exists t_no_meguri_code_refers_to_oracle
t_compare_runs_in_a_worktree_without_oracle t_no_copy_of_oracle_files_is_left_behind"

echo "1..$(echo "$TESTS" | wc -w | tr -d ' ')"
n=0
failed=0
for t in $TESTS; do
  n=$((n + 1))
  out="$("$t" 2>&1)"
  rc=$?
  if [ "$rc" -eq 0 ]; then
    echo "ok $n $t"
  elif [ "$rc" -eq 2 ] && printf '%s' "$out" | grep -q '^SKIP '; then
    echo "ok $n $t # SKIP $(printf '%s' "$out" | sed -n 's/^SKIP //p' | head -n 1)"
  else
    failed=$((failed + 1))
    echo "not ok $n $t"
    printf '%s\n' "$out" | sed 's/^/# /'
  fi
done
[ "$failed" -eq 0 ]
