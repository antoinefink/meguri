#if os(macOS)
import Foundation
import CryptoKit
import Darwin

/// Private runner input. The app remains the authority for transport installation.
/// Never interpolate this value or its credentials into XCTest diagnostics.
struct OwnedProviderJourneyInput {
    let runID: String
    let transportManifestSHA256: String
    let address: String
    let username: String
    let password: String

    private enum Refusal: Error { case refused }

    static func admit(repositoryRoot: URL) throws -> Self? {
        let environment = ProcessInfo.processInfo.environment
        let run = environment["MEGURI_OWNED_PROVIDER_JOURNEY_RUN_ID"]
        let digest = environment["MEGURI_OWNED_PROVIDER_JOURNEY_CREDENTIALS_SHA256"]
        if run == nil && digest == nil { return nil }
        do {
            guard let run, let digest, validRun(run), validDigest(digest) else { throw Refusal.refused }
            let raw = try read(repositoryRoot: repositoryRoot, run: run, name: "credentials.json", cap: 16 * 1024)
            guard sha256(raw) == digest else { throw Refusal.refused }
            let fields = try credentialFields(raw)
            guard fields["run_id"] == run,
                  let transportDigest = fields["transport_manifest_sha256"], validDigest(transportDigest),
                  let username = fields["username"], validUsername(username),
                  let password = fields["password"], (1...1024).contains(password.utf8.count),
                  !password.utf8.contains(0) else { throw Refusal.refused }
            let transport = try read(repositoryRoot: repositoryRoot, run: run, name: "transport.json", cap: 128 * 1024)
            guard sha256(transport) == transportDigest else { throw Refusal.refused }
            return Self(runID: run, transportManifestSHA256: transportDigest,
                address: try transportOrigin(transport, run: run), username: username, password: password)
        } catch { throw Refusal.refused }
    }

    private static func alphanumeric(_ byte: UInt8) -> Bool {
        (65...90).contains(byte) || (97...122).contains(byte) || (48...57).contains(byte)
    }

    private static func validRun(_ value: String) -> Bool {
        let bytes = Array(value.utf8)
        return (1...96).contains(bytes.count) && alphanumeric(bytes[0])
            && bytes.allSatisfy { alphanumeric($0) || $0 == 45 || $0 == 95 }
    }

    private static func validDigest(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }

    private static func validUsername(_ value: String) -> Bool {
        let bytes = Array(value.utf8)
        return (3...64).contains(bytes.count) && alphanumeric(bytes[0])
            && bytes.allSatisfy { (97...122).contains($0) || (48...57).contains($0) || [45, 46, 95].contains($0) }
    }

    private static func sha256(_ raw: Data) -> String {
        SHA256.hash(data: raw).map { String(format: "%02x", $0) }.joined()
    }

    /// Only this four-string object is accepted, including no duplicate keys.
    /// Foundation decodes each JSON string; no input-selected paths are decoded.
    private static func credentialFields(_ raw: Data) throws -> [String: String] {
        let bytes = Array(raw)
        var index = 0
        func whitespace() {
            while index < bytes.count && [9, 10, 13, 32].contains(bytes[index]) { index += 1 }
        }
        func take(_ byte: UInt8) throws {
            whitespace()
            guard index < bytes.count, bytes[index] == byte else { throw Refusal.refused }
            index += 1
        }
        func string() throws -> String {
            whitespace()
            let start = index
            try take(34)
            while index < bytes.count {
                let byte = bytes[index]
                index += 1
                if byte == 34 { return try JSONDecoder().decode(String.self, from: Data(bytes[start..<index])) }
                if byte == 92 {
                    guard index < bytes.count else { throw Refusal.refused }
                    index += 1
                }
            }
            throw Refusal.refused
        }
        try take(123)
        var fields: [String: String] = [:]
        let keys: Set<String> = ["run_id", "transport_manifest_sha256", "username", "password"]
        for field in 0..<4 {
            if field > 0 { try take(44) }
            let key = try string()
            guard keys.contains(key), fields[key] == nil else { throw Refusal.refused }
            try take(58)
            fields[key] = try string()
        }
        try take(125)
        whitespace()
        guard index == bytes.count, Set(fields.keys) == keys else { throw Refusal.refused }
        return fields
    }

    /// Keep the existing transport consumer's exact eight-field schema. This
    /// extracts the pinned origin, never creates a replacement transport DTO.
    private static func transportOrigin(_ raw: Data, run: String) throws -> String {
        guard let fields = try JSONSerialization.jsonObject(with: raw) as? [String: Any],
              Set(fields.keys) == Set(["format", "purpose", "run_id", "origin", "ca_der_base64",
                  "ca_der_sha256", "app_executable_sha256", "app_debug_dylib_sha256"]),
              let format = fields["format"] as? NSNumber,
              ["i", "q"].contains(String(cString: format.objCType)), format.intValue == 1,
              fields["purpose"] as? String == "meguri-owned-provider-transport",
              fields["run_id"] as? String == run,
              let origin = fields["origin"] as? String,
              origin.utf8.count <= 2048, origin.hasPrefix("https://"),
              origin.utf8.allSatisfy({ (33...126).contains($0) && $0 != 37 && $0 != 92 }),
              let url = URLComponents(string: origin), url.scheme == "https",
              let host = url.host, !host.isEmpty, url.user == nil, url.password == nil,
              url.query == nil, url.fragment == nil,
              url.percentEncodedPath.isEmpty || url.percentEncodedPath == "/",
              url.port.map({ (1...65535).contains($0) }) ?? true,
              let ca = fields["ca_der_base64"] as? String, (4...87384).contains(ca.utf8.count),
              let der = Data(base64Encoded: ca), !der.isEmpty, der.count <= 65536,
              der.base64EncodedString() == ca,
              let caDigest = fields["ca_der_sha256"] as? String, validDigest(caDigest), sha256(der) == caDigest,
              let executable = fields["app_executable_sha256"] as? String, validDigest(executable),
              let debug = fields["app_debug_dylib_sha256"] as? String, validDigest(debug) else { throw Refusal.refused }
        return origin
    }

    private static func read(repositoryRoot: URL, run: String, name: String, cap: Int) throws -> Data {
        guard repositoryRoot.isFileURL else { throw Refusal.refused }
        let path = repositoryRoot.path(percentEncoded: false)
        guard path.hasPrefix("/"), !path.utf8.contains(0) else { throw Refusal.refused }
        var components = path.split(separator: "/", omittingEmptySubsequences: false).dropFirst().map(String.init)
        if components.last == "" { components.removeLast() }
        components += ["build", "fixtures", "owned-meguri-provider", run, name]
        guard components.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." && !$0.contains("/") }) else {
            throw Refusal.refused
        }
        let flags = O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK
        let root = open("/", flags | O_DIRECTORY)
        guard root >= 0 else { throw Refusal.refused }
        var descriptors = [root]
        defer { for fd in descriptors.reversed() { Darwin.close(fd) } }
        var links: [(parent: Int32, name: String, fd: Int32, before: stat)] = []
        for (index, component) in components.enumerated() {
            let parent = descriptors[descriptors.count - 1]
            let fd = openat(parent, component, flags | (index == components.count - 1 ? 0 : O_DIRECTORY))
            guard fd >= 0 else { throw Refusal.refused }
            descriptors.append(fd)
            let before = try status(fd)
            try refuseAlias(fd)
            links.append((parent, component, fd, before))
        }
        guard let file = links.last, file.before.st_mode & S_IFMT == S_IFREG,
              file.before.st_uid == geteuid(), file.before.st_mode & 0o7777 == 0o600,
              file.before.st_nlink == 1, file.before.st_size > 0, file.before.st_size <= off_t(cap) else { throw Refusal.refused }
        let size = Int(file.before.st_size)
        var raw = Data()
        var buffer = [UInt8](repeating: 0, count: min(cap, 16 * 1024))
        var interruptions = 0
        while true {
            let amount = min(buffer.count, size - raw.count + 1)
            let count = buffer.withUnsafeMutableBytes { Darwin.read(file.fd, $0.baseAddress, amount) }
            if count < 0 {
                guard errno == EINTR, interruptions < 16 else { throw Refusal.refused }
                interruptions += 1
                continue
            }
            if count == 0 { break }
            guard raw.count + count <= size else { throw Refusal.refused }
            raw.append(contentsOf: buffer.prefix(count))
        }
        guard raw.count == size else { throw Refusal.refused }
        for link in links {
            try stable(link.before, status(link.fd))
            var named = stat()
            guard fstatat(link.parent, link.name, &named, AT_SYMLINK_NOFOLLOW) == 0 else { throw Refusal.refused }
            try stable(link.before, named)
            try refuseAlias(link.fd)
        }
        return raw
    }

    private static func status(_ fd: Int32) throws -> stat {
        var value = stat()
        guard fstat(fd, &value) == 0 else { throw Refusal.refused }
        return value
    }

    private static func stable(_ before: stat, _ after: stat) throws {
        guard before.st_dev == after.st_dev, before.st_ino == after.st_ino,
              before.st_mode == after.st_mode, before.st_uid == after.st_uid, before.st_gid == after.st_gid,
              before.st_size == after.st_size, before.st_nlink == after.st_nlink, before.st_flags == after.st_flags,
              before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
              before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
              before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
              before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec else { throw Refusal.refused }
    }

    private static func refuseAlias(_ fd: Int32) throws {
        var finder = [UInt8](repeating: 0, count: 32)
        let count = finder.withUnsafeMutableBytes { fgetxattr(fd, "com.apple.FinderInfo", $0.baseAddress, $0.count, 0, 0) }
        if count < 0 { guard errno == ENOATTR else { throw Refusal.refused }; return }
        guard count == 32, finder[8] & 0x80 == 0, Array(finder.prefix(4)) != Array("alis".utf8) else { throw Refusal.refused }
    }
}
#endif
