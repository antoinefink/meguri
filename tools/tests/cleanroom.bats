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

  mkdir -p "$task/$(dirname "$MG_SESSIONS_FILE")"
  printf '%s\n' '{"session":"worker","role":"implementer"}' \
    '{"session":"reviewer","role":"critic"}' \
    '{"session":"invalid-role","role":"implementer\n"}' > "$task/$MG_SESSIONS_FILE"
  run mg_cleanroom_source_commit worker
  [ "$status" -eq 1 ]

  git -C "$task" add "$MG_SESSIONS_FILE"
  git -C "$task" commit -qm session
  touch "$main/unrelated-work"
  run mg_cleanroom_source_commit worker
  [ "$status" -eq 0 ]
  [ "$output" = "$(git -C "$task" rev-parse HEAD)" ]

  run mg_cleanroom_source_commit reviewer
  [ "$status" -eq 1 ]
  run mg_cleanroom_source_commit invalid-role
  [ "$status" -eq 1 ]

  git -C "$task" update-index --assume-unchanged "$MG_SESSIONS_FILE"
  printf '%s\n' '{"session":"uncommitted","role":"implementer"}' >> "$task/$MG_SESSIONS_FILE"
  [ -z "$(git -C "$task" status --porcelain)" ]
  run mg_cleanroom_source_commit uncommitted
  [ "$status" -eq 1 ]

  touch "$task/uncommitted-work"
  run mg_cleanroom_source_commit worker
  [ "$status" -eq 1 ]
}
