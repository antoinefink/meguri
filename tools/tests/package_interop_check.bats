#!/usr/bin/env bats
# Generated build fixtures + fake public reports. No oracle, VM or network.
bats_require_minimum_version 1.5.0

@test "package interop: Python 3.9 host paths never open SQLite and bind independent evidence" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" HostCompatibility
}

@test "package interop: exact raw database, stamps, clock, report and media evidence" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" Evidence
}

@test "package interop: fixed native Paris clock, D3 dates, complete proof and unchanged source" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" NativeClock
}

@test "package interop: generated owned paths, streaming, mutation and fresh media isolation" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" Boundaries
}

@test "package interop: readonly helper composition, fail closed, dry-run and explicit utility" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" Wrapper
}

@test "package interop: pre-client raw proof rejects timing and open repair masking" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" PreClient
}

@test "package interop: Apple NFC inventory and exact M8 metadata filtering" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" MediaNames
}

@test "package interop: bounded JSON SQLite clocks and growing source reads" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" AllocationBounds RawSchema
}

@test "package interop: isolated DB and media archive proof, corruption, frame settings and bounds" {
  python3 "$BATS_TEST_DIRNAME/package_interop_check_test.py" Decoder
}
