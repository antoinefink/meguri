import Foundation
import Security

public struct MeguriProviderConnection: Sendable, Equatable {
    public let reference: String
    public let revision: UInt64
    public let endpoint: String
    public init(reference: String, revision: UInt64, endpoint: String) {
        self.reference = reference; self.revision = revision; self.endpoint = endpoint
    }
}
public struct MeguriProviderCredentials: Sendable, Equatable {
    public let connection: MeguriProviderConnection
    public let username: String
    public let hostKey: String
    public init(connection: MeguriProviderConnection, username: String, hostKey: String) {
        self.connection = connection; self.username = username; self.hostKey = hostKey
    }
}
public enum MeguriProviderCredentialFailure: Error, Sendable, Equatable {
    case access(Int32), malformedPayload, wrongConnection
}
public protocol MeguriProviderCredentialStore: Sendable {
    func load(connection: MeguriProviderConnection) async throws -> MeguriProviderCredentials?
    func save(_ credentials: MeguriProviderCredentials) async throws
    func delete(connection: MeguriProviderConnection) async throws
}
/// Separate from the retained manual-transfer namespace. Access is scoped to
/// the original connection; an inactive slot is never read at iCloud startup.
public actor DeviceMeguriProviderCredentialStore: MeguriProviderCredentialStore {
    private let service: String
    public init(service: String = "com.antoinefink.meguri.provider") { self.service = service }
    private func query(_ connection: MeguriProviderConnection) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
         kSecAttrAccount as String: connection.reference, kSecUseDataProtectionKeychain as String: true,
         kSecAttrSynchronizable as String: false]
    }
    static func decode(_ data: Data, connection: MeguriProviderConnection) throws -> MeguriProviderCredentials {
        guard let payload = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: String],
              Set(payload.keys) == ["endpoint", "reference", "revision", "username", "hostKey"],
              let revision = payload["revision"].flatMap(UInt64.init), revision != 0,
              String(revision) == payload["revision"], let endpoint = payload["endpoint"],
              let reference = payload["reference"], !reference.isEmpty,
              let username = payload["username"], !username.isEmpty,
              let key = payload["hostKey"], !key.isEmpty else { throw MeguriProviderCredentialFailure.malformedPayload }
        let usernameBytes = Array(username.utf8)
        let alphanumeric: (UInt8) -> Bool = { (97...122).contains($0) || (48...57).contains($0) }
        guard (3...64).contains(usernameBytes.count), let first = usernameBytes.first, alphanumeric(first),
              usernameBytes.allSatisfy({ alphanumeric($0) || $0 == 95 || $0 == 45 || $0 == 46 }),
              URL(string: endpoint)?.scheme == "https" else { throw MeguriProviderCredentialFailure.malformedPayload }
        guard endpoint == connection.endpoint, reference == connection.reference, revision == connection.revision else {
            throw MeguriProviderCredentialFailure.wrongConnection
        }
        return .init(connection: connection, username: username, hostKey: key)
    }
    public func load(connection: MeguriProviderConnection) throws -> MeguriProviderCredentials? {
        var query = query(connection)
        query[kSecReturnData as String] = true; query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw MeguriProviderCredentialFailure.access(status) }
        guard let data = result as? Data else { throw MeguriProviderCredentialFailure.malformedPayload }
        return try Self.decode(data, connection: connection)
    }
    public func save(_ credentials: MeguriProviderCredentials) throws {
        let connection = credentials.connection
        let data = try PropertyListSerialization.data(fromPropertyList: ["endpoint": connection.endpoint,
            "reference": connection.reference, "revision": String(connection.revision),
            "username": credentials.username, "hostKey": credentials.hostKey], format: .binary, options: 0)
        _ = try Self.decode(data, connection: connection)
        let attributes: [String: Any] = [kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
                                       kSecValueData as String: data]
        let query = query(connection)
        var status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            status = SecItemAdd(query.merging(attributes) { _, new in new } as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw MeguriProviderCredentialFailure.access(status) }
    }
    public func delete(connection: MeguriProviderConnection) throws {
        let status = SecItemDelete(query(connection) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw MeguriProviderCredentialFailure.access(status) }
    }
}
