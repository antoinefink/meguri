#!/usr/bin/env bats
# Clean-room tooling (U2): role worktrees, session records, the commit-msg
# provenance hook. Runs in the dev container
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
}

# mg <args>: run the sandbox's tools/mg (or $MG_AT/tools/mg).
mg() {
  run --separate-stderr "${MG_AT:-$SB}/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }

validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }

lines_of() { if [ -f "$1" ]; then wc -l <"$1" | tr -d ' '; else echo 0; fi; }

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

@test "worktree new --role implementer: no oracle/, the deny profile, one session record" {
  local before head wt marker
  before="$(lines_of "$SESSIONS")"
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
  run ! grep -q '@MAIN_CHECKOUT@\|@WORKTREES@' "$wt/.claude/settings.local.json"
  jq -e --arg p "Read(/$main/refs/**)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"
  jq -e --arg p "Read(/$main/oracle/**)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"
  jq -e --arg p "Read(/$main/local-data/**)" '.permissions.deny | index($p)' "$wt/.claude/settings.local.json"

  # One record, role implementer, linked to the profile and the role doc.
  [ "$(lines_of "$SESSIONS")" -eq $((before + 1)) ]
  local rec sum
  rec="$(tail -n 1 "$SESSIONS")"
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
}

@test "worktree new for a spec writer is a full checkout without a deny profile" {
  mg worktree new --role spec-writer s1 --branch spec/s1
  [ "$status" -eq 0 ]
  local wt="$BATS_TEST_TMPDIR/repo-wt/s1"
  [ -f "$wt/oracle/driver/run.py" ]
  [ ! -e "$wt/.claude/settings.local.json" ]
  [ "$(git -C "$wt" rev-parse --abbrev-ref HEAD)" = spec/s1 ]
  local rec
  rec="$(tail -n 1 "$SESSIONS")"
  [ "$(jq -r .role <<<"$rec")" = spec-writer ]
  [ "$(jq -c .excluded_paths <<<"$rec")" = '[]' ]
  [ "$(jq -r .deny_profile <<<"$rec")" = null ]
}

@test "worktree new reuses an existing branch at its tip" {
  git -C "$SB" branch unit/again
  echo more >>"$SB/core/rust-toolchain.toml"
  git -C "$SB" commit -qam "move main ahead"
  mg worktree new --role fixer again
  [ "$status" -eq 0 ]
  [ "$(jq -r .base_commit <<<"$(tail -n 1 "$SESSIONS")")" = "$(git -C "$SB" rev-parse unit/again)" ]
  [ ! -e "$BATS_TEST_TMPDIR/repo-wt/again/oracle" ]
}

@test "worktree new rejects a bad role, a bad name or an existing path, and records nothing" {
  local before
  before="$(lines_of "$SESSIONS")"
  mg worktree new --role critic c1
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-role ]
  mg worktree new --role implementer "../escape"
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-name ]
  mkdir -p "$BATS_TEST_TMPDIR/repo-wt/taken"
  mg worktree new --role implementer taken
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = path-exists ]
  mg worktree list
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = usage ]
  [ "$(lines_of "$SESSIONS")" -eq "$before" ]
  [ -z "$(git -C "$SB" branch --list 'unit/*')" ]
}

@test "session record appends a record for a session without a worktree" {
  local before
  before="$(lines_of "$SESSIONS")"
  mg session record --role critic --name review-home --note "packet build/critic/1"
  [ "$status" -eq 0 ]
  validate "$result"
  [ "$(lines_of "$SESSIONS")" -eq $((before + 1)) ]
  local rec
  rec="$(tail -n 1 "$SESSIONS")"
  [ "$(jq -r .role <<<"$rec")" = critic ]
  [ "$(jq -r .kind <<<"$rec")" = session ]
  [ "$(jq -r .note <<<"$rec")" = "packet build/critic/1" ]
  [ "$(jq -r .role_doc_blob <<<"$rec")" = "$(git -C "$SB" rev-parse HEAD:docs/process/roles/critic.md)" ]
  [ "$(field .details.session.session)" = "$(jq -r .session <<<"$rec")" ]
  mg session record --role wizard
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = bad-role ]
}

@test "session records merge without conflicts when branches append" {
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

  commit_with $'Add the grammar\n\nRole: implementor\nSpec: search/grammar#G3' core/crates/meguri-search/src/lib.rs
  [ "$status" -ne 0 ]
  [[ "$stderr" == *"unknown Role"* ]]

  commit_with $'Add the grammar\n\nRole: implementer\nSpec: search/grammar#G3\nSession: s-20261013T000000Z-0a1b2c3d' core/crates/meguri-search/src/lib.rs
  [ "$status" -eq 0 ]
  [ "$(git -C "$SB" log -1 --format=%s)" = "Add the grammar" ]
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

@test "the implementer deny profile template covers every quarantined source" {
  local p="$SB/.claude/settings.implementer.json" rule
  jq -e '.permissions.deny | length > 0' "$p"
  for rule in \
    'Read(/@MAIN_CHECKOUT@/refs/**)' \
    'Read(/@MAIN_CHECKOUT@/local-data/**)' \
    'Read(/@MAIN_CHECKOUT@/oracle/**)' \
    'Read(/@MAIN_CHECKOUT@/build/oracle-macos-venv/**)' \
    'Read(/@WORKTREES@/*/oracle/**)' \
    'Bash(git show *oracle/*)' \
    'Bash(git log -p*oracle*)' \
    'Bash(docker exec *)' \
    'Bash(docker cp *)' \
    'Bash(*github.com/ankitects*)' \
    'Bash(*git.foosoft.net*)' \
    'WebFetch(domain:ankiweb.net)'; do
    jq -e --arg r "$rule" '.permissions.deny | index($r)' "$p" >/dev/null || { echo "missing deny rule $rule"; return 1; }
  done
}
