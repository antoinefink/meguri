import CoreBridge
import Foundation

/// T19's three kinds of sync problem. Display only: no command authority.
public enum SyncProblemKind: Sendable, Equatable { case waiting, needsYou, issues }

/// The one action a status row may carry (T19.4, T19.5). The view runs it.
public enum SyncStatusAction: String, Sendable, Equatable, CaseIterable {
    case copyDebugInfo, chooseCollection, repair, tryAgain, resume, setUpAgain
    case cancelOp, resumeOp, abandon, checkDatabase
    case serviceRecovery, meguriConnectionRetry, meguriSignIn, meguriCancel
    case meguriTryAgain, meguriFinishSetup, meguriRetryMedia
    /// MP7 step 5's Upload…: a committed structure save's upload.
    case meguriUploadStructure
    public var titleKey: String {
        switch self {
        case .copyDebugInfo: "sync.action.copyDebugInfo"
        case .chooseCollection: "sync.decision.action"
        case .repair: "sync.needsYou.action.repair"
        case .tryAgain: "sync.action.tryAgain"
        case .resume: "sync.action.resume"
        case .setUpAgain: "sync.needsYou.action.setUp"
        case .cancelOp: "sync.action.cancelOp"
        case .resumeOp: "sync.action.resumeOp"
        case .abandon: "sync.action.abandon"
        case .checkDatabase: "sync.action.checkDatabase"
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

/// Facts that pick a row among those sharing one attention item. Each defaults
/// to the row that claims least: an unknown isolation reason is "any other".
public struct CloudKindFacts: Sendable, Equatable {
    public enum Isolation: Sendable, Equatable, CaseIterable {
        case signedOut, accountChanged, accountChangedBack, replacedElsewhere
        case missing, deleted, encryptionReset, fileMismatch, other
    }
    public enum RecoveryOp: Sendable, Equatable, CaseIterable {
        case adoptUnproven, publishConflict, publishAccount, publishWaiting, adoptAfterReview, abandonWaiting
        /// An `adopt` op whose download waits on transport (V14.3): the person can resume it.
        case adoptWaiting
    }
    public enum Fence: Sendable, Equatable, CaseIterable { case restore, importCollection, none }
    public var isolation: Isolation
    public var recoveryOp: RecoveryOp?
    /// V9.2 offers Resume for the isolated binding.
    public var resumeBindingOffered: Bool
    /// V15.3 offers Resume for the waiting op.
    public var resumeOpOffered: Bool
    /// A `publish` op is at `bound`, `zones` or `sent` while the quota is full.
    public var publishQuota: Bool
    /// V18 offers Abandon.
    public var abandonOffered: Bool
    /// `chooseCollection` is V13.6's unproved-layout decision.
    public var layoutDecision: Bool
    public var fence: Fence
    public init(isolation: Isolation = .other, recoveryOp: RecoveryOp? = nil, resumeBindingOffered: Bool = false,
                resumeOpOffered: Bool = false, publishQuota: Bool = false, abandonOffered: Bool = false,
                layoutDecision: Bool = false, fence: Fence = .none) {
        self.isolation = isolation; self.recoveryOp = recoveryOp; self.resumeBindingOffered = resumeBindingOffered
        self.resumeOpOffered = resumeOpOffered; self.publishQuota = publishQuota; self.abandonOffered = abandonOffered
        self.layoutDecision = layoutDecision; self.fence = fence
    }
}

/// Inputs of T19.5's table, in T18.4's order.
public struct MeguriKindFacts: Sendable, Equatable {
    public var handoffRecovery: Bool
    public var providerBlocked: Bool
    public var connection: MeguriProviderConnectionPhase
    public var stage: MeguriSetupStage?
    public var routine: MeguriRoutineState?
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
                signedInBefore: Bool = true, signInRefused: Bool = false) {
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

/// N23.2: five states, one system symbol each, for both services; only needs you
/// has a colour. The names are the `icloud` symbols, which Apple reserves for
/// iCloud: `glyph` gives them for iCloud only, and unrestricted system symbols
/// for Meguri Sync and Off (N23.4). Nothing is drawn over or inside a symbol.
public enum SyncSymbol {
    public static let upToDate = "checkmark.icloud"
    /// Something moves; it turns while the work lasts.
    public static let moving = "arrow.trianglehead.2.clockwise.rotate.90.icloud"
    /// Waiting, or not sent yet.
    public static let waiting = "icloud"
    public static let needsYou = "exclamationmark.icloud"
    public static let off = "icloud.slash"

    /// N23.2: the system symbol a state is drawn with. The `icloud` symbols only
    /// refer to Apple's iCloud, and SF Symbols has no unrestricted cloud with a
    /// badge, so Meguri Sync and Off use other unrestricted symbols.
    public static func glyph(_ shape: String, iCloud: Bool) -> String {
        if iCloud, shape != off { return shape }
        return switch shape {
        case upToDate: "checkmark.circle"
        case moving: "arrow.trianglehead.2.clockwise.rotate.90"
        case needsYou: "exclamationmark.arrow.trianglehead.2.clockwise.rotate.90"
        default: "cloud" // Waiting, and Sync Off (in the secondary colour in the toolbar).
        }
    }
}

public enum SyncKinds {
    public static let needsYouSymbol = SyncSymbol.needsYou

    // MARK: iCloud (T19.4)

    /// The state table: the row of a P14.2 state that is itself a kind.
    public static func cloudStateRow(_ state: CloudPolicyStatusState?, fence: CloudKindFacts.Fence = .none) -> SyncKindRow? {
        switch state {
        case .iCloudUnavailable(.noAccount): .init(.needsYou, "sync.status.unavailable.noAccount")
        case .iCloudUnavailable(.restricted): .init(.needsYou, "sync.status.unavailable.restricted")
        case .iCloudUnavailable(.temporary): .init(.waiting, "sync.status.unavailable.temporary")
        case .iCloudUnavailable(.setupNeeded): .init(.needsYou, "sync.status.unavailable.setupNeeded", .copyDebugInfo)
        case .paused(.decisionRequired):
            switch fence {
            case .restore: .init(.needsYou, "sync.needsYou.fence.restore", .chooseCollection)
            case .importCollection: .init(.needsYou, "sync.needsYou.fence.import", .chooseCollection)
            case .none: .init(.needsYou, "sync.needsYou.fence.none", .chooseCollection)
            }
        case .offline: .init(.waiting, "sync.status.offline.detail")
        default: nil
        }
    }

    /// The attention table, rows 1 to 43, in the order that decides which shows.
    /// `conflicts` (row 44) has no entry.
    public static func cloudRows(attention: [CloudPolicyAttention: UInt64], facts: CloudKindFacts = .init()) -> [SyncKindRow] {
        func has(_ item: CloudPolicyAttention) -> Bool { (attention[item] ?? 0) > 0 }
        func count(_ item: CloudPolicyAttention) -> UInt64? { attention[item] }
        var rows: [SyncKindRow] = []
        let resumeOrSetUp: SyncStatusAction = facts.resumeBindingOffered ? .resume : .setUpAgain
        if has(.collectionCorrupt) { rows.append(.init(.needsYou, "sync.attention.collectionCorrupt", .repair)) }
        if has(.localStorageFull) { rows.append(.init(.needsYou, "sync.needsYou.localStorageFull", .tryAgain)) }
        if has(.trackingError) { rows.append(.init(.needsYou, "sync.attention.trackingError", .tryAgain)) }
        if has(.unsupportedSchema) { rows.append(.init(.needsYou, "sync.needsYou.update")) }
        if has(.isolated) {
            let row: SyncKindRow = switch facts.isolation {
            case .signedOut: .init(.needsYou, "sync.attention.signedOut", resumeOrSetUp)
            case .accountChanged: .init(.needsYou, "sync.needsYou.accountChanged", .chooseCollection)
            case .accountChangedBack: .init(.needsYou, "sync.attention.accountChangedBack", resumeOrSetUp)
            case .replacedElsewhere: .init(.needsYou, "sync.attention.replacedElsewhere", .chooseCollection)
            case .missing: .init(.needsYou, "sync.needsYou.missing", .chooseCollection)
            case .deleted: .init(.needsYou, "sync.needsYou.deleted", .chooseCollection)
            case .encryptionReset: .init(.needsYou, "sync.needsYou.encryptionReset", .chooseCollection)
            case .fileMismatch: .init(.needsYou, "sync.attention.fileMismatch", .setUpAgain)
            case .other: .init(.needsYou, "sync.attention.isolated", .setUpAgain)
            }
            rows.append(row)
        }
        if has(.rootUnreadable) { rows.append(.init(.needsYou, "sync.needsYou.rootUnreadable", .chooseCollection)) }
        if has(.chooseCollection) {
            rows.append(facts.layoutDecision
                ? .init(.needsYou, "sync.attention.chooseCollection", .chooseCollection)
                : .init(.needsYou, "sync.needsYou.setUp", .setUpAgain))
        }
        if has(.crtMismatch) { rows.append(.init(.needsYou, "sync.attention.crtMismatch", .chooseCollection)) }
        if has(.mediaZoneMissing) { rows.append(.init(.needsYou, "sync.needsYou.mediaMissingInICloud", .chooseCollection)) }
        let op = has(.recoveryOpWaiting) ? facts.recoveryOp : nil
        if op == .adoptUnproven { rows.append(.init(.needsYou, "sync.attention.adoptUnproven", .cancelOp)) }
        if op == .publishConflict { rows.append(.init(.needsYou, "sync.attention.publishConflict", .chooseCollection)) }
        if has(.quotaFull), facts.publishQuota { rows.append(.init(.needsYou, "sync.attention.publishQuota", .cancelOp)) }
        if op == .publishAccount {
            rows.append(.init(.needsYou, "sync.attention.publishAccount", facts.resumeOpOffered ? .resumeOp : .cancelOp))
        }
        if op == .publishWaiting, facts.resumeOpOffered {
            rows.append(.init(.needsYou, "sync.attention.publishWaiting", .resumeOp))
        }
        if op == .adoptWaiting {
            rows.append(facts.resumeOpOffered ? .init(.needsYou, "sync.attention.adoptWaiting", .resumeOp) : .init(.waiting, "sync.attention.adoptWaiting"))
        }
        if has(.quotaFull), !facts.publishQuota { rows.append(.init(.needsYou, "sync.needsYou.quotaFull", .tryAgain)) }
        if has(.generationIncomplete), facts.abandonOffered {
            rows.append(.init(.needsYou, "sync.attention.generationIncomplete", .abandon))
        }
        // Waiting.
        if op == .publishWaiting, !facts.resumeOpOffered { rows.append(.init(.waiting, "sync.attention.publishWaiting")) }
        if op == .adoptAfterReview { rows.append(.init(.waiting, "sync.attention.adoptAfterReview")) }
        if op == .abandonWaiting { rows.append(.init(.waiting, "sync.attention.abandonWaiting")) }
        // An op whose kind this projection does not carry claims nothing: it waits.
        if has(.recoveryOpWaiting), facts.recoveryOp == nil { rows.append(.init(.waiting, "sync.error.conflictRecovery")) }
        if has(.cloudError) { rows.append(.init(.waiting, "sync.waiting.cloudError")) }
        if has(.exchangeIncomplete) { rows.append(.init(.waiting, "sync.attention.exchangeIncomplete")) }
        if has(.referenceWaiting) { rows.append(.init(.waiting, "sync.attention.referenceWaiting", count: count(.referenceWaiting))) }
        if has(.assetUnavailable) { rows.append(.init(.waiting, "sync.attention.assetUnavailable", count: count(.assetUnavailable))) }
        if has(.mediaChanged) { rows.append(.init(.waiting, "sync.attention.mediaChanged", count: count(.mediaChanged))) }
        // Issues.
        if has(.conflictsUnreadable) { rows.append(.init(.issues, "sync.attention.conflictsUnreadable")) }
        if has(.refused) { rows.append(.init(.issues, "sync.attention.refused", count: count(.refused))) }
        if has(.oversize) { rows.append(.init(.issues, "sync.attention.oversize", count: count(.oversize))) }
        if has(.unreadable) { rows.append(.init(.issues, "sync.attention.unreadable", .checkDatabase, count: count(.unreadable))) }
        if has(.unsyncable) { rows.append(.init(.issues, "sync.attention.unsyncable", count: count(.unsyncable))) }
        if has(.claimStuck) { rows.append(.init(.issues, "sync.attention.claimStuck", count: count(.claimStuck))) }
        if has(.generationIncomplete), !facts.abandonOffered {
            rows.append(.init(.issues, "sync.attention.generationIncomplete"))
        }
        if has(.applyFailed) { rows.append(.init(.issues, "sync.attention.applyFailed", count: count(.applyFailed))) }
        if has(.rekeyFailed) { rows.append(.init(.issues, "sync.attention.rekeyFailed", count: count(.rekeyFailed))) }
        if has(.mediaMissing) { rows.append(.init(.issues, "sync.attention.mediaMissing", count: count(.mediaMissing))) }
        return rows
    }

    /// T19.4's four rules. `setup` covers an installation with no projection yet.
    public static func cloud(state: CloudPolicyStatusState?, stateWithoutAttention: CloudPolicyStatusState? = nil,
                             attention: [CloudPolicyAttention: UInt64] = [:], facts: CloudKindFacts = .init(),
                             setup: SyncCoordinator.SetupState = .attached,
                             selectedNotSetUp: Bool = false) -> SyncMainStatus {
        var rows = cloudRows(attention: attention, facts: facts)
        let effective: CloudPolicyStatusState? = state == nil && setup == .configurationUnavailable
            ? .iCloudUnavailable(.setupNeeded) : state
        // Row 16: iCloud is the selected service and nothing is set up, so the
        // policy still reports `localOnly`. The status must not read "Sync Off".
        if selectedNotSetUp, effective == .localOnly {
            rows.insert(.init(.needsYou, "sync.needsYou.setUp", .setUpAgain), at: 0)
        }
        if let row = cloudStateRow(effective, fence: facts.fence) { rows.insert(row, at: row.kind == .needsYou ? 0 : rows.count) }
        let issues = rows.filter { $0.kind == .issues }
        if let row = rows.first(where: { $0.kind == .needsYou }) {
            return .init(symbol: needsYouSymbol, titleKey: "sync.status.needsAttention.title", detailKey: row.textKey,
                         detailCount: row.count, action: row.action, kind: .needsYou, issues: issues, issuesRowKey: nil)
        }
        // Rule 2: the state underneath, with the first waiting text as its detail.
        let shown = effective == .needsAttention ? stateWithoutAttention : effective
        let base = SyncStatusPresentation(state: shown, setup: setup)
        let waiting = rows.first { $0.kind == .waiting }
        return .init(symbol: base.symbol, titleKey: base.titleKey,
                     detailKey: waiting?.textKey ?? base.detailKey, detailCount: waiting?.count, action: nil,
                     kind: waiting == nil ? nil : .waiting, issues: issues, issuesRowKey: nil)
    }

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
        case .exchanging: return .init(nil, nil)
        case .mediaBlocked: return facts.mediaBlockedRow
        case .authentication: return .init(.needsYou, "sync.needsYou.meguri.signIn", .meguriSignIn)
        case .acceptanceUnknown: return .init(.waiting, "sync.meguri.round.acceptanceUnknown", .meguriTryAgain)
        case .fullSyncRequired: return .init(.needsYou, "sync.needsYou.meguri.chooseCollection", .chooseCollection)
        case .network: return .init(.waiting, "sync.meguri.round.network")
        case .server: return .init(.waiting, "sync.meguri.round.server")
        // No round has reported, or one stopped with no typed reason: nothing is claimed.
        case .unreported, .failed: return .init(.waiting, "sync.meguri.status.setupUnavailable")
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
        case .exchanging?: return (busy, "sync.status.syncing.title")
        case .mediaBlocked?: return (stopped, "sync.meguri.status.mediaBlocked.title")
        case .pending?, .waiting?, .cancelled?: return (SyncSymbol.waiting, "sync.status.pending.title")
        case .acceptanceUnknown?: return (stopped, "sync.meguri.status.acceptanceUnknown.title")
        case .network?, .server?: return (stopped, "sync.meguri.status.failed.title")
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
    public static func conflictCopies(_ attention: [CloudPolicyAttention: UInt64]) -> UInt64? {
        attention[.conflicts].flatMap { $0 > 0 ? $0 : nil }
    }

    /// T19.7's replaced reasons.
    public static let syncNowOffKey = "sync.now.unavailable.off"
}
