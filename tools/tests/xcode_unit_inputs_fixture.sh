# Controlled producer doubles for tooling tests only; no native/VM evidence.
# Sources the real bridge and xcode preparation functions.
unit_fixture_setup() {
  export MG_ROOT="$1" MG_JOB=xcode-check MG_RUN_ID="${2:-unit-input-run}"
  export MG_RUN_DIR="$MG_ROOT/build/mg/xcode-check/$MG_RUN_ID"
  export UNIT_CALLS="$MG_ROOT/build/unit-calls" UNIT_COMPILER="$MG_ROOT/build/native-target/debug/meguri-cli"
  mkdir -p "$MG_ROOT/tools/scripts" "$MG_ROOT/core" "$MG_RUN_DIR" "$(dirname "$UNIT_COMPILER")" "$MG_ROOT/build/unit-bin"
  cp "$REPO_ROOT/tools/scripts/xcode-unit-inputs.py" \
    "$REPO_ROOT/tools/scripts/swift-cloud-service-helper.py" \
    "$REPO_ROOT/tools/scripts/native-media-review-capture.py" \
    "$REPO_ROOT/tools/scripts/native-media-export-capture.py" "$MG_ROOT/tools/scripts/"
  printf '#!/bin/sh\nexit 97\n' >"$UNIT_COMPILER"
  chmod 755 "$UNIT_COMPILER"
  cat >"$MG_ROOT/build/unit-bin/cargo" <<'CARGO'
#!/bin/bash
[ "$*" = 'build -q -p meguri-cli --message-format=json' ] || exit 97
[ "$PWD" = "$MG_ROOT/core" ] || exit 97
echo native-cargo >>"$UNIT_CALLS"
[ "${UNIT_CARGO_FAIL:-0}" = 0 ] || exit 1
python3 - <<'PY'
import json, os
if os.environ.get('UNIT_WARNING'):
    print(json.dumps({'reason': 'compiler-message', 'message': {'level': 'warning'}}))
print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': 'meguri-cli'},
                  'executable': os.environ['UNIT_COMPILER']}))
PY
CARGO
  cat >"$MG_ROOT/tools/mg" <<'MG'
#!/bin/bash
[ "$*" = 'core-test -- --test cloud_initial_app_fixture -E package(meguri-ffi) & test(=export_initial_app_adoption_replies_from_real_publication)' ] || exit 97
echo rust-producer >>"$UNIT_CALLS"
[ "${UNIT_PRODUCER_FAIL:-0}" = 0 ] || exit 1
out="$MG_ROOT/${MEGURI_INITIAL_ADOPTION_FIXTURE#/work/}"
[ "$out" = "$MG_RUN_DIR/initial-adoption-fixture.json" ] || exit 97
[ "${UNIT_MISSING_FIXTURE:-0}" = 1 ] || printf '{"synthetic":"producer-double-only"}\n' >"$out"
result="$MG_ROOT/build/mg/core-test/$MG_RUN_ID-producer/result.json"
mkdir -p "$(dirname "$result")"
python3 - "$result" <<'PY'
import json, os, subprocess, sys
r = {'job': 'core-test', 'status': 'pass', 'dry_run': False, 'root': os.environ['MG_ROOT'],
     'run_id': os.environ['MG_RUN_ID'] + '-producer', 'args': sys.argv[2:],
     'commit': {'sha': subprocess.check_output(['git', '-C', os.environ['MG_ROOT'], 'rev-parse', 'HEAD'], text=True).strip(),
                'dirty': bool(os.environ.get('UNIT_DIRTY_RECEIPT'))},
     'summary': {'total': 1, 'passed': 1, 'failed': 0, 'skipped': 0}}
r['args'] = ['--', '--test', 'cloud_initial_app_fixture', '-E',
             'package(meguri-ffi) & test(=export_initial_app_adoption_replies_from_real_publication)']
if os.environ.get('UNIT_WRONG_REVISION'): r['commit']['sha'] = '0' * 40
if os.environ.get('UNIT_SKIPPED'): r['summary']['skipped'] = 1
with open(sys.argv[1], 'w') as f: json.dump(r, f)
PY
printf '%s\n' "$result"
MG
  chmod +x "$MG_ROOT/build/unit-bin/cargo" "$MG_ROOT/tools/mg"
  export PATH="$MG_ROOT/build/unit-bin:$PATH"
  printf '/build/\n' >"$MG_ROOT/.gitignore"
  # Only the generated test repository is committed; never the real checkout.
  (unset GIT_DIR GIT_WORK_TREE; git -C "$MG_ROOT" init -q &&
    git -C "$MG_ROOT" add -A &&
    git -C "$MG_ROOT" -c user.name=Test -c user.email=test@example.invalid commit -qm 'Generated tooling fixture' --allow-empty) || return
}

unit_fixture_prepare() {
  (
    mg_job() { :; }
    mg_failure() { printf 'failure=%s\n' "$1"; }
    mg_status() { printf 'status=%s\n' "$1"; }
    mg_artifact() { :; }
    mg_detail() { :; }
    source "$REPO_ROOT/tools/mg.d/bridge.sh"
    source "$REPO_ROOT/tools/mg.d/xcode.sh"
    mg__xcode_unit_inputs
  )
}
