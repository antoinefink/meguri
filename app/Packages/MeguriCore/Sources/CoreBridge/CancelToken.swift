internal import MeguriCoreBindings
import Instrumentation

/// An explicit cancellation token for one core call or job (KTD9). Swift task
/// cancellation never reaches Rust: cancel a core operation only with
/// `cancel()`.
public final class CancelToken: Sendable {
    let raw: MeguriCoreBindings.CancelToken

    init(raw: MeguriCoreBindings.CancelToken) {
        self.raw = raw
    }

    /// Creates a token in the core (control lane).
    public static func make() async throws(BridgeError) -> CancelToken {
        try await ControlLane.shared.call(.cancelTokenNew) { CancelToken(raw: try MeguriCoreBindings.CancelToken()) }
    }

    /// Requests cancellation (control lane). Idempotent.
    public func cancel() async throws(BridgeError) {
        try await ControlLane.shared.call(.cancelTokenCancel) { try raw.cancel() }
    }

    public func isCancelled() async throws(BridgeError) -> Bool {
        try await ControlLane.shared.call(.cancelTokenIsCancelled) { try raw.isCancelled() }
    }
}
