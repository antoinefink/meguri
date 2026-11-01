# AppModelTests

- Test-only FFI uses `@testable import Instrumentation` and `BridgeCall`; closures crossing `MaintenanceLane` need explicit `@Sendable`/nonisolated isolation as applicable.
- Real collection fixtures keep backups beside the collection directory, never inside it.
- Opaque commands must be actual issued handles; replace only their execution with a fake.
