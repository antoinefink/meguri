#!/usr/bin/env bats
# Runs inside the existing MG Docker selftest lane, with no Docker socket.
@test "conformance edge ownership lifecycle and TCP sockets" {
  run python3 -I -B -m unittest discover -s "$BATS_TEST_DIRNAME/conformance_edge" -v
  printf '%s\n' "$output"
  [ "$status" -eq 0 ]
}
