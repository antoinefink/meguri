import Foundation
import Testing

// Prefix evidence only. Real owned progress is proved in Core/Service tests.
@Suite struct AgentSelectedRestorationTests {
    @MainActor private func state(phone: Bool = false, completed: Int = 3, pending: Bool = false,
                                  editGeneration: UInt64 = 7, recordTerminal: Bool = true,
                                  recordCapture: Bool = true, recordWork: Bool = true,
                                  workAfterTerminal: Bool = false, incomingWaiting: UInt64 = 0,
                                  terminalHeld: UInt64 = 0, terminalNewer: UInt64 = 0,
                                  terminalMedia: [AgentFact] = [],
                                  sdkFacts: [[AgentFact]] = [],
                                  workFacts: [AgentFact] = [.init(name: .syncResult, operation: "work-selected-clean-entity1")]) throws -> AgentRestoration {
        var object = try #require(JSONSerialization.jsonObject(with: CloudKitAgentProtocolTests.header) as? [String: Any])
        object["selection"] = "sync_now"; object["device_kind"] = phone ? "iphone" : "mac"
        let headerBytes = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        let header = try AgentHeader.read(headerBytes), digest = AgentJSON.hash(headerBytes), program = try header.program
        let commands = try (1...(completed + 1)).map { position in
            let action = program.actions[position - 1]
            var bytes: [String: Any] = ["header_sha256": digest, "command_seq": position, "leg": "sync_now",
                "action": action.rawValue, "arguments": action.arguments, "account_before": "acct1", "account_after": "acct1"]
            if action == .edit { bytes["recovery_check"] = "sync_now.edit" }
            if action == .syncNow { bytes["recovery_check"] = "sync_now.exchange" }
            return try AgentCommand.read(JSONSerialization.data(withJSONObject: bytes), header: digest, sequence: position, program: program)
        }
        var records: [AgentRecord] = []
        func append(_ kind: AgentRecord.Kind, command: AgentCommand, facts: [AgentFact] = []) {
            let checks = kind == .done ? AgentEngine.requiredChecks(command.action).map {
                AgentCheck(name: $0, outcome: .pass, facts: [.init(name: .commandCompleted)])
            } : []
            records.append(.init(header_sha256: digest, record_seq: records.count + 1, command_seq: command.command_seq,
                command_sha256: String(repeating: "1", count: 64), agent_instance: "original1",
                observed_at: "2026-11-10T00:00:00Z", kind: kind, action: kind == .started ? command.action : nil,
                facts: facts, checks: checks))
        }
        for command in commands.prefix(completed + (pending ? 1 : 0)) {
            append(.started, command: command)
            var facts: [AgentFact] = []
            if command.command_seq == 3 {
                facts = [.init(name: .applied, digest: String(repeating: "2", count: 64), operation: "canonical-active"),
                         .init(name: .applied, digest: String(repeating: "3", count: 64), operation: "canonical-complete-epoch"),
                         .init(name: .applied, count: 7, digest: String(repeating: "4", count: 64), operation: "original-control-witness")]
            } else if command.action == .edit {
                facts = [.init(name: .localCommitted, count: 9, digest: String(repeating: "5", count: 64), operation: "n1"),
                         .init(name: .localDurable, count: 2, digest: String(repeating: "5", count: 64), operation: "n1"),
                         .init(name: .localDurable, count: editGeneration, digest: String(repeating: "6", count: 64), operation: "original-edit-witness")]
            } else if command.action == .syncNow {
                facts = [.init(name: .syncResult, count: 1, digest: String(repeating: "7", count: 64), operation: "original-exchange-issued")]
            }
            for fact in facts { append(.observation, command: command, facts: [fact]) }
            if command.action == .syncNow && recordCapture {
                append(.observation, command: command, facts: [
                    .init(name: .syncResult, count: 1, operation: "exchange-capture"),
                    .init(name: .syncResult, count: 9, operation: "exchange-boundary"),
                    .init(name: .syncResult, operation: "exchange-selected-entity1")
                ])
            }
            if command.action == .syncNow {
                for facts in sdkFacts { append(.observation, command: command, facts: facts) }
            }
            func appendWork() {
                if command.action == .syncNow && recordWork {
                    append(.observation, command: command, facts: [
                        .init(name: .syncResult, count: 1, operation: "exchange-work")
                    ] + workFacts)
                }
            }
            if !workAfterTerminal { appendWork() }
            if command.action == .syncNow && recordTerminal {
                append(.observation, command: command, facts: [
                    .init(name: .syncResult, count: 1, operation: "terminal-exchange-succeeded"),
                    .init(name: .syncResult, count: 30, operation: "terminal-ended-ms"),
                    .init(name: .syncResult, count: 1, operation: "terminal-selected"),
                    .init(name: .syncResult, count: terminalHeld, operation: "terminal-held-back"),
                    .init(name: .syncResult, count: terminalNewer, operation: "terminal-newer-pending"),
                    .init(name: .syncResult, count: incomingWaiting, operation: "terminal-incoming-waiting")
                ] + terminalMedia)
            }
            if workAfterTerminal { appendWork() }
            if command.command_seq <= completed { append(.done, command: command) }
        }
        return .init(header: header, commands: commands, records: records)
    }
    @MainActor @Test func onlyBoundedPrefixesSelectOriginalOwnedReads() throws {
        let mac = try AgentSelectedRestoration(state())
        #expect(mac.nextCommand == 4 && mac.edit == nil && mac.pending == nil)
        let edited = try AgentSelectedRestoration(state(completed: 4))
        #expect(edited.nextCommand == 5 && edited.edit?.seq == 9 && edited.edit?.revision == 2)
        let interruptedEdit = try AgentSelectedRestoration(state(pending: true))
        #expect(interruptedEdit.pending?.action == .edit && interruptedEdit.edit?.seq == 9)
        let exchange = try AgentSelectedRestoration(state(completed: 4, pending: true))
        #expect(exchange.exchange?.exchange == 1 && exchange.pending?.action == .syncNow)
        #expect(exchange.exchange?.terminal.count == 6)
        #expect(exchange.exchange?.boundary == 9 && exchange.exchange?.selected == ["entity1"])
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, recordCapture: false))
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, recordTerminal: false))
        }
        let phone = try AgentSelectedRestoration(state(phone: true))
        #expect(phone.nextCommand == 4 && phone.edit == nil)
        let phoneExchange = try AgentSelectedRestoration(state(phone: true, pending: true))
        #expect(phoneExchange.exchange?.exchange == 1)
        #expect(throws: AgentFailure.implementationMissing) { try AgentSelectedRestoration(state(completed: 2)) }
        #expect(throws: AgentFailure.implementationMissing) { try AgentSelectedRestoration(state(phone: true, completed: 4)) }
        #expect(throws: AgentFailure.implementationMissing) { try AgentSelectedRestoration(state(completed: 5)) }
    }
    @MainActor @Test func partialWrongInstanceReorderedOrMissingOriginalWitnessFails() throws {
        let original = try state(pending: true)
        for operation in ["n1", "original-edit-witness", "original-control-witness"] {
            var records = original.records
            let index = try #require(records.firstIndex { $0.facts.first?.operation == operation })
            records[index].facts[0].digest = nil
            #expect(throws: (any Error).self) {
                try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
            }
        }
        for field in ["agent_instance", "record_seq", "command_sha256"] {
            var records = original.records
            let index = try #require(records.firstIndex { $0.facts.first?.operation == "original-edit-witness" })
            var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(records[index])) as? [String: Any])
            if field == "record_seq" { json[field] = 1 }
            else { json[field] = field == "agent_instance" ? "fresh2" : String(repeating: "9", count: 64) }
            records[index] = try AgentRecord.read(JSONSerialization.data(withJSONObject: json))
            #expect(throws: (any Error).self) {
                try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
            }
        }
    }
    @MainActor @Test func originalEditGenerationBelongsToItsOwnOpening() throws {
        for completed in [3, 4] {
            let restored = try AgentSelectedRestoration(state(completed: completed, pending: true, editGeneration: 8))
            #expect(restored.originalGeneration == 7)
            #expect(restored.edit?.generation == 8)
            #expect(restored.edit?.seq == 9 && restored.edit?.revision == 2)
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(pending: true, editGeneration: 0))
        }
    }
    @MainActor @Test func interruptedExchangeRequiresACompleteResultFromItsOriginalProcess() throws {
        let original = try state(completed: 4, pending: true)
        for foreign in [false, true] {
            var records = original.records
            let index = try #require(records.firstIndex { $0.facts.first?.operation == "terminal-exchange-succeeded" })
            if foreign {
                var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(records[index])) as? [String: Any])
                json["agent_instance"] = "fresh2"
                records[index] = try AgentRecord.read(JSONSerialization.data(withJSONObject: json))
            } else {
                records[index].facts.removeLast()
            }
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
            }
        }
    }
    @MainActor @Test func interruptedExchangeNeedsOriginalWorkBeforeItsTerminalResult() throws {
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, recordWork: false))
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, workAfterTerminal: true))
        }
        let original = try state(completed: 4, pending: true)
        var records = original.records
        let index = try #require(records.firstIndex { $0.facts.first?.operation == "exchange-work" })
        var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(records[index])) as? [String: Any])
        json["agent_instance"] = "fresh2"
        records[index] = try AgentRecord.read(JSONSerialization.data(withJSONObject: json))
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
        }
    }
    @MainActor @Test func originalWorkMustPartitionTheSelectionAndMatchTerminalCounts() throws {
        for labels in [[], ["work-selected-clean-entity2"],
                       ["work-selected-clean-entity1", "work-selected-acked-entity1"],
                       ["work-selected-clean-entity1", "work-selected-clean-entity1"],
                       ["work-selected-blocked-entity1"], ["work-selected-outstanding-entity1"],
                       ["work-selected-clean-entity1", "work-held-acked-entity1"],
                       ["work-selected-clean-entity1", "work-held-pending-entity2", "work-pending-unheld-entity2"],
                       ["work-selected-clean-entity1", "work-pending-unheld-entity2"],
                       ["work-selected-clean-entity1", "work-incoming1-entity2-ready"],
                       ["work-selected-clean-entity1", "work-incoming1-entity2-apply-failed"]] {
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(state(completed: 4, pending: true,
                    workFacts: labels.map { .init(name: .syncResult, operation: $0) }))
            }
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, workFacts: [
                .init(name: .syncResult, operation: "work-selected-clean-entity1"),
                .init(name: .syncResult, count: 1, operation: "work-media-upload-pending")]))
        }
    }
    @MainActor @Test func originalIncomingCountsEntriesAndKeepsOnlyWaitingReasonsForSuccess() throws {
        let labels = ["work-selected-clean-entity1", "work-incoming1-entity2-generation", "work-incoming2-entity2-reference"]
        let facts = labels.map { AgentFact(name: .syncResult, operation: $0) }
        let restored = try AgentSelectedRestoration(state(completed: 4, pending: true, incomingWaiting: 2, workFacts: facts))
        let accounting = try #require(restored.exchange?.accounting)
        #expect(accounting.outcomes[.clean] == ["entity1"])
        #expect(accounting.incoming.map(\.entity) == ["entity2", "entity2"])
        #expect(accounting.incoming.map(\.reason) == [.generation, .reference])
        #expect(accounting.media.isEmpty)
        for invalid in [Array(facts.dropLast()), facts + [facts[1]], [facts[0], facts[2]],
                        [facts[0], facts[1], .init(name: .syncResult, operation: "work-incoming2-entity2-asset")]] {
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(state(completed: 4, pending: true, incomingWaiting: 2, workFacts: invalid))
            }
        }
    }
    @MainActor @Test func originalHeldUnionAndObservedMediaRemainSeparate() throws {
        let members = ["work-selected-acked-entity1", "work-held-acked-entity1",
                       "work-held-pending-entity1", "work-held-pending-entity2", "work-pending-unheld-entity3"]
            .map { AgentFact(name: .syncResult, operation: $0) }
        let media = [AgentFact(name: .syncResult, count: 0, operation: "work-media-upload-pending"),
                     .init(name: .syncResult, count: 1, operation: "work-media-root-acked")]
        let terminalMedia = try media.map { fact in
            let operation = try #require(fact.operation)
            return AgentFact(name: .syncResult, count: fact.count, operation: "terminal-" + String(operation.dropFirst(5)))
        }
        let original = try AgentSelectedRestoration(state(completed: 4, pending: true, terminalHeld: 2,
            terminalNewer: 1, terminalMedia: terminalMedia, sdkFacts: originalSDKSave(held: true), workFacts: members + media))
        let accounting = try #require(original.exchange?.accounting)
        #expect(accounting.heldPending == ["entity1", "entity2"])
        #expect(accounting.heldAcknowledged == ["entity1"] && accounting.pendingUnheld == ["entity3"])
        #expect(accounting.media == ["media-upload-pending": 0, "media-root-acked": 1])
        #expect(accounting.media["media-download-pending"] == nil)
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, terminalHeld: 3,
                terminalNewer: 1, terminalMedia: terminalMedia, workFacts: members + media))
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, terminalHeld: 2,
                terminalNewer: 1, terminalMedia: terminalMedia, workFacts: members + [
                    .init(name: .syncResult, count: 0, operation: "work-media-upload-pending"),
                    .init(name: .syncResult, count: 2, operation: "work-media-root-acked")]))
        }
    }
    @MainActor private func originalSDKSave(held: Bool = false) -> [[AgentFact]] {
        var tick: UInt64 = 0
        func event(_ name: AgentFact.Name, _ operation: String) -> AgentFact {
            tick += 1
            return .init(name: name, count: tick, engine: "engine1", operation: operation)
        }
        return [[event(.applied, "required-scan-before-native-sdk")],
                [event(.applied, "batch1-collection-handoff"),
                 .init(name: .applied, count: 1, engine: "engine1", operation: "batch1-save-count")],
                [event(.applied, "send1-batch1-manual-build")],
                [event(.applied, "send1-batch1-entity1-member")],
                [event(.engineCallback, "sdk1-sent-batch1-delivered")],
                [event(.engineCallback, "sdk1-sent-batch1-committed"),
                 event(.engineCallback, "send1-batch1-entity1-saved"),
                 .init(name: .engineCallback, count: 9, digest: String(repeating: "8", count: 64), engine: "engine1",
                       operation: held ? "send1-batch1-entity1-image-held" : "send1-batch1-entity1-image-live")],
                [event(.engineCallback, "sdk1-sent-batch1-returned"),
                 event(.engineCallback, "send1-batch1-batch-returned")],
                [event(.applied, "send1-manual-returned")],
                [event(.engineCallback, "sdk2-state-delivered")],
                [event(.stateSaved, "sdk2-state-committed"), event(.stateSaved, "sdk2-state-batches"),
                 event(.stateSaved, "sdk2-entity1-read-absent"),
                 event(.stateSaved, "sdk2-send1-batch1-entity1-reflected-absent")],
                [event(.engineCallback, "sdk2-state-returned")]]
    }
    @MainActor @Test func originalAcknowledgementNeedsItsOwnSDKResultAndImage() throws {
        for classification in ["acked", "absent"] {
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(state(completed: 4, pending: true,
                    workFacts: [.init(name: .syncResult, operation: "work-selected-\(classification)-entity1")]))
            }
        }
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        _ = try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: originalSDKSave(), workFacts: work))
        for label in ["send1-batch1-entity1-saved", "send1-batch1-entity1-image-live"] {
            let incomplete = originalSDKSave().map { $0.filter { $0.operation != label } }
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: incomplete, workFacts: work))
            }
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: originalSDKSave()))
        }
    }

    @MainActor @Test func originalAcknowledgementRequiresItsExactEnvelopeDigest() throws {
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        for digest: String? in [nil, "invalid", String(repeating: "8", count: 64)] {
            let groups = originalSDKSave().map { group in
                group.map { fact in
                    var value = fact
                    if value.operation == "send1-batch1-entity1-image-live" { value.digest = digest }
                    return value
                }
            }
            if digest?.count == 64 {
                let original = try AgentSelectedRestoration(state(completed: 4, pending: true,
                    sdkFacts: groups, workFacts: work))
                #expect(original.exchange?.sdk.acknowledgements["entity1"]?.count == 1)
                #expect(original.exchange?.sdk.acknowledgements["entity1"]?.first?.envelopeDigest == digest)
            } else {
                #expect(throws: AgentFailure.recovery) {
                    try AgentSelectedRestoration(state(completed: 4, pending: true,
                        sdkFacts: groups, workFacts: work))
                }
            }
        }
    }
    @MainActor private func retick(_ groups: [[AgentFact]]) -> [[AgentFact]] {
        var tick: UInt64 = 0
        return groups.map { group in group.map { original in
            var fact = original
            let label = fact.operation ?? ""
            if fact.count != nil && !label.hasSuffix("-save-count") && !label.hasSuffix("-delete-count") &&
                !label.contains("-image-") && !label.contains("-own-landed-") {
                tick += 1; fact.count = tick
            }
            return fact
        } }
    }
    @MainActor @Test func originalResultOrderNeedsTheActualReflectedBatchOfTheSameEngine() throws {
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        for label in ["required-scan-before-native-sdk", "send1-batch1-manual-build", "send1-batch1-entity1-member",
                      "sdk1-sent-batch1-delivered", "sdk1-sent-batch1-committed", "sdk1-sent-batch1-returned",
                      "sdk2-state-batches", "sdk2-entity1-read-absent", "sdk2-send1-batch1-entity1-reflected-absent"] {
            let incomplete = originalSDKSave().map { $0.filter { $0.operation != label } }
            #expect(throws: AgentFailure.engineOrderNotShown) {
                try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: incomplete, workFacts: work))
            }
        }
        for variant in ["before", "unreflected", "foreign", "wrong-member", "tick", "non-sdk"] {
            var groups = originalSDKSave()
            if variant == "before" {
                groups = Array(groups.prefix(4)) + groups.suffix(3) + groups.dropFirst(4).dropLast(3)
                groups = retick(groups)
            } else {
                groups = groups.map { group in group.compactMap { original in
                    var fact = original
                    if variant == "unreflected", fact.operation == "sdk2-send1-batch1-entity1-reflected-absent" { return nil }
                    if variant == "unreflected", fact.operation == "sdk2-entity1-read-absent" { fact.operation = "sdk2-entity1-read-held" }
                    if variant == "foreign", fact.operation?.hasPrefix("sdk2-") == true { fact.engine = "engine2" }
                    if variant == "non-sdk" { fact.operation = fact.operation?.replacingOccurrences(of: "sdk2", with: "bad") }
                    if variant == "wrong-member", fact.operation == "sdk2-send1-batch1-entity1-reflected-absent" {
                        fact.operation = "sdk2-send1-batch1-entity2-reflected-absent"
                    }
                    if variant == "tick", fact.operation == "sdk2-state-committed" { fact.count = 1 }
                    return fact
                } }
            }
            #expect(throws: AgentFailure.engineOrderNotShown) {
                try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: groups, workFacts: work))
            }
        }
    }
    @MainActor @Test func originalSavedImageMustMeetTheCapturedRevisionAndHeldFlag() throws {
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        for variant in ["older", "held"] {
            let groups = originalSDKSave().map { group in group.map { original in
                var fact = original
                if fact.operation == "send1-batch1-entity1-image-live" {
                    if variant == "older" { fact.count = 8 }
                    else { fact.operation = "send1-batch1-entity1-image-held" }
                }
                return fact
            } }
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: groups, workFacts: work))
            }
        }
    }
    @MainActor @Test func originalLandedSaveNeedsItsFetchedMemberAndReturnedOriginalCallback() throws {
        let groups: [[AgentFact]] = [
            [.init(name: .applied, count: 1, engine: "engine1", operation: "required-scan-before-native-sdk")],
            [.init(name: .engineCallback, count: 2, engine: "engine1", operation: "sdk1-fetched-delivered")],
            [.init(name: .engineCallback, count: 3, engine: "engine1", operation: "sdk1-fetched-committed"),
             .init(name: .engineCallback, engine: "engine1", operation: "sdk1-fetched-entity1")],
            [.init(name: .engineCallback, count: 4, engine: "engine1", operation: "sdk1-fetched-returned")],
            [.init(name: .applied, count: 9, digest: String(repeating: "8", count: 64), engine: "engine1", operation: "sdk1-own-landed-entity1-live")]]
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        let original = try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: groups, workFacts: work))
        let image = try #require(original.exchange?.sdk.acknowledgements["entity1"]?.first)
        #expect(image.callback == "sdk1" && image.batch == nil && image.seq == 9 && !image.held)
        #expect(image.envelopeDigest == String(repeating: "8", count: 64))
        for label in ["sdk1-fetched-entity1", "sdk1-fetched-returned"] {
            #expect(throws: AgentFailure.recovery) {
                try AgentSelectedRestoration(state(completed: 4, pending: true,
                    sdkFacts: groups.map { $0.filter { $0.operation != label } }, workFacts: work))
            }
        }
    }
    @MainActor @Test func originalBatchMappingRefusesAnAmbiguousRebuildButAllowsAClosedPredecessor() throws {
        let first = originalSDKSave()
        var second = first.dropFirst().map { group in group.map { original in
            var fact = original
            fact.operation = fact.operation?.replacingOccurrences(of: "send1", with: "send2")
                .replacingOccurrences(of: "batch1", with: "batch2")
                .replacingOccurrences(of: "sdk2", with: "sdk4").replacingOccurrences(of: "sdk1", with: "sdk3")
            return fact
        } }
        let stateIndex = try #require(second.firstIndex { $0.first?.operation == "sdk4-state-committed" })
        second[stateIndex].append(.init(name: .stateSaved, count: 1, engine: "engine1",
                                       operation: "sdk4-send1-batch1-entity1-reflected-absent"))
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        let original = try AgentSelectedRestoration(state(completed: 4, pending: true,
            sdkFacts: retick(first + second), workFacts: work))
        #expect(original.exchange?.sdk.acknowledgements["entity1"]?.count == 2)
        let ambiguous = Array(first.prefix(4)) + second.prefix(3) + first.dropFirst(4)
        #expect(throws: AgentFailure.engineOrderNotShown) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: retick(ambiguous), workFacts: work))
        }
    }
    @MainActor @Test func keptErrorsNeedNoPairButOtherResultsStillDo() throws {
        let work = [AgentFact(name: .syncResult, operation: "work-selected-clean-entity1"),
                    .init(name: .syncResult, operation: "work-pending-unheld-entity2")]
        for code in [4, 14] {
            let groups = originalSDKSave().map { group in group.compactMap { original -> AgentFact? in
                var fact = original
                guard fact.operation != "send1-batch1-entity1-image-live",
                      fact.operation != "sdk2-send1-batch1-entity1-reflected-absent" else { return nil }
                fact.operation = fact.operation?.replacingOccurrences(of: "entity1", with: "entity2")
                if fact.operation == "send1-batch1-entity2-saved" { fact.operation = "send1-batch1-entity2-save-error-\(code)" }
                if fact.operation == "sdk2-entity2-read-absent" { fact.operation = "sdk2-entity2-read-held" }
                return fact
            } }
            if code == 4 {
                _ = try AgentSelectedRestoration(state(completed: 4, pending: true, terminalNewer: 1,
                    sdkFacts: groups, workFacts: work))
            } else {
                #expect(throws: AgentFailure.engineOrderNotShown) {
                    try AgentSelectedRestoration(state(completed: 4, pending: true, terminalNewer: 1,
                        sdkFacts: groups, workFacts: work))
                }
            }
        }
    }
    @MainActor @Test func unreportedBatchExemptionNeedsTheObservedStillHeldRemoval() throws {
        let base = originalSDKSave()
        for removal in ["removed-held", "removed-absent"] {
            let build = base.prefix(4).map { group in group.map { original in
                var fact = original
                fact.operation = fact.operation?.replacingOccurrences(of: "entity1", with: "entity2")
                return fact
            } }
            let update = [AgentFact(name: .applied, count: 1, engine: "engine1", operation: "pending-update"),
                          .init(name: .applied, count: 1, engine: "engine1", operation: "entity2-" + removal)]
            let saves = base.suffix(3).map { group in group.compactMap { original -> AgentFact? in
                var fact = original
                if removal == "removed-held", fact.operation == "sdk2-send1-batch1-entity1-reflected-absent" { return nil }
                fact.operation = fact.operation?.replacingOccurrences(of: "sdk2", with: "sdk1")
                    .replacingOccurrences(of: "entity1", with: "entity2")
                return fact
            } }
            let groups = retick(build + [update] + saves)
            if removal == "removed-held" {
                _ = try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: groups))
            } else {
                #expect(throws: AgentFailure.engineOrderNotShown) {
                    try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: groups))
                }
            }
        }
    }
    @MainActor @Test func terminalAbsenceNeedsTheOriginalUnknownItemResult() throws {
        let work = [AgentFact(name: .syncResult, operation: "work-selected-absent-entity1")]
        for code in [11, 4] {
            let groups = originalSDKSave().map { group in group.compactMap { original -> AgentFact? in
                var fact = original
                if fact.operation == "send1-batch1-entity1-image-live" { return nil }
                if fact.operation == "send1-batch1-entity1-saved" { fact.operation = "send1-batch1-entity1-save-error-\(code)" }
                return fact
            } }
            if code == 11 {
                _ = try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: groups, workFacts: work))
            } else {
                #expect(throws: AgentFailure.recovery) {
                    try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: groups, workFacts: work))
                }
            }
        }
    }
    @MainActor @Test func aFreshProcessCannotSupplyTheOriginalAcknowledgement() throws {
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        let original = try state(completed: 4, pending: true, sdkFacts: originalSDKSave(), workFacts: work)
        let records = try original.records.map { record -> AgentRecord in
            guard record.facts.contains(where: { $0.engine != nil }) else { return record }
            var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(record)) as? [String: Any])
            json["agent_instance"] = "fresh2"
            return try AgentRecord.read(JSONSerialization.data(withJSONObject: json))
        }
        #expect(throws: AgentFailure.recovery) {
            try AgentSelectedRestoration(.init(header: original.header, commands: original.commands, records: records))
        }
    }
    @MainActor @Test func originalAdapterChangesNeedTheirUpdateMarker() throws {
        var groups = originalSDKSave()
        groups.insert([.init(name: .applied, count: 1, engine: "engine1", operation: "entity1-removed-held")], at: 8)
        let work = [AgentFact(name: .syncResult, operation: "work-selected-acked-entity1")]
        #expect(throws: AgentFailure.engineOrderNotShown) {
            try AgentSelectedRestoration(state(completed: 4, pending: true, sdkFacts: retick(groups), workFacts: work))
        }
    }
}
