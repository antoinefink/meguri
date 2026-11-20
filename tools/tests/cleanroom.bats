#!/usr/bin/env bats

@test "clean-room source views include server sources, exclude forbidden inputs and refuse indirect paths" {
  run python3 -m unittest discover -s "$BATS_TEST_DIRNAME/../cleanroom" -p 'test_*.py' -v
  echo "$output"
  [ "$status" -eq 0 ]
}

@test "clean-room source admission uses the task checkout and its committed session" {
  local main="$BATS_TEST_TMPDIR/main" task="$BATS_TEST_TMPDIR/task"
  mg_job() { :; }
  source "$BATS_TEST_DIRNAME/../mg.d/session.sh"
  source "$BATS_TEST_DIRNAME/../mg.d/cleanroom.sh"

  git init -q -b master "$main"
  git -C "$main" config user.name 'Cleanroom fixture'
  git -C "$main" config user.email 'fixture@example.invalid'
  git -C "$main" commit -qm base --allow-empty
  git -C "$main" worktree add -qb task/notes "$task"
  export MG_ROOT="$task"

  mkdir -p "$task/$(dirname "$MG_SESSIONS_ARCHIVE")"
  printf '%s\n' '{"session":"worker","role":"implementer"}' \
    '{"session":"reviewer","role":"critic"}' \
    '{"session":"invalid-role","role":"implementer\n"}' > "$task/$MG_SESSIONS_ARCHIVE"
  run mg_cleanroom_source_commit worker
  [ "$status" -eq 1 ]

  git -C "$task" add "$MG_SESSIONS_ARCHIVE"
  git -C "$task" commit -qm session
  touch "$main/unrelated-work"
  run mg_cleanroom_source_commit worker
  [ "$status" -eq 0 ]
  [ "$output" = "$(git -C "$task" rev-parse HEAD)" ]

  run mg_cleanroom_source_commit reviewer
  [ "$status" -eq 1 ]
  run mg_cleanroom_source_commit invalid-role
  [ "$status" -eq 1 ]

  git -C "$task" update-index --assume-unchanged "$MG_SESSIONS_ARCHIVE"
  printf '%s\n' '{"session":"uncommitted","role":"implementer"}' >> "$task/$MG_SESSIONS_ARCHIVE"
  [ -z "$(git -C "$task" status --porcelain)" ]
  run mg_cleanroom_source_commit uncommitted
  [ "$status" -eq 1 ]

  # A record in the per-session directory counts once it is committed.
  git -C "$task" update-index --no-assume-unchanged "$MG_SESSIONS_ARCHIVE"
  git -C "$task" checkout -q -- "$MG_SESSIONS_ARCHIVE"
  mkdir -p "$task/$MG_SESSIONS_DIR"
  printf '%s\n' '{"session":"fixer-1","role":"fixer"}' >"$task/$MG_SESSIONS_DIR/fixer-1.json"
  git -C "$task" add "$MG_SESSIONS_DIR/fixer-1.json"
  run mg_cleanroom_source_commit fixer-1
  [ "$status" -eq 1 ]
  git -C "$task" commit -qm "fixer session"
  run mg_cleanroom_source_commit fixer-1
  [ "$status" -eq 0 ]
  [ "$output" = "$(git -C "$task" rev-parse HEAD)" ]
  run mg_cleanroom_source_commit worker
  [ "$status" -eq 0 ]
  # A working-tree change to a committed record, hidden from status, never counts.
  git -C "$task" update-index --assume-unchanged "$MG_SESSIONS_DIR/fixer-1.json"
  printf '%s\n' '{"session":"fixer-1","role":"critic"}' >"$task/$MG_SESSIONS_DIR/fixer-1.json"
  [ -z "$(git -C "$task" status --porcelain)" ]
  run mg_cleanroom_source_commit fixer-1
  [ "$status" -eq 0 ]
  git -C "$task" update-index --no-assume-unchanged "$MG_SESSIONS_DIR/fixer-1.json"
  git -C "$task" checkout -q -- "$MG_SESSIONS_DIR/fixer-1.json"
  # A file whose session is not its name is no record.
  printf '%s\n' '{"session":"fixer-2","role":"fixer"}' >"$task/$MG_SESSIONS_DIR/fixer-x.json"
  git -C "$task" add "$MG_SESSIONS_DIR/fixer-x.json"
  git -C "$task" commit -qm "misnamed session"
  run mg_cleanroom_source_commit fixer-2
  [ "$status" -eq 1 ]
  # The same session in both places is ambiguous.
  printf '%s\n' '{"session":"fixer-1","role":"fixer"}' >>"$task/$MG_SESSIONS_ARCHIVE"
  git -C "$task" commit -qam "duplicate session"
  run mg_cleanroom_source_commit fixer-1
  [ "$status" -eq 1 ]

  touch "$task/uncommitted-work"
  run mg_cleanroom_source_commit worker
  [ "$status" -eq 1 ]
}
