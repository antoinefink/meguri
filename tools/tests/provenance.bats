#!/usr/bin/env bats
# Clean-room tooling (U2): role worktrees, session records, the commit-msg
# provenance hook and the similarity jobs' guards. Runs in the dev container
# by `tools/mg selftest`; each test works on a throwaway copy of the repo
# whose oracle/ holds a stand-in file.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core" "$t/oracle/driver" "$t/docs/process" "$t/specs"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp -R "$REPO_ROOT/.claude" "$t/.claude"
  cp -R "$REPO_ROOT/docs/process/roles" "$REPO_ROOT/docs/process/provenance" "$t/docs/process/"
  cp "$REPO_ROOT/core/rust-toolchain.toml" "$t/core/"
  cp "$REPO_ROOT/.gitignore" "$REPO_ROOT/.gitattributes" "$t/"
  echo "stand-in for the AGPL driver" >"$t/oracle/driver/run.py"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  git -C "$SB" config user.name selftest
  git -C "$SB" config user.email selftest@localhost
  SESSIONS="$SB/docs/process/provenance/sessions.jsonl"
  RECORDS="$SB/docs/process/provenance/sessions"
}

# mg <args>: run the sandbox's tools/mg (or $MG_AT/tools/mg).
mg() {
  run --separate-stderr "${MG_AT:-$SB}/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }

validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }

lines_of() { if [ -f "$1" ]; then wc -l <"$1" | tr -d ' '; else echo 0; fi; }

# records_of <checkout>: how many record files its working tree holds.
records_of() { find "$1/docs/process/provenance/sessions" -name '*.json' 2>/dev/null | wc -l | tr -d ' '; }

# record_at <checkout> <id> [rev]: the committed record of a session.
record_at() { git -C "$1" show "${3:-HEAD}:docs/process/provenance/sessions/$2.json"; }

# newest_record <checkout>: the record the checkout's last commit added.
newest_record() {
  local path
  path="$(git -C "$1" diff-tree --no-commit-id --name-only -r --diff-filter=A HEAD -- docs/process/provenance/sessions/)"
  git -C "$1" show "HEAD:$path"
}

# trailer <checkout> <key> [rev]: the trailer's value in a commit message.
trailer() { git -C "$1" log -1 --format="%(trailers:key=$2,valueonly)" "${3:-HEAD}" | sed '/^$/d'; }

# No worktree, no unit branch, no session record, a clean main checkout.
nothing_created() {
  [ ! -e "$BATS_TEST_TMPDIR/repo-wt/$1" ]
  [ -z "$(git -C "$SB" branch --list "unit/$1")" ]
  [ "$(git -C "$SB" worktree list | wc -l | tr -d ' ')" -eq 1 ]
  [ "$(records_of "$SB")" -eq "${2:-0}" ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
}

# commit_with <message> [paths to create and stage...]
commit_with() {
  local msg="$1" p
  shift
  for p in "$@"; do
    mkdir -p "$(dirname "$WT/$p")"
    echo "// $RANDOM" >>"$WT/$p"
    git -C "$WT" add "$p"
  done
  run --separate-stderr git -C "$WT" commit -q -F - <<<"$msg"
}

@test "worktree new --role implementer: no oracle/, the deny profile, one session record committed on the branch" {
  local before archived head wt marker
  before="$(records_of "$SB")"
  archived="$(lines_of "$SESSIONS")"
  head="$(git -C "$SB" rev-parse HEAD)"
  mg worktree new --role implementer w1
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  validate "$result"
  wt="$(cd "$BATS_TEST_TMPDIR/repo-wt/w1" && pwd -P)"
  [ "$(field .details.worktree.path)" = "$wt" ]
  [ "$(field .details.worktree.branch)" = unit/w1 ]

  # oracle/ is never on disk; everything else is checked out; the tree is clean.
  [ ! -e "$wt/oracle" ]
  [ -f "$wt/tools/mg" ]
  [ -f "$wt/core/rust-toolchain.toml" ]
  [ -z "$(git -C "$wt" status --porcelain)" ]

  # The deny profile is rendered with absolute paths of the main checkout.
  local main
  main="$(cd "$SB" && pwd -P)"
  [ -f "$wt/.claude/settings.local.json" ]
  run ! grep -q '@[A-Z_]*@' "$wt/.claude/settings.local.json"
  jq -e --arg p "Read(/$main/refs/**)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"
  jq -e --arg p "Read(/$main/oracle/**)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"
  jq -e --arg p "Read(/$main/local-data/**)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"
  jq -e --arg p "Bash(*$main/oracle*)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"
  jq -e --arg p "Bash(*$(dirname "$wt")/*/oracle/*)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"
  # The WebFetch hook runs the worktree's own copy of the script.
  local hook
  hook="$(jq -r '.hooks.PreToolUse[] | select(.matcher == "WebFetch") | .hooks[0].command' "$wt/.claude/settings.local.json")"
  [ "$hook" = "\"$wt/tools/hooks/deny-quarantined-fetch\"" ]
  [ -x "$wt/tools/hooks/deny-quarantined-fetch" ]

  # The record is committed as the branch's first commit, as its own file in
  # the worktree; the archive, the main checkout and its status are untouched.
  [ "$(records_of "$SB")" -eq "$before" ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
  [ "$(git -C "$wt" rev-parse HEAD~1)" = "$head" ]
  local rec sum id
  id="$(trailer "$wt" Session)"
  [ "$(git -C "$wt" diff --name-status HEAD~1 HEAD)" = "$(printf 'A\tdocs/process/provenance/sessions/%s.json' "$id")" ]
  [ "$(records_of "$wt")" -eq $((before + 1)) ]
  [ "$(lines_of "$wt/docs/process/provenance/sessions.jsonl")" -eq "$archived" ]
  rec="$(record_at "$wt" "$id")"
  [ "$(wc -l <<<"$rec" | tr -d ' ')" -eq 1 ]
  [ "$(trailer "$wt" Role)" = infra ]
  [ "$(jq -r .session <<<"$rec")" = "$id" ]
  [ "$(git -C "$wt" log -1 --format=%s)" = "Record session $id (implementer, w1)" ]

  # One record, role implementer, linked to the profile and the role doc.
  [ "$(jq -r .role <<<"$rec")" = implementer ]
  [ "$(jq -r .kind <<<"$rec")" = worktree ]
  [ "$(jq -r .name <<<"$rec")" = w1 ]
  [ "$(jq -r .base_commit <<<"$rec")" = "$head" ]
  [ "$(jq -c .excluded_paths <<<"$rec")" = '["oracle/"]' ]
  [ "$(jq -c .sparse <<<"$rec")" = '["/*","!/oracle/"]' ]
  [ "$(jq -r .worktree <<<"$rec")" = "$wt" ]
  [ "$(jq -r .role_doc <<<"$rec")" = docs/process/roles/implementer.md ]
  [ "$(jq -r .role_doc_blob <<<"$rec")" = "$(git -C "$SB" rev-parse HEAD:docs/process/roles/implementer.md)" ]
  sum="$(sha256sum "$wt/.claude/settings.local.json" | cut -d' ' -f1)"
  [ "$(jq -r .deny_profile.sha256 <<<"$rec")" = "$sum" ]
  [[ "$(jq -r .session <<<"$rec")" =~ ^s-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{8}$ ]]

  # The worktree's git dir carries the same record as its role marker.
  marker="$(git -C "$wt" rev-parse --absolute-git-dir)/meguri-session.json"
  [ "$(jq -r .session "$marker")" = "$(jq -r .session <<<"$rec")" ]

  # The main checkout keeps oracle/ and no sparse checkout of its own.
  [ -f "$SB/oracle/driver/run.py" ]
  [ -z "$(git -C "$SB" config --get core.sparseCheckout || true)" ]

  # The commit-msg hook is installed for every worktree.
  grep -q 'tools/hooks/commit-msg' "$SB/.git/hooks/commit-msg"

  # A record made later on the branch merges into a main checkout that
  # made its own, and the merge is not blocked.
  git -C "$SB" checkout -q -b integration
  mg session record --role critic --name main-side
  local main_id later_id
  main_id="$(field .details.session.session)"
  MG_AT="$wt" mg session record --role implementer --name later
  later_id="$(field .details.session.session)"
  git -C "$SB" merge -q --no-edit unit/w1
  [ "$(jq -r .session "$RECORDS/$main_id.json")" = "$main_id" ]
  [ "$(jq -r .session "$RECORDS/$later_id.json")" = "$later_id" ]
  [ "$(jq -r .session "$RECORDS/$id.json")" = "$id" ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
}

@test "worktree new renders the deny profile literally for paths with &, | and backslashes" {
  local main="$BATS_TEST_TMPDIR/m&a|i\\n" wts="$BATS_TEST_TMPDIR/w&t|s\\1" wt s
  cp -R "$SB" "$main"
  MG_AT="$main" mg worktree new --role implementer w1 --path "$wts/w1"
  [ "$status" -eq 0 ]
  main="$(cd "$main" && pwd -P)"
  wt="$(cd "$wts/w1" && pwd -P)"
  s="$wt/.claude/settings.local.json"
  run ! grep -q '@[A-Z_]*@' "$s"
  [ "$(jq '.permissions.deny | length' "$s")" -eq "$(jq '.permissions.deny | length' "$wt/.claude/settings.implementer.json")" ]
  jq -e --arg p "Read(/$main/oracle/**)" '.permissions.deny | index($p)' "$s"
  jq -e --arg p "Bash(*$main/refs*)" '.permissions.deny | index($p)' "$s"
  jq -e --arg p "Bash(*$(dirname "$wt")/*/oracle/*)" '.permissions.deny | index($p)' "$s"
  [ "$(jq -r '.hooks.PreToolUse[] | select(.matcher == "WebFetch") | .hooks[0].command' "$s")" = "\"$wt/tools/hooks/deny-quarantined-fetch\"" ]
}

@test "worktree new refuses a deny profile with a placeholder it does not know" {
  jq '.permissions.deny += ["Read(/@HOME_DIR@/x)"]' "$SB/.claude/settings.implementer.json" >"$BATS_TEST_TMPDIR/p.json"
  cp "$BATS_TEST_TMPDIR/p.json" "$SB/.claude/settings.implementer.json"
  git -C "$SB" commit -qam "Unknown placeholder"
  mg worktree new --role implementer w9
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-profile ]
  [[ "$(field '.failures[0].message')" == *"@HOME_DIR@"* ]]
  validate "$result"
  nothing_created w9 "$(records_of "$SB")"
}

@test "worktree new for a spec writer is a full checkout without a deny profile" {
  mg worktree new --role spec-writer s1 --branch spec/s1
  [ "$status" -eq 0 ]
  local wt="$BATS_TEST_TMPDIR/repo-wt/s1"
  [ -f "$wt/oracle/driver/run.py" ]
  [ ! -e "$wt/.claude/settings.local.json" ]
  [ "$(git -C "$wt" rev-parse --abbrev-ref HEAD)" = spec/s1 ]
  local rec
  rec="$(newest_record "$wt")"
  [ "$(jq -r .role <<<"$rec")" = spec-writer ]
  [ -z "$(git -C "$wt" status --porcelain)" ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
  [ "$(jq -c .excluded_paths <<<"$rec")" = '[]' ]
  [ "$(jq -r .deny_profile <<<"$rec")" = null ]
}

@test "worktree new reuses an existing branch at its tip" {
  git -C "$SB" branch unit/again
  echo more >>"$SB/core/rust-toolchain.toml"
  git -C "$SB" commit -qam "move main ahead"
  local tip
  tip="$(git -C "$SB" rev-parse unit/again)"
  mg worktree new --role fixer again
  [ "$status" -eq 0 ]
  local wt="$BATS_TEST_TMPDIR/repo-wt/again"
  [ "$(jq -r .base_commit <<<"$(newest_record "$wt")")" = "$tip" ]
  [ "$(git -C "$SB" rev-parse unit/again~1)" = "$tip" ]
  [ ! -e "$wt/oracle" ]
}

@test "worktree new rejects a bad role, a bad name or an existing path, and records nothing" {
  local before role
  before="$(records_of "$SB")"
  mg worktree new --role critic c1
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-role ]
  # Compound or padded values are not roles: each would skip the sparse
  # checkout and the deny profile.
  for role in "implementer fixer" "fixer spec-writer" " implementer" "implementer " ""; do
    mg worktree new --role "$role" c2
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = bad-role ]
    nothing_created c2 "$before"
  done
  mg worktree new --role implementer "../escape"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-name ]
  mg worktree new --role implementer $'w\nx'
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-name ]
  mg worktree new --role implementer "$(printf 'n%.0s' $(seq 1 65))"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-name ]
  mkdir -p "$BATS_TEST_TMPDIR/repo-wt/taken"
  mg worktree new --role implementer taken
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = path-exists ]
  mg worktree list
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  [ "$(records_of "$SB")" -eq "$before" ]
  [ -z "$(git -C "$SB" branch --list 'unit/*')" ]
}

@test "session record commits a record for a session without a worktree" {
  local before archive
  before="$(records_of "$SB")"
  archive="$(git -C "$SB" rev-parse HEAD:docs/process/provenance/sessions.jsonl)"
  mg session record --role critic --name review-home --note "packet build/critic/1"
  [ "$status" -eq 0 ]
  validate "$result"
  [ "$(records_of "$SB")" -eq $((before + 1)) ]
  local rec
  rec="$(newest_record "$SB")"
  # Committed on the checkout's branch, that file only, with an infra role;
  # the archive is never appended to.
  [ -z "$(git -C "$SB" status --porcelain)" ]
  [ "$(git -C "$SB" diff --name-only HEAD~1 HEAD)" = "docs/process/provenance/sessions/$(jq -r .session <<<"$rec").json" ]
  [ "$(git -C "$SB" rev-parse HEAD:docs/process/provenance/sessions.jsonl)" = "$archive" ]
  [ "$(trailer "$SB" Role)" = infra ]
  [ "$(trailer "$SB" Session)" = "$(jq -r .session <<<"$rec")" ]
  [ "$(jq -r .role <<<"$rec")" = critic ]
  [ "$(jq -r .kind <<<"$rec")" = session ]
  [ "$(jq -r .note <<<"$rec")" = "packet build/critic/1" ]
  [ "$(jq -r .role_doc_blob <<<"$rec")" = "$(git -C "$SB" rev-parse HEAD:docs/process/roles/critic.md)" ]
  [ "$(field .details.session.session)" = "$(jq -r .session <<<"$rec")" ]
  local role head
  head="$(git -C "$SB" rev-parse HEAD)"
  for role in wizard "critic integrator" "implementer fixer" " critic"; do
    mg session record --role "$role"
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = bad-role ]
  done
  local name
  for name in "../up" "a/b" " lead" "tab	name" $'two\nlines' "$(printf 'n%.0s' $(seq 1 81))"; do
    mg session record --role critic --name "$name"
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = bad-name ]
  done
  [ "$(records_of "$SB")" -eq $((before + 1)) ]
  [ "$(git -C "$SB" rev-parse HEAD)" = "$head" ]
}

@test "concurrent session records in one checkout all pass, each in its own commit, and leave a clean tree" {
  local n=8 i head0 before pids=()
  head0="$(git -C "$SB" rev-parse HEAD)"
  before="$(records_of "$SB")"
  for i in $(seq 1 "$n"); do
    "$SB/tools/mg" session record --role critic --name "check-$i" >"$BATS_TEST_TMPDIR/out.$i" 2>"$BATS_TEST_TMPDIR/err.$i" &
    pids[${#pids[@]}]=$!
  done
  for i in "${pids[@]}"; do
    wait "$i" || { echo "a run exited $?"; cat "$BATS_TEST_TMPDIR"/err.*; return 1; }
  done
  for i in $(seq 1 "$n"); do
    result="$(tail -n 1 "$BATS_TEST_TMPDIR/out.$i")"
    [ "$(field .status)" = pass ] || { field .failures; return 1; }
  done
  [ "$(records_of "$SB")" -eq $((before + n)) ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
  [ "$(git -C "$SB" rev-list --count "$head0..HEAD")" -eq "$n" ]
  # Each commit adds one record and names that record's session.
  local c id
  for c in $(git -C "$SB" rev-list "$head0..HEAD"); do
    id="$(trailer "$SB" Session "$c")"
    [ "$(git -C "$SB" diff --name-status "$c~1" "$c")" = "$(printf 'A\tdocs/process/provenance/sessions/%s.json' "$id")" ]
    [ "$(record_at "$SB" "$id" "$c" | jq -r .session)" = "$id" ]
  done
  [ ! -e "$SB/.git/meguri-session.lock" ]
}

@test "session record waits for the ledger lock and gives up after its bound" {
  local lock="$SB/.git/meguri-session.lock" pid head0
  head0="$(git -C "$SB" rev-parse HEAD)"
  mkdir "$lock"
  "$SB/tools/mg" session record --role critic >"$BATS_TEST_TMPDIR/out" 2>/dev/null &
  pid=$!
  sleep 2
  [ "$(git -C "$SB" rev-parse HEAD)" = "$head0" ]
  rmdir "$lock"
  wait "$pid"
  result="$(tail -n 1 "$BATS_TEST_TMPDIR/out")"
  [ "$(field .status)" = pass ]
  [ "$(git -C "$SB" rev-list --count "$head0..HEAD")" -eq 1 ]

  mkdir "$lock"
  MG_SESSION_LOCK_SECONDS=1 mg session record --role critic
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = session-lock ]
  [[ "$(field '.failures[0].message')" == *"$lock"* ]]
  validate "$result"
  [ -d "$lock" ]
  [ "$(git -C "$SB" rev-list --count "$head0..HEAD")" -eq 1 ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
}

@test "session record keeps other staged changes out of its commit" {
  mkdir -p "$SB/core/x"
  echo "fn x() {}" >"$SB/core/x/lib.rs"
  git -C "$SB" add core/x/lib.rs
  mg session record --role spec-writer
  [ "$status" -eq 0 ]
  [ "$(git -C "$SB" diff --name-only HEAD~1 HEAD)" = "docs/process/provenance/sessions/$(field .details.session.session).json" ]
  [ "$(git -C "$SB" diff --cached --name-only)" = core/x/lib.rs ]
}

@test "a ledger that cannot be written or committed is an error, and worktree new leaves nothing behind" {
  # The record directory is a file in the checkout: session record cannot write.
  rm -rf "$RECORDS"
  echo "not a directory" >"$RECORDS"
  mg session record --role critic
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = session-ledger ]
  validate "$result"

  # ... and at the base commit of a new worktree.
  git -C "$SB" add -A
  git -C "$SB" commit -qm "Break the ledger"
  mg worktree new --role implementer w4
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = session-ledger ]
  validate "$result"
  [ ! -e "$BATS_TEST_TMPDIR/repo-wt/w4" ]
  [ -z "$(git -C "$SB" branch --list unit/w4)" ]
  [ "$(git -C "$SB" worktree list | wc -l | tr -d ' ')" -eq 1 ]
}

@test "worktree new removes the worktree when its record commit fails" {
  # A hook tools/mg does not install (it owns commit-msg and pre-commit).
  printf '#!/bin/sh\necho "prepare-commit-msg: refused" >&2\nexit 1\n' >"$SB/.git/hooks/prepare-commit-msg"
  chmod +x "$SB/.git/hooks/prepare-commit-msg"
  mg worktree new --role implementer w5
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = session-commit ]
  validate "$result"
  nothing_created w5 "$(records_of "$SB")"
  # An existing branch keeps its tip.
  git -C "$SB" branch unit/w6
  local tip
  tip="$(git -C "$SB" rev-parse unit/w6)"
  mg worktree new --role implementer w6
  [ "$status" -eq 2 ]
  [ "$(git -C "$SB" rev-parse unit/w6)" = "$tip" ]
  [ ! -e "$BATS_TEST_TMPDIR/repo-wt/w6" ]
}

@test "worktree new removes the worktree when its role marker cannot be written" {
  # A post-checkout hook puts a directory where the marker goes.
  printf '#!/bin/sh\nmkdir -p "$(git rev-parse --absolute-git-dir)/meguri-session.json"\n' >"$SB/.git/hooks/post-checkout"
  chmod +x "$SB/.git/hooks/post-checkout"
  mg worktree new --role implementer w7
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = session-marker ]
  validate "$result"
  nothing_created w7 "$(records_of "$SB")"
}

@test "two branches that each record a session merge without a conflict, with no merge driver" {
  local a b
  git -C "$SB" checkout -qb a
  mg session record --role critic --name side-a
  a="$(field .details.session.session)"
  git -C "$SB" checkout -q -
  git -C "$SB" checkout -qb b
  mg session record --role critic --name side-b
  b="$(field .details.session.session)"
  # The records have no merge attribute: plain merges, as GitHub makes them.
  [ "$(git -C "$SB" check-attr merge -- "docs/process/provenance/sessions/$a.json" | sed 's/.*: //')" = unspecified ]
  git -C "$SB" merge -q --no-edit a
  [ "$(jq -r .session "$RECORDS/$a.json")" = "$a" ]
  [ "$(jq -r .session "$RECORDS/$b.json")" = "$b" ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
  # git merge-tree, the server-side merge, finds no conflict either.
  git -C "$SB" checkout -q -b c HEAD~2
  mg session record --role critic --name side-c
  git -C "$SB" merge-tree --write-tree b c >/dev/null
}

@test "the archive still merges lines that branches made before the switch append" {
  git -C "$SB" checkout -qb a
  echo '{"session":"s-a"}' >>"$SESSIONS"
  git -C "$SB" commit -qam a
  git -C "$SB" checkout -q -
  git -C "$SB" checkout -qb b
  echo '{"session":"s-b"}' >>"$SESSIONS"
  git -C "$SB" commit -qam b
  git -C "$SB" merge -q --no-edit a
  grep -q '"s-a"' "$SESSIONS"
  grep -q '"s-b"' "$SESSIONS"
}

@test "a record the private-token scan refuses is not recorded, and the message never names the word" {
  local head0 before
  mkdir -p "$SB/build/private/tokens"
  printf 'quokkasprung\n' >"$SB/build/private/tokens/tokens.txt"
  mg hooks install
  [ "$status" -eq 0 ]
  head0="$(git -C "$SB" rev-parse HEAD)"
  before="$(records_of "$SB")"
  mg session record --role critic --name "quokkasprung check"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = session-private ]
  [[ "$(field '.failures[0].message')" == *"another name"* ]]
  [[ "$(field '[.failures[].message] | join(" ")')" != *quokkasprung* ]]
  validate "$result"
  [ "$(git -C "$SB" rev-parse HEAD)" = "$head0" ]
  [ "$(records_of "$SB")" -eq "$before" ]
  [ -z "$(git -C "$SB" status --porcelain)" ]
  [ ! -e "$SB/.git/meguri-session.lock" ]
  # The note counts too; another name and note record the session.
  mg session record --role critic --name check --note "about quokkasprung"
  [ "$(field '.failures[0].id')" = session-private ]
  mg session record --role critic --name check --note "about the home screen"
  [ "$status" -eq 0 ]
  [ "$(records_of "$SB")" -eq $((before + 1)) ]
  # A worktree whose name the scan refuses is removed again.
  mg worktree new --role implementer quokkasprung
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = session-private ]
  nothing_created quokkasprung "$((before + 1))"
}

# A record file written and committed by hand (sessions.py checks content,
# not who wrote it).
# hand_record <checkout> <id> <created_at> [jq edit]: commits a record.
hand_record() {
  local dir="$1" id="$2" at="$3" edit="${4:-.}"
  mkdir -p "$dir/docs/process/provenance/sessions"
  jq -nc --arg id "$id" --arg at "$at" --arg base "$(git -C "$dir" rev-parse HEAD)" \
    --arg blob "$(git -C "$dir" rev-parse HEAD:docs/process/roles/critic.md)" \
    '{session: $id, role: "critic", kind: "session", name: "hand", base_commit: $base,
      role_doc: "docs/process/roles/critic.md", role_doc_blob: $blob, created_at: $at}' |
    jq -c "$edit" >"$dir/docs/process/provenance/sessions/$id.json"
  git -C "$dir" add "docs/process/provenance/sessions/$id.json"
  git -C "$dir" commit -qm "Record session $id (critic, hand)" -m "Role: infra
Session: $id"
}

# archive_line <checkout> <id> <created_at>: appends an old-style record.
archive_line() {
  jq -nc --arg id "$2" --arg at "$3" '{session: $id, role: "critic", kind: "session", name: "old", created_at: $at}' \
    >>"$1/docs/process/provenance/sessions.jsonl"
  git -C "$1" commit -qm "Record session $2 (critic, old)" -m "Role: infra
Session: $2" -- docs/process/provenance/sessions.jsonl
}

sessions_check() { mg static-checks --check sessions --base "$BASE" "$@"; }

@test "the sessions check passes records tools/mg makes and resolves Session: trailers in the archive and the directory" {
  BASE="$(git -C "$SB" rev-parse HEAD)"
  mg session record --role critic --name one
  local dir_id archive_id
  dir_id="$(field .details.session.session)"
  archive_id="$(head -n 1 "$SESSIONS" | jq -r .session)"
  [ -n "$archive_id" ]
  mg worktree new --role implementer w8 --base HEAD
  sessions_check
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  validate "$result"
  MG_AT="$BATS_TEST_TMPDIR/repo-wt/w8" sessions_check
  [ "$status" -eq 0 ]
  # Trailers naming an archived record and a directory record resolve.
  echo one >"$SB/docs/notes.md"
  git -C "$SB" add docs/notes.md
  git -C "$SB" commit -qm "Notes" -m "Role: infra
Session: $archive_id
Session: $dir_id"
  sessions_check
  [ "$status" -eq 0 ]
  # One that names no record does not.
  echo two >>"$SB/docs/notes.md"
  git -C "$SB" commit -qam "More notes" -m "Role: infra
Session: s-20991231T000000Z-00000000"
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[0].message')" == *"Session: s-20991231T000000Z-00000000 names no session record"* ]]
  validate "$result"
  # A record the base branch made after the branch left it resolves too.
  git -C "$SB" checkout -q -b later "$BASE"
  hand_record "$SB" s-20991231T000000Z-00000000 2026-11-21T00:00:00Z
  BASE="$(git -C "$SB" rev-parse HEAD)"
  git -C "$SB" checkout -q -
  sessions_check
  [ "$status" -eq 0 ]
}

@test "the sessions check refuses a record that is modified, deleted, renamed or committed with other changes" {
  BASE="$(git -C "$SB" rev-parse HEAD)"
  hand_record "$SB" s-20261201T000000Z-0000000a 2026-12-01T00:00:00Z
  sessions_check
  [ "$status" -eq 0 ]
  local rec="docs/process/provenance/sessions/s-20261201T000000Z-0000000a.json"
  jq -c '.name = "edited"' "$SB/$rec" >"$BATS_TEST_TMPDIR/r" && cp "$BATS_TEST_TMPDIR/r" "$SB/$rec"
  git -C "$SB" commit -qam "Edit a record"
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"modifies $rec; session records are only ever added"* ]]
  git -C "$SB" reset -q --hard HEAD~1
  git -C "$SB" rm -q "$rec"
  git -C "$SB" commit -qm "Drop a record"
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"deletes $rec"* ]]
  git -C "$SB" reset -q --hard HEAD~1
  git -C "$SB" mv "$rec" "docs/process/provenance/sessions/s-20261201T000000Z-0000000b.json"
  git -C "$SB" commit -qm "Rename a record"
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"deletes $rec"* ]]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"holds session"* ]]
  git -C "$SB" reset -q --hard HEAD~1
  # A record added with another file, or without its trailers.
  mkdir -p "$SB/docs/process/provenance/sessions"
  printf '{"session":"s-20261201T000000Z-0000000c","role":"critic","kind":"session","created_at":"2026-12-01T00:00:00Z","base_commit":"%s","role_doc":"docs/process/roles/critic.md","role_doc_blob":"%s"}\n' \
    "$(git -C "$SB" rev-parse HEAD)" "$(git -C "$SB" rev-parse HEAD:docs/process/roles/critic.md)" \
    >"$SB/docs/process/provenance/sessions/s-20261201T000000Z-0000000c.json"
  echo x >"$SB/docs/other.md"
  git -C "$SB" add -A docs
  git -C "$SB" commit -qm "Record and more"
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"together with other changes"* ]]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"without the trailers Role: infra and Session: s-20261201T000000Z-0000000c"* ]]
  git -C "$SB" reset -q --hard HEAD~1
  # Fields: a role outside the list, a role_doc that is not the role's.
  hand_record "$SB" s-20261201T000000Z-0000000d 2026-12-01T00:00:00Z '.role = "wizard"'
  hand_record "$SB" s-20261201T000000Z-0000000e 2026-12-01T00:00:00Z '.role = "implementer"'
  hand_record "$SB" s-20261201T000000Z-0000000f 2026-12-01T00:00:00Z '.base_commit = "0000000000000000000000000000000000000000"'
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *'has role "wizard"'* ]]
  [[ "$(field '[.failures[].message] | join(" ")')" == *'0000000e.json has role_doc'* ]]
  [[ "$(field '[.failures[].message] | join(" ")')" == *'0000000f.json has base_commit'* ]]
}

@test "the sessions check refuses a merge that drops or rewrites a record" {
  BASE="$(git -C "$SB" rev-parse HEAD)"
  hand_record "$SB" s-20261201T000000Z-00000001 2026-12-01T00:00:00Z
  local main
  main="$(git -C "$SB" rev-parse HEAD)"
  git -C "$SB" checkout -q -b side "$BASE"
  echo side >"$SB/docs/side.md"
  git -C "$SB" add docs/side.md
  git -C "$SB" commit -qm side
  git -C "$SB" merge -q --no-commit "$main"
  git -C "$SB" rm -qf docs/process/provenance/sessions/s-20261201T000000Z-00000001.json
  git -C "$SB" commit -qm "Merge, dropping a record"
  BASE="$main" sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"drops docs/process/provenance/sessions/s-20261201T000000Z-00000001.json"* ]]
  # An archive line removed in a merge fails the same way.
  git -C "$SB" reset -q --hard HEAD~1
  git -C "$SB" merge -q --no-commit "$main"
  sed -i.bak '1d' "$SESSIONS" && rm "$SESSIONS.bak"
  git -C "$SB" add "$SESSIONS"
  git -C "$SB" commit -qm "Merge, dropping an archive line"
  BASE="$main" sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"removes or rewrites 1 line(s) of the archive"* ]]
}

@test "the archive takes records created before the cutoff, from branches made before the switch, and nothing later" {
  local cutoff
  cutoff="$(sed -n 's/^ARCHIVE_CUTOFF = "\(.*\)"$/\1/p' "$SB/tools/gate/static-checks/sessions.py")"
  [ "$cutoff" = 2026-11-27T00:00:00Z ]
  BASE="$(git -C "$SB" rev-parse HEAD)"
  # An old-style record, as tools/mg made it before the switch: accepted,
  # however late the branch merges.
  archive_line "$SB" s-20261126T235959Z-0000000a 2026-11-26T23:59:59Z
  sessions_check
  [ "$status" -eq 0 ]
  local ok
  ok="$(git -C "$SB" rev-parse HEAD)"
  # One created at the cutoff or later is refused, and names the new layout.
  archive_line "$SB" s-20261127T000000Z-0000000b 2026-11-27T00:00:00Z
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[0].message')" == *"only records created before $cutoff"*"docs/process/provenance/sessions/s-20261127T000000Z-0000000b.json"* ]]
  # A back-dated created_at does not hide a later session id.
  git -C "$SB" reset -q --hard "$ok"
  archive_line "$SB" s-20261203T000000Z-0000000c 2026-11-20T00:00:00Z
  sessions_check
  [ "$status" -eq 1 ]
  # Lines are never removed or edited.
  git -C "$SB" reset -q --hard "$ok"
  sed -i.bak '1s/"role":"[a-z-]*"/"role":"integrator"/' "$SESSIONS" && rm "$SESSIONS.bak"
  git -C "$SB" commit -qam "Rewrite an archived record"
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"removes or rewrites 1 line(s) of the archive"* ]]
  # A session recorded in both places is refused.
  git -C "$SB" reset -q --hard "$ok"
  hand_record "$SB" s-20261126T235959Z-0000000a 2026-11-26T23:59:59Z
  sessions_check
  [ "$status" -eq 1 ]
  [[ "$(field '[.failures[].message] | join(" ")')" == *"the archive already records session s-20261126T235959Z-0000000a"* ]]
}

@test "hooks install puts a shim in the shared hooks dir and refuses a foreign hook" {
  mg hooks install
  [ "$status" -eq 0 ]
  local hook="$SB/.git/hooks/commit-msg"
  [ -x "$hook" ]
  grep -q 'tools/hooks/commit-msg' "$hook"
  mg hooks install
  [ "$status" -eq 0 ]
  printf '#!/bin/sh\nexit 0\n' >"$hook"
  mg hooks install
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = hook-exists ]
}

@test "parallel hooks installs publish complete executable shims without temporary leftovers" {
  local stubs="$BATS_TEST_TMPDIR/stubs" outputs="$BATS_TEST_TMPDIR/installs"
  mkdir -p "$stubs" "$outputs"
  export HOOK_BARRIER="$BATS_TEST_TMPDIR/hook-barrier" HOOK_INSTALLERS=4
  export REAL_HOOK_MV="$(command -v mv)"
  mkdir -p "$HOOK_BARRIER"
  # Hold every installer just before its first rename, after all writes and
  # chmods. A shared temporary path can then be renamed successfully only once.
  cat >"$stubs/mv" <<'STUB'
#!/usr/bin/env bash
for last; do :; done
case "$last" in
  */hooks/commit-msg)
    : >"$HOOK_BARRIER/$$"
    hook_deadline=$((SECONDS + 10))
    while [ "$(find "$HOOK_BARRIER" -type f | wc -l)" -lt "$HOOK_INSTALLERS" ]; do
      [ "$SECONDS" -lt "$hook_deadline" ] || exit 98
      sleep 0.02
    done
    ;;
esac
exec "$REAL_HOOK_MV" "$@"
STUB
  chmod +x "$stubs/mv"
  local i pid failed=0 pids=() name
  for ((i = 0; i < HOOK_INSTALLERS; i++)); do
    PATH="$stubs:$PATH" "$SB/tools/mg" hooks install >"$outputs/$i.out" 2>"$outputs/$i.err" &
    pids+=("$!")
  done
  for pid in "${pids[@]}"; do
    wait "$pid" || failed=1
  done
  [ "$failed" -eq 0 ] || { cat "$outputs/"*.err; return 1; }
  for ((i = 0; i < HOOK_INSTALLERS; i++)); do
    result="$(cat "$outputs/$i.out")"
    [ "$(field .status)" = pass ]
    [ "$(field '.failures | length')" -eq 0 ]
    validate "$result"
  done
  for name in commit-msg pre-commit pre-merge-commit; do
    [ -x "$SB/.git/hooks/$name" ]
    grep -qF "# meguri: $name shim installed by tools/mg hooks install" "$SB/.git/hooks/$name"
    grep -qF "tools/hooks/$name" "$SB/.git/hooks/$name"
    [ "$(tail -n 1 "$SB/.git/hooks/$name")" = 'exec "$hook" "$@"' ]
  done
  [ -z "$(find "$SB/.git/hooks" -name '*.tmp*' -print)" ]
}

@test "hooks install reports an error when the hook cannot be written or renamed" {
  local hook="$SB/.git/hooks/commit-msg" stubs="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$stubs"
  # Temporary-file creation fails for this hook only.
  export REAL_HOOK_MKTEMP="$(command -v mktemp)"
  cat >"$stubs/mktemp" <<'STUB'
#!/bin/sh
for last; do :; done
case "$last" in */hooks/commit-msg.tmp.*) exit 1 ;; esac
exec "$REAL_HOOK_MKTEMP" "$@"
STUB
  chmod +x "$stubs/mktemp"
  PATH="$stubs:$PATH" mg hooks install
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = hook-write ]
  validate "$result"
  [ ! -e "$hook" ]
  rm "$stubs/mktemp"

  # chmod fails after the temporary file was written; it must be cleaned up.
  export REAL_HOOK_CHMOD="$(command -v chmod)"
  cat >"$stubs/chmod" <<'STUB'
#!/bin/sh
for last; do :; done
case "$last" in */hooks/commit-msg.tmp.*) exit 1 ;; esac
exec "$REAL_HOOK_CHMOD" "$@"
STUB
  chmod +x "$stubs/chmod"
  PATH="$stubs:$PATH" mg hooks install
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = hook-write ]
  validate "$result"
  [ ! -e "$hook" ]
  [ -z "$(find "$SB/.git/hooks" -name '*.tmp*' -print)" ]
  rm "$stubs/chmod"

  # mv fails for the hook only.
  printf '#!/bin/sh\nfor last; do :; done\ncase "$last" in */hooks/commit-msg) echo "mv: injected failure" >&2; exit 1 ;; esac\nexec %s "$@"\n' \
    "$(command -v mv)" >"$stubs/mv"
  chmod +x "$stubs/mv"
  PATH="$stubs:$PATH" mg hooks install
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = hook-write ]
  validate "$result"
  [ ! -e "$hook" ]
  [ -z "$(find "$SB/.git/hooks" -name '*.tmp*' -print)" ]

  # worktree new still creates the worktree, but warns.
  PATH="$stubs:$PATH" mg worktree new --role implementer w8
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field '.failures[0].id')" = hook-write ]
  validate "$result"
  [ -d "$BATS_TEST_TMPDIR/repo-wt/w8" ]
  [ -z "$(find "$SB/.git/hooks" -name '*.tmp*' -print)" ]
}

@test "worktree new installs the hook: a fresh implementer worktree rejects a core/ commit without Spec" {
  mg worktree new --role implementer w9
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  WT="$BATS_TEST_TMPDIR/repo-wt/w9"
  local id
  id="$(jq -r .session "$(git -C "$WT" rev-parse --absolute-git-dir)/meguri-session.json")"
  commit_with $'Add parser\n\nRole: implementer\nSession: '"$id" core/crates/meguri-search/src/parse.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"need a Spec: trailer"* ]]
  commit_with $'Add parser\n\nRole: implementer\nSpec: search/grammar#G1\nSession: '"$id" core/crates/meguri-search/src/parse.rs
  [ "$status" -eq 0 ]
}

@test "commit-msg: implementer commits on core/ need Spec and Session; one-line reasons" {
  mg hooks install
  WT="$SB"
  commit_with $'Add the grammar\n\nRole: implementer\nSession: s-20261013T000000Z-0a1b2c3d' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [ "${#stderr_lines[@]}" -eq 1 ]
  [[ "$stderr" == *"need a Spec: trailer"* ]]

  commit_with $'Add the grammar\n\nRole: implementer\nSpec: none\nSession: s-20261013T000000Z-0a1b2c3d' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"is not a spec id"* ]]

  commit_with $'Add the grammar\n\nRole: implementer\nSpec: search/grammar#G3' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [ "${#stderr_lines[@]}" -eq 1 ]
  [[ "$stderr" == *"need a Session: trailer"* ]]

  commit_with $'Add the grammar\n\nSpec: search/grammar#G3' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"need a Role: trailer"* ]]
  # Names git would quote are code paths too.
  commit_with $'Add the grammar\n\nSpec: search/grammar#G3' $'core/crates/meguri-search/src/gr\303\244mmar.rs'
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"need a Role: trailer"* ]]
  git -C "$SB" reset -q

  commit_with $'Add the grammar\n\nRole: implementor\nSpec: search/grammar#G3' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"unknown Role"* ]]

  # A compound value is not a role: it must not skip the Spec and Session checks.
  commit_with $'Add the grammar\n\nRole: implementer fixer' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"unknown Role: 'implementer fixer'"* ]]
  commit_with $'Add the grammar\n\nRole: fixer spec-writer' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"unknown Role"* ]]

  commit_with $'Add the grammar\n\nRole: implementer\nSpec: search/grammar#G3\nSession: s-20261013T000000Z-0a1b2c3d' core/crates/meguri-search/src/lib.rs
  [ "$status" -eq 0 ]
  [ "$(git -C "$SB" log -1 --format=%s)" = "Add the grammar" ]
}

@test "commit-msg: implementer renames out of core/ need Spec and Session" {
  mg hooks install
  [ "$status" -eq 0 ]
  WT="$SB"
  git -C "$SB" config diff.renames true
  commit_with $'Add demo\n\nRole: infra' core/demo.rs
  [ "$status" -eq 0 ]
  git -C "$SB" mv core/demo.rs tools/demo.rs
  [ "$(git -C "$SB" diff --cached --name-status)" = $'R100\tcore/demo.rs\ttools/demo.rs' ]

  commit_with $'Move demo\n\nRole: implementer'
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"need a Spec: trailer"* ]]
  commit_with $'Move demo\n\nRole: implementer\nSpec: search/grammar#G3'
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"need a Session: trailer"* ]]
  commit_with $'Move demo\n\nRole: implementer\nSpec: search/grammar#G3\nSession: s-20261013T000000Z-0a1b2c3d'
  [ "$status" -eq 0 ]
}

@test "commit-msg: Rust and frontend server changes require role, spec and session provenance" {
  mg hooks install
  [ "$status" -eq 0 ]
  WT="$SB"
  local path
  for path in server/src/main.rs server/web/src/Login.tsx; do
    commit_with 'Server source' "$path"
    [ "$status" -ne 0 ]
    [[ "$stderr" == *"need a Role: trailer"* ]]
    commit_with $'Server source\n\nRole: implementer' "$path"
    [ "$status" -ne 0 ]
    [[ "$stderr" == *"need a Spec: trailer"* ]]
    commit_with $'Server source\n\nRole: fixer\nSpec: R4' "$path"
    [ "$status" -ne 0 ]
    [[ "$stderr" == *"need a Session: trailer"* ]]
    commit_with $'Server source\n\nRole: implementer\nSpec: R4, R32\nSession: s-server-tooling' "$path"
    [ "$status" -eq 0 ]
    run bash -c 'cd "$1" && tools/hooks/commit-msg --commit HEAD' _ "$SB"
    [ "$status" -eq 0 ]
  done
  git -C "$WT" mv server/src/main.rs tools/moved.rs
  commit_with $'Move server source\n\nRole: implementer'
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"need a Spec: trailer"* ]]
  git -C "$WT" -c core.hooksPath=/dev/null commit -qm 'Synthetic bypassed server commit'
  run bash -c 'cd "$1" && tools/hooks/commit-msg --commit HEAD' _ "$SB"
  [ "$status" -ne 0 ]
  [[ "$output" == *"need a Role: trailer"* ]]
}

@test "commit-msg: spec paths, several ids, other roles and paths outside the code tree" {
  mg hooks install
  WT="$SB"
  commit_with $'Fix rollover\n\nRole: fixer\nSpec: specs/sched/day-clock.md#D2, sched/day-clock#D3\nSession: 0b7c9f2e-uuid-like' app/Meguri/App/MeguriApp.swift
  [ "$status" -eq 0 ]
  commit_with $'Fix rollover\n\nRole: fixer\nSession: s-1' conformance/runner/src/lib.rs
  [ "$status" -ne 0 ]
  commit_with $'Merge unit\n\nRole: integrator' core/Cargo.toml
  [ "$status" -eq 0 ]
  commit_with $'Write the spec\n\nRole: spec-writer\nSpec: none' specs/search/grammar.md
  [ "$status" -eq 0 ]
  commit_with 'Update docs only' docs/process/notes.md
  [ "$status" -eq 0 ]
  commit_with $'Docs\n\nRole: wizard' docs/process/notes.md
  [ "$status" -ne 0 ]
  commit_with $'Docs\n\nRole: infra critic' docs/process/notes.md
  [ "$status" -ne 0 ]
}

@test "commit-msg: UI implementer commits may cite Product Contract requirement ids" {
  mg hooks install
  WT="$SB"
  commit_with $'Add the deck list\n\nRole: implementer\nSpec: R1987, R150\nSession: s-20261013T000000Z-0a1b2c3d' app/Meguri/Decks/DeckList.swift
  [ "$status" -eq 0 ]
  commit_with $'Add the deck list\n\nRole: implementer\nSpec: R1987, search/grammar#G3\nSession: s-1x' app/Meguri/Decks/DeckList.swift
  [ "$status" -eq 0 ]
  local bad
  for bad in R r150 R15a AE85 R-1; do
    commit_with $'Add the deck list\n\nRole: implementer\nSpec: '"$bad"$'\nSession: s-1x' app/Meguri/Decks/DeckList.swift
    [ "$status" -ne 0 ]
    [[ "$stderr" == *"is not a spec id"* ]]
  done
  # Infra units (U5-U7) touch core/ and app/ with Role: infra and no Spec.
  commit_with $'Add the workspace\n\nRole: infra' core/Cargo.toml
  [ "$status" -eq 0 ]
}

@test "commit-msg: merge commits are not checked" {
  mg hooks install
  git -C "$SB" checkout -qb side
  mkdir -p "$SB/core/a"
  echo x >"$SB/core/a/x.rs"
  git -C "$SB" add core/a/x.rs
  git -C "$SB" -c core.hooksPath=/dev/null commit -qm side
  git -C "$SB" checkout -q -
  echo y >"$SB/core/rust-toolchain.toml.note"
  git -C "$SB" add core/rust-toolchain.toml.note
  git -C "$SB" -c core.hooksPath=/dev/null commit -qm main-side
  run git -C "$SB" merge --no-ff -m "Merge side" side
  [ "$status" -eq 0 ]
}

@test "commit-msg in an implementer worktree names the worktree's session id" {
  mg worktree new --role implementer w2
  mg hooks install
  WT="$BATS_TEST_TMPDIR/repo-wt/w2"
  local id
  id="$(jq -r .session "$(git -C "$WT" rev-parse --absolute-git-dir)/meguri-session.json")"
  commit_with $'Add parser\n\nRole: implementer\nSpec: search/grammar#G1' core/crates/meguri-search/src/parse.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"Session: $id"* ]]
}

@test "similarity jobs refuse to run in an implementer worktree" {
  mg worktree new --role implementer w3
  local job
  for job in similarity similarity-selftest similarity-nightly; do
    MG_AT="$BATS_TEST_TMPDIR/repo-wt/w3" MG_SIMILARITY_REFS="$SB" mg "$job"
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = implementer-worktree ]
    validate "$result"
  done
}

@test "similarity without an Anki corpus is an error naming refs/anki" {
  mg similarity
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = no-corpus ]
  [[ "$(field '.failures[0].message')" == *"refs/anki"* ]]
  validate "$result"
}

@test "similarity rejects unknown arguments" {
  mkdir -p "$SB/refs/anki"
  mg similarity --frobnicate
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
}

@test "similarity search data requires explicit paths and rejects revision modes" {
  mg similarity --search-data
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  mg similarity --search-data --rev HEAD -- conformance/scenarios/search conformance/goldens/search
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  mg similarity --search-data --base HEAD -- conformance/scenarios/search conformance/goldens/search
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
}

@test "similarity search data keeps the implementer guard" {
  mg worktree new --role implementer search-data
  MG_AT="$BATS_TEST_TMPDIR/repo-wt/search-data" mg similarity --search-data -- conformance/scenarios/search conformance/goldens/search
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = implementer-worktree ]
}

@test "similarity search data forwards the mode and retains coverage in the receipt" {
  mkdir -p "$SB/refs/anki"
  export FX_SEARCH_ARGS="$BATS_TEST_TMPDIR/search-args"
  cat >"$SB/tools/mg.d/zz-search-data-fake.sh" <<'EOF'
mg__similarity_run() {
  shift
  printf '%s\n' "$@" >"$FX_SEARCH_ARGS"
  local out="$MG_RUN_DIR/scan" file
  mkdir -p "$out"
  for file in implementer.md implementer.json reviewer.md reviewer.json; do
    echo '{}' >"$out/$file"
  done
  cat >"$out/summary.json" <<'JSON'
{"status":"warn","counts":{"scanned":2,"flagged":0,"allowlisted":0,"clean":2,"errors":0,"skipped":0,"missing":0},"corpus":{"tags":2,"first_tag":"a","last_tag":"b"},"timing":{},"flags":[],"errors":[],"missing":[],"search_data":{"pairs":1,"manifest":[{"path":"conformance/scenarios/search/a.yaml","sha256":"synthetic"}],"coverage":[{"path":"conformance/scenarios/search/a.yaml","fingerprints":{"lex":0,"com":0,"text":0},"meaningful":false}],"limited":["conformance/scenarios/search/a.yaml"]}}
JSON
}
EOF
  mg similarity --search-data -- conformance/scenarios/search conformance/goldens/search
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  grep -Fx -- '--search-data' "$FX_SEARCH_ARGS"
  [ "$(field '.details.search_data.pairs')" -eq 1 ]
  [ "$(field '.details.search_data.limited | length')" -eq 1 ]
  validate "$result"
}

@test "the implementer deny profile template covers every quarantined source" {
  local p="$SB/.claude/settings.implementer.json" rule
  # Built from parts: the merge gate's static check refuses an AnkiWeb host
  # written out in a test (R1876, tools/gate/static-checks/ankiweb-hosts.sh).
  local ankiweb_domain
  ankiweb_domain="$(printf '%s.%s' ankiweb net)"
  jq -e '.permissions.deny | length > 0' "$p"
  for rule in \
    'Read(/@MAIN_CHECKOUT@/refs/**)' \
    'Read(/@MAIN_CHECKOUT@/local-data/**)' \
    'Read(/@MAIN_CHECKOUT@/oracle/**)' \
    'Read(/@MAIN_CHECKOUT@/build/oracle-macos-venv/**)' \
    'Read(/@WORKTREES@/*/oracle/**)' \
    'Bash(*git *show *oracle*)' \
    'Bash(*git *log *-p*oracle*)' \
    'Bash(*git *diff*oracle*)' \
    'Bash(*git *blame *oracle*)' \
    'Bash(*git *cat-file*)' \
    'Bash(*docker *exec *)' \
    'Bash(*docker *cp *)' \
    'Bash(*docker *run *)' \
    'Bash(*docker *build *)' \
    'Bash(*docker container *)' \
    'Bash(*docker-compose *)' \
    'Bash(*@MAIN_CHECKOUT@/build/mg/similarity*)' \
    'Bash(*@MAIN_CHECKOUT@/build/similarity-index*)' \
    'Bash(*scan/reviewer.*)' \
    'Bash(*.jplag*)' \
    'Bash(*github.com/ankitects*)' \
    'Bash(*git.foosoft.net*)' \
    'Bash(*@MAIN_CHECKOUT@/oracle*)' \
    'Bash(*@WORKTREES@/*/oracle/*)' \
    'Bash(*@WORKTREES@/*/oracle)' \
    'Bash(*@MAIN_CHECKOUT@/refs*)' \
    'Bash(*Application Support/Anki2*)' \
    'Bash(*github.com/migaku*)' \
    'Bash(*githubusercontent.com/migaku*)' \
    'Read(/@MAIN_CHECKOUT@/build/mg/similarity/**/tree/**)' \
    'WebFetch(domain:github.com)' \
    'WebFetch(domain:raw.githubusercontent.com)' \
    'WebFetch(domain:codeload.github.com)' \
    "WebFetch(domain:$ankiweb_domain)"; do
    jq -e --arg r "$rule" '.permissions.deny | index($r)' "$p" >/dev/null || { echo "missing deny rule $rule"; return 1; }
  done
  jq -e '.hooks.PreToolUse[] | select(.matcher == "WebFetch") | .hooks[0].command == "\"@WORKTREE@/tools/hooks/deny-quarantined-fetch\""' "$p"
}

# denied_by <settings file> <command>: prints the first Bash deny rule that
# matches the command and succeeds, or fails. Matching as Claude Code does it:
# `*` matches any characters, spaces included, anything else matches itself,
# and the rule covers the whole command.
denied_by() {
  python3 - "$1" "$2" <<'PY'
import json, re, sys
for rule in json.load(open(sys.argv[1]))["permissions"]["deny"]:
    if rule.startswith("Bash(") and rule.endswith(")"):
        if re.fullmatch("".join(".*" if c == "*" else re.escape(c) for c in rule[5:-1]), sys.argv[2], re.S):
            print(rule)
            sys.exit(0)
sys.exit(1)
PY
}

@test "rendered Bash deny rules match quarantine commands in any argument order, and spare everyday ones" {
  mg worktree new --role implementer w1
  [ "$status" -eq 0 ]
  local main wt wts s cmd
  main="$(cd "$SB" && pwd -P)"
  wt="$(cd "$BATS_TEST_TMPDIR/repo-wt/w1" && pwd -P)"
  wts="$(dirname "$wt")"
  s="$wt/.claude/settings.local.json"
  while IFS= read -r cmd; do
    [ -n "$cmd" ] || continue
    denied_by "$s" "$cmd" >/dev/null || { echo "not denied: $cmd"; return 1; }
  done <<EOF
docker compose -f docker/compose.yml exec oracle sh
docker compose -p meguri-x -f $main/docker/compose.yml exec -T oracle cat /opt/x.py
docker compose -f docker/compose.yml run --rm oracle bash
docker compose -f docker/compose.yml run --rm --entrypoint sh oracle
docker compose -f docker/compose.yml cp oracle:/opt/x .
docker-compose exec oracle sh
docker run --rm -it meguri-oracle sh
docker run --rm --entrypoint cat meguri-oracle /opt/x.py
docker --context default exec -it c sh
docker container exec c sh
docker container cp c:/opt/x .
docker cp c:/opt/x .
docker image save meguri-oracle -o o.tar
docker save meguri-oracle
docker export c
docker buildx build -f docker/oracle.Dockerfile --output type=local,dest=out .
docker build -o out -f docker/oracle.Dockerfile .
docker debug c
env DOCKER_HOST=unix:///x docker run --rm meguri-oracle cat /opt/x.py
cd docker && docker compose run --rm oracle sh
git show HEAD:oracle/driver/run.py
git --no-pager show HEAD:oracle/driver/run.py
git -C $main show HEAD:oracle/driver/run.py
git -C $wt show HEAD -- oracle/
git -c core.pager=cat log -p -- oracle/driver
git log --patch --follow oracle/driver/run.py
git --no-pager diff HEAD~3 -- oracle
git diff-tree -p HEAD -- oracle/
git blame -- oracle/driver/run.py
git -C $wt cat-file -p HEAD:oracle/driver/run.py
git cat-file --batch
/usr/bin/git show HEAD:oracle/driver/run.py
unzip -p $main/build/mg/similarity-nightly/20261013T000000Z-1/jplag/rust.jplag submissions/x
cat $main/build/mg/similarity/20261013T000000Z-1/scan/reviewer.json
jq . $main/build/mg/similarity/latest/scan/reviewer.md
cat similarity/latest/scan/reviewer.md
ls $main/build/similarity-index/
python3 -c 'import zipfile; print(zipfile.ZipFile("rust.jplag").namelist())'
cat $main/refs/anki/rslib/src/lib.rs
ls $main/oracle/driver
cat $wts/U4/oracle/driver/run.py
curl -sL https://raw.githubusercontent.com/ankitects/anki/main/rslib/src/lib.rs
cat ~/Library/Application\\ Support/Anki2/addons21/x/__init__.py
pip download anki==25.9
pip install anki
pip3 download anki
pip3 install anki
python3 -m pip download anki
python -m pip install anki
python3 -m pip install --upgrade anki
pip install --upgrade anki
pip download --no-deps anki==25.9
pip3 install --upgrade anki
pip3 --disable-pip-version-check download --no-deps anki
python3 -m pip --disable-pip-version-check download --no-deps anki
env PIP_NO_CACHE_DIR=1 pip install --upgrade anki
cd /tmp && python3 -m pip download --no-deps anki
/usr/bin/pip3 install --upgrade anki
pip wheel --no-deps anki
python3 -m pip wheel --no-deps -w out anki
uv pip install anki
pip install aqt[qt6]
pip install --upgrade aqt[qt6]
pip download aqt==25.9
python3 -m pip install aqt
pip install Anki
pip download ANKI==25.9
uv add anki
uv tool install aqt
uvx --from aqt anki
pipx run --spec aqt anki
EOF
  while IFS= read -r cmd; do
    [ -n "$cmd" ] || continue
    if denied_by "$s" "$cmd"; then echo "denied: $cmd"; return 1; fi
  done <<EOF
tools/mg core-test
tools/mg oracle compare sched
git status
git log --oneline -5
git log -p -- core/crates/meguri-search/src/lib.rs
git diff
git diff --stat HEAD~1
git show --stat HEAD
git add core/crates/meguri-search/src/lib.rs
git commit -F /tmp/msg.txt
docker ps
docker compose ps
docker images
cat $wt/build/mg/core-test/latest/result.json
jq . $main/build/mg/core-test/latest/result.json
rg -n oracle specs/
pip download requests
pip install --upgrade requests
pip3 download --no-deps requests
pip3 install --upgrade requests
python3 -m pip download requests
python3 -m pip install --upgrade requests
env PIP_NO_CACHE_DIR=1 pip install --upgrade requests
python3 -m pip --version
EOF
}

@test "the WebFetch hook blocks Anki, AnkiDroid, AnkiConnect and Migaku sources on any host" {
  local hook="$SB/tools/hooks/deny-quarantined-fetch" url
  for url in \
    https://github.com/ankitects/anki/blob/main/rslib/src/lib.rs \
    https://cdn.jsdelivr.net/gh/ankitects/anki@26.09.3/rslib/src/lib.rs \
    https://sourcegraph.com/github.com/AnkiTects/anki \
    https://github.com/ankidroid/Anki-Android \
    https://git.foosoft.net/alex/anki-connect \
    https://github.com/FooSoft/anki-connect/blob/master/plugin/__init__.py \
    https://github.com/migaku-official/Migaku-Anki; do
    run --separate-stderr "$hook" <<<"$(jq -nc --arg u "$url" '{tool_name: "WebFetch", tool_input: {url: $u, prompt: "read it"}}')"
    [ "$status" -eq 2 ] || { echo "not blocked: $url"; return 1; }
    [[ "$stderr" == *"implementer deny profile"* ]]
  done
  for url in https://docs.rs/fsrs/latest/fsrs/ https://static.crates.io/crates/fsrs/fsrs-6.6.2.crate https://developer.apple.com/documentation/swiftui; do
    run "$hook" <<<"$(jq -nc --arg u "$url" '{tool_name: "WebFetch", tool_input: {url: $u, prompt: "how do I compare with ankitects?"}}')"
    [ "$status" -eq 0 ] || { echo "blocked: $url"; return 1; }
  done
  # Input it cannot parse is matched as a whole (fails closed).
  run "$hook" <<<'not json https://github.com/ankitects/anki'
  [ "$status" -eq 2 ]
}

# make_docker_stub: a `docker` that stands in for the similarity service. It
# records what the job hands the scanner (the NUL-separated file list, whether
# --null was passed, the files under --root while the scan runs) in $STUB_OUT
# and writes the summary a scan with $STUB_STATUS and $STUB_MISSING would, or
# the scan/nightly error summary for $STUB_ERROR.
make_docker_stub() {
  local stubs="$BATS_TEST_TMPDIR/docker-stubs"
  mkdir -p "$stubs"
  export STUB_OUT="$BATS_TEST_TMPDIR/stub-out"
  cat >"$stubs/docker" <<'STUB'
#!/usr/bin/env bash
case "$1 $*" in
  "info "*) exit 0 ;;
  compose*" build "*) exit 0 ;;
  compose*" run "*) ;;
  *) exit 97 ;;
esac
args=("$@") root="" files="" out="" null="" compose="" mode=scan i=0
while [ "$i" -lt "${#args[@]}" ]; do
  case "${args[$i]}" in
    -f) compose="${args[$((i + 1))]}" ;;
    --root) root="${args[$((i + 1))]}" ;;
    --files-from) files="${args[$((i + 1))]}" ;;
    --out) out="${args[$((i + 1))]}" ;;
    --null) null=yes ;;
    jplag) mode=jplag ;;
  esac
  i=$((i + 1))
done
host="$(dirname "$(dirname "$compose")")"
root="$host${root#/work}" files="$host${files#/work}" out="$host${out#/work}"
rm -rf "$STUB_OUT"
mkdir -p "$STUB_OUT" "$out"
if [ "$mode" = jplag ]; then
  jq -n --arg e "$STUB_ERROR" '{status: "error", error: $e,
    tags: ["synthetic-selected"], missing_tags: ["synthetic-missing"], languages: {}}' >"$out/jplag-summary.json"
  exit 2
fi
cp "$files" "$STUB_OUT/files.lst"
echo "$null" >"$STUB_OUT/null"
(cd "$root" && find . -type f -print0 | sed -z 's|^\./||') >"$STUB_OUT/tree.lst"
if [ -n "${STUB_ERROR:-}" ]; then
  jq -n --arg e "$STUB_ERROR" '{status: "error", error: $e}' >"$out/summary.json"
  exit 2
fi
jq -n --arg st "${STUB_STATUS:-pass}" --argjson missing "${STUB_MISSING:-[]}" '{
  status: $st, flags: [], errors: [], missing: $missing, timing: {}, corpus: {tags: 54, first_tag: "2.1.50", last_tag: "26.09.3"},
  counts: {scanned: 1, clean: 1, allowlisted: 0, flagged: 0, errors: 0, skipped: 0, missing: ($missing | length)}}' >"$out/summary.json"
STUB
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
}

# same_paths <NUL list file> <path>...: the list holds exactly these paths.
same_paths() {
  local list="$1"
  shift
  cmp <(sort -z "$list") <(printf '%s\0' "$@" | sort -z)
}

AWKWARD=($'core/f\303\251.rs' $'core/tab\there.rs' $'core/new\nline.rs' 'core/plain.rs')

# Tracked paths of a tree that git lists unquoted, without the plain AWKWARD one.
plain_paths() { git -C "$SB" ls-tree -r --name-only "${1:-HEAD}" | grep -v '^"' | grep -vx core/plain.rs; }

commit_awkward_names() {
  local n
  for n in "${AWKWARD[@]}"; do
    mkdir -p "$SB/$(dirname "$n")"
    echo "pub fn f() -> u8 { 1 }" >"$SB/$n"
  done
  mkdir -p "$SB/specs/storage"
  echo cards >"$SB/specs/storage/interop-vocab.txt"
  git -C "$SB" add -A
  git -C "$SB" commit -qm "Awkward names"
}

@test "similarity --rev materializes no excluded path, oracle/ included, and removes its tree" {
  make_docker_stub
  mkdir -p "$SB/refs/anki"
  commit_awkward_names
  mg similarity --rev HEAD
  [ "$status" -eq 0 ]
  validate "$result"
  [ "$(cat "$STUB_OUT/null")" = yes ]
  # The scanner got every file of the commit, and a tree without oracle/,
  # docs/ or specs/ (but with the interop vocabulary).
  same_paths "$STUB_OUT/files.lst" $(plain_paths) "${AWKWARD[@]}"
  run ! grep -qz '^oracle/' "$STUB_OUT/tree.lst"
  run ! grep -qz '^docs/' "$STUB_OUT/tree.lst"
  grep -qzx 'specs/storage/interop-vocab.txt' "$STUB_OUT/tree.lst"
  grep -qzx 'tools/mg' "$STUB_OUT/tree.lst"
  cmp <(grep -z '^core/' "$STUB_OUT/tree.lst" | sort -z) <(git -C "$SB" ls-tree -r -z --name-only HEAD -- core | sort -z)
  # Nothing is left in the run directory but the reports.
  [ ! -e "$(dirname "$result")/tree" ]
  [ ! -e "$(dirname "$result")/rev-index" ]
}

@test "similarity --base and --rev pass names with non-ASCII, tab and newline characters intact" {
  make_docker_stub
  mkdir -p "$SB/refs/anki"
  commit_awkward_names
  mg similarity --rev HEAD --base HEAD~1
  [ "$status" -eq 0 ]
  same_paths "$STUB_OUT/files.lst" "${AWKWARD[@]}" specs/storage/interop-vocab.txt
  cmp <(grep -z '^core/' "$STUB_OUT/tree.lst" | sort -z) <(printf '%s\0' "${AWKWARD[@]}" | sort -z)

  # Working-tree scan: changed and untracked files.
  local untracked=$'core/un\ttracked\n\303\251.rs'
  echo "pub fn g() {}" >"$SB/$untracked"
  mg similarity --base HEAD~1
  [ "$status" -eq 0 ]
  same_paths "$STUB_OUT/files.lst" "${AWKWARD[@]}" specs/storage/interop-vocab.txt "$untracked"

  # Given paths.
  mg similarity -- "$untracked" core/plain.rs
  [ "$status" -eq 0 ]
  same_paths "$STUB_OUT/files.lst" "$untracked" core/plain.rs

  # A full scan of the checkout.
  mg similarity
  [ "$status" -eq 0 ]
  same_paths "$STUB_OUT/files.lst" $(plain_paths) "${AWKWARD[@]}" "$untracked"
}

@test "similarity normalizes given paths: no ./ or .. spelling materializes oracle/, outside paths are refused" {
  make_docker_stub
  mkdir -p "$SB/refs/anki"
  local root
  root="$(cd "$SB" && pwd -P)"
  mg similarity --rev HEAD -- ./oracle/driver/run.py core/../oracle//driver/run.py \
    ./core/rust-toolchain.toml "$root/core/./rust-toolchain.toml" core/x/../rust-toolchain.toml
  [ "$status" -eq 0 ]
  same_paths "$STUB_OUT/files.lst" oracle/driver/run.py core/rust-toolchain.toml
  run ! grep -qz 'oracle' "$STUB_OUT/tree.lst"
  grep -qzx 'core/rust-toolchain.toml' "$STUB_OUT/tree.lst"
  mg similarity -- ./core/rust-toolchain.toml core//rust-toolchain.toml
  [ "$status" -eq 0 ]
  same_paths "$STUB_OUT/files.lst" core/rust-toolchain.toml
  local outside
  for outside in ../x.rs core/../../x.rs /etc/x.rs; do
    rm -rf "$STUB_OUT"
    mg similarity --rev HEAD -- core/rust-toolchain.toml "$outside"
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = bad-path ]
    [[ "$(field '.failures[0].message')" == *"'$outside' is outside the checkout"* ]]
    [ ! -e "$STUB_OUT" ]
    validate "$result"
  done
}

@test "similarity warns about listed files the scanner did not find" {
  make_docker_stub
  mkdir -p "$SB/refs/anki"
  STUB_STATUS=warn STUB_MISSING='["core/gone.rs"]' mg similarity -- core/gone.rs
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field '.failures[0].id')" = similarity:missing ]
  [[ "$(field '.failures[0].message')" == *core/gone.rs* ]]
  validate "$result"
}

@test "a scanner error reaches the result with its reason" {
  make_docker_stub
  mkdir -p "$SB/refs/anki"
  STUB_ERROR="the Anki corpus is incomplete: it has 53 of the 54 release tags" mg similarity -- core/a.rs
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = similarity:error ]
  [[ "$(field '.failures[0].message')" == *"53 of the 54"* ]]
  [[ "$(field .message)" == *"53 of the 54"* ]]
  validate "$result"
}

@test "a nightly root summary error reaches failures without reference names" {
  make_docker_stub
  mkdir -p "$SB/refs/anki"
  local error="required JPlag references are incomplete: 1 configured tag(s) missing; 1 tag(s) selected"
  STUB_ERROR="$error" mg similarity-nightly
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures | length')" -eq 1 ]
  [ "$(field '.failures[0].id')" = jplag:error ]
  [ "$(field '.failures[0].message')" = "$error" ]
  [ "$(field '.failures[0].repro')" = 'tools/mg similarity-nightly' ]
  [ "$(field .message)" = "similarity-nightly: $error" ]
  validate "$result"
}
