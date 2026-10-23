import Foundation
import Security
import Testing
import CoreBridge

@Suite struct ManualTransferCredentialIntegrationTests {
    @Test func deviceSlotIsNonsynchronizingAndNeverContainsPassword() async throws {
        let service = "com.antoinefink.meguri.manual-transfer.test.\(UUID().uuidString)"
        let store = DeviceManualTransferCredentialStore(service: service)
        let endpoint = "http://127.0.0.1:28711/prefix/"
        try await store.save(.init(username: "Synthetic User", hostKey: "synthetic-key"), endpoint: endpoint)
        do {
            let restored = try #require(try await store.load(endpoint: endpoint))
            #expect(restored.username == "Synthetic User" && restored.hostKey == "synthetic-key")
            #expect(try await store.load(endpoint: "http://127.0.0.1:28712/") == nil)
            let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                kSecUseDataProtectionKeychain as String: true, kSecAttrSynchronizable as String: false,
                kSecReturnAttributes as String: true, kSecReturnData as String: true]
            var found: CFTypeRef?
            #expect(SecItemCopyMatching(query as CFDictionary, &found) == errSecSuccess)
            let attributes = try #require(found as? [String: Any])
            #expect(attributes[kSecAttrAccessible as String] as? String == kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String)
            let data = try #require(attributes[kSecValueData as String] as? Data)
            let payload = try #require(try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: String])
            #expect(Set(payload.keys) == ["endpoint", "username", "hostKey"])
            try await store.clear()
            #expect(try await store.load(endpoint: endpoint) == nil)
        } catch { try? await store.clear(); throw error }
    }
}
