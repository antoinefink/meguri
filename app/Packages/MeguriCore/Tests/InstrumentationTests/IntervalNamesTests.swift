import Testing

@testable import Instrumentation

/// The registry (R1861, U12): one name per interval, the R1861 set present
/// under the names the plan's units use, bridge calls in their own category.
@Suite struct IntervalNamesTests {
    /// R1861's intervals, as the plan names them (U175, U176, U237, U241)
    /// or as U12 named them.
    static let r1861 = [
        "review.answerToNextVisible",
        "review.revealToAnswerVisible",
        "launch.toDeckListInteractive",
        "search.keystrokeToResultsVisible",
        "browser.openToFirstPage",
        "stats.firstGraphs",
        "editor.openToEditable",
        "sync.startToEnd",
    ]

    @Test func everyR1861IntervalIsRegisteredAndReportedToFieldDiagnostics() throws {
        for name in Self.r1861 {
            let interval = try #require(IntervalNames.named(name), "\(name) is not registered")
            #expect(interval.fieldReported, "\(name) must reach MetricKit (R1867)")
            #expect(interval.category != .bridge && interval.category != .probe)
        }
        // Only R1861's intervals go to MetricKit, which keeps a few per app.
        #expect(Set(IntervalNames.all.filter(\.fieldReported).map(\.name)) == Set(Self.r1861))
    }

    @Test func namesAreUniqueAndAreaQualified() {
        let names = IntervalNames.all.map(\.name)
        #expect(Set(names).count == names.count)
        for interval in IntervalNames.all where interval.category != .bridge {
            #expect(
                interval.name.wholeMatch(of: /[a-z][A-Za-z]*\.[a-z][A-Za-z]*/) != nil,
                "\(interval.name) is not area.camelCase")
        }
    }

    @Test func unitsRegisteredNamesTheyEmitLater() {
        // Named in the plan as registered here (U176, U237, U241).
        for name in [
            "review.startToFirstCard", "review.presentToNextSettled", "review.onDemandRender",
            "review.answerMainThread", "review.undoToCardVisible", "review.actionToNextVisible",
            "review.touchDownToTint", "search.firstPage", "stats.allGraphs", "stats.scopeChange",
            "heatmap.render", "cardInfo.open",
        ] {
            #expect(IntervalNames.named(name) != nil, "\(name)")
        }
        // The rendering spike's baseline swap (U65), a test probe.
        #expect(IntervalNames.named("probe.nativeSwap")?.category == .probe)
    }

    @Test func bridgeCallsAreSignpostedUnderTheirExportNamesOnly() {
        #expect(BridgeCall.all.map(\.name) == [
            "core_version", "core_build_info", "set_event_listener",
            "CancelToken.new", "CancelToken.cancel", "CancelToken.isCancelled",
        ])
        for call in BridgeCall.all {
            #expect(call.interval.category == .bridge)
            #expect(!call.interval.fieldReported, "bridge calls never go to MetricKit")
            #expect(IntervalNames.named(call.name) == call.interval)
        }
    }

    @Test func anUnknownNameIsNotFound() {
        #expect(IntervalNames.named("review.answerToNext") == nil)
        #expect(IntervalNames.named("") == nil)
    }

    @Test func oneSubsystemForEveryInterval() {
        #expect(Set(IntervalNames.all.map(\.subsystem)) == ["com.antoinefink.meguri"])
        #expect(IntervalNames.answerToNextVisible.category.rawValue == "Review")
        #expect(BridgeCall.coreVersion.interval.category.rawValue == "CoreBridge")
    }

    @Test func signpostsBeginAndEndEveryKindOfInterval() throws {
        for interval in IntervalNames.all {
            Signposts.end(Signposts.begin(interval))
        }
        #expect(Signposts.measure(IntervalNames.probeSyntheticDelay) { 42 } == 42)
        #expect(Signposts.bridge(.coreVersion, lane: "control") { "v" } == "v")
        struct Failure: Error {}
        #expect(throws: Failure.self) {
            try Signposts.measure(IntervalNames.answerToNextVisible) { () throws(Failure) in throw Failure() }
        }
    }
}
