#if os(macOS)
import Darwin
import Foundation
import Synchronization

// Headless host test support only. Data is the concurrency boundary: neither
// JSONSerialization's Any values nor ServiceSystemFields enter CoreBridge.
nonisolated enum CloudServiceProcessFailure: Error {
    case configuration, launch, closed, input, output, malformedReply
    case fixtureInputLimit, fixtureReplyLimit, fixtureDiagnosticLimit
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

    private init(worker: CloudServiceProcessWorker) { self.worker = worker }

    static func open(stateDirectory: URL) async throws -> CloudServiceProcess {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global().async {
                do {
                    let worker = try CloudServiceProcessWorker(stateDirectory: stateDirectory)
                    worker.startDiagnostics()
                    continuation.resume(returning: CloudServiceProcess(worker: worker))
                } catch { continuation.resume(throwing: error) }
            }
        }
    }

    // A caller's cancellation takes effect only after its original reply has
    // drained. The queue owns the whole write/read exchange, without retries.
    // Test barriers expose admission and a write with its reply still pending.
    func request(_ frame: Data, afterEnqueue: (@Sendable () -> Void)? = nil,
                 afterWrite: (@Sendable () -> Void)? = nil) async throws -> Data {
        let reply: Data = try await withCheckedThrowingContinuation { continuation in
            worker.queue.async { [worker] in
                do { continuation.resume(returning: try worker.exchange(frame, afterWrite: afterWrite)) }
                catch { continuation.resume(throwing: error) }
            }
            afterEnqueue?()
        }
        try Task.checkCancellation()
        return reply
    }

    // Own/media submit guards need synchronous acceptance by the actual stdio
    // queue. Preparation owns the exact frame and buffered original reply.
    func prepare(_ frame: Data) throws -> CloudServicePreparedFrame {
        guard frame.count <= 1_048_576 else { throw CloudServiceProcessFailure.fixtureInputLimit }
        guard !frame.contains(10) else { throw CloudServiceProcessFailure.input }
        return CloudServicePreparedFrame { [worker] finish in
            worker.queue.async {
                do { finish(.success(try worker.exchange(frame, afterWrite: nil))) }
                catch { finish(.failure(error)) }
            }
        }
    }

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

nonisolated final class CloudServicePreparedFrame: Sendable {
    private struct State {
        var queued = false
        var cancelled = false
        var result: Result<Data, any Error>?
        var waiter: CheckedContinuation<Data, any Error>?
    }
    private let state = Mutex(State())
    private let schedule: @Sendable (@escaping @Sendable (Result<Data, any Error>) -> Void) -> Void
    init(schedule: @escaping @Sendable (@escaping @Sendable (Result<Data, any Error>) -> Void) -> Void) {
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
    private func finish(_ result: Result<Data, any Error>) {
        let waiter = state.withLock { value in
            guard value.result == nil else { return Optional<CheckedContinuation<Data, any Error>>.none }
            value.result = result
            let waiter = value.waiter; value.waiter = nil
            return waiter
        }
        waiter?.resume(with: result)
    }
    func reply() async throws -> Data {
        let data: Data = try await withCheckedThrowingContinuation { continuation in
            let ready = state.withLock { value -> Result<Data, any Error>? in
                guard value.queued else { return .failure(CloudServiceProcessFailure.input) }
                if let result = value.result { return result }
                guard value.waiter == nil else { return .failure(CloudServiceProcessFailure.input) }
                value.waiter = continuation
                return nil
            }
            if let ready { continuation.resume(with: ready) }
        }
        // Cancellation never truncates a frame or leaves its reply in stdout.
        if state.withLock({ $0.cancelled }) { throw CancellationError() }
        try Task.checkCancellation()
        return data
    }
}

// All mutable stdio state is confined to queue; diagnostics use their own
// drain queue and Mutex. Process termination may interrupt a blocked exchange.
private nonisolated final class CloudServiceProcessWorker: @unchecked Sendable {
    let queue = DispatchQueue(label: "Meguri.CloudServiceProcess.stdio")
    private let process = Process()
    private let input = Pipe()
    private let output = Pipe()
    private let error = Pipe()
    private let diagnostics = Mutex((bytes: Data(), limited: false, failed: false))
    private let diagnosticJoin = DispatchGroup()
    private var pending = Data()
    private var result: CloudServiceProcessExit?
    private var inputClosed = false
    // Small synthetic IPC fixture limits, independent of production budgets.
    private let frameLimit = 1_048_576
    private let diagnosticLimit = 16_384

    init(stateDirectory: URL) throws {
        guard let path = ProcessInfo.processInfo.environment["MEGURI_CLOUD_SERVICE_CLI"],
              path.hasPrefix("/"),
              let attributes = try? FileManager.default.attributesOfItem(atPath: path),
              attributes[.type] as? FileAttributeType == .typeRegular,
              (attributes[.referenceCount] as? NSNumber)?.intValue == 1,
              FileManager.default.isExecutableFile(atPath: path),
              stateDirectory.isFileURL else { throw CloudServiceProcessFailure.configuration }
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = ["internal-cloud-service", stateDirectory.path, "18446744073709551615"]
        process.standardInput = input
        process.standardOutput = output
        process.standardError = error
        // A terminated helper must turn a pending write into a classified error.
        guard fcntl(input.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1) != -1 else {
            throw CloudServiceProcessFailure.configuration
        }
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

    func exchange(_ frame: Data, afterWrite: (@Sendable () -> Void)?) throws -> Data {
        guard result == nil else { throw CloudServiceProcessFailure.closed }
        do {
            guard frame.count <= frameLimit else { throw CloudServiceProcessFailure.fixtureInputLimit }
            guard !frame.contains(10) else { throw CloudServiceProcessFailure.input }
            var line = frame
            line.append(10)
            try line.withUnsafeBytes { bytes in
                var offset = 0
                while offset < bytes.count {
                    let count = Darwin.write(input.fileHandleForWriting.fileDescriptor,
                                             bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                    if count < 0 && errno == EINTR { continue }
                    guard count > 0 else { throw CloudServiceProcessFailure.input }
                    offset += count
                }
            }
            afterWrite?()
            guard let reply = try readFrame() else { throw CloudServiceProcessFailure.output }
            guard (try? JSONSerialization.jsonObject(with: reply)) != nil else {
                throw CloudServiceProcessFailure.malformedReply
            }
            return reply
        } catch {
            let exit = finish(terminate: true)
            if exit.diagnosticLimitExceeded { throw CloudServiceProcessFailure.fixtureDiagnosticLimit }
            throw error // Only the fixed failures above leave this worker.
        }
    }

    private func readFrame() throws -> Data? {
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
        process.waitUntilExit()
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
