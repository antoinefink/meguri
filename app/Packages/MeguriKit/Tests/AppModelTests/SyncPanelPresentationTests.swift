@testable import AppModel
@testable import CoreBridge
#if canImport(AppKit)
import AppKit
#endif
import Foundation
import Testing

/// `sync/status-strings` T20.3 and native-interface-contract N23 (round 10).
@MainActor @Suite struct SyncPanelPresentationTests {
    private typealias Slot = SyncPanel.Slot


    @Test func barShowsMeguriSetupAndRoundsButNeverNeedsYouOrOff() {
        let setup = SyncKinds.meguri(.init(stage: .media))
        #expect(SyncPanel.progress(setup, setupValue: .fraction(0.25), moving: true) == .fraction(0.25))
        #expect(SyncPanel.progress(setup, setupValue: .indeterminate, moving: true) == .indeterminate)
        let round = SyncKinds.meguri(.init(routine: .exchanging))
        #expect(SyncPanel.progress(round, setupValue: nil, moving: true) == .indeterminate)
        #expect(SyncPanel.progress(SyncKinds.off, setupValue: nil, moving: false) == nil)
        let needsYou = SyncKinds.meguri(.init(routine: .authentication))
        #expect(SyncPanel.progress(needsYou, setupValue: .fraction(0.5), moving: true) == nil)
    }
    @Test func statusUsesNeutralGlyphsWhileOffKeepsItsOwnSemanticValue() {
        #expect(SyncSymbol.glyph(SyncSymbol.off) == "cloud")
        #expect(SyncSymbol.off != SyncSymbol.waiting)
        #expect(SyncSymbol.glyph(SyncSymbol.upToDate) == "checkmark.circle")
        #expect(SyncSymbol.glyph(SyncSymbol.moving) == "arrow.trianglehead.2.clockwise.rotate.90")
        #expect(SyncSymbol.glyph(SyncSymbol.needsYou) == "exclamationmark.arrow.trianglehead.2.clockwise.rotate.90")
    }
}
