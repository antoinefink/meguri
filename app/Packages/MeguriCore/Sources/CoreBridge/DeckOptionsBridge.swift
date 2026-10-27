import Foundation
internal import MeguriCoreBindings

public enum OptionField: Sendable, Hashable, CaseIterable {
    case newPerDay
    case reviewsPerDay
    case desiredRetention
    case minimumLapseInterval
    case leechThreshold
    case capAnswerTimeToSecs
    case secondsToShowQuestion
    case secondsToShowAnswer
    case maximumReviewInterval
    case historicalRetention
    case newCardInsertOrder
    case leechAction
    case newCardGatherPriority
    case newCardSortOrder
    case newMix
    case interdayLearningMix
    case reviewOrder
    case questionAction
    case answerAction
    case buryNew
    case buryReviews
    case buryInterdayLearning
    case disableAutoplay
    case skipQuestionWhenReplayingAnswer
    case showTimer
    case stopTimerOnAnswer
    case waitForAudio
    case learnSteps
    case relearnSteps
    case ignoreRevlogsBeforeDate
    case newLimit
    case reviewLimit
    case newLimitToday
    case reviewLimitToday
    case deckDesiredRetention
    case newCardsIgnoreReviewLimit
    case applyAllParentLimits
    case loadBalancerEnabled
    case rollover
    case learnAheadMinutes
    init(_ raw: MeguriCoreBindings.OptionField) {
        switch raw {
        case .newPerDay: self = .newPerDay
        case .reviewsPerDay: self = .reviewsPerDay
        case .desiredRetention: self = .desiredRetention
        case .minimumLapseInterval: self = .minimumLapseInterval
        case .leechThreshold: self = .leechThreshold
        case .capAnswerTimeToSecs: self = .capAnswerTimeToSecs
        case .secondsToShowQuestion: self = .secondsToShowQuestion
        case .secondsToShowAnswer: self = .secondsToShowAnswer
        case .maximumReviewInterval: self = .maximumReviewInterval
        case .historicalRetention: self = .historicalRetention
        case .newCardInsertOrder: self = .newCardInsertOrder
        case .leechAction: self = .leechAction
        case .newCardGatherPriority: self = .newCardGatherPriority
        case .newCardSortOrder: self = .newCardSortOrder
        case .newMix: self = .newMix
        case .interdayLearningMix: self = .interdayLearningMix
        case .reviewOrder: self = .reviewOrder
        case .questionAction: self = .questionAction
        case .answerAction: self = .answerAction
        case .buryNew: self = .buryNew
        case .buryReviews: self = .buryReviews
        case .buryInterdayLearning: self = .buryInterdayLearning
        case .disableAutoplay: self = .disableAutoplay
        case .skipQuestionWhenReplayingAnswer: self = .skipQuestionWhenReplayingAnswer
        case .showTimer: self = .showTimer
        case .stopTimerOnAnswer: self = .stopTimerOnAnswer
        case .waitForAudio: self = .waitForAudio
        case .learnSteps: self = .learnSteps
        case .relearnSteps: self = .relearnSteps
        case .ignoreRevlogsBeforeDate: self = .ignoreRevlogsBeforeDate
        case .newLimit: self = .newLimit
        case .reviewLimit: self = .reviewLimit
        case .newLimitToday: self = .newLimitToday
        case .reviewLimitToday: self = .reviewLimitToday
        case .deckDesiredRetention: self = .deckDesiredRetention
        case .newCardsIgnoreReviewLimit: self = .newCardsIgnoreReviewLimit
        case .applyAllParentLimits: self = .applyAllParentLimits
        case .loadBalancerEnabled: self = .loadBalancerEnabled
        case .rollover: self = .rollover
        case .learnAheadMinutes: self = .learnAheadMinutes
        }
    }
    var raw: MeguriCoreBindings.OptionField {
        switch self {
        case .newPerDay: return .newPerDay
        case .reviewsPerDay: return .reviewsPerDay
        case .desiredRetention: return .desiredRetention
        case .minimumLapseInterval: return .minimumLapseInterval
        case .leechThreshold: return .leechThreshold
        case .capAnswerTimeToSecs: return .capAnswerTimeToSecs
        case .secondsToShowQuestion: return .secondsToShowQuestion
        case .secondsToShowAnswer: return .secondsToShowAnswer
        case .maximumReviewInterval: return .maximumReviewInterval
        case .historicalRetention: return .historicalRetention
        case .newCardInsertOrder: return .newCardInsertOrder
        case .leechAction: return .leechAction
        case .newCardGatherPriority: return .newCardGatherPriority
        case .newCardSortOrder: return .newCardSortOrder
        case .newMix: return .newMix
        case .interdayLearningMix: return .interdayLearningMix
        case .reviewOrder: return .reviewOrder
        case .questionAction: return .questionAction
        case .answerAction: return .answerAction
        case .buryNew: return .buryNew
        case .buryReviews: return .buryReviews
        case .buryInterdayLearning: return .buryInterdayLearning
        case .disableAutoplay: return .disableAutoplay
        case .skipQuestionWhenReplayingAnswer: return .skipQuestionWhenReplayingAnswer
        case .showTimer: return .showTimer
        case .stopTimerOnAnswer: return .stopTimerOnAnswer
        case .waitForAudio: return .waitForAudio
        case .learnSteps: return .learnSteps
        case .relearnSteps: return .relearnSteps
        case .ignoreRevlogsBeforeDate: return .ignoreRevlogsBeforeDate
        case .newLimit: return .newLimit
        case .reviewLimit: return .reviewLimit
        case .newLimitToday: return .newLimitToday
        case .reviewLimitToday: return .reviewLimitToday
        case .deckDesiredRetention: return .deckDesiredRetention
        case .newCardsIgnoreReviewLimit: return .newCardsIgnoreReviewLimit
        case .applyAllParentLimits: return .applyAllParentLimits
        case .loadBalancerEnabled: return .loadBalancerEnabled
        case .rollover: return .rollover
        case .learnAheadMinutes: return .learnAheadMinutes
        }
    }
}
public enum OptionIssueCode: Sendable, Hashable, CaseIterable {
    case outOfRange
    case notWhole
    case invalidChoice
    case stepMalformed
    case stepZero
    case stepNegative
    case stepTooLong
    case invalidDate
    case dateInFuture
    case stepsOneDayOrMore
    case reviewsBelowTenTimesNew
    case retentionLow
    case retentionHigh
    case maxIntervalShort
    case answerSecondsHigh
    init(_ raw: MeguriCoreBindings.OptionIssueCode) {
        switch raw {
        case .outOfRange: self = .outOfRange
        case .notWhole: self = .notWhole
        case .invalidChoice: self = .invalidChoice
        case .stepMalformed: self = .stepMalformed
        case .stepZero: self = .stepZero
        case .stepNegative: self = .stepNegative
        case .stepTooLong: self = .stepTooLong
        case .invalidDate: self = .invalidDate
        case .dateInFuture: self = .dateInFuture
        case .stepsOneDayOrMore: self = .stepsOneDayOrMore
        case .reviewsBelowTenTimesNew: self = .reviewsBelowTenTimesNew
        case .retentionLow: self = .retentionLow
        case .retentionHigh: self = .retentionHigh
        case .maxIntervalShort: self = .maxIntervalShort
        case .answerSecondsHigh: self = .answerSecondsHigh
        }
    }
    var raw: MeguriCoreBindings.OptionIssueCode {
        switch self {
        case .outOfRange: return .outOfRange
        case .notWhole: return .notWhole
        case .invalidChoice: return .invalidChoice
        case .stepMalformed: return .stepMalformed
        case .stepZero: return .stepZero
        case .stepNegative: return .stepNegative
        case .stepTooLong: return .stepTooLong
        case .invalidDate: return .invalidDate
        case .dateInFuture: return .dateInFuture
        case .stepsOneDayOrMore: return .stepsOneDayOrMore
        case .reviewsBelowTenTimesNew: return .reviewsBelowTenTimesNew
        case .retentionLow: return .retentionLow
        case .retentionHigh: return .retentionHigh
        case .maxIntervalShort: return .maxIntervalShort
        case .answerSecondsHigh: return .answerSecondsHigh
        }
    }
}
public enum OptionSeverity: Sendable, Hashable, CaseIterable {
    case error
    case warning
    case strongWarning
    init(_ raw: MeguriCoreBindings.OptionSeverity) {
        switch raw {
        case .error: self = .error
        case .warning: self = .warning
        case .strongWarning: self = .strongWarning
        }
    }
    var raw: MeguriCoreBindings.OptionSeverity {
        switch self {
        case .error: return .error
        case .warning: return .warning
        case .strongWarning: return .strongWarning
        }
    }
}
public enum OptionValue: Sendable, Hashable {
    case number(Double), boolean(Bool), text(String), steps([Float]), none
    init(_ raw: MeguriCoreBindings.OptionValue) {
        switch raw {case .number(let value): self = .number(value); case .boolean(let value): self = .boolean(value); case .text(let value): self = .text(value); case .steps(let minutes): self = .steps(minutes); case .none: self = .none}
    }
    var raw: MeguriCoreBindings.OptionValue {
        switch self {case .number(let value): return .number(value: value); case .boolean(let value): return .boolean(value: value); case .text(let value): return .text(value: value); case .steps(let minutes): return .steps(minutes: minutes); case .none: return .none}
    }
}
public struct OptionIssue: Sendable, Hashable {
    public let field: OptionField
    public let preset: Int64?
    public let code: OptionIssueCode
    public let severity: OptionSeverity
    public let params: [String: String]
    init(_ raw: MeguriCoreBindings.OptionIssue) {field = OptionField(raw.field); preset = raw.preset; code = OptionIssueCode(raw.code); severity = OptionSeverity(raw.severity); params = raw.params}
}
public struct OptionMetadata: Sendable, Hashable {
    public let field: OptionField
    public let defaultValue: OptionValue
    public let minimum: Double?
    public let maximum: Double?
    public let step: Double?
    public let whole: Bool
    public let choices: [Int32]
    public let global: Bool
    public let optional: Bool
    init(_ raw: MeguriCoreBindings.OptionMetadata) {field = OptionField(raw.field); defaultValue = OptionValue(raw.defaultValue); minimum = raw.minimum; maximum = raw.maximum; step = raw.step; whole = raw.whole; choices = raw.choices; global = raw.global; optional = raw.optional}
}
private func optionFields(_ raw: [MeguriCoreBindings.OptionFieldValue]) -> [OptionField: OptionValue] {Dictionary(uniqueKeysWithValues: raw.map { (OptionField($0.field), OptionValue($0.value)) })}
public struct OptionPreset: Sendable, Hashable {
    public let key: Int64
    public let name: String
    public let useCount: UInt64
    public let values: [OptionField: OptionValue]
    init(_ raw: MeguriCoreBindings.OptionPreset) {key = raw.key; name = raw.name; useCount = raw.useCount; values = optionFields(raw.values)}
}
public struct OptionTodayLimit: Sendable, Hashable {
    public let limit: UInt32
    public let today: UInt32
    public let active: Bool
    init(_ raw: MeguriCoreBindings.OptionTodayLimit) {limit = raw.limit; today = raw.today; active = raw.active}
}
public struct OptionIgnoreCount: Sendable, Hashable {
    public let included: UInt64
    public let total: UInt64
    public let warning: Bool
    init(_ raw: MeguriCoreBindings.OptionIgnoreCount) {included = raw.included; total = raw.total; warning = raw.warning}
}
public enum OptionLimitScope: Sendable, Hashable {
    case preset, thisDeck, todayOnly
    init(_ raw: MeguriCoreBindings.OptionLimitScope) {switch raw {case .preset: self = .preset; case .thisDeck: self = .thisDeck; case .todayOnly: self = .todayOnly}}
    var raw: MeguriCoreBindings.OptionLimitScope {switch self {case .preset: return .preset; case .thisDeck: return .thisDeck; case .todayOnly: return .todayOnly}}
}
public struct DeckOptionsSnapshot: Sendable, Hashable {
    public let deck: Int64
    public let name: String
    public let newLimitScope: OptionLimitScope
    public let reviewLimitScope: OptionLimitScope
    public let retentionThisDeck: Bool
    public let assignedPreset: Int64
    public let selectedPreset: Int64
    public let presetMissing: Bool
    public let today: UInt32
    public let todayDate: String
    public let presets: [OptionPreset]
    public let deckValues: [OptionField: OptionValue]
    public let globals: [OptionField: OptionValue]
    public let metadata: [OptionMetadata]
    public let newToday: OptionTodayLimit?
    public let reviewToday: OptionTodayLimit?
    public let ignoreCount: OptionIgnoreCount?
    public let unsavedChanges: Bool
    public let subtree: Bool
    public let effectiveNewLimit: UInt32
    public let effectiveReviewLimit: UInt32
    public let effectiveRetention: Float
    init(_ raw: MeguriCoreBindings.DeckOptionsSnapshot) {
        newLimitScope = OptionLimitScope(raw.newLimitScope); reviewLimitScope = OptionLimitScope(raw.reviewLimitScope); retentionThisDeck = raw.retentionThisDeck
        deck = raw.deck; name = raw.name; assignedPreset = raw.assignedPreset; selectedPreset = raw.selectedPreset; presetMissing = raw.presetMissing; today = raw.today; todayDate = raw.todayDate
        presets = raw.presets.map(OptionPreset.init); deckValues = optionFields(raw.deckValues); globals = optionFields(raw.globals); metadata = raw.metadata.map(OptionMetadata.init)
        newToday = raw.newToday.map(OptionTodayLimit.init); reviewToday = raw.reviewToday.map(OptionTodayLimit.init); ignoreCount = raw.ignoreCount.map(OptionIgnoreCount.init)
        unsavedChanges = raw.unsavedChanges; subtree = raw.subtree; effectiveNewLimit = raw.effectiveNewLimit; effectiveReviewLimit = raw.effectiveReviewLimit; effectiveRetention = raw.effectiveRetention
    }
}
public enum DeckOptionsEdit: Sendable, Hashable {
    case setLimit(review: Bool, scope: OptionLimitScope, value: Double)
    case setRetention(thisDeck: Bool, value: Double)
    case set(preset: Int64?, field: OptionField, value: OptionValue)
    case select(preset: Int64), add(name: String), clone(name: String), rename(name: String), remove(preset: Int64)
    case restoreDefault(preset: Int64?, field: OptionField)
    case configure(subtree: Bool)
    var raw: MeguriCoreBindings.DeckOptionsEdit {
        switch self {
        case .setLimit(let review, let scope, let value): return .setLimit(review: review, scope: scope.raw, value: value)
        case .setRetention(let thisDeck, let value): return .setRetention(thisDeck: thisDeck, value: value)
        case .set(let preset, let field, let value): return .set(preset: preset, field: field.raw, value: value.raw)
        case .select(let preset): return .select(preset: preset)
        case .add(let name): return .add(name: name)
        case .clone(let name): return .clone(name: name)
        case .rename(let name): return .rename(name: name)
        case .remove(let preset): return .remove(preset: preset)
        case .restoreDefault(let preset, let field): return .restoreDefault(preset: preset, field: field.raw)
        case .configure(let subtree): return .configure(subtree: subtree)
        }
    }
}
public struct ParsedOptionSteps: Sendable, Hashable {
    public let minutes: [Float]
    public let issues: [OptionIssue]
    init(_ raw: MeguriCoreBindings.ParsedOptionSteps) {minutes = raw.minutes; issues = raw.issues.map(OptionIssue.init)}
}
public struct DeckOptionsMutation: Sendable, Hashable {
    public let summary: MutationSummary
    public let preset: Int64
    public let processed: UInt64
    public let changed: UInt64
    public let skippedPresets: [Int64]
    init(_ raw: MeguriCoreBindings.DeckOptionsMutation) {summary = MutationSummary(raw.summary); preset = raw.preset; processed = raw.processed; changed = raw.changed; skippedPresets = raw.skippedPresets}
}
public struct SchedulingPreferences: Sendable, Hashable {
    public let rollover: UInt8
    public let learnAheadMinutes: UInt32
    public let loadBalancer: Bool
    public let metadata: [OptionMetadata]
    init(_ raw: MeguriCoreBindings.SchedulingPreferences) {rollover = raw.rollover; learnAheadMinutes = raw.learnAheadMinutes; loadBalancer = raw.loadBalancer; metadata = raw.metadata.map(OptionMetadata.init)}
}
public struct SchedulingPreferencesChange: Sendable, Hashable {
    public var rollover: Double?
    public var learnAheadMinutes: Double?
    public var loadBalancer: Bool?
    public init(rollover: Double? = nil, learnAheadMinutes: Double? = nil, loadBalancer: Bool? = nil) {self.rollover = rollover; self.learnAheadMinutes = learnAheadMinutes; self.loadBalancer = loadBalancer}
    var raw: MeguriCoreBindings.SchedulingPreferencesChange { .init(rollover: rollover, learnAheadMinutes: learnAheadMinutes, loadBalancer: loadBalancer) }
}
public final class CoreDeckOptionsEditor: Sendable {
    let raw: MeguriCoreBindings.DeckOptionsEditor
    init(_ raw: MeguriCoreBindings.DeckOptionsEditor) {self.raw = raw}
    public func snapshot() async throws(BridgeError) -> DeckOptionsSnapshot {
        let raw = raw
        return try await InteractiveLane.shared.call(.deckOptionsEditorSnapshot) {DeckOptionsSnapshot(try raw.snapshot())}
    }
    public func edit(_ edit: DeckOptionsEdit) async throws(BridgeError) -> DeckOptionsSnapshot {
        let raw = raw
        return try await InteractiveLane.shared.call(.deckOptionsEditorEdit) {DeckOptionsSnapshot(try raw.edit(edit: edit.raw))}
    }
    public func validate() async throws(BridgeError) -> [OptionIssue] {
        let raw = raw
        return try await InteractiveLane.shared.call(.deckOptionsEditorValidate) {try raw.validate().map(OptionIssue.init)}
    }
    public func startSave() async throws(BridgeError) -> CoreDeckOptionsSaveJob {
        try await CoreEventStream.shared.start()
        let raw = raw
        return try await MaintenanceLane.shared.call(.startSaveDeckOptions) {let start = try MeguriCoreBindings.startSaveDeckOptions(editor: raw); return CoreDeckOptionsSaveJob(job: try Jobs.track(start.handle), raw: start.result)}
    }
    public func startIgnoreCount(preset: Int64, date: String) async throws(BridgeError) -> CoreOptionIgnoreCountJob {
        try await CoreEventStream.shared.start()
        let raw = raw
        return try await MaintenanceLane.shared.call(.startOptionIgnoreCount) {let start = try MeguriCoreBindings.startOptionIgnoreCount(editor: raw, preset: preset, date: date); return CoreOptionIgnoreCountJob(job: try Jobs.track(start.handle), raw: start.result)}
    }
}
public final class CoreDeckOptionsReadJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.DeckOptionsReadJob
    init(job: CoreJob, raw: MeguriCoreBindings.DeckOptionsReadJob) {self.job = job; self.raw = raw}
    public func value() async throws(BridgeError) -> CoreDeckOptionsEditor {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.deckOptionsReadJobResult) {
            guard let value = try raw.result() else {throw BridgeError.internal("Successful job has no result")}
            return CoreDeckOptionsEditor(value)
        }
    }
}
public final class CoreDeckOptionsSaveJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.DeckOptionsSaveJob
    init(job: CoreJob, raw: MeguriCoreBindings.DeckOptionsSaveJob) {self.job = job; self.raw = raw}
    public func value() async throws(BridgeError) -> DeckOptionsMutation {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.deckOptionsSaveJobResult) {
            guard let value = try raw.result() else {throw BridgeError.internal("Successful job has no result")}
            return DeckOptionsMutation(value)
        }
    }
}
public final class CoreOptionIgnoreCountJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.OptionIgnoreCountJob
    init(job: CoreJob, raw: MeguriCoreBindings.OptionIgnoreCountJob) {self.job = job; self.raw = raw}
    public func value() async throws(BridgeError) -> OptionIgnoreCount {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.optionIgnoreCountJobResult) {
            guard let value = try raw.result() else {throw BridgeError.internal("Successful job has no result")}
            return OptionIgnoreCount(value)
        }
    }
}
public final class CoreSchedulingPreferencesJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.SchedulingPreferencesJob
    init(job: CoreJob, raw: MeguriCoreBindings.SchedulingPreferencesJob) {self.job = job; self.raw = raw}
    public func value() async throws(BridgeError) -> SchedulingPreferences {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.schedulingPreferencesJobResult) {
            guard let value = try raw.result() else {throw BridgeError.internal("Successful job has no result")}
            return SchedulingPreferences(value)
        }
    }
}
public final class CoreSchedulingPreferencesSaveJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.SchedulingPreferencesSaveJob
    init(job: CoreJob, raw: MeguriCoreBindings.SchedulingPreferencesSaveJob) {self.job = job; self.raw = raw}
    public func value() async throws(BridgeError) -> MutationSummary {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.schedulingPreferencesSaveJobResult) {
            guard let value = try raw.result() else {throw BridgeError.internal("Successful job has no result")}
            return MutationSummary(value)
        }
    }
}
public actor CoreDeckOptionsService {
    public init() {}
    public func startRead(deck: Int64) async throws(BridgeError) -> CoreDeckOptionsReadJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startDeckOptions) {let start = try MeguriCoreBindings.startDeckOptions(deck: deck); return CoreDeckOptionsReadJob(job: try Jobs.track(start.handle), raw: start.result)}
    }
    public func parseSteps(_ text: String) async throws(BridgeError) -> ParsedOptionSteps {
        try await InteractiveLane.shared.call(.parseOptionSteps) {ParsedOptionSteps(try MeguriCoreBindings.parseOptionSteps(text: text))}
    }
    public func formatSteps(_ minutes: [Float]) async throws(BridgeError) -> String {
        try await InteractiveLane.shared.call(.formatOptionSteps) {try MeguriCoreBindings.formatOptionSteps(minutes: minutes)}
    }
    public func startPreferencesRead() async throws(BridgeError) -> CoreSchedulingPreferencesJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startSchedulingPreferences) {let start = try MeguriCoreBindings.startSchedulingPreferences(); return CoreSchedulingPreferencesJob(job: try Jobs.track(start.handle), raw: start.result)}
    }
    public func validatePreferences(_ change: SchedulingPreferencesChange) async throws(BridgeError) -> [OptionIssue] {
        try await InteractiveLane.shared.call(.validateSchedulingPreferences) {try MeguriCoreBindings.validateSchedulingPreferences(change: change.raw).map(OptionIssue.init)}
    }
    public func startPreferencesSave(_ change: SchedulingPreferencesChange) async throws(BridgeError) -> CoreSchedulingPreferencesSaveJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startSaveSchedulingPreferences) {let start = try MeguriCoreBindings.startSaveSchedulingPreferences(change: change.raw); return CoreSchedulingPreferencesSaveJob(job: try Jobs.track(start.handle), raw: start.result)}
    }
}
