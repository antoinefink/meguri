import CoreBridge
import Foundation
import Security
import Testing

@Suite struct MeguriProviderCredentialIntegrationTests {
    @Test func providerSlotIsDeviceOnlyStrictAndSeparateFromTheManualSlot() async throws {
        let service = "com.antoinefink.meguri.provider.test.\(UUID().uuidString)"
        let store = DeviceMeguriProviderCredentialStore(service: service)
        let manual = DeviceManualTransferCredentialStore(service: "com.antoinefink.meguri.manual-transfer.test.\(UUID().uuidString)")
        let connection = MeguriProviderConnection(reference: UUID().uuidString, revision: 1, endpoint: "https://synthetic.example/prefix/")
        let credentials = MeguriProviderCredentials(connection: connection, username: "synthetic.user", hostKey: "synthetic-key")
        // Cleanup includes a partial add/save failure, not only successful save.
        do {
            try await manual.save(.init(username: "Synthetic Manual", hostKey: "synthetic-manual-key"), endpoint: "https://manual.synthetic.example/")
            try await store.save(credentials)
            #expect(try await store.load(connection: connection) == credentials)
            let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service, kSecAttrAccount as String: connection.reference,
                kSecUseDataProtectionKeychain as String: true, kSecAttrSynchronizable as String: false,
                kSecReturnAttributes as String: true, kSecReturnData as String: true]
            var result: CFTypeRef?
            #expect(SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess)
            let attributes = try #require(result as? [String: Any])
            #expect(attributes[kSecAttrAccessible as String] as? String == kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String)
            #expect(attributes[kSecAttrSynchronizable as String] as? Bool == false)
            let data = try #require(attributes[kSecValueData as String] as? Data)
            let payload = try #require(try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: String])
            #expect(Set(payload.keys) == ["endpoint", "reference", "revision", "username", "hostKey"])
            #expect(payload["password"] == nil)
            var manualQuery = query
            manualQuery[kSecAttrAccount as String] = "active"
            #expect(SecItemCopyMatching(manualQuery as CFDictionary, nil) == errSecItemNotFound)
            try await store.delete(connection: connection)
            #expect(try await store.load(connection: connection) == nil)
            let originalManual = try #require(try await manual.load(endpoint: "https://manual.synthetic.example/"))
            #expect(originalManual.username == "Synthetic Manual" && originalManual.hostKey == "synthetic-manual-key")
            try await manual.clear()
        } catch { try? await store.delete(connection: connection); try? await manual.clear(); throw error }
    }
}
