#if os(macOS)
import Darwin
import Foundation

/// Public BSD transport. Callers and all callbacks use the listener's queue.
nonisolated enum MiningSocket {
    static func configure(_ fd: Int32) throws {
        var yes: Int32 = 1
        guard fcntl(fd, F_SETFD, FD_CLOEXEC) == 0,
              fcntl(fd, F_SETFL, O_NONBLOCK) == 0,
              setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &yes, socklen_t(MemoryLayout.size(ofValue: yes))) == 0 else {
            throw error()
        }
    }

    static func error() -> POSIXError { POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }

    static func isLoopbackPeer(_ fd: Int32) -> Bool {
        var address = sockaddr_storage()
        var size = socklen_t(MemoryLayout<sockaddr_storage>.size)
        guard withUnsafeMutablePointer(to: &address, {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getpeername(fd, $0, &size) }
        }) == 0 else { return false }
        return withUnsafePointer(to: &address) { pointer in
            switch Int32(pointer.pointee.ss_family) {
            case AF_INET:
                return pointer.withMemoryRebound(to: sockaddr_in.self, capacity: 1) {
                    $0.pointee.sin_addr.s_addr == inet_addr("127.0.0.1")
                }
            case AF_INET6:
                return pointer.withMemoryRebound(to: sockaddr_in6.self, capacity: 1) {
                    withUnsafeBytes(of: $0.pointee.sin6_addr) { Data($0) }
                        == withUnsafeBytes(of: in6addr_loopback) { Data($0) }
                }
            default: return false
            }
        }
    }

    /// Bind owns the live endpoint exclusively; never enable SO_REUSEPORT.
    /// SO_REUSEADDR allows rebinding after prior connections enter TIME_WAIT; the
    /// receiver can reclaim its port after a stop. Darwin wildcard listeners
    /// are checked by the caller before this more-specific bind.
    static func listen(port: UInt16, ipv6: Bool) throws -> (fd: Int32, port: UInt16) {
        let fd = Darwin.socket(ipv6 ? AF_INET6 : AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw error() }
        do {
            try configure(fd)
            var reuseAddress: Int32 = 1
            guard setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &reuseAddress, socklen_t(MemoryLayout.size(ofValue: reuseAddress))) == 0 else {
                throw error()
            }
            let bound: Int32
            if ipv6 {
                var yes: Int32 = 1
                guard setsockopt(fd, Int32(IPPROTO_IPV6), IPV6_V6ONLY, &yes, socklen_t(MemoryLayout.size(ofValue: yes))) == 0 else {
                    throw error()
                }
                var address = sockaddr_in6()
                address.sin6_len = UInt8(MemoryLayout<sockaddr_in6>.size)
                address.sin6_family = sa_family_t(AF_INET6)
                address.sin6_port = port.bigEndian
                address.sin6_addr = in6addr_loopback
                bound = withUnsafePointer(to: &address) {
                    $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in6>.size)) }
                }
            } else {
                var address = sockaddr_in()
                address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
                address.sin_family = sa_family_t(AF_INET)
                address.sin_port = port.bigEndian
                address.sin_addr = in_addr(s_addr: inet_addr("127.0.0.1"))
                bound = withUnsafePointer(to: &address) {
                    $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
                }
            }
            guard bound == 0, Darwin.listen(fd, 128) == 0 else { throw error() }
            if ipv6 { return (fd, port) }
            var address = sockaddr_in()
            var size = socklen_t(MemoryLayout<sockaddr_in>.size)
            let named = withUnsafeMutablePointer(to: &address) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &size) }
            }
            guard named == 0, address.sin_port != 0 else { throw error() }
            return (fd, UInt16(bigEndian: address.sin_port))
        } catch {
            Darwin.close(fd)
            throw error
        }
    }
}

/// One read at a time, FIFO writes with partial-send offsets, and readiness
/// sources suspended while idle. Cancellation releases the FD exactly once,
/// after both sources have stopped using it (including queued events).
nonisolated final class MiningSocketStream: @unchecked Sendable {
    typealias Read = @Sendable (Data, Bool, (any Error)?) -> Void
    typealias Written = @Sendable ((any Error)?) -> Void
    private nonisolated struct Write {
        let bytes: Data
        let completion: Written
        var offset = 0
    }
    private let fd: Int32
    private let reader: any DispatchSourceRead
    private let writer: any DispatchSourceWrite
    private var read: Read?
    private var writes: [Write] = []
    private var reading = false
    private var writing = false
    private var cancelled = false

    init(fd: Int32, queue: DispatchQueue, closed: @escaping @Sendable () -> Void) {
        self.fd = fd
        reader = DispatchSource.makeReadSource(fileDescriptor: fd, queue: queue)
        writer = DispatchSource.makeWriteSource(fileDescriptor: fd, queue: queue)
        // Both handlers run serially on queue. The second cancellation owns
        // close; neither descriptor can be recycled under a pending source.
        let lifetime = Lifetime(fd: fd, closed: closed)
        reader.setCancelHandler { lifetime.release() }
        writer.setCancelHandler { lifetime.release() }
        reader.setEventHandler { [weak self] in self?.readReady() }
        writer.setEventHandler { [weak self] in self?.writeReady() }
    }

    private nonisolated final class Lifetime: @unchecked Sendable {
        let fd: Int32
        let closed: @Sendable () -> Void
        var remaining = 2
        init(fd: Int32, closed: @escaping @Sendable () -> Void) { self.fd = fd; self.closed = closed }
        func release() {
            remaining -= 1
            if remaining == 0 { Darwin.close(fd); closed() }
        }
    }

    func cancel() {
        guard !cancelled else { return }
        cancelled = true
        read = nil
        writes.removeAll()
        reader.cancel()
        writer.cancel()
        if !reading { reader.resume() }
        if !writing { writer.resume() }
    }

    func receive(_ completion: @escaping Read) {
        guard !cancelled else { return }
        precondition(read == nil)
        read = completion
        reading = true
        reader.resume()
    }

    private func readReady() {
        guard !cancelled, let completion = read else { return }
        var bytes = Data(count: 65_536)
        let count = bytes.withUnsafeMutableBytes { Darwin.recv(fd, $0.baseAddress, $0.count, 0) }
        if count < 0, [EAGAIN, EWOULDBLOCK, EINTR].contains(errno) { return }
        let error = count < 0 ? MiningSocket.error() : nil
        read = nil
        reader.suspend()
        reading = false
        if count > 0 { bytes.count = count } else { bytes = Data() }
        completion(bytes, count == 0, error)
    }

    func send(_ bytes: Data, completion: @escaping Written) {
        guard !cancelled else { return }
        guard !bytes.isEmpty else { completion(nil); return }
        writes.append(Write(bytes: bytes, completion: completion))
        if !writing {
            writing = true
            writer.resume()
        }
    }

    private func writeReady() {
        guard !cancelled else { return }
        // Bound work per event so a large reply cannot starve other peers.
        var budget = 65_536
        while !writes.isEmpty, budget > 0 {
            let item = writes[0]
            let count = item.bytes.withUnsafeBytes {
                Darwin.send(fd, $0.baseAddress!.advanced(by: item.offset), min(budget, $0.count - item.offset), 0)
            }
            if count < 0, [EAGAIN, EWOULDBLOCK, EINTR].contains(errno) { return }
            if count <= 0 {
                item.completion(count < 0 ? MiningSocket.error() : POSIXError(.EPIPE))
                return
            }
            writes[0].offset += count
            budget -= count
            if writes[0].offset == item.bytes.count {
                writes.removeFirst()
                item.completion(nil)
                if cancelled { return }
            }
        }
        if writes.isEmpty {
            writer.suspend()
            writing = false
        }
    }
}
#endif
