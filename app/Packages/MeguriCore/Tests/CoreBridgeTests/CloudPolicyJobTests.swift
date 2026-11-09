import Dispatch
import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

private func rawJobPolicy() async throws -> MeguriCoreBindings.CloudPolicy {
    let workflow = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
    return try await MaintenanceLane.shared.call(.cloudWorkflowPolicy) { try workflow.policy() }
}

private func retainCompletedPolicyResource<Value: Sendable>(
    _ startName: BridgeCall, result resultName: BridgeCall,
    attempted: @escaping @Sendable (UInt64) -> Void, message: String,
    _ started: CloudPolicyJobValue<Value>
) async throws -> Value {
    try await CoreCloudPolicyService.runOrdered(
        startName, result: resultName,
        track: { handle in
            attempted(handle.id)
            throw BridgeError.invalidArgument(message)
        }, retainResourceResultOnTrackingFailure: true
    ) { started }
}

private func policyJobActions(_ value: MeguriCoreBindings.CloudPolicyOutput) throws -> [String] {
    guard case .commands(let commands) = value else { throw BridgeError.internal("Expected policy outputs") }
    return commands.map { String(describing: $0.action) }
}

private func requiredCompletionExecution(_ fixture: CloudKitBridgeFixture) async throws -> (CoreCloudPolicyService, CoreCloudPolicyExecution) {
    let (policy, _) = try await nativePolicy(fixture)
    guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
        throw BridgeError.internal("Expected exchange commands")
    }
    let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
    return (policy, try #require(try await policy.prepareExecution(command)))
}

private func drainPolicyPoolHolders(_ jobs: [CoreJob]) async {
    for job in jobs { try? await job.cancel() }
    for job in jobs { _ = await job.outcome() }
}

#if DEBUG
private enum PolicySuspendProofError: Error { case deadline }

extension EventTests {
    @Test(.timeLimit(.minutes(1)))
    func policyRequiredCompletionActiveSendFreezesBeforeCoreSuspend() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            guard case .commands(let commands) = try await policy.input(.flushTimerFired) else {
                throw BridgeError.internal("Expected send commands")
            }
            let send = try #require(commands.first { if case .requestSend(let zones) = $0.action { zones == [.collection] } else { false } })
            let engine = try await fixture.engine()
            let originalGuard = try await fixture.core.currentGuard()
            let held = CloudCallbackGate()
            await engine.pauseAfterBatch(held)
            let finalized = Mutex(false)
            let freezeReturned = Mutex(false)
            let pauseStarted = Mutex(false)
            await fixture.adapter.setSendCallObserver { sdk, _, finished in
                #expect(sdk == engine.instanceID)
                if finished { finalized.withLock { $0 = true } }
            }
            let results = AsyncStream<CloudPolicyNativeResult>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let freezing = Mutex<Task<Void, any Error>?>(nil)
            let completed = CloudPolicyReadinessDecision()
            let limit = ContinuousClock.now + .seconds(10)
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if completed.expire() {
                    await held.abort(PolicySuspendProofError.deadline)
                    results.continuation.finish()
                    freezing.withLock { $0 }?.cancel()
                }
            }
            do {
                try await withCloudTask(gates: [held], operation: {
                    defer { results.continuation.finish() }
                    results.continuation.yield(await executor.execute(send))
                }, whileRunning: {
                    try await held.waitForArrival()
                    try #require(try await policy.status().state == .syncingCollection)
                    let calls = engine.acceptedNetworkCalls.value
                    let freeze = Task {
                        try await fixture.adapter.freezeForSuspend()
                        freezeReturned.withLock { $0 = true }
                        // finishSend follows the awaited required report.
                        try #require(finalized.withLock { $0 })
                        var returned = results.stream.makeAsyncIterator()
                        let result = try #require(await returned.next())
                        try #require(result.error == nil && !result.deferred)
                        let actions = try result.outputs.flatMap { output -> [CoreBridge.CloudPolicyCommand] in
                            guard case .commands(let values) = output else { throw BridgeError.internal("Expected send outputs") }
                            return values
                        }
                        let states = actions.compactMap { command -> CoreBridge.CloudPolicyStatus? in
                            if case .statusChanged(let status) = command.action { status } else { nil }
                        }
                        let started = try #require(states.firstIndex { $0.state == .syncingCollection })
                        let ended = try #require(states.indices.first { $0 > started && states[$0].state != .syncingCollection })
                        #expect(states[ended].error == nil)
                        try #require(try await policy.status().state != .syncingCollection)
                        try await fixture.core.validateSDK(engine.instanceID)
                        #expect(try await fixture.core.currentGuard() == originalGuard)
                        #expect(await fixture.adapter.retainedInstanceIDs() == [engine.instanceID])
                        #expect(!(await engine.recordedTrace()).contains(.released))
                        pauseStarted.withLock { $0 = true }
                        try #require(try await fixture.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
                    }
                    freezing.withLock { $0 = freeze }
                    do {
                        // Observe freeze's actual admission barrier without
                        // starting another SDK operation.
                        while await fixture.adapter.resumeDeliveries() {
                            guard ContinuousClock.now < limit else { throw PolicySuspendProofError.deadline }
                            try Task.checkCancellation()
                            await Task.yield()
                        }
                        try #require(!freezeReturned.withLock { $0 })
                        try #require(!pauseStarted.withLock { $0 })
                        try #require(!finalized.withLock { $0 })
                        try await fixture.core.validateSDK(engine.instanceID)
                        #expect(try await fixture.core.currentGuard() == originalGuard)
                        #expect(await fixture.adapter.retainedInstanceIDs() == [engine.instanceID])
                        let trace = await engine.recordedTrace()
                        #expect(trace.contains { if case .handed = $0 { true } else { false } })
                        #expect(!trace.contains { if case .sendReturned = $0 { true } else { false } })
                        #expect(!trace.contains(.released))
                        await held.open()
                        try await freeze.value
                        #expect(engine.acceptedNetworkCalls.value == calls)
                    } catch {
                        await held.abort(error)
                        freeze.cancel()
                        _ = try? await freeze.value
                        throw error
                    }
                })
                try #require(completed.observeReady())
            } catch {
                await held.abort(error)
                results.continuation.finish()
                freezing.withLock { $0 }?.cancel()
                watchdog.cancel()
                await watchdog.value
                throw error
            }
            watchdog.cancel()
            await watchdog.value
            #expect(freezeReturned.withLock { $0 } && pauseStarted.withLock { $0 })
            _ = try await fixture.collection.resume()
        }
    }
}
#endif

extension EventTests {
    @Test(arguments: [false, true])
    func policyRequiredCompletionListenerFailureRetainsErrorBeforeEndedOutputsAfterClose(reopen: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, execution) = try await requiredCompletionExecution(fixture)
            _ = try await execution.reportInvocation(zones: [.root, .collection], kind: .fetch)
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartReportCompletion) {
                try raw.startReportCompletion(facts: .init(zones: [.root, .collection], kind: .fetch,
                    started: false, failed: true, code: 4, retryAfterMs: 4_321, saved: 17, appCancelled: false))
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            let expected = try policyJobActions(actual)
            guard case .commands(let commands) = actual else { throw BridgeError.internal("Expected Ended outputs") }
            // EngineError must be projected before Ended. Match its complete
            // numeric facts and both zones, rather than just an output count.
            for zone in [MeguriCoreBindings.CloudPolicyZone.root, .collection] {
                #expect(commands.contains { command in
                    if case .statusChanged(let status) = command.action {
                        status.error?.code == 4 && status.error?.retryAfterMs == 4_321 &&
                            status.error?.operation.zone == zone && status.error?.operation.kind == .fetch
                    } else { false }
                })
            }
            _ = try await policy.status()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            if reopen {
                let location = CollectionLocation(collectionDirectory: fixture.directory.appending(path: "collection"),
                    backupsDirectory: fixture.directory.appending(path: "backups"),
                    appGroupSnapshot: fixture.directory.appending(path: "widget.snapshot"))
                try await fixture.collection.open(location, randomSeed: 115)
            }
            let attempted = Mutex<UInt64?>(nil)
            let retained: MeguriCoreBindings.CloudPolicyOutput = try await CoreCloudPolicyService.runOrdered(
                .cloudPolicyExecutionStartReportCompletion, result: .cloudPolicyOutputResult,
                track: { handle in
                    attempted.withLock { $0 = handle.id }
                    throw BridgeError.invalidArgument("Ended listener setup")
                }, requiredCompletion: true
            ) { CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() }) }
            let recovered = try policyJobActions(retained)
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            #expect(!expected.isEmpty && recovered == expected)
            // No owner validation or new projection may replace the old cell.
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            #expect(try policyJobActions(reread) == expected)
            if reopen { try await fixture.collection.close() }
        }
    }

    @Test func policyRequiredCompletionReportKeepsSDKErrorAndAppCancellationFacts() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, execution) = try await requiredCompletionExecution(fixture)
            _ = try await execution.report(zones: [.collection], kind: .fetch, started: true)
            _ = try await execution.report(zones: [.collection], kind: .fetch, started: false, error: CancellationError())
            #expect(try await policy.status().error == nil, "App cancellation must not become a failing cycle")
            _ = try await execution.report(zones: [.collection], kind: .fetch, started: true)
            let sdkFailure = CloudKitSDKOperationFailure(underlying: NSError(domain: "synthetic-sdk", code: 4),
                code: 4, retryAfterMilliseconds: 9_876)
            _ = try await execution.report(zones: [.collection], kind: .fetch, started: false, error: sdkFailure, saved: 23)
            let detail = try #require(try await policy.status().error)
            #expect(detail.code == 4 && detail.retryAfterMilliseconds == 9_876)
            #expect(detail.operation.zone == .collection && detail.operation.kind == .fetch)
        }
    }

    @Test(arguments: [false, true])
    func policyRequiredCompletionReadSeamDistinguishesTypedCancelledFromNone(empty: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, execution) = try await requiredCompletionExecution(fixture)
            _ = try await execution.reportInvocation(zones: [.collection], kind: .fetch)
            let raw = execution.raw
            let setupError = BridgeError.invalidArgument("required result listener")
            let attempted = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            await #expect(throws: empty ? setupError : BridgeError.cancelled) {
                let _: MeguriCoreBindings.CloudPolicyOutput = try await CoreCloudPolicyService.runOrdered(
                    .cloudPolicyExecutionStartReportCompletion, result: .cloudPolicyOutputResult,
                    track: { handle in
                        attempted.withLock { $0 = handle }
                        throw setupError
                    }, requiredCompletion: true
                ) {
                    let job = try raw.startReportCompletion(facts: .init(zones: [.collection], kind: .fetch,
                        started: false, failed: false, code: nil, retryAfterMs: nil, saved: 0, appCancelled: false))
                    return CloudPolicyJobValue(handle: job.handle, read: {
                        // Exercise Swift's read-error/None precedence seam on
                        // a real required job; Rust owns the actual cell and
                        // panic/no-body witnesses in its parallel suite.
                        guard try job.result.result() != nil else {
                            throw BridgeError.internal("Required job did not store its return")
                        }
                        if empty { return nil }
                        try testReturnError(error: .Cancelled)
                        throw BridgeError.internal("testReturnError did not throw")
                    })
                }
            }
            let handle = try #require(attempted.withLock { $0 })
            #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
            _ = try await policy.status() // Both throwing paths release ordering.
        }
    }

    @Test func policyRequiredCompletionEmptyCellKeepsTerminalFailureAheadOfSetup() async throws {
        try await withCloudConnection { _ in
            try await CoreEventStream.shared.start()
            // Existing Core-job seam supplies the same Failed/Internal
            // terminal used for a panic, with no invented Host or capability.
            let handle = try await MaintenanceLane.shared.call("test.startJob") {
                try testStartJob(durationMs: 0, reportsPerSecond: 1, failWith: .Internal(message: "required body failed"))
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == handle.id { break }
            }
            await #expect(throws: BridgeError.internal("required body failed")) {
                let _: Bool = try await CoreCloudPolicyService.runOrdered(
                    .cloudPolicyExecutionStartReportCompletion, result: .cloudPolicyOutputResult,
                    track: { _ in throw BridgeError.invalidArgument("no-body listener") }, requiredCompletion: true
                ) { CloudPolicyJobValue(handle: handle, read: { nil }) }
            }
            #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
            _ = try await CoreCloudPolicyService(try await rawJobPolicy()).status()
        }
    }

    @Test(arguments: [false, true])
    func policyRequiredCompletionKeepsOrderUntilAcceptedJobDrains(listenerFailure: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let (_, execution) = try await requiredCompletionExecution(fixture)
            _ = try await execution.reportInvocation(zones: [.collection], kind: .fetch)
            let raw = execution.raw
            let endedFacts = MeguriCoreBindings.CloudPolicyOperationFacts(zones: [.collection], kind: .fetch,
                started: false, failed: false, code: nil, retryAfterMs: nil, saved: 0, appCancelled: false)
            let baseline = try await MaintenanceLane.shared.call(.cloudPolicyExecutionOperationActivity) {
                try raw.operationActivity(facts: endedFacts)
            }
            let baselineActions = try policyJobActions(baseline)
            try #require(baselineActions == [])
            _ = try await execution.reportInvocation(zones: [.collection], kind: .fetch)
            let rawPolicy = try await rawJobPolicy()
            var holders: [CoreJob] = []
            do {
                for _ in 0..<4 {
                    let job = try await Jobs.start("test.startJob") { try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil) }
                    holders.append(job)
                    var progress = job.progress.makeAsyncIterator()
                    try #require(await progress.next() != nil, "positive entry into each existing pool worker")
                }
            } catch {
                await drainPolicyPoolHolders(holders)
                throw error
            }
            let accepted = AsyncStream<Void>.makeStream()
            let queued = AsyncStream<Void>.makeStream()
            let handle = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let read = Mutex(false)
            let returned = Mutex(false)
            let completed = Task {
                defer { returned.withLock { $0 = true }; accepted.continuation.finish() }
                return try await CoreCloudPolicyService.runOrdered(
                    .cloudPolicyExecutionStartReportCompletion, result: .cloudPolicyOutputResult,
                    track: { admitted in
                        handle.withLock { $0 = admitted }
                        accepted.continuation.yield(())
                        if listenerFailure { throw BridgeError.invalidArgument("queued Ended listener") }
                        return try Jobs.track(admitted)
                    }, requiredCompletion: true
                ) {
                    let job = try raw.startReportCompletion(facts: endedFacts)
                    return CloudPolicyJobValue(handle: job.handle, read: {
                        read.withLock { $0 = true }
                        return try job.result.result()
                    })
                }
            }
            var following: Task<CoreBridge.CloudPolicyStatus, any Error>?
            do {
                var arrivals = accepted.stream.makeAsyncIterator()
                try #require(await arrivals.next() != nil)
                let successor = CoreCloudPolicyService(rawPolicy, onJobQueued: { queued.continuation.yield(()) })
                let next = Task {
                    defer { queued.continuation.finish() }
                    return try await successor.status()
                }
                following = next
                var waiting = queued.stream.makeAsyncIterator()
                try #require(await waiting.next() != nil, "following facade bypassed required completion drain")
                completed.cancel(); next.cancel()
                #expect(!read.withLock { $0 } && !returned.withLock { $0 })
                await drainPolicyPoolHolders(holders)
                let output = try await completed.value
                _ = try await next.value
                let admitted = try #require(handle.withLock { $0 })
                #expect(try await CoreBridge.CancelToken(raw: admitted.cancel).isCancelled() == listenerFailure)
                #expect(read.withLock { $0 } && returned.withLock { $0 })
                #expect(try policyJobActions(output) == baselineActions)
                // A third acquisition detects an unreleased final permit.
                _ = try await successor.status()
            } catch {
                await drainPolicyPoolHolders(holders)
                _ = try? await completed.value
                _ = try? await following?.value
                throw error
            }
        }
    }

    @Test func policyOrdinaryResourceCancelledPlaceholderStillKeepsSetupError() async throws {
        try await withCloudConnection { fixture in
            let raw = try await rawJobPolicy()
            try #require(try await fixture.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
            let setupError = BridgeError.invalidArgument("ordinary resource listener")
            await #expect(throws: setupError) {
                let _: MeguriCoreBindings.CloudPolicyStatus = try await CoreCloudPolicyService.runOrdered(
                    .cloudPolicyStartStatus, result: .cloudPolicyStatusResult,
                    track: { _ in throw setupError }, retainResourceResultOnTrackingFailure: true
                ) {
                    let job = try raw.startStatus()
                    return CloudPolicyJobValue(handle: job.handle, read: { try job.result.result() })
                }
            }
        }
    }

    @Test(arguments: [false, true])
    func policyAssetReadPassListenerFailureRetainsOriginalWorkAuthorityAfterClose(reopen: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let execution = try await assetExecutorRetryExecution(fixture, policy: policy, records: [remote])
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginAssetReadPass) {
                try raw.startBeginAssetReadPass()
            }
            // Observe the real completed pass before forcing listener setup
            // failure, so cancellation must retain its issued resource.
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudPolicyAssetReadPassResult) {
                try started.result.result()
            })
            let original = CoreCloudAssetReadPass(actual)
            var other: CoreCloudAssetReadPass?
            do {
                let selected = try #require(try await original.next())
                let attempted = Mutex<UInt64?>(nil)
                let retained: MeguriCoreBindings.CloudAssetReadPass = try await retainCompletedPolicyResource(
                    .cloudPolicyExecutionStartBeginAssetReadPass, result: .cloudPolicyAssetReadPassResult,
                    attempted: { id in attempted.withLock { $0 = id } }, message: "retry pass listener setup",
                    CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
                )
                let recovered = CoreCloudAssetReadPass(retained)
                // Ownership is checked by Rust against the actual selected
                // work. Distinct UniFFI wrappers can still retain the same Arc.
                let cancelled = try await recovered.cancel(selected.handle)
                try await recovered.finish()
                let sdk = try await fixture.engine().instanceID
                let unrelated = try await fixture.core.beginFirstAssetReadPass(sdk: sdk)
                other = unrelated
                let foreignCancel = await Task { try await unrelated.cancel(selected.handle) }.result
                try await unrelated.finish()
                let observed = try await remote.observe()
                _ = try await policy.status() // The failure drain released ordering.

                _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
                let stale = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginAssetReadPass) {
                    try raw.startBeginAssetReadPass()
                }
                for await event in CoreEventStream.shared.events {
                    if case .jobFinished(let id, _) = event.kind, id.rawValue == stale.handle.id { break }
                }
                let staleAttempt = Mutex<UInt64?>(nil)
                await #expect(throws: BridgeError.unavailable(reason: "cloud policy command is stale")) {
                    let _: MeguriCoreBindings.CloudAssetReadPass = try await retainCompletedPolicyResource(
                        .cloudPolicyExecutionStartBeginAssetReadPass, result: .cloudPolicyAssetReadPassResult,
                        attempted: { id in staleAttempt.withLock { $0 = id } }, message: "stale retry pass listener setup",
                        CloudPolicyJobValue(handle: stale.handle, read: { try stale.result.result() })
                    )
                }
                await #expect(throws: BridgeError.unavailable(reason: "cloud policy command is stale")) {
                    _ = try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginAssetReadPass) {
                        try raw.beginAssetReadPass()
                    }
                }
                try await fixture.adapter.stop()
                try await fixture.collection.close()
                if reopen {
                    let location = CollectionLocation(collectionDirectory: fixture.directory.appending(path: "collection"),
                        backupsDirectory: fixture.directory.appending(path: "backups"),
                        appGroupSnapshot: fixture.directory.appending(path: "widget.snapshot"))
                    try await fixture.collection.open(location, randomSeed: 115)
                }
                // Only the original typed cells are read after owner change.
                // They must neither revalidate nor obtain a successor facade.
                let reread = try await MaintenanceLane.shared.call(.cloudPolicyAssetReadPassResult) {
                    try started.result.result()
                }
                var rereadCancel: Result<Bool, any Error>?
                if let reread {
                    let rereadPass = CoreCloudAssetReadPass(reread)
                    rereadCancel = await Task { try await rereadPass.cancel(selected.handle) }.result
                    try await rereadPass.finish()
                }
                await #expect(throws: BridgeError.unavailable(reason: "cloud policy command is stale")) {
                    _ = try await MaintenanceLane.shared.call(.cloudPolicyAssetReadPassResult) { try stale.result.result() }
                }
                if reopen { try await fixture.collection.close() }
                #expect(reread != nil)
                #expect(try rereadCancel?.get() == false)
                #expect(selected.name == remote.record.name && selected.zone == fixture.zone && selected.kind == .retry)
                #expect(cancelled && observed.ownedCount == 0 && observed.retryCount == 0 && observed.rowFields == nil)
                #expect(attempted.withLock { $0 } == started.handle.id)
                #expect(staleAttempt.withLock { $0 } == stale.handle.id)
                #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
                #expect(try await CoreBridge.CancelToken(raw: stale.handle.cancel).isCancelled())
                if case .failure(let error) = foreignCancel {
                    #expect(error as? BridgeError == .unavailable(reason: "wrong asset read owner"))
                }
                else { Issue.record("A different pass accepted the original pass's work") }
            } catch {
                try? await other?.finish()
                try? await original.finish()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func policyFirstAssetReadPassListenerFailureRetainsCompletedEmptyPassAfterClose(reopen: Bool) async throws {
        try await withCloudKitFixture { fixture in
            let sdk = try await fixture.engine().instanceID
            // Construct on the registered lane while the fixture's original
            // Host is current; result reads retain only this workflow's cell.
            let raw = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudWorkflowStartBeginFirstAssetReadPass) {
                try raw.startBeginFirstAssetReadPass(sdk: sdk)
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudPolicyAssetReadPassResult) {
                try started.result.result()
            }, "A completed empty pass is an object, not a pending result")
            let original = CoreCloudAssetReadPass(actual)
            do {
                let initial = try await original.next()
                let attempted = Mutex<UInt64?>(nil)
                let retained: MeguriCoreBindings.CloudAssetReadPass = try await retainCompletedPolicyResource(
                    .cloudWorkflowStartBeginFirstAssetReadPass, result: .cloudPolicyAssetReadPassResult,
                    attempted: { id in attempted.withLock { $0 = id } }, message: "first pass listener setup",
                    CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
                )
                let recovered = CoreCloudAssetReadPass(retained)
                let empty = try await recovered.next()
                try await recovered.finish()
                _ = try await fixture.core.policy().status()
                try await fixture.adapter.stop()
                try await fixture.collection.close()
                if reopen {
                    let location = CollectionLocation(collectionDirectory: fixture.directory.appending(path: "collection"),
                        backupsDirectory: fixture.directory.appending(path: "backups"),
                        appGroupSnapshot: fixture.directory.appending(path: "widget.snapshot"))
                    try await fixture.collection.open(location, randomSeed: 115)
                }
                let reread = try await MaintenanceLane.shared.call(.cloudPolicyAssetReadPassResult) {
                    try started.result.result()
                }
                if reopen { try await fixture.collection.close() }
                #expect(initial == nil && empty == nil)
                #expect(reread != nil)
                #expect(attempted.withLock { $0 } == started.handle.id)
                #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            } catch {
                try? await original.finish()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func policyAssetNextListenerFailureRetainsExactWorkAndCellAfterClose(reopen: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let execution = try await assetExecutorRetryExecution(fixture, policy: policy, records: [remote])
            let rawExecution = execution.raw
            let raw = try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginAssetReadPass) {
                try rawExecution.beginAssetReadPass()
            }
            let original = CoreCloudAssetReadPass(raw)
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudAssetReadPassStartNext) { try raw.startNext() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudAssetReadNextResult) {
                try started.result.result()
            })
            let selected = try #require(actual.work)
            let attempted = Mutex<UInt64?>(nil)
            do {
                let recovered: MeguriCoreBindings.CloudAssetReadNextValue = try await retainCompletedPolicyResource(
                    .cloudAssetReadPassStartNext, result: .cloudAssetReadNextResult,
                    attempted: { id in attempted.withLock { $0 = id } }, message: "asset next listener setup",
                    CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
                )
                let work = try #require(recovered.work)
                let sdk = try await fixture.engine().instanceID
                let other = try await fixture.core.beginFirstAssetReadPass(sdk: sdk)
                let foreign = await Task { try await other.cancel(.init(raw: work.handle)) }.result
                try await other.finish()
                #expect(try await original.cancel(.init(raw: work.handle)))
                try await original.finish()
                let observed = try await remote.observe()
                #expect(observed.retryCount == 0 && observed.ownedCount == 0 && observed.rowFields == nil)
                #expect(work.name == selected.name && work.name == remote.record.name)
                #expect(work.zone == selected.zone && work.kind == .retry)
                if case .failure(let error) = foreign {
                    #expect(error as? BridgeError == .unavailable(reason: "wrong asset read owner"))
                } else { Issue.record("Foreign pass accepted selected work") }
                _ = try await policy.status() // failure drain released ordering
                try await fixture.adapter.stop()
                try await fixture.collection.close()
                if reopen {
                    let location = CollectionLocation(collectionDirectory: fixture.directory.appending(path: "collection"),
                        backupsDirectory: fixture.directory.appending(path: "backups"),
                        appGroupSnapshot: fixture.directory.appending(path: "widget.snapshot"))
                    try await fixture.collection.open(location, randomSeed: 115)
                }
                let retained = try #require(try await MaintenanceLane.shared.call(.cloudAssetReadNextResult) {
                    try started.result.result()
                })
                let retainedWork = try #require(retained.work)
                #expect(retainedWork.name == selected.name && retainedWork.zone == selected.zone)
                #expect(try await original.cancel(.init(raw: retainedWork.handle)) == false)
                #expect(attempted.withLock { $0 } == started.handle.id)
                #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
                if reopen { try await fixture.collection.close() }
            } catch {
                try? await original.finish()
                throw error
            }
        }
    }

    @Test func policyAssetNextListenerFailureKeepsInitiallyEmptyFirstPassExhaustion() async throws {
        try await withCloudKitFixture { fixture in
            let sdk = try await fixture.engine().instanceID
            let workflow = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
            let raw = try await MaintenanceLane.shared.call(.cloudWorkflowBeginFirstAssetReadPass) {
                try workflow.beginFirstAssetReadPass(sdk: sdk)
            }
            let original = CoreCloudAssetReadPass(raw)
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudAssetReadPassStartNext) { try raw.startNext() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let retained: MeguriCoreBindings.CloudAssetReadNextValue = try await retainCompletedPolicyResource(
                .cloudAssetReadPassStartNext, result: .cloudAssetReadNextResult,
                attempted: { _ in }, message: "empty next listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(retained.work == nil)
            try await original.finish()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudAssetReadNextResult) {
                try started.result.result()
            })
            #expect(reread.work == nil)
        }
    }

    @Test func policyOriginalOwnerMetadataSurvivesSuspendAndKeepsOrdinaryJobsCancelled() async throws {
        try await withCloudConnection { fixture in
            let policy = try await fixture.workflow.policy()
            try #require(try await fixture.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
            try await policy.validateOriginalOwner()
            await #expect(throws: BridgeError.cancelled) { _ = try await policy.status() }
            await #expect(throws: BridgeError.cancelled) { _ = try await policy.input(.foreground) }
            try await policy.validateOriginalOwner()
            try await fixture.collection.close()
            await #expect(throws: BridgeError.cloudPolicy(.closed)) {
                try await policy.validateOriginalOwner()
            }
        }
    }

    @Test func policyFacadeOrdersDelayedInputsAndKeepsCancelledTaskEffectsAndReadiness() async throws {
        try await withCloudConnection { _ in
            let hold = CloudCallbackGate()
            let (queued, didQueue) = AsyncStream.makeStream(of: Void.self)
            let first = Mutex(true)
            let raw = try await rawJobPolicy()
            let policy = CoreCloudPolicyService(raw, beforeJobStart: { name in
                let pause = first.withLock { value in let old = value; value = false; return old }
                if name == .cloudPolicyStartInput && pause { await hold.enter() }
            }, onJobQueued: { didQueue.yield(()) })
            let notifications = await policy.readinessChanges()
            let a = Task { try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false))) }
            var second: Task<CoreBridge.CloudPolicyOutput, any Error>?
            do {
                try await hold.waitForArrival()
                let b = Task {
                    defer { didQueue.finish() }
                    return try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false)))
                }
                second = b
                var waiting = queued.makeAsyncIterator()
                try #require(await waiting.next() != nil, "second input bypassed the ordering permit")
                a.cancel(); b.cancel()
                await hold.open()
                _ = try await a.value
                _ = try await b.value
                let status = try await policy.status()
                #expect(status.controls.userPaused == false, "second input must follow the delayed first input")
                var readiness = notifications.makeAsyncIterator()
                #expect(await readiness.next() != nil, "cancelled Swift tasks still notify readiness")
            } catch {
                await hold.open()
                _ = try? await a.value
                _ = try? await second?.value
                throw error
            }
            didQueue.finish()
        }
    }

    @Test func policyFacadeListenerFailureCancelsAndDrainsStartedWork() async throws {
        try await withCloudConnection { _ in
            let witness = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let raw = try await rawJobPolicy()
            let policy = CoreCloudPolicyService(raw, track: { handle in
                witness.withLock { $0 = handle }
                throw BridgeError.invalidArgument("policy listener setup")
            })
            do { _ = try await policy.input(.foreground); Issue.record("setup failure lost") }
            catch { #expect(error as? BridgeError == .invalidArgument("policy listener setup")) }
            let handle = try #require(witness.withLock { $0 })
            #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
            // A following facade call must acquire the permit after the drain.
            _ = try await CoreCloudPolicyService(raw).status()
        }
    }

    @Test func policyListenerFailureDrainsAnAlreadyDeliveredTerminal() async throws {
        try await withCloudConnection { _ in
            try await CoreEventStream.shared.start()
            let raw = try await rawJobPolicy()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyStartStatus) { try raw.startStatus() }
            // Observe real terminal delivery before trying listener setup. Its
            // unclaimed outcome must be consumed by the fallback drain waiter.
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            do {
                _ = try await Jobs.trackDraining(started.handle, setup: { _ in throw BridgeError.invalidArgument("early terminal setup") })
                Issue.record("setup failure lost")
            } catch { #expect(error as? BridgeError == BridgeError.invalidArgument("early terminal setup")) }
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            let value = try await MaintenanceLane.shared.call(.cloudPolicyStatusResult) { try started.result.result() }
            #expect(value != nil)
        }
    }

    @Test func policyRegistrationListenerFailureRetainsInstalledIdentity() async throws {
        try await withNativePolicyFixture { fixture in
            try await fixture.requireNativePolicyBaseline()
            let policy = try await fixture.core.policy()
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter)
            _ = try await consumePolicySetup(policy.input(.settings(.init(enabled: false, paused: false, automatic: false, media: false))), executor: executor)
            _ = try await consumePolicySetup(policy.refreshAccount(), executor: executor)
            _ = try await consumePolicySetup(policy.input(.network(reachable: true)), executor: executor)
            let output = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false)))
            guard case .commands(let commands) = output else {
                throw BridgeError.internal("Expected policy startup commands")
            }
            let command = try #require(commands.first { if case .startEngine = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartRegisterSdk) {
                try raw.startRegisterSdk()
            }
            // Real terminal delivery fixes the race at the already-installed
            // registration, before the injected listener setup error.
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let installed = try #require(try await MaintenanceLane.shared.call(.cloudPolicyRegistrationResult) {
                try started.result.result()
            })
            try await fixture.core.validateSDK(installed.instanceId)
            let attempted = Mutex<UInt64?>(nil)
            let retained: MeguriCoreBindings.CloudEngineRegistration = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartRegisterSdk, result: .cloudPolicyRegistrationResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "registration listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            #expect(retained.instanceId == installed.instanceId)
            #expect(retained.serialization == installed.serialization)
            #expect(retained.collectionZone == installed.collectionZone)
            #expect(retained.needsRootFetch == installed.needsRootFetch)
            try await fixture.core.validateSDK(retained.instanceId)
            try await execution.validate()
            // A following ordered call proves the drained registration released
            // its permit. Cleanup names only the recovered original identity.
            _ = try await policy.status()
            try await fixture.core.retireSDK(retained.instanceId, policyStop: false)
            try await fixture.core.releaseSDK(retained.instanceId)
            let successor = try await fixture.core.registerSDK(automatic: false)
            #expect(successor.instanceID != retained.instanceId)
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudPolicyRegistrationResult) {
                try started.result.result()
            })
            #expect(reread.instanceId == retained.instanceId)
            try await fixture.core.validateSDK(successor.instanceID)
            try await fixture.core.retireSDK(successor.instanceID, policyStop: false)
            try await fixture.core.releaseSDK(successor.instanceID)
        }
    }

    @Test func policyFetchAndSendListenerFailureRetainsCallIdentityAndCompletedNoCall() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected exchange commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let sdk = try #require(try await execution.originalSDK())
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let fetch = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginFetch) {
                try raw.startBeginFetch(zones: ["meguri-root", fixture.zone])
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == fetch.handle.id { break }
            }
            let fetchID = try #require(try await MaintenanceLane.shared.call(.cloudPolicyFetchResult) { try fetch.result.result() })
            let attemptedFetch = Mutex<UInt64?>(nil)
            let retainedFetch: UInt64 = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartBeginFetch, result: .cloudPolicyFetchResult,
                attempted: { id in attemptedFetch.withLock { $0 = id } }, message: "fetch listener setup",
                CloudPolicyJobValue(handle: fetch.handle, read: { try fetch.result.result() })
            )
            #expect(attemptedFetch.withLock { $0 } == fetch.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: fetch.handle.cancel).isCancelled())
            #expect(retainedFetch == fetchID && fetchID > 0)
            try await fixture.core.willFetch(sdk: sdk, group: "meguri-\(retainedFetch)", manual: true)
            try await fixture.core.didFetch(sdk: sdk, group: "meguri-\(retainedFetch)", manual: true)
            _ = try await fixture.core.finishFetch(sdk: sdk, id: retainedFetch)

            let send = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginSend) { try raw.startBeginSend() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == send.handle.id { break }
            }
            let sent = try #require(try await MaintenanceLane.shared.call(.cloudPolicySendResult) { try send.result.result() })
            let sendID = try #require(sent.call)
            let attemptedSend = Mutex<UInt64?>(nil)
            let retainedSend: MeguriCoreBindings.CloudPolicySendCall = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartBeginSend, result: .cloudPolicySendResult,
                attempted: { id in attemptedSend.withLock { $0 = id } }, message: "send listener setup",
                CloudPolicyJobValue(handle: send.handle, read: { try send.result.result() })
            )
            #expect(attemptedSend.withLock { $0 } == send.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: send.handle.cancel).isCancelled())
            #expect(retainedSend.call == sendID && sendID > 0)
            try await fixture.core.finishSend(sdk: sdk, id: sendID, pending: [:])

            _ = try await policy.captureExchange(command)
            let noCall = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginSend) { try raw.startBeginSend() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == noCall.handle.id { break }
            }
            let attemptedNoCall = Mutex<UInt64?>(nil)
            let retainedNoCall: MeguriCoreBindings.CloudPolicySendCall = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartBeginSend, result: .cloudPolicySendResult,
                attempted: { id in attemptedNoCall.withLock { $0 = id } }, message: "no-call listener setup",
                CloudPolicyJobValue(handle: noCall.handle, read: { try noCall.result.result() })
            )
            #expect(attemptedNoCall.withLock { $0 } == noCall.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: noCall.handle.cancel).isCancelled())
            #expect(retainedNoCall.call == nil)
            #expect(try await execution.beginSend() == nil)
            _ = try await policy.status()
            try await execution.validate()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyFetchResult) { try fetch.result.result() } == retainedFetch)
            let rereadSend = try #require(try await MaintenanceLane.shared.call(.cloudPolicySendResult) { try send.result.result() })
            let rereadNoCall = try #require(try await MaintenanceLane.shared.call(.cloudPolicySendResult) { try noCall.result.result() })
            #expect(rereadSend.call == retainedSend.call && rereadNoCall.call == nil)
        }
    }

    @Test func policyStartedListenerFailureRetainsActualMultizoneOutputsAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected exchange commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let pendingBefore = try await policy.status().pending
            let added = try await CoreDeckService().addDeck(name: "Started listener pending deck")
            try await fixture.core.awaitTracking(generation: added.summary.generation, seq: added.summary.seq)
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartReportInvocation) {
                try raw.startReportInvocation(zones: [.root, .collection], kind: .fetch)
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            guard case .commands(let startedCommands) = actual else {
                throw BridgeError.internal("Expected Started outputs")
            }
            #expect(startedCommands.contains { command in
                if case .statusChanged(let status) = command.action { status.pending == pendingBefore + 1 } else { false }
            })
            let attempted = Mutex<UInt64?>(nil)
            let retained: MeguriCoreBindings.CloudPolicyOutput = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartReportInvocation, result: .cloudPolicyOutputResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "Started listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            // Compare full action payloads (not just command counts) from the
            // exact retained cell. No new projection is sampled as expected data.
            let expected = try policyJobActions(actual)
            let recovered = try policyJobActions(retained)
            #expect(!expected.isEmpty && recovered == expected)
            _ = try await policy.status() // The listener-failure drain released ordering.
            _ = try await execution.report(zones: [.root, .collection], kind: .fetch, started: false, error: CancellationError())
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            #expect(try policyJobActions(reread) == expected)
        }
    }

    @Test func policyFacadeKeepsTypedFailureBeforeGenericJobFailure() async throws {
        try await withCloudConnection { _ in
            let policy = CoreCloudPolicyService(try await rawJobPolicy())
            await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                _ = try await policy.projectIdentityRead(UInt64.max)
            }
        }
    }
}


extension EventTests {
    @Test func policyControlListenerFailureRetainsValidationAndExactMediaBooleansAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected exchange commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let validated = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartValidateControl) { try raw.startValidateControl() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == validated.handle.id { break }
            }
            let validationAttempt = Mutex<UInt64?>(nil)
            let validation: Bool = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartValidateControl, result: .cloudPolicyControlResult,
                attempted: { id in validationAttempt.withLock { $0 = id } }, message: "control listener setup",
                CloudPolicyJobValue(handle: validated.handle, read: { try validated.result.result() })
            )
            #expect(validation && validationAttempt.withLock { $0 } == validated.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: validated.handle.cancel).isCancelled())
            _ = try await policy.status()
            let stopped = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartContinueMediaAfterSend) { try raw.startContinueMediaAfterSend(code: 25) }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == stopped.handle.id { break }
            }
            let mediaAttempt = Mutex<UInt64?>(nil)
            let continued: Bool = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartContinueMediaAfterSend, result: .cloudPolicyMediaContinuationResult,
                attempted: { id in mediaAttempt.withLock { $0 = id } }, message: "media continuation listener setup",
                CloudPolicyJobValue(handle: stopped.handle, read: { try stopped.result.result() })
            )
            #expect(!continued && mediaAttempt.withLock { $0 } == stopped.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: stopped.handle.cancel).isCancelled())
            _ = try await policy.status()
            // The same issued validation retains its precise stale error,
            // including recovery after a listener setup failure.
            _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
            let stale = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartValidateControl) { try raw.startValidateControl() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == stale.handle.id { break }
            }
            let staleAttempt = Mutex<UInt64?>(nil)
            await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                let _: Bool = try await retainCompletedPolicyResource(
                    .cloudPolicyExecutionStartValidateControl, result: .cloudPolicyControlResult,
                    attempted: { id in staleAttempt.withLock { $0 = id } }, message: "stale control listener setup",
                    CloudPolicyJobValue(handle: stale.handle, read: { try stale.result.result() })
                )
            }
            #expect(staleAttempt.withLock { $0 } == stale.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: stale.handle.cancel).isCancelled())
            _ = try await policy.status()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyControlResult) { try validated.result.result() } == true)
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyMediaContinuationResult) { try stopped.result.result() } == false)
            await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                _ = try await CoreCloudPolicyService.call(.cloudPolicyControlResult) { try stale.result.result() }
            }
        }
    }

    @Test func policyMediaContinuationListenerFailureRetainsActualTrueAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true))) else {
                throw BridgeError.internal("Expected media commands")
            }
            let command = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let sdk = try #require(try await execution.originalSDK())
            _ = try await policy.input(.operationError(instance: sdk, operation: .init(zone: .media, kind: .send),
                error: .quotaFull, code: 25, retryAfterMilliseconds: nil))
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartContinueMediaAfterSend) { try raw.startContinueMediaAfterSend(code: 25) }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let attempted = Mutex<UInt64?>(nil)
            let continued: Bool = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartContinueMediaAfterSend, result: .cloudPolicyMediaContinuationResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "quota continuation listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(continued && attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            _ = try await policy.status()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyMediaContinuationResult) { try started.result.result() } == true)
        }
    }

    @Test func policyFlushListenerFailureRetainsActualRefreshOutputsAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            _ = try await policy.input(.syncNowRequested(.command))
            guard case .commands(let commands) = try await policy.input(.background(deadlineMilliseconds: 1_900_000_000_000)) else {
                throw BridgeError.internal("Expected flush commands")
            }
            let command = try #require(commands.first { if case .backgroundFlush = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let pendingBefore = try await policy.status().pending
            let added = try await CoreDeckService().addDeck(name: "Flush listener pending deck")
            try await fixture.core.awaitTracking(generation: added.summary.generation, seq: added.summary.seq)
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartPrepareFlush) { try raw.startPrepareFlush() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            guard case .commands(let refreshed) = actual else { throw BridgeError.internal("Expected flush Refresh outputs") }
            #expect(refreshed.contains { command in
                if case .statusChanged(let status) = command.action { status.pending == pendingBefore + 1 } else { false }
            })
            let attempted = Mutex<UInt64?>(nil)
            let retained: MeguriCoreBindings.CloudPolicyOutput = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartPrepareFlush, result: .cloudPolicyOutputResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "flush listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            let expected = try policyJobActions(actual)
            let recovered = try policyJobActions(retained)
            #expect(!expected.isEmpty && recovered == expected)
            _ = try await policy.status()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            #expect(try policyJobActions(reread) == expected)
        }
    }
}


extension EventTests {
    @Test(arguments: [false, true])
    func policyGenericStartedFullFactsMatchSynchronousProjection(cancelled: Bool) async throws {
        var baseline: [String]?
        for asynchronous in [false, true] {
            try await withNativePolicyFixture { fixture in
                let (_, execution) = try await requiredCompletionExecution(fixture)
                let failure: any Error = cancelled ? CancellationError() : CloudKitSDKOperationFailure(
                    underlying: NSError(domain: "synthetic-sdk", code: 7), code: 7, retryAfterMilliseconds: 4_321)
                let actions: [String]
                if asynchronous {
                    guard case .commands(let commands) = try await execution.report(
                        zones: [.root, .collection, .media], kind: .fetch, started: true, error: failure, saved: 17
                    ) else { throw BridgeError.internal("Expected generic Started outputs") }
                    actions = commands.map { String(describing: $0.action) }
                } else {
                    let raw = execution.raw
                    let output = try await MaintenanceLane.shared.call(.cloudPolicyExecutionOperationActivity) {
                        try raw.operationActivity(facts: .init(zones: [.root, .collection, .media], kind: .fetch,
                            started: true, failed: true, code: cancelled ? nil : 7,
                            retryAfterMs: cancelled ? nil : 4_321, saved: 17, appCancelled: cancelled))
                    }
                    // Compare bridge DTO descriptions on both paths.
                    guard case .commands(let commands) = CoreBridge.CloudPolicyOutput(output) else {
                        throw BridgeError.internal("Expected synchronous Started outputs")
                    }
                    actions = commands.map { String(describing: $0.action) }
                }
                if asynchronous { #expect(actions == baseline) }
                else {
                    try #require(actions == [])
                    baseline = actions
                }
            }
        }
    }

    @Test func policyGenericStartedReportDrainsQueuedJobOnSetupFailure() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, execution) = try await requiredCompletionExecution(fixture)
            let before = try await policy.status().state
            let rawPolicy = try await rawJobPolicy()
            var holders: [CoreJob] = []
            do {
                for _ in 0..<4 {
                    let job = try await Jobs.start("test.startJob") {
                        try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
                    }
                    holders.append(job)
                    var progress = job.progress.makeAsyncIterator()
                    try #require(await progress.next() != nil, "actual pool worker entry")
                }
            } catch {
                await drainPolicyPoolHolders(holders)
                throw error
            }
            let accepted = AsyncStream<Void>.makeStream()
            let queued = AsyncStream<Void>.makeStream()
            let handle = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let returned = Mutex(false)
            let setupError = BridgeError.invalidArgument("generic Started listener setup")
            let reporting = Task {
                defer { returned.withLock { $0 = true }; accepted.continuation.finish() }
                return try await execution.report(zones: [.root, .collection], kind: .fetch, started: true,
                    error: CancellationError(), saved: 17, trackStarted: { admitted in
                        handle.withLock { $0 = admitted }
                        accepted.continuation.yield(())
                        throw setupError
                    })
            }
            var following: Task<CoreBridge.CloudPolicyStatus, any Error>?
            do {
                var arrivals = accepted.stream.makeAsyncIterator()
                try #require(await arrivals.next() != nil)
                let successor = CoreCloudPolicyService(rawPolicy, onJobQueued: { queued.continuation.yield(()) })
                let next = Task {
                    defer { queued.continuation.finish() }
                    return try await successor.status()
                }
                following = next
                var waiting = queued.stream.makeAsyncIterator()
                try #require(await waiting.next() != nil, "generic Started must retain ordering through drain")
                #expect(!returned.withLock { $0 })
                reporting.cancel()
                await drainPolicyPoolHolders(holders)
                do {
                    _ = try await reporting.value
                    Issue.record("Generic Started listener failure was lost")
                } catch { #expect(error as? BridgeError == setupError) }
                _ = try await next.value
                let admitted = try #require(handle.withLock { $0 })
                #expect(try await CoreBridge.CancelToken(raw: admitted.cancel).isCancelled())
                for await event in CoreEventStream.shared.events {
                    if case .jobFinished(let id, let outcome) = event.kind, id.rawValue == admitted.id {
                        #expect(outcome == .cancelled)
                        break
                    }
                }
                #expect(try await policy.status().state == before)
                _ = try await successor.status()
            } catch {
                await drainPolicyPoolHolders(holders)
                _ = try? await reporting.value
                _ = try? await following?.value
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func policyGenericStartedListenerFailureKeepsCompletedOutputsAndExactError(stale: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, execution) = try await requiredCompletionExecution(fixture)
            if stale {
                _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
            }
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartReportStarted) {
                try raw.startReportStarted(facts: .init(zones: [.root, .collection, .media], kind: .fetch,
                    started: true, failed: true, code: 7, retryAfterMs: 4_321, saved: 17, appCancelled: true))
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let attempted = Mutex<UInt64?>(nil)
            if stale {
                await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                    let _: MeguriCoreBindings.CloudPolicyOutput = try await retainCompletedPolicyResource(
                        .cloudPolicyExecutionStartReportStarted, result: .cloudPolicyOutputResult,
                        attempted: { id in attempted.withLock { $0 = id } }, message: "generic stale listener",
                        CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() }))
                }
            } else {
                let expected = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
                let retained: MeguriCoreBindings.CloudPolicyOutput = try await retainCompletedPolicyResource(
                    .cloudPolicyExecutionStartReportStarted, result: .cloudPolicyOutputResult,
                    attempted: { id in attempted.withLock { $0 = id } }, message: "generic completed listener",
                    CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() }))
                #expect(try policyJobActions(retained) == policyJobActions(expected))
            }
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            _ = try await policy.status()
        }
    }
}
