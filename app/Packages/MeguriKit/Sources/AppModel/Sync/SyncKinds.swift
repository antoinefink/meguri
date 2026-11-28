import CoreBridge
import Foundation

/// T19's three kinds of sync problem. Display only: no command authority.
public enum SyncProblemKind: Sendable, Equatable { case waiting, needsYou, issues }

/// The one action a status row may carry (T19.4, T19.5). The view runs it.
public enum SyncStatusAction: String, Sendable, Equatable, CaseIterable {
    case copyDebugInfo, chooseCollection
    case serviceRecovery, meguriConnectionRetry, meguriSignIn, meguriCancel
    case meguriTryAgain, meguriFinishSetup, meguriRetryMedia
    /// MP7 step 5's Upload…: a committed structure save's upload.
    case meguriUploadStructure
    public var titleKey: String {
        switch self {
        case .copyDebugInfo: "sync.action.copyDebugInfo"
        case .chooseCollection: "sync.decision.action"
        case .serviceRecovery: "sync.service.recovery.action"
        case .meguriConnectionRetry: "sync.meguri.connection.retry"
        case .meguriSignIn: "sync.meguri.signIn"
        case .meguriCancel: "sync.meguri.cancel"
        case .meguriTryAgain: "sync.recovery.tryAgain"
        case .meguriFinishSetup: "sync.meguri.setup.finish"
        // Resume fix: the status row's Try Again (T19.5); Troubleshooting's Retry has its own key.
        case .meguriRetryMedia: "sync.recovery.tryAgain"
        case .meguriUploadStructure: "sync.needsYou.meguri.action.upload"
        }
    }
}

/// One row of T19.4's or T19.5's tables.
public struct SyncKindRow: Sendable, Equatable {
    public let kind: SyncProblemKind?
    public let textKey: String?
    public let action: SyncStatusAction?
    /// The count a plural text takes; nil for a plain sentence.
    public let count: UInt64?
    public init(_ kind: SyncProblemKind?, _ textKey: String?, _ action: SyncStatusAction? = nil, count: UInt64? = nil) {
        self.kind = kind; self.textKey = textKey; self.action = action; self.count = count
    }
}

public enum MeguriRoutinePhase: String, Sendable, Codable {
    case preparing, collection, applying, checkingMedia, syncingMedia
    public var titleKey: String {
        switch self {
        case .preparing: "sync.meguri.round.preparing.title"
        case .collection: "sync.meguri.round.collection.title"
        case .applying: "sync.meguri.round.applying.title"
        case .checkingMedia: "sync.meguri.round.checkingMedia.title"
        case .syncingMedia: "sync.meguri.round.syncingMedia.title"
        }
    }
}

/// Inputs of T19.5's table, in T18.4's order.
public struct MeguriKindFacts: Sendable, Equatable {
    public var handoffRecovery: Bool
    public var providerBlocked: Bool
    public var connection: MeguriProviderConnectionPhase
    public var stage: MeguriSetupStage?
    public var routine: MeguriRoutineState?
    public var routinePhase: MeguriRoutinePhase?
    public var routineFilesLeft: UInt64?
    /// T18.5's failure key of a failed setup stage.
    public var failureKey: String?
    /// The core permits Finish Setup.
    public var finishOffered: Bool
    public var automatic: Bool
    /// The setup media step's files left to download, as last recorded; nil
    /// when none is recorded. Files left are downloads, so a count names the direction.
    public var mediaFilesLeft: UInt64?
    /// Media blocked has a list of affected files to open (resume fix: no issues
    /// row without one; its Try Again is then on the status row).
    public var mediaListKnown: Bool
    /// MP7: the phase of a note type structure save job (or Upload…) that runs.
    public var structureStage: MeguriStructureStage?
    /// MP7 step 5: a committed structure save waits for Upload….
    public var structureUploadPending: Bool
    /// This device was signed in to the connection since the collection opened:
    /// a sign-out then needs the person, while before any sign-in setup is only
    /// unfinished (T21.2).
    public var signedInBefore: Bool
    /// The last Sign In from the fields failed and its error shows under them.
    public var signInRefused: Bool
    public init(handoffRecovery: Bool = false, providerBlocked: Bool = false,
                connection: MeguriProviderConnectionPhase = .setupRequired, stage: MeguriSetupStage? = nil,
                routine: MeguriRoutineState? = nil, failureKey: String? = nil, finishOffered: Bool = false,
                automatic: Bool = true, mediaFilesLeft: UInt64? = nil, mediaListKnown: Bool = true,
                structureStage: MeguriStructureStage? = nil, structureUploadPending: Bool = false,
                signedInBefore: Bool = true, signInRefused: Bool = false,
                routinePhase: MeguriRoutinePhase? = nil, routineFilesLeft: UInt64? = nil) {
        self.routinePhase = routinePhase; self.routineFilesLeft = routineFilesLeft
        self.signInRefused = signInRefused
        self.structureStage = structureStage; self.structureUploadPending = structureUploadPending
        self.signedInBefore = signedInBefore
        self.handoffRecovery = handoffRecovery; self.providerBlocked = providerBlocked; self.connection = connection
        self.stage = stage; self.routine = routine; self.failureKey = failureKey
        self.finishOffered = finishOffered; self.automatic = automatic
        self.mediaFilesLeft = mediaFilesLeft.flatMap { $0 > 0 ? $0 : nil }
        self.mediaListKnown = mediaListKnown
    }
    /// Media blocked: an issues row that opens its list, or, with no list known,
    /// a waiting row whose Try Again is on the status row (resume fix).
    var mediaBlockedRow: SyncKindRow {
        mediaListKnown ? .init(.issues, "sync.meguri.media.blocked", .meguriRetryMedia)
            : .init(.waiting, "sync.meguri.media.blocked", .meguriRetryMedia)
    }
    /// The media step downloads: files are left to come from the server.
    var mediaDownloads: Bool { mediaFilesLeft != nil }
}

/// The main status of T19.4's four rules, for either service.
public struct SyncMainStatus: Sendable, Equatable {
    public let symbol: String
    public let titleKey: String
    public let detailKey: String?
    public let detailCount: UInt64?
    public let action: SyncStatusAction?
    /// The kind of the row that gave the detail; nil for a plain state.
    public let kind: SyncProblemKind?
    /// The rows of the issues list, in table order.
    public let issues: [SyncKindRow]
    /// Meguri Sync's media-blocked row opens Affected Files instead of the list.
    public let issuesRowKey: String?
    public var needsYou: Bool { kind == .needsYou }
    public var hasIssues: Bool { !issues.isEmpty }
}

/// Semantic status values keep Off distinct from Waiting even though both use
/// the same neutral system glyph. Presentation never selects a service variant.
public enum SyncSymbol {
    public static let upToDate = "checkmark.circle"
    public static let moving = "arrow.trianglehead.2.clockwise.rotate.90"
    public static let waiting = "cloud"
    public static let needsYou = "exclamationmark.arrow.trianglehead.2.clockwise.rotate.90"
    public static let off = "off"

    public static func glyph(_ shape: String) -> String { shape == off ? "cloud" : shape }
}

public enum SyncKinds {
    public static let needsYouSymbol = SyncSymbol.needsYou

    public static let off = SyncMainStatus(symbol: SyncSymbol.off, titleKey: "sync.status.off.title",
        detailKey: "sync.status.localOnly.detail", detailCount: nil, action: nil, kind: nil, issues: [], issuesRowKey: nil)

    // MARK: Meguri Sync (T19.5)

    /// T21.2's "setup not finished": Meguri Sync before its first sign-in, which
    /// is none of the three kinds. The provider's own problems come first.
    public static func meguriSetupNotFinished(_ facts: MeguriKindFacts) -> Bool {
        guard !facts.handoffRecovery, !facts.providerBlocked else { return false }
        switch facts.connection {
        case .unconfigured: return true
        // Signing in for the first time is still setup: the form stays until it succeeds.
        case .signedOut, .cancelled, .authenticating: return !facts.signedInBefore
        // A first Sign In that failed reports a blocked connection; its error is under the fields.
        case .blocked: return !facts.signedInBefore && facts.signInRefused
        case .setupRequired: return false
        }
    }

    /// The first condition that applies, in T18.4's order.
    public static func meguriRow(_ facts: MeguriKindFacts) -> SyncKindRow {
        if facts.handoffRecovery { return .init(.needsYou, "sync.needsYou.switchUnfinished", .serviceRecovery) }
        if facts.providerBlocked { return .init(.needsYou, "sync.needsYou.blocked", .copyDebugInfo) }
        switch facts.connection {
        case .blocked:
            if meguriSetupNotFinished(facts) { return .init(nil, "sync.meguri.status.notSetUp.detail", .meguriSignIn) }
            return .init(.needsYou, "sync.needsYou.meguri.signInStore", .meguriConnectionRetry)
        case .unconfigured, .signedOut, .cancelled:
            // T21.2: before the first sign-in the fields are the next step, not a problem.
            if meguriSetupNotFinished(facts) { return .init(nil, "sync.meguri.status.notSetUp.detail", .meguriSignIn) }
            return .init(.needsYou, "sync.meguri.status.signedOut.detail", .meguriSignIn)
        case .authenticating: return .init(nil, nil)
        case .setupRequired: break
        }
        // MP7: the job's own title says what moves.
        if facts.structureStage != nil { return .init(nil, nil) }
        // MP7 step 5: in place of MP6.3's choice for this binding, also while
        // its upload holds the binding (setup required, nothing retained); a
        // collection choice that Upload… found needed, and other setup steps, come first.
        // A round's acceptance unknown is settled first (MP6.9, MP7 step 5): its row stays before Upload….
        if facts.structureUploadPending, facts.stage == nil || facts.stage == .required,
           facts.routine != .acceptanceUnknown {
            // The server refused the sign-in during the upload: Sign In first, then Upload….
            if facts.routine == .authentication { return .init(.needsYou, "sync.needsYou.meguri.signIn", .meguriSignIn) }
            return .init(.needsYou, "sync.needsYou.meguri.structureUpload", .meguriUploadStructure)
        }
        if let stage = facts.stage {
            switch stage {
            case .required: return .init(nil, "sync.service.settingUp")
            case .preparing: return .init(nil, "sync.meguri.setup.preparing", .meguriCancel)
            case .preview: return .init(nil, nil)
            case .collection: return .init(nil, nil)
            // One sentence with the files left (resume fix), in place of "the collection transferred" and a files line.
            case .media:
                return facts.mediaFilesLeft.map { .init(nil, "sync.meguri.media.left", count: $0) }
                    ?? .init(nil, "sync.meguri.media.left.unknown")
            case .acceptanceUnknown: return .init(.waiting, "sync.meguri.setup.acceptanceUnknown", .meguriTryAgain)
            case .mediaBlocked: return facts.mediaBlockedRow
            // Resume fix: it continues by itself; Try Again runs it now.
            case .mediaPaused:
                return facts.mediaFilesLeft.map { .init(.waiting, "sync.meguri.media.left", .meguriRetryMedia, count: $0) }
                    ?? .init(.waiting, "sync.meguri.media.left.unknown", .meguriRetryMedia)
            case .mediaStopped:
                return .init(.needsYou, facts.mediaDownloads ? "sync.needsYou.meguri.mediaStopped.download"
                    : "sync.needsYou.meguri.mediaStopped", .meguriRetryMedia)
            case .acceptedLocalIncomplete:
                return facts.finishOffered
                    ? .init(.needsYou, "sync.needsYou.meguri.finishSetup", .meguriFinishSetup)
                    : .init(.waiting, "sync.needsYou.meguri.finishSetup")
            case .failed:
                switch facts.failureKey {
                case "sync.meguri.setup.error.activationInterrupted":
                    return facts.finishOffered
                        ? .init(.needsYou, "sync.needsYou.meguri.activation", .meguriFinishSetup)
                        : .init(.waiting, "sync.needsYou.meguri.activation")
                case "sync.meguri.setup.error.authentication":
                    return .init(.needsYou, "sync.needsYou.meguri.signIn", .meguriSignIn)
                case "sync.meguri.setup.error.identityChanged":
                    return .init(.needsYou, "sync.needsYou.meguri.identityChanged", .meguriSignIn)
                case "sync.meguri.setup.error.storage":
                    return .init(.needsYou, "sync.needsYou.meguri.storage", .meguriTryAgain)
                // MP6.3: a cancelled setup leaves the service Off; no row.
                case MeguriSyncStatusPresentation.cancelledFailureKey: return .init(nil, nil)
                case MeguriSetupSequence.fullSyncKey:
                    return .init(.needsYou, MeguriSetupSequence.overtakenKey, .chooseCollection)
                case MeguriSetupSequence.overtakenKey:
                    // MP6.9 neither: T19.5's "full sync required" row.
                    return .init(.needsYou, MeguriSetupSequence.overtakenKey, .chooseCollection)
                case MeguriSetupSequence.localUnusableKey:
                    // A collection file that is closed or damaged: setup cannot read it.
                    return .init(.needsYou, MeguriSetupSequence.localUnusableKey, .copyDebugInfo)
                default:
                    // remoteUnavailable, remoteChanged, localChanged, backupFailed, server,
                    // invalidDownload, and a failure with no typed key: all precede activation.
                    return .init(.needsYou, "sync.needsYou.meguri.setupFailed", .meguriTryAgain)
                }
            }
        }
        guard let routine = facts.routine else { return .init(.waiting, "sync.meguri.status.setupUnavailable") }
        switch routine {
        case .idle: return .init(nil, facts.automatic ? nil : "sync.status.automaticOff.detail")
        case .pending, .waiting, .cancelled: return .init(nil, "sync.status.pending.detail")
        case .exchanging:
            let left = facts.routinePhase == .syncingMedia ? facts.routineFilesLeft : nil
            return .init(nil, left == nil ? nil : "sync.meguri.round.filesLeft", count: left)
        case .mediaBlocked: return facts.mediaBlockedRow
        case .authentication: return .init(.needsYou, "sync.needsYou.meguri.signIn", .meguriSignIn)
        case .acceptanceUnknown: return .init(.waiting, "sync.meguri.round.acceptanceUnknown", .meguriTryAgain)
        case .fullSyncRequired: return .init(.needsYou, "sync.needsYou.meguri.chooseCollection", .chooseCollection)
        case .network: return .init(.waiting, "sync.meguri.round.network")
        case .server: return .init(.waiting, "sync.meguri.round.server")
        // No round has reported: retain the existing setup-unavailable detail.
        case .unreported: return .init(.waiting, "sync.meguri.status.setupUnavailable")
        case .failed: return .init(.waiting, nil)
        }
    }

    private static func meguriTitle(_ facts: MeguriKindFacts) -> (symbol: String, key: String) {
        let busy = SyncSymbol.moving
        let stopped = SyncSymbol.waiting
        if facts.connection == .authenticating { return (busy, "sync.meguri.status.signingIn.title") }
        // T21.2: quiet, as Sync Off is.
        if meguriSetupNotFinished(facts) { return (SyncSymbol.off, "sync.meguri.status.notSetUp.title") }
        // MP7: the round and the save sync; the upload transfers the collection, then its media.
        if let structure = facts.structureStage {
            return switch structure {
            case .round, .save: (busy, "sync.status.syncing.title")
            case .facts, .serverCopy, .upload: (busy, "sync.meguri.status.transferringCollection.title")
            case .mediaPrepare, .media: (busy, "sync.meguri.status.transferringMedia.title")
            }
        }
        if let stage = facts.stage {
            return switch stage {
            // Nothing moves before preparation or while the sheet asks (N23.2).
            case .required: (stopped, "sync.service.settingUp")
            case .preparing: (busy, "sync.meguri.status.preparing.title")
            case .preview: (stopped, "sync.service.settingUp")
            case .collection: (busy, "sync.meguri.status.transferringCollection.title")
            case .media:
                (busy, facts.mediaDownloads ? "sync.meguri.status.downloadingMedia.title" : "sync.meguri.status.transferringMedia.title")
            case .mediaBlocked: (stopped, "sync.meguri.status.mediaBlocked.title")
            case .mediaPaused, .mediaStopped:
                (stopped, facts.mediaDownloads ? "sync.meguri.status.downloadPaused.title" : "sync.meguri.status.transferPaused.title")
            case .acceptanceUnknown: (stopped, "sync.meguri.status.acceptanceUnknown.title")
            case .acceptedLocalIncomplete: (stopped, "sync.meguri.status.acceptedLocalIncomplete.title")
            case .failed: (stopped, "sync.meguri.status.setupFailed.title")
            }
        }
        switch facts.routine {
        case .idle?:
            return facts.automatic ? (SyncSymbol.upToDate, "sync.status.upToDate.title")
                : (SyncSymbol.waiting, "sync.status.automaticOff.title")
        case .exchanging?: return (busy, (facts.routinePhase ?? .preparing).titleKey)
        case .mediaBlocked?: return (stopped, "sync.meguri.status.mediaBlocked.title")
        case .pending?, .waiting?, .cancelled?: return (SyncSymbol.waiting, "sync.status.pending.title")
        case .acceptanceUnknown?: return (stopped, "sync.meguri.status.acceptanceUnknown.title")
        case .network?, .server?, .failed?: return (stopped, "sync.meguri.status.failed.title")
        default: return (stopped, "sync.meguri.status.signedIn.title")
        }
    }

    public static func meguri(_ facts: MeguriKindFacts) -> SyncMainStatus {
        let row = meguriRow(facts)
        if row.kind == .needsYou {
            return .init(symbol: needsYouSymbol, titleKey: "sync.status.needsAttention.title", detailKey: row.textKey,
                         detailCount: row.count, action: row.action, kind: .needsYou, issues: [], issuesRowKey: nil)
        }
        let title = meguriTitle(facts)
        let issues = row.kind == .issues ? [row] : []
        return .init(symbol: title.symbol, titleKey: title.key, detailKey: row.textKey, detailCount: row.count,
                     action: row.kind == .issues ? nil : row.action, kind: row.kind, issues: issues,
                     issuesRowKey: row.kind == .issues ? "sync.meguri.media.notTransferred" : nil)
    }

    // MARK: Troubleshooting and Sync Now

    /// V27.7: the count line of Troubleshooting, the copies' only entry.
    /// T19.7's replaced reasons.
    public static let syncNowOffKey = "sync.now.unavailable.off"
}
