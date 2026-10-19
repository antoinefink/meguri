import Foundation
internal import MeguriCoreBindings

/// The errors a core call can throw, as Swift sees them (KTD9). Every
/// CoreBridge call throws only this type.
public enum BridgeError: Error, Sendable, Hashable {
    case invalidTypeAnswerReplacement
    /// The call's `CancelToken` was cancelled.
    case cancelled
    /// Another long operation holds the slot this one needs (R171).
    case busy(runningOperation: String)
    /// The collection cannot take the call now, for example during an
    /// exclusive phase (KTD17).
    case unavailable(reason: String)
    /// Compiled out for this platform or build (KTD9 iOS stubs, KTD21).
    case unsupported(feature: String)
    /// The core rejected an argument.
    case invalidArgument(String)
    /// A core bug or a caught Rust panic. The message is for logs only.
    case `internal`(String)
    /// The note write committed. Recover its media without repeating it.
    case mediaCommitPending(sequence: UInt64, message: String)
    /// The write committed. Refresh its state without repeating it.
    case commitCompleted(sequence: UInt64, message: String)
}

extension BridgeError {
    /// Maps anything a generated binding throws: the core's typed error, or
    /// UniFFI's internal error for a caught Rust panic.
    init(_ error: any Error) {
        switch error {
        case let error as BridgeError:
            self = error
        case let error as MeguriCoreBindings.CoreError:
            self.init(error)
        default:
            // UniFFI's own error type is fileprivate to the bindings; its
            // description carries the Rust panic message.
            self = .internal((error as? any LocalizedError)?.errorDescription ?? String(describing: error))
        }
    }

    init(_ error: MeguriCoreBindings.CoreError) {
        switch error {
        case .InvalidTypeAnswerReplacement: self = .invalidTypeAnswerReplacement
        case .Cancelled: self = .cancelled
        case .Busy(let runningOp): self = .busy(runningOperation: runningOp)
        case .Unavailable(let reason): self = .unavailable(reason: reason)
        case .Unsupported(let feature): self = .unsupported(feature: feature)
        case .InvalidArgument(let message): self = .invalidArgument(message)
        case .Internal(let message): self = .internal(message)
        case .MediaCommitPending(let seq, let message): self = .mediaCommitPending(sequence: seq, message: message)
        case .CommitCompleted(let seq, let message): self = .commitCompleted(sequence: seq, message: message)
        }
    }
}
