import AppModel
import Foundation
import Testing
@testable import Meguri

@MainActor struct FieldDiagnosticCompositionTests {
    @Test func testHostLaunchExcludesProductionStoreSubscriberAndClipboardCallbacks() throws {
        try #require(ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil)
        let actual = MeguriApp().diagnosticComposition
        #expect(actual.collection == nil)
        #expect(actual.subscriber == nil)
        #expect(actual.configuration.fieldDiagnostics == nil)
        // Exercise the actual default recorder after composition, without
        // consulting a singleton store or reading/writing a clipboard.
        actual.configuration.recordDiagnostic(.init(component: .startup, operation: .open,
            stage: .requested, outcome: .observed))
        #expect(actual.configuration.fieldDiagnostics == nil)
    }
}
