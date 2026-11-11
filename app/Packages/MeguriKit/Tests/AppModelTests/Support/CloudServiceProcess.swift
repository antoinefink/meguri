#if os(macOS)
import Darwin
import Foundation
import Synchronization

// Headless host test support only. Sendable packets and typed fixture values
// cross the queue; neither untyped JSON nor ServiceSystemFields enter CoreBridge.
nonisolated enum CloudServiceProcessFailure: Error {
    case configuration, launch, closed, input, output, malformedReply
    case fixtureInputLimit, fixtureReplyLimit, fixtureDiagnosticLimit, fixtureAttachmentLimit
}

nonisolated struct CloudServiceProcessExit: Sendable, Equatable, CustomStringConvertible {
    let status: Int32
    let diagnostics: Data
    let diagnosticLimitExceeded: Bool
    let joined: Bool
    let unexpectedOutput: Bool
    let ioFailed: Bool
    var description: String { "CloudServiceProcessExit" }
}

nonisolated final class CloudServiceProcess: Sendable {
    private let worker: CloudServiceProcessWorker
    private let transport: CloudServiceTransport

    private init(worker: CloudServiceProcessWorker, transport: CloudServiceTransport) {
        self.worker = worker; self.transport = transport
    }

    static func open(stateDirectory: URL, transport: CloudServiceTransport = .legacy,
                     pageSize: Int? = nil) async throws -> CloudServiceProcess {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global().async {
                do {
                    let worker = try CloudServiceProcessWorker(stateDirectory: stateDirectory, transport: transport, pageSize: pageSize)
                    worker.startDiagnostics()
                    continuation.resume(returning: CloudServiceProcess(worker: worker, transport: transport))
                } catch { continuation.resume(throwing: error) }
            }
        }
    }

    // A caller's cancellation takes effect only after its original reply has
    // drained. The queue owns the whole write/read exchange, without retries.
    // Test barriers expose admission and a write with its reply still pending.
    func request(_ frame: Data, afterEnqueue: (@Sendable () -> Void)? = nil,
                 afterWrite: (@Sendable () -> Void)? = nil) async throws -> Data {
        let packet = try CloudServiceBinary.prepare(frame, transport: transport)
        return try await request(packet, afterEnqueue: afterEnqueue, afterWrite: afterWrite).data()
    }

    func request(message: ServiceJSON, afterEnqueue: (@Sendable () -> Void)? = nil,
                 afterWrite: (@Sendable () -> Void)? = nil) async throws -> ServiceJSON {
        let packet = try CloudServiceBinary.prepare(message: message, transport: transport)
        return try await request(packet, afterEnqueue: afterEnqueue, afterWrite: afterWrite).message()
    }

    private func request(_ packet: CloudServicePacket, afterEnqueue: (@Sendable () -> Void)?,
                         afterWrite: (@Sendable () -> Void)?) async throws -> CloudServiceProcessReply {
        let reply: CloudServiceProcessReply = try await withCheckedThrowingContinuation { continuation in
            worker.queue.async { [worker] in
                do { continuation.resume(returning: try worker.exchange(packet, afterWrite: afterWrite)) }
                catch { continuation.resume(throwing: error) }
            }
            afterEnqueue?()
        }
        try Task.checkCancellation()
        return reply
    }

    // Both APIs use the same one-shot owner and actual stdio queue acceptance.
    func prepare(_ frame: Data, afterWrite: (@Sendable () -> Void)? = nil) throws -> CloudServicePreparedFrame {
        try prepare(CloudServiceBinary.prepare(frame, transport: transport), afterWrite: afterWrite)
    }
    func prepare(message: ServiceJSON, afterWrite: (@Sendable () -> Void)? = nil) throws -> CloudServicePreparedFrame {
        try prepare(CloudServiceBinary.prepare(message: message, transport: transport), afterWrite: afterWrite)
    }
    private func prepare(_ packet: CloudServicePacket, afterWrite: (@Sendable () -> Void)?) -> CloudServicePreparedFrame {
        CloudServicePreparedFrame { [worker] finish in
            worker.queue.async {
                do { finish(.success(try worker.exchange(packet, afterWrite: afterWrite))) }
                catch { finish(.failure(error)) }
            }
        }
    }

    func transportExchanges() -> [CloudServiceExchange] { worker.exchanges.withLock { $0 } }

    func close() async -> CloudServiceProcessExit {
        await withCheckedContinuation { continuation in
            worker.queue.async { [worker] in continuation.resume(returning: worker.finish(terminate: false)) }
        }
    }

    // Fixture-wide cancellation alone terminates the retained original child.
    func cancel() async -> CloudServiceProcessExit {
        worker.stop()
        return await close()
    }

    deinit {
        // Last-owner fallback also joins, off the caller's actor. Tests always
        // close explicitly so they can assert the original child's exit status.
        worker.queue.async { [worker] in _ = worker.finish(terminate: true) }
    }
}

// Legacy replies retain exact pipe JSON. Binary replies retain validated logical
// values; only an explicit Data accessor materializes integer-array asset JSON.
nonisolated enum CloudServiceProcessReply: Sendable {
    case raw(Data), logical(ServiceJSON)
    func data() throws -> Data {
        switch self {
        case .raw(let data): return data
        case .logical(let value): return try value.encoded()
        }
    }
    func message() throws -> ServiceJSON {
        switch self {
        case .logical(let value): return value
        case .raw(let data):
            do { return try JSONDecoder().decode(ServiceJSON.self, from: data) }
            catch { throw CloudServiceProcessFailure.malformedReply }
        }
    }
}

nonisolated final class CloudServicePreparedFrame: Sendable {
    private struct State {
        var queued = false
        var cancelled = false
        var result: Result<CloudServiceProcessReply, any Error>?
        var waiter: CheckedContinuation<CloudServiceProcessReply, any Error>?
    }
    private let state = Mutex(State())
    private let schedule: @Sendable (@escaping @Sendable (Result<CloudServiceProcessReply, any Error>) -> Void) -> Void
    init(schedule: @escaping @Sendable (@escaping @Sendable (Result<CloudServiceProcessReply, any Error>) -> Void) -> Void) {
        self.schedule = schedule
    }
    func enqueue() -> Bool {
        state.withLock { value in
            guard !value.queued, !value.cancelled else { return false }
            value.queued = true
            schedule { self.finish($0) } // DispatchQueue.async accepts this original exchange now.
            return true
        }
    }
    func cancel() { state.withLock { $0.cancelled = true } }
    private func finish(_ result: Result<CloudServiceProcessReply, any Error>) {
        let waiter = state.withLock { value in
            guard value.result == nil else { return Optional<CheckedContinuation<CloudServiceProcessReply, any Error>>.none }
            value.result = result
            let waiter = value.waiter; value.waiter = nil
            return waiter
        }
        waiter?.resume(with: result)
    }
    func reply(preservingCancelledResult: Bool = false) async throws -> Data {
        try await completed(preservingCancelledResult: preservingCancelledResult).data()
    }
    func logicalReply(preservingCancelledResult: Bool = false) async throws -> ServiceJSON {
        try await completed(preservingCancelledResult: preservingCancelledResult).message()
    }
    private func completed(preservingCancelledResult: Bool) async throws -> CloudServiceProcessReply {
        let reply: CloudServiceProcessReply = try await withCheckedThrowingContinuation { continuation in
            let ready = state.withLock { value -> Result<CloudServiceProcessReply, any Error>? in
                guard value.queued else { return .failure(CloudServiceProcessFailure.input) }
                if let result = value.result { return result }
                guard value.waiter == nil else { return .failure(CloudServiceProcessFailure.input) }
                value.waiter = continuation
                return nil
            }
            if let ready { continuation.resume(with: ready) }
        }
        // Cancellation never truncates a frame or leaves its reply in stdout.
        if !preservingCancelledResult {
            if state.withLock({ $0.cancelled }) { throw CancellationError() }
            try Task.checkCancellation()
        }
        return reply
    }
}

// All mutable stdio state is confined to queue; diagnostics use their own
// drain queue and Mutex. Process termination may interrupt a blocked exchange.
private nonisolated final class CloudServiceProcessWorker: @unchecked Sendable {
    let queue = DispatchQueue(label: "Meguri.CloudServiceProcess.stdio")
    private let process = Process()
    private let processCompletion = DispatchSemaphore(value: 0)
    private let input = Pipe()
    private let output = Pipe()
    private let error = Pipe()
    private let diagnostics = Mutex((bytes: Data(), limited: false, failed: false))
    private let diagnosticJoin = DispatchGroup()
    private var pending = Data()
    private var result: CloudServiceProcessExit?
    private var inputClosed = false
    private let transport: CloudServiceTransport
    let exchanges = Mutex<[CloudServiceExchange]>([])
    // Small synthetic IPC fixture limits, independent of production budgets.
    private let frameLimit = 1_048_576
    private let diagnosticLimit = 16_384

    init(stateDirectory: URL, transport: CloudServiceTransport, pageSize: Int?) throws {
        self.transport = transport
        if let pageSize, !(1...200).contains(pageSize) { throw CloudServiceProcessFailure.configuration }
        guard let path = ProcessInfo.processInfo.environment["MEGURI_CLOUD_SERVICE_CLI"],
              path.hasPrefix("/"),
              let attributes = try? FileManager.default.attributesOfItem(atPath: path),
              attributes[.type] as? FileAttributeType == .typeRegular,
              (attributes[.referenceCount] as? NSNumber)?.intValue == 1,
              FileManager.default.isExecutableFile(atPath: path),
              stateDirectory.isFileURL else { throw CloudServiceProcessFailure.configuration }
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = ["internal-cloud-service", stateDirectory.path, "18446744073709551615"]
        if case .binaryAssetsV1 = transport { process.arguments?.append("--binary-assets-v1") }
        if let pageSize { process.arguments?.append(contentsOf: ["--page-size", String(pageSize)]) }
        process.standardInput = input
        process.standardOutput = output
        process.standardError = error
        // A terminated helper must turn a pending write into a classified error.
        guard fcntl(input.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1) != -1 else {
            throw CloudServiceProcessFailure.configuration
        }
        // Retain an early completion signal without capturing this worker or process.
        process.terminationHandler = { [processCompletion] _ in processCompletion.signal() }
        do { try process.run() }
        catch { throw CloudServiceProcessFailure.launch }
        input.fileHandleForReading.closeFile()
        output.fileHandleForWriting.closeFile()
        error.fileHandleForWriting.closeFile()
    }

    func startDiagnostics() {
        diagnosticJoin.enter()
        DispatchQueue.global().async { [self] in
            defer { error.fileHandleForReading.closeFile(); diagnosticJoin.leave() }
            var bytes = [UInt8](repeating: 0, count: 4096)
            while true {
                let count = Darwin.read(error.fileHandleForReading.fileDescriptor, &bytes, bytes.count)
                if count < 0 && errno == EINTR { continue }
                if count < 0 { diagnostics.withLock { $0.failed = true } }
                if count <= 0 { break }
                let exceeded = diagnostics.withLock { state in
                    let remaining = diagnosticLimit - state.bytes.count
                    state.bytes.append(contentsOf: bytes.prefix(min(count, remaining)))
                    state.limited = state.limited || count > remaining
                    return state.limited
                }
                if exceeded { stop() }
            }
        }
    }

    func stop() { if process.isRunning { process.terminate() } }

    func exchange(_ packet: CloudServicePacket, afterWrite: (@Sendable () -> Void)?) throws -> CloudServiceProcessReply {
        guard result == nil else { throw CloudServiceProcessFailure.closed }
        do {
            var line = packet.header
            line.append(10)
            try write(line)
            for body in packet.bodies { try write(body) }
            afterWrite?()
            guard let reply = try readFrame() else { throw CloudServiceProcessFailure.output }
            guard (try? JSONSerialization.jsonObject(with: reply)) != nil else {
                throw CloudServiceProcessFailure.malformedReply
            }
            let response: CloudServicePacket
            let logical: CloudServiceProcessReply
            if case .binaryAssetsV1 = transport,
               (try? JSONDecoder().decode(ServiceJSON.self, from: reply))?["protocol_error"] == nil {
                let header = try CloudServiceBinary.replyHeader(reply)
                var bodies: [Data] = []
                for length in header.lengths { bodies.append(try readBody(length)) }
                response = .init(header: reply, bodies: bodies)
                logical = .logical(try header.logical(bodies))
            } else {
                response = .init(header: reply, bodies: [])
                logical = .raw(reply)
            }
            exchanges.withLock { $0.append(.init(request: packet, response: response)) }
            return logical
        } catch {
            let exit = finish(terminate: true)
            if exit.diagnosticLimitExceeded { throw CloudServiceProcessFailure.fixtureDiagnosticLimit }
            throw error // Only the fixed failures above leave this worker.
        }
    }

    private func write(_ data: Data) throws {
        try data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(input.fileHandleForWriting.fileDescriptor,
                    bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw CloudServiceProcessFailure.input }
                offset += count
            }
        }
    }

    private func readBody(_ length: Int) throws -> Data {
        var body = Data(count: length)
        try body.withUnsafeMutableBytes { bytes in
            var offset = 0
            while offset < length {
                let count = Darwin.read(output.fileHandleForReading.fileDescriptor,
                    bytes.baseAddress!.advanced(by: offset), length - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw CloudServiceProcessFailure.output }
                offset += count
            }
        }
        return body
    }

    private func readFrame() throws -> Data? {
        // Read the binary header without reading any body until validation.
        if case .binaryAssetsV1 = transport {
            var header = Data()
            var byte: UInt8 = 0
            while true {
                let count = Darwin.read(output.fileHandleForReading.fileDescriptor, &byte, 1)
                if count < 0 && errno == EINTR { continue }
                guard count >= 0 else { throw CloudServiceProcessFailure.output }
                if count == 0 {
                    guard header.isEmpty else { throw CloudServiceProcessFailure.malformedReply }
                    return nil
                }
                if byte == 10 { return header }
                guard header.count < frameLimit else { throw CloudServiceProcessFailure.fixtureReplyLimit }
                header.append(byte)
            }
        }
        var bytes = [UInt8](repeating: 0, count: 4096)
        while true {
            if let newline = pending.firstIndex(of: 10) {
                let frame = Data(pending[..<newline])
                pending.removeSubrange(...newline)
                guard frame.count <= frameLimit else { throw CloudServiceProcessFailure.fixtureReplyLimit }
                return frame
            }
            guard pending.count <= frameLimit else { throw CloudServiceProcessFailure.fixtureReplyLimit }
            let count = Darwin.read(output.fileHandleForReading.fileDescriptor, &bytes,
                                    min(bytes.count, frameLimit + 1 - pending.count))
            if count < 0 && errno == EINTR { continue }
            guard count >= 0 else { throw CloudServiceProcessFailure.output }
            if count == 0 {
                guard pending.isEmpty else { throw CloudServiceProcessFailure.malformedReply }
                return nil
            }
            pending.append(contentsOf: bytes.prefix(count))
        }
    }

    func finish(terminate: Bool) -> CloudServiceProcessExit {
        if let result { return result }
        if terminate { stop() }
        if !inputClosed { input.fileHandleForWriting.closeFile(); inputClosed = true }
        // EOF is the normal shutdown protocol. No request is sent a second time.
        // Drain stdout even on failure, and join stderr before releasing handles.
        var trailing = !pending.isEmpty
        pending.removeAll()
        var bytes = [UInt8](repeating: 0, count: 4096)
        var drained = 0
        var outputFailed = false
        while true {
            let count = Darwin.read(output.fileHandleForReading.fileDescriptor, &bytes, bytes.count)
            if count < 0 && errno == EINTR { continue }
            if count < 0 { outputFailed = true }
            if count <= 0 { break }
            trailing = true
            drained += count
            if drained > frameLimit { stop() }
        }
        output.fileHandleForReading.closeFile()
        // Launch occurs on another queue; join its callback before reading status.
        processCompletion.wait()
        diagnosticJoin.wait()
        let diagnostic = diagnostics.withLock { $0 }
        let exit = CloudServiceProcessExit(status: process.terminationStatus, diagnostics: diagnostic.bytes,
            diagnosticLimitExceeded: diagnostic.limited, joined: true, unexpectedOutput: trailing,
            ioFailed: outputFailed || diagnostic.failed)
        result = exit
        return exit
    }
}
#endif
