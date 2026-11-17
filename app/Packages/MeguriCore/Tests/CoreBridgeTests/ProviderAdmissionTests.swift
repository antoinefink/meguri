import Foundation
import Testing
@testable import CoreBridge

@Suite struct ProviderAdmissionTests {
    @Test func malformedLegacyValuesAreNotPerFieldDefaults() throws {
        let values: [Any] = [NSNull(), [:] as [String: Any], ["enabled": 1, "paused": false, "automatic": true, "media": true] as [String: Any]]
        for value in values {
            #expect(throws: (any Error).self) { try CoreProviderService.strictLegacyJSON(value) }
        }
        #expect(try CoreProviderService.strictLegacyJSON(nil) == nil)
        let settings: [String: Any] = ["enabled": true, "paused": true, "automatic": false, "media": false]
        #expect(try CoreProviderService.strictLegacyJSON(settings) != nil)
    }
}
