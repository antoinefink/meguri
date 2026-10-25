import Foundation
internal import MeguriCoreBindings

/// The errors a core call can throw, as Swift sees them (KTD9). Every
/// CoreBridge call throws only this type.
public enum BridgeError: Error, Sendable, Hashable {
    case exportSourceChanged
    case exportSourceInvalid
    case exportFormatLimit
    case deckOptionsInvalid([OptionIssue])
    case stale(table: CoreTable, key: String)
    case staleGeneration(planned: UInt64, current: UInt64)
    case undoUnavailable(reason: ReviewDisabledReason)
    case needsSchemaConfirmation(reasons: [SchemaChangeReason])
    case notetypeRefused(NotetypeRefusal)
    case deckRefused(DeckRefusal)
    case noteNotFound(id: Int64)
    case notFound(table: CoreTable, key: String)
    case insufficientSpace(required: UInt64, available: UInt64)
    case open(CollectionOpenFailure)
    case suspendDeadline(phase: String)
    case schedOpRefused(SchedOpRefusal)
    case sessionActive
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
        case .DeckOptionsInvalid(let issues): self = .deckOptionsInvalid(issues.map(OptionIssue.init))
        case .Stale(let table, let key): self = .stale(table: CoreTable(table), key: key)
        case .StaleGeneration(let planned, let current): self = .staleGeneration(planned: planned, current: current)
        case .UndoUnavailable(let reason): self = .undoUnavailable(reason: ReviewDisabledReason(reason))
        case .NeedsSchemaConfirmation(let reasons): self = .needsSchemaConfirmation(reasons: reasons.map(SchemaChangeReason.init))
        case .NotetypeRefused(let refusal): self = .notetypeRefused(NotetypeRefusal(refusal))
        case .DeckRefused(let refusal): self = .deckRefused(DeckRefusal(refusal))
        case .NoteNotFound(let id): self = .noteNotFound(id: id)
        case .NotFound(let table, let key): self = .notFound(table: CoreTable(table), key: key)
        case .InsufficientSpace(let required, let available): self = .insufficientSpace(required: required, available: available)
        case .Open(let failure): self = .open(CollectionOpenFailure(failure))
        case .SuspendDeadline(let phase): self = .suspendDeadline(phase: phase)
        case .SchedOpRefused(let refusal): self = .schedOpRefused(SchedOpRefusal(refusal))
        case .SessionActive: self = .sessionActive
        case .InvalidTypeAnswerReplacement: self = .invalidTypeAnswerReplacement
        case .ExportSourceChanged: self = .exportSourceChanged
        case .ExportSourceInvalid: self = .exportSourceInvalid
        case .ExportFormatLimit: self = .exportFormatLimit
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
