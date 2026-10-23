import Foundation
import Security

public protocol ManualTransferCredentialStore: Sendable {
    func load(endpoint: String) async throws -> ManualTransferCredentials?
    func save(_ credentials: ManualTransferCredentials, endpoint: String) async throws
    func clear() async throws
}
/// One device-local slot. Endpoint configuration is stored separately. A slot
/// never synchronizes, migrates to another device or contains a password.
public actor DeviceManualTransferCredentialStore: ManualTransferCredentialStore {
    private let service: String
    public init(service: String = "com.antoinefink.meguri.manual-transfer") { self.service = service }
    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
         kSecUseDataProtectionKeychain as String: true, kSecAttrAccount as String: "active", kSecAttrSynchronizable as String: false]
    }
    public func load(endpoint: String) throws -> ManualTransferCredentials? {
        var query = query
        query[kSecReturnData as String] = true; query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data,
              let payload = try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: String],
              payload["endpoint"] == endpoint, let username = payload["username"], let key = payload["hostKey"] else {
            if status == errSecSuccess { return nil }
            throw CredentialStoreFailure(status: status)
        }
        return .init(username: username, hostKey: key)
    }
    public func save(_ credentials: ManualTransferCredentials, endpoint: String) throws {
        let data = try PropertyListSerialization.data(fromPropertyList: ["endpoint": endpoint, "username": credentials.username, "hostKey": credentials.hostKey], format: .binary, options: 0)
        try clear()
        var query = query
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        query[kSecValueData as String] = data
        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else { throw CredentialStoreFailure(status: status) }
    }
    public func clear() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw CredentialStoreFailure(status: status) }
    }
}
public struct CredentialStoreFailure: Error, Sendable { public let status: Int32 }
