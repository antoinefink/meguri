# AppModelTests

- Optional Bool in Swift Testing needs `#require(value as Bool?)` to select unwrapping; a typed local alone stays ambiguous. If lazy actor-isolated fixture initialization conflicts with inferred `@concurrent` defaults, construct it in an explicit actor-isolated helper.

- Test-only FFI uses `@testable import Instrumentation` and `BridgeCall`; closures crossing `MaintenanceLane` need explicit `@Sendable`/nonisolated isolation as applicable.
- Real collection fixtures keep backups beside the collection directory, never inside it.
- Opaque commands must be actual issued handles; replace only their execution with a fake.
- Real command fixtures use ServiceAppHarness startup; raw registerSDK alone does not establish policy engine-start authority.
- Changing automatic sync mode replaces the SDK. Drain configuration before selecting an engine for holds, waits or validation.
- A held Core result blocks ordinary input barriers until its ACK. Observe that wait before awaiting a barrier; account-proof fixtures must admit the resolver before holding a later status command.
- No-follow file fixtures use POSIX `realpath` on the trusted OS temp root before creating their run directory. Foundation's `resolvingSymlinksInPath` can retain `/var`'s alias. Do not normalize runtime input paths to bypass a guard.

- `selectedBootstrapSource` uses the system clock. Harnesses opening its bytes must use `clock: .system` when checking edit stamps.
- With a healthy Service, join and close the owner before stopping its helper. `cleanupAfterServiceFailure` is for a broken transport.
