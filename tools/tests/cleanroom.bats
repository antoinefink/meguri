#!/usr/bin/env bats

@test "clean-room source views exclude forbidden inputs and refuse indirect paths" {
  run python3 -m unittest discover -s "$BATS_TEST_DIRNAME/../cleanroom" -p 'test_*.py' -v
  echo "$output"
  [ "$status" -eq 0 ]
}
