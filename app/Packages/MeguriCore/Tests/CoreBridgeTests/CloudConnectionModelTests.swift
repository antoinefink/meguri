import Foundation
import MeguriCoreBindings
import Testing
@testable import CoreBridge

struct CloudConnectionModelTests {
    @Test func resumeDecisionAndCountsAreMappedWithoutCreatingAuthority() {
        let summary = CoreBridge.CloudConnectionSummary(MeguriCoreBindings.CloudConnectionSummary(
            localCollectionPresent: true, initialAdoptionRequired: false,
            localCounts: [.init(kind: .note, count: 12)], localEmpty: false,
            manifestCounts: [.init(kind: .card, count: 15)], publishedAtMs: 123,
            decision: .confirmResume, unpublishedCounts: [.init(kind: .tag, count: 2)],
            resumeBindingId: Data([1, 2]), unsentCounts: [.init(kind: .note, count: 3), .init(kind: .revlog, count: 4)]))
        #expect(summary.decision == .confirmResume && !summary.initialAdoptionRequired)
        #expect(summary.resumeBindingId == Data([1, 2]))
        #expect(summary.unsentCounts == [.note: 3, .revlog: 4] && summary.unsentChangeCount == 7)
        #expect(summary.unpublishedCounts == [.tag: 2])
        #expect(summary.localCounts == [.note: 12] && summary.manifestCounts == [.card: 15])
        #expect(summary.publishedAtMilliseconds == 123)
    }
    @Test func allConnectionDecisionsAndAbsentResumeFieldsMap() {
        let decisions: [(MeguriCoreBindings.CloudConnectionDecision, CoreBridge.CloudConnectionDecision)] = [
            (.noCollection, .noCollection), (.confirmUpload, .confirmUpload), (.confirmAdoption, .confirmAdoption),
            (.chooseCollection, .chooseCollection), (.cloudUnreadable, .cloudUnreadable), (.confirmResume, .confirmResume)
        ]
        for (raw, expected) in decisions {
            let summary = CoreBridge.CloudConnectionSummary(MeguriCoreBindings.CloudConnectionSummary(
                localCollectionPresent: false, initialAdoptionRequired: false, localCounts: [], localEmpty: true,
                manifestCounts: nil, publishedAtMs: nil, decision: raw, unpublishedCounts: [], resumeBindingId: nil, unsentCounts: []))
            #expect(summary.decision == expected && summary.resumeBindingId == nil)
            #expect(summary.unsentCounts.isEmpty && summary.unsentChangeCount == 0)
        }
    }
    @Test func countPresentationSaturatesAndAccountMismatchRemainsTyped() {
        let summary = CoreBridge.CloudConnectionSummary(MeguriCoreBindings.CloudConnectionSummary(
            localCollectionPresent: true, initialAdoptionRequired: false, localCounts: [], localEmpty: false,
            manifestCounts: nil, publishedAtMs: nil, decision: .confirmResume, unpublishedCounts: [], resumeBindingId: nil,
            unsentCounts: [.init(kind: .note, count: .max), .init(kind: .card, count: 1)]))
        #expect(summary.unsentChangeCount == .max)
        #expect(CoreBridge.CloudConnectionFailure(MeguriCoreBindings.CloudConnectionFailure.accountMismatch) == .accountMismatch)
    }
}
