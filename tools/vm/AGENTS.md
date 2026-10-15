# tools/vm/

The macOS test VM that runs every Mac XCUITest suite (U8b; setup and verification: `docs/process/mac-ui-vm.md`; runner: `tools/mg.d/mac-vm.sh`).

- The VM lives on the external T9 drive and is named per host, never in the repository: `~/Library/Application Support/meguri/mac-vm.json` (or `$MEGURI_MAC_VM_CONFIG`), `{"tart": <wrapper>, "ssh": <wrapper>, "vms": [<name> | {"name", "ssh", "tart"}]}`. `ssh` runs its arguments as one shell command in that VM; `/Volumes/T9/meguri-vm/bin/vm-ssh` serves `meguri-mac` only, so a second VM (`tart clone`) needs its own wrapper in its entry. Runs take the first free VM.
- Jobs start a stopped VM headless in a session of their own (perl `setsid`), so it outlives the job. Never stop, delete or re-create a VM from a job; changing things inside the VM is allowed (Antoine, 2026-10-15). Never change the host's settings for it.
- Products, fixtures and drive scripts go to the same absolute path in the VM, by tar over SSH: the UI tests read the app and fixtures at their build paths (`BuildUnderTest.swift`), and `/Users/antoine` in the VM belongs to its user. Do not use tart's shared folders: virtiofs breaks Xcode's symlinks.
- Start tests in a VM only while holding its lease. A remote run outlives a lost SSH session or a killed job, so the lease holder stops any `xcodebuild test-without-building` or UI-test runner it finds there (`mg__mac_vm_stop_tests`): when it takes the VM, and right after a lost session. A run it cannot stop ends the job's remaining suites (`MG_MAC_VM_LOST`), and the VM gets no new run until a later job stops it.
- The VM's login shell is zsh: build remote commands from single-quoted words (`mg_mac_vm_q`). `TEST_RUNNER_*` variables do not cross SSH by themselves; `mg_mac_vm_xcodebuild_test` passes them with `env`.
- `log show` in the VM needs `sudo` to see testmanagerd and WindowServer; take `--start` times from the VM's clock (`mg_app_log_now`).
- The host cannot record the VM's screen: Mac `drive --record` in the VM samples frames in the test runner.
- `setup-guest.sh` is the one-time provisioning, run in the VM's Terminal (it asks for the VM account's password); it changes the VM only.
