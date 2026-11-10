import Darwin
import Foundation
import MachO
import ObjectiveC

// Compiled with the agent's shared support in MeguriSnapshotTests. Bundle(for:)
// and class_getImageName identify this code, never a caller-selected bundle.
private final class AgentRunningProductAnchor: NSObject {}

nonisolated struct AgentRunningImageIdentity: Equatable {
    let cpu: UInt32, subtype: UInt32, uuid: Data
}
nonisolated struct AgentRunningImage: Equatable {
    let path: String
    let identity: AgentRunningImageIdentity
}
nonisolated struct AgentRunningProductObservation: Equatable {
    struct BundleIdentity: Equatable {
        let root: String, canonicalRoot: String, bundleID: String, executable: String
    }
    enum OS: Equatable {
        case mac(build: String)
        // The runtime input is an observation, not independent destination proof.
        case simulator(runtimeBuild: String)
    }
    let host: BundleIdentity, agent: BundleIdentity
    let appBuild: String
    let executablePath: String, anchorImagePath: String
    let images: [AgentRunningImage]
    let os: OS

    // Select the loaded executable by its OS path, not by dyld order.
    // Binding checks internal aliases declared in the manifest.
    // Producers require exact observed paths.
    func executableImage(resolvingOwnedPath resolve: (String) throws -> String = { $0 }) throws -> AgentRunningImage {
        func owned(_ path: String) -> Bool {
            path.hasPrefix(host.root + "/") || path.hasPrefix(host.canonicalRoot + "/")
        }
        guard !images.isEmpty, images.count <= 4096,
              owned(executablePath), owned(host.executable) else { throw AgentFailure.binding }
        let executable = try resolve(executablePath)
        guard try resolve(host.executable) == executable else { throw AgentFailure.binding }
        let matches = try images.filter { image in
            try owned(image.path) && resolve(image.path) == executable
        }
        guard matches.count == 1 else { throw AgentFailure.binding }
        return matches[0]
    }
}

// Bounded little-endian Mach-O 64 and universal (32/64-bit fat table) parsing.
// The loaded CPU/subtype selects exactly one disk slice, including arm64/arm64e.
nonisolated enum AgentRunningMachO {
    static let commandLimit = 1_048_576
    static func identity(_ bytes: Data, matching wanted: AgentRunningImageIdentity? = nil) throws -> AgentRunningImageIdentity {
        let b = [UInt8](bytes)
        func word(_ p: Int, _ big: Bool = false) throws -> UInt32 {
            guard p >= 0, p <= b.count - 4 else { throw AgentFailure.binding }
            return (0..<4).reduce(UInt32(0)) { $0 | UInt32(b[p + $1]) << UInt32((big ? 3 - $1 : $1) * 8) }
        }
        func wide(_ p: Int, _ big: Bool) throws -> UInt64 {
            let a = UInt64(try word(p, big)), c = UInt64(try word(p + 4, big))
            return big ? a << 32 | c : a | c << 32
        }
        var start = 0, length = b.count
        let magic = try word(0)
        if magic != UInt32(MH_MAGIC_64) {
            guard let wanted else { throw AgentFailure.implementationMissing }
            let big: Bool, fat64: Bool
            switch magic {
            case 0xbebafeca: big = true; fat64 = false
            case 0xcafebabe: big = false; fat64 = false
            case 0xbfbafeca: big = true; fat64 = true
            case 0xcafebabf: big = false; fat64 = true
            default: throw AgentFailure.implementationMissing
            }
            let count = Int(try word(4, big)), stride = fat64 ? 32 : 20
            guard count > 0, count <= 64, 8 + count * stride <= b.count else { throw AgentFailure.binding }
            var matches: [(Int, Int)] = []
            for index in 0..<count {
                let p = 8 + index * stride
                let cpu = try word(p, big), subtype = try word(p + 4, big)
                let offset = fat64 ? try wide(p + 8, big) : UInt64(try word(p + 8, big))
                let size = fat64 ? try wide(p + 16, big) : UInt64(try word(p + 12, big))
                let alignment = try word(p + (fat64 ? 24 : 16), big)
                guard offset >= UInt64(8 + count * stride), offset <= UInt64(b.count), size >= 32,
                      size <= UInt64(b.count) - offset, alignment <= 31,
                      offset % (UInt64(1) << alignment) == 0 else { throw AgentFailure.binding }
                if cpu == wanted.cpu, subtype == wanted.subtype { matches.append((Int(offset), Int(size))) }
            }
            guard matches.count == 1 else { throw AgentFailure.binding }
            (start, length) = matches[0]
        }
        guard length >= 32 else { throw AgentFailure.binding }
        guard try word(start) == UInt32(MH_MAGIC_64) else { throw AgentFailure.implementationMissing }
        let cpu = try word(start + 4), subtype = try word(start + 8)
        let count = Int(try word(start + 16)), size = Int(try word(start + 20))
        guard count > 0, count <= 4096, size <= commandLimit, size <= length - 32 else { throw AgentFailure.binding }
        var cursor = start + 32, uuid: Data?
        let end = cursor + size
        for _ in 0..<count {
            guard cursor <= end - 8 else { throw AgentFailure.binding }
            let command = try word(cursor), size = Int(try word(cursor + 4))
            guard size >= 8, size.isMultiple(of: 8), size <= end - cursor else { throw AgentFailure.binding }
            if command == UInt32(LC_UUID) {
                guard size == 24, uuid == nil else { throw AgentFailure.binding }
                uuid = Data(b[(cursor + 8)..<(cursor + 24)])
            }
            cursor += size
        }
        guard cursor == end, let uuid, uuid.contains(where: { $0 != 0 }) else { throw AgentFailure.binding }
        let found = AgentRunningImageIdentity(cpu: cpu, subtype: subtype, uuid: uuid)
        guard wanted == nil || wanted == found else { throw AgentFailure.binding }
        return found
    }
}

@MainActor enum AgentRunningProductInspector {
    static func observe() throws -> AgentRunningProductObservation {
        func bundle(_ bundle: Bundle) throws -> AgentRunningProductObservation.BundleIdentity {
            guard let id = bundle.bundleIdentifier, !id.isEmpty, let executable = bundle.executableURL,
                  let canonical = realpath(bundle.bundleURL.path, nil) else { throw AgentFailure.binding }
            defer { free(canonical) }
            return .init(root: bundle.bundleURL.path, canonicalRoot: String(cString: canonical),
                         bundleID: id, executable: executable.path)
        }
        let host = try bundle(Bundle.main), agent = try bundle(Bundle(for: AgentRunningProductAnchor.self))
        guard let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String,
              !build.isEmpty, let imageName = class_getImageName(AgentRunningProductAnchor.self) else { throw AgentFailure.binding }
        var size: UInt32 = 4096
        var path = [CChar](repeating: 0, count: Int(size))
        guard _NSGetExecutablePath(&path, &size) == 0 else { throw AgentFailure.implementationMissing }
        let executablePath = try boundedString(path, count: Int(size), maximum: 4096)
        let count = _dyld_image_count()
        guard count > 0, count <= 4096 else { throw AgentFailure.implementationMissing }
        var images: [AgentRunningImage] = []
        for index in 0..<count {
            guard let header = _dyld_get_image_header(index), let name = _dyld_get_image_name(index),
                  header.pointee.magic == UInt32(MH_MAGIC_64) else { throw AgentFailure.implementationMissing }
            let pointer = UnsafeRawPointer(header)
            let raw = pointer.load(as: mach_header_64.self)
            guard Int(raw.sizeofcmds) <= AgentRunningMachO.commandLimit else { throw AgentFailure.binding }
            let bytes = Data(bytes: pointer, count: 32 + Int(raw.sizeofcmds))
            images.append(.init(path: try boundedPath(name), identity: try AgentRunningMachO.identity(bytes)))
        }
        // System injection can precede the executable in dyld's image list.
        // Consumers select the executable from the OS path, never its index.
        let os: AgentRunningProductObservation.OS
        #if targetEnvironment(simulator)
        guard let runtimeBuild = ProcessInfo.processInfo.environment["SIMULATOR_RUNTIME_BUILD_VERSION"],
              !runtimeBuild.isEmpty, runtimeBuild.utf8.count <= 256 else { throw AgentFailure.implementationMissing }
        os = .simulator(runtimeBuild: runtimeBuild)
        #elseif os(macOS)
        var osSize = 0
        guard sysctlbyname("kern.osversion", nil, &osSize, nil, 0) == 0,
              osSize > 1, osSize <= 256 else { throw AgentFailure.implementationMissing }
        var osBytes = [CChar](repeating: 0, count: osSize)
        guard sysctlbyname("kern.osversion", &osBytes, &osSize, nil, 0) == 0 else { throw AgentFailure.implementationMissing }
        os = .mac(build: try boundedString(osBytes, count: osSize, maximum: 256))
        #else
        throw AgentFailure.implementationMissing
        #endif
        return .init(host: host, agent: agent, appBuild: build, executablePath: executablePath,
                     anchorImagePath: try boundedPath(imageName), images: images, os: os)
    }
    static func boundedString(_ bytes: [CChar], count: Int, maximum: Int) throws -> String {
        guard count > 0, count <= maximum, count <= bytes.count,
              let end = bytes.prefix(count).firstIndex(of: 0) else { throw AgentFailure.implementationMissing }
        guard let text = String(bytes: bytes[..<end].map { UInt8(bitPattern: $0) }, encoding: .utf8)
        else { throw AgentFailure.binding }
        return text
    }
    private static func boundedPath(_ pointer: UnsafePointer<CChar>) throws -> String {
        let count = strnlen(pointer, 4097)
        guard count > 0, count <= 4096,
              let text = String(bytes: UnsafeRawBufferPointer(start: pointer, count: count), encoding: .utf8)
        else { throw AgentFailure.binding }
        return text
    }
}

@MainActor final class AgentRunningProductBinding {
    enum Coverage: Equatable { case actualClient, generatedUnitOnly }
    enum MissingIndependentObservation: CaseIterable, Hashable {
        case signaturesGrantsProvisioningAndConfiguration, sourceFreezeAndLaunchPlan
        case deliveredFullInventory, liveDestinationVMLeaseAndInstallation, completeSignedLane
    }
    let receipts: AgentProductReceiptBinding
    let observation: AgentRunningProductObservation
    let coverage: Coverage
    let missingIndependentObservations = Set(MissingIndependentObservation.allCases)
    private let authority: AgentRunningFileAuthority
    private let admittedPath: (String) throws -> String
    private let refresh: (() throws -> AgentRunningProductObservation)?
    private var invalidated = false

    static func inspectActual(receipts: AgentProductReceiptBinding) throws -> AgentRunningProductBinding {
        #if DEBUG && MEGURI_TEST_HOOKS
        return try AgentRunningProductBinding(receipts: receipts, observation: AgentRunningProductInspector.observe(),
            coverage: .actualClient, refresh: { try AgentRunningProductInspector.observe() })
        #else
        // SwiftPM tests cannot establish app-hosted execution.
        throw AgentFailure.implementationMissing
        #endif
    }
    // Explicitly fake-only. No callback or injected observation can construct
    // actualClient coverage or satisfy the independent counterpart requirement.
    static func generatedUnitOnly(receipts: AgentProductReceiptBinding, observation: AgentRunningProductObservation,
                                  refresh: (() throws -> AgentRunningProductObservation)? = nil,
                                  boundary: ((AgentRunningFileAuthority.Boundary) throws -> Void)? = nil) throws -> AgentRunningProductBinding {
        try AgentRunningProductBinding(receipts: receipts, observation: observation, coverage: .generatedUnitOnly,
                                       refresh: refresh, boundary: boundary)
    }
    private init(receipts: AgentProductReceiptBinding, observation o: AgentRunningProductObservation,
                 coverage: Coverage, refresh: (() throws -> AgentRunningProductObservation)?,
                 boundary: ((AgentRunningFileAuthority.Boundary) throws -> Void)? = nil) throws {
        try receipts.revalidate()
        let build = receipts.build, inventory = build.products, header = receipts.header
        guard o.host.bundleID == build.app.bundleID, o.agent.bundleID == build.agent.bundleID,
              !o.appBuild.isEmpty, o.appBuild == build.appBuild, o.appBuild == header.app_build,
              !o.images.isEmpty, o.images.count <= 4096 else { throw AgentFailure.binding }
        switch o.os {
        case .mac(let os): guard header.device_kind == "mac", os == header.os_build else { throw AgentFailure.binding }
        case .simulator(let os): guard header.device_kind != "mac", os == header.os_build else { throw AgentFailure.binding }
        }
        let appRoot = try inventory.resolved(build.app.path)
        let authority = try AgentRunningFileAuthority(root: o.host.canonicalRoot, inventory: inventory,
                                                      appRoot: appRoot, boundary: boundary)
        func inventoryKey(_ path: String) throws -> String {
            for root in [o.host.root, o.host.canonicalRoot] where path.hasPrefix(root + "/") {
                let suffix = String(path.dropFirst(root.count + 1))
                return appRoot + "/" + suffix
            }
            throw AgentFailure.binding
        }
        func admitted(_ path: String) throws -> String {
            let key = try inventoryKey(path)
            try authority.admitLinks(key)
            let resolved = try inventory.resolved(key)
            guard resolved.hasPrefix(appRoot + "/") else { throw AgentFailure.binding }
            return resolved
        }
        let main = try inventory.resolved(build.app.path + "/" + build.app.executable)
        let agent = try inventory.resolved(build.agent.path + "/" + build.agent.executable)
        let agentRoot = try inventory.resolved(build.agent.path)
        let mainImage = try o.executableImage(resolvingOwnedPath: admitted)
        guard o.agent.canonicalRoot == authority.path(agentRoot),
              try admitted(o.host.executable) == main, try admitted(o.executablePath) == main,
              try admitted(mainImage.path) == main, try admitted(o.agent.executable) == agent,
              try admitted(o.anchorImagePath) == agent else { throw AgentFailure.binding }
        // Check aliases declared in the manifest as well as aliases dyld used.
        try authority.admitLinks(build.app.path + "/" + build.app.executable)
        try authority.admitLinks(build.agent.path + "/" + build.agent.executable)
        var expected: Set<String> = [main, agent]
        // Xcode's host implementation image sits beside its actual executable.
        // Require that exact manifest-bound image when present. Widget products
        // execute separately; their delivery remains independent evidence.
        let hostDebugPath = o.host.executable + ".debug.dylib"
        let hostDebugKey = try inventoryKey(hostDebugPath)
        if inventory.entries[hostDebugKey] != nil {
            expected.insert(try admitted(hostDebugPath))
        }
        guard expected.count <= 128 else { throw AgentFailure.implementationMissing }
        var loaded: [String: [AgentRunningImageIdentity]] = [:]
        for image in o.images {
            // System frameworks remain outside this client's subset of the
            // frozen inventory; full dependency inventory is still independent.
            if [o.host.root, o.host.canonicalRoot].contains(where: { image.path.hasPrefix($0 + "/") }) {
                let key = try admitted(image.path)
                guard case .file? = inventory.entries[key] else { throw AgentFailure.binding }
                loaded[key, default: []].append(image.identity)
                expected.insert(key)
            }
        }
        guard expected.count <= 128 else { throw AgentFailure.implementationMissing }
        for key in expected.sorted() {
            guard let images = loaded[key], images.count == 1 else { throw AgentFailure.binding }
            try authority.retainImage(key, identity: images[0])
        }
        try authority.revalidate(); try receipts.revalidate()
        self.receipts = receipts; observation = o; self.coverage = coverage
        self.authority = authority; admittedPath = admitted; self.refresh = refresh
    }
    func requireHeader(_ header: AgentHeader, originalHeaderBytes: Data) throws {
        guard receipts.header == header, receipts.originalHeaderBytes == originalHeaderBytes else { throw AgentFailure.binding }
        try revalidate()
    }
    func revalidate() throws {
        do {
            guard !invalidated else { throw AgentFailure.storage }
            try receipts.revalidate(); try authority.revalidate()
            if let refresh {
                let current = try refresh()
                // No loaded-code substitution after the retained observation.
                func ownedImages(_ value: AgentRunningProductObservation) -> [AgentRunningImage] {
                    value.images.filter { image in
                        image.path.hasPrefix(value.host.root + "/") || image.path.hasPrefix(value.host.canonicalRoot + "/")
                    }
                }
                guard current.host == observation.host, current.agent == observation.agent,
                      current.appBuild == observation.appBuild, current.os == observation.os,
                      current.executablePath == observation.executablePath,
                      current.anchorImagePath == observation.anchorImagePath,
                      try current.executableImage(resolvingOwnedPath: admittedPath)
                        == observation.executableImage(resolvingOwnedPath: admittedPath),
                      ownedImages(current) == ownedImages(observation) else { throw AgentFailure.binding }
            }
            try authority.revalidate(); try receipts.revalidate()
        } catch { invalidated = true; throw (error as? AgentFailure) ?? .storage }
    }
    func requireIndependentCounterpart() throws {
        try revalidate()
        // J2.1's independent source/signature/install/live-lease checks are
        // parent-owned. Actual client coverage retains all four receipt copies,
        // live loaded-image hashes/identities, OS and app-container observations.
        // A generated observation is never admitted to this complementary split.
        guard coverage == .actualClient else { throw AgentFailure.implementationMissing }
    }
}

// Retain direct physical terminal files and every ancestor with no-follow
// descriptors. Only live OS bundle roots and inventoried descendants enter here.
@MainActor final class AgentRunningFileAuthority {
    enum Boundary: Equatable { case beforeRead, afterRead }
    private nonisolated final class Descriptor {
        let value: Int32
        init(_ value: Int32) throws { guard value >= 0 else { throw AgentFailure.storage }; self.value = value }
        deinit { _ = Darwin.close(value) }
    }
    private struct DirectoryIdentity: Equatable {
        let device: dev_t, inode: ino_t, owner: uid_t, group: gid_t, mode: mode_t
        init(_ s: stat) { device = s.st_dev; inode = s.st_ino; owner = s.st_uid; group = s.st_gid; mode = s.st_mode }
    }
    private struct Directory { let fd: Descriptor, parent: Int32?, name: String, identity: DirectoryIdentity }
    private struct File { let fd: Descriptor, parent: Int32, name: String, info: stat, hash: String, image: AgentRunningImageIdentity }
    private struct Link { let parent: Int32, name: String, info: stat, target: String }
    private let root: String, inventory: AgentProductsInventoryBinding, appRoot: String
    private let events: Descriptor
    private let boundary: ((Boundary) throws -> Void)?
    private var directories: [String: Directory] = [:]
    private var files: [File] = [], links: [String: Link] = [:]
    private var invalidated = false
    private static let byteLimit = 256 * 1024 * 1024

    fileprivate init(root: String, inventory: AgentProductsInventoryBinding, appRoot: String,
                     boundary: ((Boundary) throws -> Void)?) throws {
        guard Self.valid(root, absolute: true), root != "/",
              case .directory(let mode)? = inventory.entries[appRoot] else { throw AgentFailure.binding }
        self.root = root; self.inventory = inventory; self.appRoot = appRoot; self.boundary = boundary
        events = try Descriptor(kqueue())
        guard fcntl(events.value, F_SETFD, FD_CLOEXEC) == 0 else { throw AgentFailure.storage }
        var path = "/"
        try retainDirectory(path, name: "/", parent: nil, mode: nil)
        for name in root.split(separator: "/").map(String.init) {
            let parent = directories[path]!.fd.value
            path = path == "/" ? path + name : path + "/" + name
            try retainDirectory(path, name: name, parent: parent, mode: path == root ? mode : nil)
        }
        try revalidate()
    }
    fileprivate func path(_ key: String) -> String {
        key == appRoot ? root : root + "/" + String(key.dropFirst(appRoot.count + 1))
    }
    private static func valid(_ path: String, absolute: Bool) -> Bool {
        guard !path.isEmpty, path.utf8.count <= 4096, !path.utf8.contains(0), path.hasPrefix("/") == absolute else { return false }
        let parts = (absolute ? String(path.dropFirst()) : path).split(separator: "/", omittingEmptySubsequences: false)
        return parts.count <= 128 && parts.allSatisfy { !$0.isEmpty && $0 != "." && $0 != ".." }
    }
    private func watch(_ fd: Int32, file: Bool) throws {
        let flags = file ? NOTE_WRITE | NOTE_EXTEND | NOTE_ATTRIB | NOTE_LINK | NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE : NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE
        var event = kevent(ident: UInt(fd), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                           fflags: UInt32(flags), data: 0, udata: nil)
        guard kevent(events.value, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
    }
    private func retainDirectory(_ path: String, name: String, parent: Int32?, mode: UInt16?) throws {
        if directories[path] != nil { return }
        guard directories.count < 4096 else { throw AgentFailure.storage }
        let fd = try Descriptor(parent.map { openat($0, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK) }
            ?? open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK))
        var info = stat()
        guard fstat(fd.value, &info) == 0, info.st_mode & S_IFMT == S_IFDIR else { throw AgentFailure.storage }
        if let mode {
            guard info.st_uid == getuid(), info.st_mode & 0o022 == 0, info.st_mode & 0o7777 == mode_t(mode) else { throw AgentFailure.storage }
        }
        try watch(fd.value, file: false)
        directories[path] = Directory(fd: fd, parent: parent, name: name, identity: DirectoryIdentity(info))
    }
    private func parent(_ key: String) throws -> (Int32, String) {
        guard Self.valid(key, absolute: false), key.hasPrefix(appRoot + "/") else { throw AgentFailure.binding }
        let components = key.dropFirst(appRoot.count + 1).split(separator: "/").map(String.init)
        var inventoryKey = appRoot, physical = root
        for name in components.dropLast() {
            inventoryKey += "/" + name
            guard case .directory(let mode)? = inventory.entries[inventoryKey] else { throw AgentFailure.binding }
            let parent = directories[physical]!.fd.value
            physical += "/" + name
            try retainDirectory(physical, name: name, parent: parent, mode: mode)
        }
        return (directories[physical]!.fd.value, components.last!)
    }
    fileprivate func admitLinks(_ key: String) throws {
        _ = try inventory.resolved(key)
        var pending = key.split(separator: "/").map(String.init), stack: [String] = []
        var expansions = 0
        while !pending.isEmpty {
            let name = pending.removeFirst()
            if name == "." { continue }
            if name == ".." { guard !stack.isEmpty else { throw AgentFailure.binding }; stack.removeLast(); continue }
            let current = (stack + [name]).joined(separator: "/")
            guard let entry = inventory.entries[current] else { throw AgentFailure.binding }
            if case .symlink(let target) = entry {
                expansions += 1
                guard expansions <= 128, current.hasPrefix(appRoot + "/") else { throw AgentFailure.binding }
                if links[current] == nil {
                    guard links.count < 4096 else { throw AgentFailure.storage }
                    let (parent, name) = try parent(current)
                    var info = stat()
                    guard fstatat(parent, name, &info, AT_SYMLINK_NOFOLLOW) == 0,
                          info.st_mode & S_IFMT == S_IFLNK, info.st_uid == getuid(), info.st_nlink == 1,
                          try Self.readLink(parent, name) == target else { throw AgentFailure.storage }
                    links[current] = Link(parent: parent, name: name, info: info, target: target)
                }
                pending = target.split(separator: "/", omittingEmptySubsequences: false).map(String.init) + pending
            } else { stack.append(name) }
        }
        try revalidate()
    }
    fileprivate func retainImage(_ key: String, identity: AgentRunningImageIdentity) throws {
        guard files.count < 128, case .file(let hash, let bytes, let mode)? = inventory.entries[key],
              bytes <= UInt64(Self.byteLimit) else { throw AgentFailure.binding }
        let (parent, name) = try parent(key)
        var named = stat(), held = stat()
        guard fstatat(parent, name, &named, AT_SYMLINK_NOFOLLOW) == 0 else { throw AgentFailure.storage }
        try Self.regular(named)
        guard UInt64(named.st_size) == bytes, named.st_mode & 0o7777 == mode_t(mode) else { throw AgentFailure.binding }
        let fd = try Descriptor(openat(parent, name, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK))
        guard fstat(fd.value, &held) == 0, Self.same(named, held) else { throw AgentFailure.storage }
        try watch(fd.value, file: true)
        let file = File(fd: fd, parent: parent, name: name, info: held, hash: hash, image: identity)
        files.append(file)
        try boundary?(.beforeRead)
        try checkNamespaces(); try check(file)
        let content = try Self.read(fd.value, size: held.st_size)
        try boundary?(.afterRead)
        try check(file); try checkNamespaces()
        guard AgentJSON.hash(content) == hash else { throw AgentFailure.binding }
        _ = try AgentRunningMachO.identity(content, matching: identity)
        try revalidate()
    }
    func revalidate() throws {
        do {
            guard !invalidated else { throw AgentFailure.storage }
            try checkNamespaces()
            for file in files {
                try check(file)
                let bytes = try Self.read(file.fd.value, size: file.info.st_size)
                guard AgentJSON.hash(bytes) == file.hash else { throw AgentFailure.storage }
                _ = try AgentRunningMachO.identity(bytes, matching: file.image)
                try check(file)
            }
            try checkNamespaces()
        } catch { invalidated = true; throw (error as? AgentFailure) ?? .storage }
    }
    private func checkNamespaces() throws {
        for directory in directories.values {
            var held = stat(), named = stat()
            guard fstat(directory.fd.value, &held) == 0, DirectoryIdentity(held) == directory.identity else { throw AgentFailure.storage }
            let result = directory.parent.map { fstatat($0, directory.name, &named, AT_SYMLINK_NOFOLLOW) } ?? lstat("/", &named)
            guard result == 0, DirectoryIdentity(named) == directory.identity else { throw AgentFailure.storage }
        }
        for link in links.values {
            var named = stat()
            guard fstatat(link.parent, link.name, &named, AT_SYMLINK_NOFOLLOW) == 0, Self.same(named, link.info),
                  try Self.readLink(link.parent, link.name) == link.target else { throw AgentFailure.storage }
        }
        var event = kevent(), timeout = timespec(tv_sec: 0, tv_nsec: 0)
        guard kevent(events.value, nil, 0, &event, 1, &timeout) == 0 else { throw AgentFailure.storage }
    }
    private func check(_ file: File) throws {
        var held = stat(), named = stat()
        guard fstat(file.fd.value, &held) == 0, fstatat(file.parent, file.name, &named, AT_SYMLINK_NOFOLLOW) == 0,
              Self.same(held, file.info), Self.same(named, file.info) else { throw AgentFailure.storage }
        try Self.regular(held); try Self.regular(named)
    }
    private static func regular(_ info: stat) throws {
        guard info.st_mode & S_IFMT == S_IFREG, info.st_uid == getuid(), info.st_nlink == 1,
              info.st_mode & 0o022 == 0, info.st_size >= 0, info.st_size <= byteLimit else { throw AgentFailure.storage }
    }
    private static func same(_ a: stat, _ b: stat) -> Bool {
        DirectoryIdentity(a) == DirectoryIdentity(b) && a.st_nlink == b.st_nlink && a.st_size == b.st_size &&
            a.st_mtimespec.tv_sec == b.st_mtimespec.tv_sec && a.st_mtimespec.tv_nsec == b.st_mtimespec.tv_nsec &&
            a.st_ctimespec.tv_sec == b.st_ctimespec.tv_sec && a.st_ctimespec.tv_nsec == b.st_ctimespec.tv_nsec
    }
    private static func readLink(_ parent: Int32, _ name: String) throws -> String {
        var buffer = [UInt8](repeating: 0, count: 4097)
        let count = readlinkat(parent, name, &buffer, buffer.count)
        guard count > 0, count <= 4096, let text = String(bytes: buffer.prefix(count), encoding: .utf8) else { throw AgentFailure.storage }
        return text
    }
    private static func read(_ fd: Int32, size: off_t) throws -> Data {
        var bytes = Data(), buffer = [UInt8](repeating: 0, count: 65_536)
        while true {
            let count = buffer.withUnsafeMutableBytes { pread(fd, $0.baseAddress, $0.count, off_t(bytes.count)) }
            guard count >= 0, count <= byteLimit - bytes.count else { throw AgentFailure.storage }
            if count == 0 { break }
            bytes.append(contentsOf: buffer.prefix(count))
        }
        guard bytes.count == size else { throw AgentFailure.storage }
        return bytes
    }
}
