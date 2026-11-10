#if DEBUG && MEGURI_TEST_HOOKS
@testable import AppModel
import CoreBridge
import CloudKit
import Darwin
import Foundation

// This adapter uses the production composition, including its real native SDK
// factory. The test-only admission fences approved acct1 before Core sees a
// fresh identity; production Core still owns ordered identity acceptance.
@MainActor final class CloudKitSmokeRuntime: AgentRuntime {
    nonisolated enum Dependency: String, Codable {
        case signedProductVerifier = "signed-product-receipt-verifier"
        case generatedBase = "verified-generated-small-base"
        case integrityInspector = "smoke-integrity-inspector"
        case recoveryInspector = "durable-smoke-recovery-inspector"
        case originalRoleInspector = "verified-original-small-role-inspector"
        case epochInspector = "original-root-epoch-payload-inspector"
        case adoptionInspector = "original-adoption-backup-activation-inspector"
        case peerApplyInspector = "durable-selected-note-apply-inspector"
        case selectedRecoveryInspector = "durable-selected-operation-recovery-inspector"
    }
    var missing: [Dependency] {
        let selected = program ?? (try? AgentHeader.read(originalHeaderBytes).program)
        return selected == .smoke || selected == nil ? [.recoveryInspector]
            : [.selectedRecoveryInspector]
    }
    private(set) var userSteps: [String] = []
    private let admittedRun: AgentRunDirectory
    private let directory: URL
    private var owner: ReviewCollection?
    private let originalHeaderBytes: Data
    private var admission: AgentAccountAdmission?
    private var runningProduct: AgentRunningProductBinding?
    private var mutation: NoteMutation?
    private var expectedFields: [String] = []
    private var backend: CloudKitSmokeBackend?
    private let inspection = CoreSmokeInspection()
    private let changes = CloudKitSmokeChanges()
    private let evidence = CloudKitSmokeEvidence()
    private lazy var callbackJournal = CloudKitSmokeJournal(evidence: evidence)
    private var preview: CloudConnectionPreview?
    private var deckID: Int64?
    private var marker: String?
    private var sessionID: UInt64?
    private var head: ReviewHeadItem?
    private var headRevision: SmokeCloudEntity?
    private var lastReviewID: Int64?
    private var reviewRowID: Int64?
    private var retainedEntities: [String: SmokeCloudEntity] = [:]
    private var mediaName: String?
    private var mediaBytes: Data?
    private var nextCommand = 1
    private var program: AgentProgram?
    private var originalRole: CloudKitSelectedOriginalRole?
    private var selectedEdit: OriginalCloudEditFacts?
    private var restoredEdit: HistoricalOriginalCloudEdit?
    private var restoration: AgentSelectedRestoration?
    private var selectedExchangeAttempted = false
    private var selectedExchangeRecord: CloudPolicyExchangeRecord?
    // Set only after the earlier command's actual source ACK or peer durable
    // apply + original fetched callback proof has succeeded and been observed.
    private var selectedCloseProof: (program: AgentProgram, generation: UInt64)?

    init(admittedRun: AgentRunDirectory) {
        self.admittedRun = admittedRun
        self.directory = admittedRun.directory
        self.originalHeaderBytes = admittedRun.originalHeaderBytes
    }
    func installCallbackJournal(_ journal: @escaping AgentCallbackJournal) { callbackJournal.install(journal) }

    func verifyProduct(_ header: AgentHeader) throws {
        userSteps = []
        try admittedRun.revalidate()
        guard try AgentHeader.read(originalHeaderBytes) == header else { throw AgentFailure.binding }
        if let admission {
            try checkActualProduct(header)
            try admission.requireHeader(header)
        } else {
            admission = try AgentAccountAdmission(originalHeaderBytes: originalHeaderBytes, header: header,
                productCheck: { try self.checkActualProduct(header) },
                applicationSupportRoot: { try self.admittedRun.retainedApplicationSupportRoot() })
        }
        program = try header.program
    }
    private func checkActualProduct(_ header: AgentHeader) throws {
        if let runningProduct {
            try runningProduct.requireHeader(header, originalHeaderBytes: originalHeaderBytes)
        } else {
            let receipts = try AgentProductReceiptBinding(admittedRunDirectory: directory,
                                                         originalHeaderBytes: originalHeaderBytes)
            let observed = try AgentRunningProductBinding.inspectActual(receipts: receipts)
            try observed.requireHeader(header, originalHeaderBytes: originalHeaderBytes)
            runningProduct = observed
        }
        guard let configuration = CloudBuildConfiguration.fromBundle else {
            userSteps = ["cloudkit-dev-container"]; throw AgentFailure.awaitingUser
        }
        guard case .development = configuration.environment, configuration.container == header.container,
              header.environment == "development" else { throw AgentFailure.wrongContainer }
        if !configuration.schemaConfigured {
            userSteps.append("cloudkit-dev-container"); throw AgentFailure.awaitingUser
        }
        // Parent owns source/signature/install/live-lease admission. This
        // client owns retained receipts and actual image/OS/container checks.
        guard let runningProduct else { throw AgentFailure.implementationMissing }
        try runningProduct.requireIndependentCounterpart()
    }
    private func retainedAdmission() throws -> AgentAccountAdmission {
        try admittedRun.revalidate()
        guard let runningProduct else { throw AgentFailure.implementationMissing }
        try runningProduct.revalidate()
        guard let admission else { throw AgentFailure.implementationMissing }
        try admission.revalidate()
        return admission
    }
    private func resolveAccount(container: CKContainer) async throws -> CloudResolvedAccount {
        let admission = try retainedAdmission()
        let resolved = try await admission.resolveAccount {
            switch try await container.accountStatus() {
            case .available:
                let record = try await container.userRecordID()
                return .init(status: .available, identity: Data(record.recordName.utf8))
            case .noAccount: return .init(status: .noAccount, identity: nil)
            case .restricted: return .init(status: .restricted, identity: nil)
            case .temporarilyUnavailable: return .init(status: .temporarilyUnavailable, identity: nil)
            case .couldNotDetermine: return .init(status: .couldNotDetermine, identity: nil)
            @unknown default: return .init(status: .couldNotDetermine, identity: nil)
            }
        }
        _ = try retainedAdmission()
        return resolved
    }
    private func freshAccount() async throws {
        _ = try retainedAdmission()
        guard let configuration = CloudBuildConfiguration.fromBundle else { throw AgentFailure.implementationMissing }
        _ = try await resolveAccount(container: CKContainer(identifier: configuration.container))
    }
    func verifyAccount(_ header: AgentHeader) async throws {
        try retainedAdmission().requireHeader(header)
        // AgentEngine has durably appended started before entering this method.
        try await freshAccount()
    }
    private func requireVerifiedGeneratedBase() throws {
        try AgentGeneratedBase(admittedRunDirectory: directory, originalHeaderBytes: originalHeaderBytes)
            .requireInstalled(at: directory.appendingPathComponent("data", isDirectory: true))
    }
    func execute(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        guard let program else { throw AgentFailure.binding }
        try program.requirePosition(command)
        guard command.command_seq == nextCommand else { throw AgentFailure.sequence }
        // A failed operation never advances this program.
        do {
            let checks = try await executeStep(command, observe: observe)
            for fact in evidence.drain() { try observe(fact) }
            try evidence.check()
            nextCommand += 1
            return checks
        } catch {
            for fact in evidence.drain() { try observe(fact) }
            throw error
        }
    }
    private func executeStep(_ command: AgentCommand, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        // Closing/draining owned work does not depend on renewed authority.
        if command.action != .close {
            try await freshAccount()
            _ = try retainedAdmission()
            try originalRole?.revalidate()
        }
        switch command.action {
        case .launch:
            return try await openRuntime(restoration: nil, observe: observe)
        case .connect:
            guard let owner, preview == nil, owner.review == nil else { throw AgentFailure.execution }
            try await owner.prepareCloudConnectionPreview()
            await owner.sync.waitForConnectionPreview()
            guard !owner.sync.operationFailed, case .ready(let current, let summary) = owner.sync.preview else { throw AgentFailure.execution }
            if program == .syncNowPhone { try CloudKitSelectedSyncProof.requireReceiverPreview(summary) }
            else { try requireInitialPreview(summary) }
            preview = current
            let fact = AgentFact(name: .applied, count: summary.localCounts[.note], operation: "p1")
            try observe(fact)
            return [.init(name: .coordinatorResult, outcome: .pass, facts: [fact])]
        case .publish:
            guard let owner, let backend, let preview,
                  case .ready(let current, let summary) = owner.sync.preview,
                  CoreSmokeInspection.samePreview(preview, current) else { throw AgentFailure.execution }
            try requireInitialPreview(summary)
            self.preview = nil // consumed once, including failure
            backend.watchPublication = true
            await owner.sync.confirmPublication()
            await owner.sync.waitForInputs()
            try backend.checkFailure()
            let completed = try await backend.publicationCompleted()
            guard !owner.sync.operationFailed, owner.sync.preview == nil,
                  owner.sync.status?.controls.bindingUsable == true,
                  completed.generation == owner.generation,
                  completed.publicationComplete, completed.bindingUsable, completed.engine != nil,
                  let issued = backend.publicationOperations.first,
                  issued.generation == owner.generation, issued.publicationReason == 1, issued.publicationInitial,
                  issued.publicationID != nil, completed.publicationID == nil,
                  backend.publicationOperations.allSatisfy({ $0.publicationID == issued.publicationID && $0.publicationInitial }),
                  backend.publicationOperations.contains(where: { $0.publicationCompleting && $0.publicationBindingID == completed.bindingID }) else { throw AgentFailure.execution }
            guard let publicationID = issued.publicationID else { throw AgentFailure.execution }
            try evidence.requirePublication(publicationID, completed: completed)
            backend.watchPublication = false
            let fact = AgentFact(name: .stateSaved, count: completed.seq, engine: "engine1", operation: "p1")
            try observe(fact)
            if program != .smoke {
                let control = try await SelectedCloudControlInspection().current()
                _ = try retainedAdmission()
                try evidence.requireSelectedPublication(control)
                try evidence.requireSelectedAdmission(current: completed, control: control)
                var facts = try CloudKitSelectedSyncProof.controlFacts(control, generation: owner.generation)
                facts.append(try await originalControlFact(control))
                for fact in facts { try observe(fact) }
                return [.init(name: .coordinatorResult, outcome: .pass, facts: [fact] + facts)]
            }
            return [.init(name: .coordinatorResult, outcome: .pass, facts: [fact])]
        case .adopt:
            guard program == .syncNowPhone, let owner, let preview, let originalRole,
                  owner.state == .ready, owner.review == nil,
                  case .ready(let current, let summary) = owner.sync.preview,
                  CoreSmokeInspection.samePreview(preview, current) else { throw AgentFailure.execution }
            try CloudKitSelectedSyncProof.requireReceiverPreview(summary)
            let old = owner.generation
            let before = changes.events.last?.seq ?? 0
            try evidence.beginAdoption()
            self.preview = nil
            try Task.checkCancellation()
            await owner.sync.confirmAdoption()
            await owner.sync.waitForConnectionPreview()
            await owner.sync.waitForReplacement()
            await owner.waitForCloudActivation()
            await owner.sync.prepareConnectionAuthority()
            await owner.sync.waitForInputs()
            _ = try retainedAdmission()
            try Task.checkCancellation()
            try changes.checkFailure()
            // These barriers joined the original confirmation, replacement and
            // activation. A refusal/failure has no future replacement to await.
            // Generation changes reach owner and this sink in the same
            // synchronous ChangeFanout delivery.
            guard !owner.sync.operationFailed, owner.state == .ready,
                  owner.generation > old, owner.sync.preview == nil, owner.sync.recovery == nil,
                  let replaced = changes.events.first(where: { event in
                      guard event.seq > before, event.generation == owner.generation,
                            case .collectionReplaced = event.kind else { return false }
                      return true
                  }) else { throw AgentFailure.execution }
            let control = try await SelectedCloudControlInspection().current()
            let completed = try await inspection.cloud()
            let role = try await originalRole.validateOwned(generation: control.generation)
            _ = try retainedAdmission()
            try Task.checkCancellation()
            guard owner.state == .ready, owner.generation == replaced.generation, control.generation == owner.generation,
                  !owner.sync.operationFailed, owner.sync.preview == nil, owner.sync.recovery == nil,
                  completed.bindingUsable, completed.engine != nil, !role.qChanged,
                  control.provenance == .retainedAdoption, control.activation == .proven,
                  control.ownedBackup == .proven else { throw AgentFailure.execution }
            try evidence.requireSelectedAdmission(current: completed, control: control)
            var facts = try CloudKitSelectedSyncProof.controlFacts(control, generation: owner.generation)
            facts.append(try await originalControlFact(control))
            facts.append(.init(name: .applied, count: owner.generation, operation: "owned-adoption-backup-activation"))
            facts.append(.init(name: .localDurable, digest: CloudKitSmokeGeneratedInput.hex(role.noteDigest), operation: "original-n1-preserved"))
            for fact in facts { try observe(fact) }
            return [try await integrityCheck(owner), .init(name: .coordinatorResult, outcome: .pass, facts: facts)]
        case .addNote:
            guard let owner, owner.state == .ready, mutation == nil else { throw AgentFailure.execution }
            let types = try await CoreNotetypeService().notetypeNames()
            guard types.count == 1, let type = types.first, type.name == "QA",
                  let deckID, let marker else { throw AgentFailure.execution }
            _ = try retainedAdmission()
            let record = try await CoreNotetypeService().readNotetype(id: type.id)
            guard record.fields.map(\.name) == ["Q", "A"], record.templates.count == 1 else { throw AgentFailure.execution }
            expectedFields = [marker + "-smoke-n1-Q", marker + "-smoke-n1-A"]
            _ = try retainedAdmission()
            let result = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: expectedFields,
                tags: [], deckId: deckID, guid: nil, updateAddDefaults: false))
            guard result.written, result.cards.count == 1 else { throw AgentFailure.execution }
            mutation = result
            try observe(.init(name: .localCommitted, count: result.summary.seq, operation: "n1"))
            _ = try retainedAdmission()
            try await CoreCollectionService().flushForBackground()
            _ = try retainedAdmission()
            let durable = try await CoreNoteService().readNote(id: result.id)
            guard durable.fields == expectedFields, durable.tags.isEmpty, durable.notetypeId == type.id else { throw AgentFailure.execution }
            guard result.summary.generation == owner.generation,
                  result.summary.changes.notes.filter({ $0.change == .added }).map(\.id) == [result.id],
                  result.summary.changes.cards.filter({ $0.change == .added }).map(\.id) == result.cards else { throw AgentFailure.execution }
            let rows = try await inspection.cloud(noteID: result.id, cardID: result.cards[0])
            guard rows.generation == owner.generation, rows.seq >= result.summary.seq,
                  rows.cardNoteID == result.id else { throw AgentFailure.execution }
            try retain(rows, labels: ["n1"])
            let fact = try entityFact("n1")
            try observe(fact)
            let receipt = AgentFact(name: .localCommitted, count: result.summary.seq, operation: "n1")
            return [.init(name: .localDurability, outcome: .pass, facts: [receipt, fact])]
        case .startSession:
            guard let owner, let backend, let deckID, sessionID == nil, mutation != nil else { throw AgentFailure.execution }
            if owner.policy != .ok { await owner.enableFsrs() }
            guard owner.policy == .ok else { throw AgentFailure.execution }
            await owner.startStudy(deckID: deckID)
            guard let review = owner.review, review.phase == .question,
                  let session = review.sessionID, let item = review.item else { throw AgentFailure.execution }
            let current = try await CoreReviewSessionClient().head(sessionID: session)
            guard current.items.first?.cardId == item.cardId,
                  current.items.first?.stateToken == item.stateToken else { throw AgentFailure.execution }
            sessionID = session; head = item
            let before = try await inspection.cloud(cardID: item.cardId)
            guard before.generation == owner.generation,
                  let baseline = before.entities.first(where: { $0.label == "c1" }) else { throw AgentFailure.execution }
            headRevision = baseline
            try observe(.init(name: .localDurable, count: baseline.revision,
                digest: CloudKitSmokeGeneratedInput.hex(baseline.digest), operation: "c1-before-answer"))
            lastReviewID = before.lastReviewID
            await owner.sync.waitForInputs()
            try backend.checkFailure()
            let fact = AgentFact(name: .applied, count: 1, operation: "c1")
            try observe(fact)
            return [.init(name: .localDurability, outcome: .pass, facts: [fact])]
        case .answer:
            guard let owner, let review = owner.review, let sessionID, let head,
                  review.sessionID == sessionID, review.record.answers.isEmpty,
                  review.item?.cardId == head.cardId, review.item?.stateToken == head.stateToken else { throw AgentFailure.execution }
            let current = try await CoreReviewSessionClient().head(sessionID: sessionID)
            guard current.items.first?.cardId == head.cardId,
                  current.items.first?.stateToken == head.stateToken else { throw AgentFailure.execution }
            let before = changes.events.last?.seq ?? 0
            review.reveal(); await review.settled()
            guard review.phase == .answer, review.item?.stateToken == head.stateToken else { throw AgentFailure.execution }
            review.rate(.good); await review.settled()
            guard review.phase != .error, review.record.answers.count == 1,
                  review.record.answers.first?.cardID == head.cardId else { throw AgentFailure.execution }
            try await CoreCollectionService().flushForBackground()
            let cardID = head.cardId, generation = owner.generation
            let committed = try await changes.next(after: before) {
                guard $0.generation == generation, case .changes(let c) = $0.kind else { return false }
                return c.cards.contains { $0.id == cardID } && !c.revlogAdded.isEmpty
            }
            guard case .changes(let c) = committed.kind, c.revlogAdded.count == 1,
                  let row = c.revlogAdded.first, row > (lastReviewID ?? 0),
                  review.record.answers.first?.durable == true else { throw AgentFailure.execution }
            reviewRowID = row
            let rows = try await inspection.cloud(cardID: cardID, revlogID: row)
            guard rows.generation == generation, rows.seq >= committed.seq, rows.lastReviewID == row,
                  let prior = headRevision, let card = rows.entities.first(where: { $0.label == "c1" }),
                  card.revision > prior.revision, card.digest != prior.digest else { throw AgentFailure.execution }
            try retain(rows, labels: ["c1", "r1"])
            guard let engine = rows.engine, let revlog = retainedEntities["r1"] else { throw AgentFailure.execution }
            let projection = try await inspection.review(sdk: engine, cardID: cardID, revlogID: row)
            let heldFacts = try evidence.held(projection, session: sessionID, answerSeq: committed.seq,
                card: card, review: revlog, baseline: prior)
            for fact in heldFacts { try observe(fact) }
            let facts = [AgentFact(name: .localCommitted, count: committed.seq, operation: "s1"), try entityFact("c1"), try entityFact("r1")]
            for fact in facts { try observe(fact) }
            return [.init(name: .localDurability, outcome: .pass, facts: facts)]
        case .endSession:
            guard let owner, let backend, let sessionID, reviewRowID != nil,
                  owner.review?.sessionID == sessionID else { throw AgentFailure.execution }
            let before = changes.events.last?.seq ?? 0
            await owner.finishReview()
            guard owner.review == nil, owner.previousRecord?.answers.count == 1,
                  owner.previousRecord?.answers.first?.durable == true else { throw AgentFailure.execution }
            _ = try await changes.next(after: before) {
                if case .sessionEnded(let ended) = $0.kind { return ended == sessionID }; return false
            }
            await owner.sync.waitForInputs()
            let observations = try await backend.workflow.observations()
            guard let ended = observations.lastIndex(where: {
                if case .sessionEnded(let actual) = $0 { return actual == sessionID }; return false
            }), observations.dropFirst(ended + 1).contains(where: {
                if case .corePendingEmitted = $0 { return true }; return false
            }) else { throw AgentFailure.execution }
            let fact = AgentFact(name: .applied, count: 1, operation: "s1")
            try observe(fact)
            return [.init(name: .localDurability, outcome: .pass, facts: [fact])]
        case .addMedia:
            guard let owner, let marker, owner.review == nil, reviewRowID != nil, mediaName == nil else { throw AgentFailure.execution }
            let header = try AgentHeader.read(originalHeaderBytes)
            let bytes = CloudKitSmokeGeneratedInput.media(run: header.run_id, device: header.device)
            guard bytes.count == 10_240 else { throw AgentFailure.execution }
            let desired = marker + "-smoke-m1.bin"
            let before = try await CoreMediaService().trackedNames()
            guard !before.names.contains(where: { $0.caseKey == desired.lowercased() }) else { throw AgentFailure.execution }
            let baseline = changes.events.last?.seq ?? 0
            let stored = try await CoreMediaService().addMedia(name: desired, bytes: Array(bytes))
            let operation = try await changes.next(after: baseline) {
                if case .mediaArrived(let names) = $0.kind { return names == [stored] }; return false
            }
            guard operation.generation == owner.generation else { throw AgentFailure.execution }
            mediaName = stored; mediaBytes = bytes
            try await CoreCollectionService().flushForBackground()
            try await requirePhysicalMedia(owner)
            let tracked = try await inspection.cloud(mediaName: stored)
            try retain(tracked, labels: ["m1"])
            guard let counter = retainedEntities["m1"]?.mediaChangeCounter, counter > 0,
                  retainedEntities["m1"]?.mediaBytes == UInt64(bytes.count),
                  retainedEntities["m1"]?.mediaSHA256.map(CloudKitSmokeGeneratedInput.hex) == AgentJSON.hash(bytes) else { throw AgentFailure.execution }
            let facts = [AgentFact(name: .localCommitted, count: counter, operation: "m1"), try entityFact("m1"), AgentFact(name: .localDurable, count: UInt64(bytes.count), digest: AgentJSON.hash(bytes), operation: "m1")]
            for fact in facts { try observe(fact) }
            return [.init(name: .localDurability, outcome: .pass, facts: facts)]
        case .syncNow:
            if program != .smoke { return try await selectedExchange(observe: observe) }
            guard let owner, let backend, let mutation, let head, let reviewRowID, let mediaName,
                  owner.review == nil, retainedEntities.count == 4 else { throw AgentFailure.execution }
            let exchange = try await originalExchange(owner: owner, backend: backend)
            guard !owner.sync.operationFailed,
                  case .succeeded(_, let held, let newer, let incoming, let media) = exchange.outcome,
                  held == 0, newer == 0, incoming == 0 else { throw AgentFailure.execution }
            // Record actual media state even if the required upload proof fails.
            for (label, count) in [("media-pending", media.uploadFiles), ("media-missing", media.missingNames), ("media-unknown", media.unknownBlobs)] {
                guard let count else { throw AgentFailure.implementationMissing }
                try observe(.init(name: .syncResult, count: count, operation: label))
            }
            let current = try await inspection.cloud(noteID: mutation.id, cardID: head.cardId,
                                                     revlogID: reviewRowID, mediaName: mediaName)
            let observations = try await backend.workflow.observations()
            guard current.generation == owner.generation, current.entities.count == 4 else { throw AgentFailure.execution }
            var facts = try CloudKitSmokeProof.acknowledgements(current: current.entities,
                retained: retainedEntities, observations: observations)
            facts += try evidence.requireAcknowledgements(current.entities, observations: observations)
            for fact in facts { try observe(fact) }
            try await requirePhysicalMedia(owner)
            let result = AgentFact(name: .syncResult, count: exchange.exchange, operation: "exchange1")
            try observe(result); facts.append(result)
            return [.init(name: .coordinatorResult, outcome: .pass, facts: facts)]
        case .close:
            guard let owner, owner.review == nil, await owner.sync.cancelAndDrain() else { throw AgentFailure.execution }
            if program != .smoke {
                guard let program, program == .syncNowMac || program == .syncNowPhone,
                      let backend, !owner.sync.operationFailed, selectedExchangeRecord != nil,
                      selectedCloseProof?.program == program,
                      selectedCloseProof?.generation == owner.generation else { throw AgentFailure.execution }
                try backend.checkFailure()
                try changes.checkFailure()
                try evidence.check()
            }
            let integrity = try await integrityCheck(owner)
            guard await owner.close() else { throw AgentFailure.execution }
            self.owner = nil
            changes.finish()
            let proof: [AgentFact]
            if program == .smoke { proof = try evidence.joined() }
            else { proof = try evidence.joinedOriginalWork() }
            for fact in proof { try observe(fact) }
            let joined = AgentFact(name: .ownedWorkJoined)
            try observe(joined)
            return [integrity, .init(name: .coordinatorResult, outcome: .pass, facts: proof),
                    .init(name: .localDurability, outcome: .pass, facts: [joined])]
        case .edit:
            guard program == .syncNowMac, let owner, let originalRole,
                  owner.review == nil, selectedEdit == nil, restoredEdit == nil else { throw AgentFailure.execution }
            _ = try retainedAdmission()
            let edited = try await originalRole.fixedEdit(generation: owner.generation)
            _ = try retainedAdmission()
            guard edited.source == .userUndoable, edited.updatedNotes == 1,
                  let revision = edited.localRevision, revision > 0 else { throw AgentFailure.execution }
            selectedEdit = edited
            let digest = CloudKitSmokeGeneratedInput.hex(edited.noteDigest)
            guard let witness = edited.originalWitness, witness.count == 32 else { throw AgentFailure.recovery }
            let facts = [AgentFact(name: .localCommitted, count: edited.seq, digest: digest, operation: "n1"),
                         AgentFact(name: .localDurable, count: revision, digest: digest, operation: "n1"),
                         AgentFact(name: .localDurable, count: edited.generation,
                            digest: CloudKitSmokeGeneratedInput.hex(witness), operation: "original-edit-witness")]
            for fact in facts { try observe(fact) }
            return [.init(name: .localDurability, outcome: .pass, facts: facts)]
        case .observe:
            guard program == .syncNowPhone, let owner, let originalRole, let backend,
                  selectedExchangeRecord != nil, owner.review == nil else { throw AgentFailure.execution }
            await owner.sync.waitForInputs()
            await owner.sync.drainOperations()
            await owner.sync.waitForInputs()
            guard !owner.sync.operationFailed else { throw AgentFailure.execution }
            try Task.checkCancellation()
            _ = try retainedAdmission()
            let current = try await originalRole.observe(generation: owner.generation)
            _ = try retainedAdmission()
            try Task.checkCancellation()
            var facts = try CloudKitSelectedSyncProof.peerFacts(current)
            facts += try evidence.requireSelectedFetch(current.entity)
            try backend.checkFailure()
            for fact in facts { try observe(fact) }
            selectedCloseProof = (.syncNowPhone, owner.generation)
            return [.init(name: .localDurability, outcome: .pass, facts: facts)]
        }
    }
    private func openRuntime(restoration: AgentSelectedRestoration?, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        guard owner == nil, let configuration = CloudBuildConfiguration.fromBundle, configuration.schemaConfigured else { throw AgentFailure.execution }
        if restoration == nil { try requireVerifiedGeneratedBase() }
        if program != .smoke {
            originalRole = try await CloudKitSelectedOriginalRole.prepare(base:
                AgentGeneratedBase(admittedRunDirectory: directory, originalHeaderBytes: originalHeaderBytes))
            _ = try retainedAdmission()
        }
        // The host must install an authenticated generated small base in
        // this run's data directory. Never substitute a user's collection.
        let data = directory.appendingPathComponent("data", isDirectory: true)
        guard FileManager.default.fileExists(atPath: data.appendingPathComponent("collection.anki2").path) else { throw AgentFailure.execution }
        let reopenAccount: CloudResolvedAccount?
        if restoration != nil {
            reopenAccount = try await resolveAccount(container: CKContainer(identifier: configuration.container))
        } else { reopenAccount = nil }
        let sync = SyncCoordinator(bootstrap: {
            try await CoreCloudWorkflowService.configurePlatform(container: configuration.container,
                environment: configuration.environment, status: reopenAccount?.status ?? .couldNotDetermine, identity: reopenAccount?.identity)
        }, makeBackend: { [self] persisted in
            let admission = try retainedAdmission()
            let container = CKContainer(identifier: configuration.container)
            let database = container.privateCloudDatabase
            // Both the app account reader and adapter use this factory.
            // Its fresh result matches the frozen input before Core accepts
            // identity; J2.2 preserves automatic SDK scheduling.
            let nativeFactory = NativeCloudKitEngineFactory(database: database, accountResolver: { [self] in
                try await resolveAccount(container: container)
            })
            let stagingRoot = data.appendingPathComponent("cloud-native-staging", isDirectory: true)
            try admission.revalidate()
            try FileManager.default.createDirectory(at: stagingRoot, withIntermediateDirectories: true,
                                                    attributes: [.posixPermissions: 0o700])
            let staging = try CloudMediaStaging(directory: stagingRoot)
            let ownTransport = try NativeCloudOwnTransport(database: database, staging: staging)
            let mediaTransport = try NativeCloudMediaTransport(database: database, staging: staging)
            try admission.revalidate()
            let workflow = CoreCloudWorkflowService()
            if restoration != nil {
                _ = try await validateRestoredProgress(requireScan: false)
                try evidence.setGeneration((try await inspection.integrity()).generation, selectedPublication: true)
                try await workflow.prepareRuntimeBeforePolicy()
                _ = try await validateRestoredProgress(requireScan: true)
            }
            let factory: any CloudKitEngineFactory
            if program == .smoke { factory = nativeFactory }
            else {
                factory = CloudKitSelectedSDKFactory(original: nativeFactory, beforeMake: { [self] configuration in
                    _ = try retainedAdmission()
                    if restoration != nil {
                        let control = try await validateRestoredProgress(requireScan: true)
                        try evidence.admitRestoredSDK(origin: configuration.instanceID, control: control)
                        return
                    }
                    let control = try await SelectedCloudControlInspection().current()
                    let observations = try await workflow.observations()
                    let kinds = Set(observations.compactMap { observation -> UInt8? in
                        if case .reconcileKindCompleted(let kind, _) = observation { return kind }; return nil
                    })
                    guard kinds == Set(UInt8(1)...UInt8(9)), control.requiredScanComplete else { throw AgentFailure.execution }
                    if program == .syncNowPhone {
                        guard let originalRole else { throw AgentFailure.implementationMissing }
                        let role = try await originalRole.validateOwned(generation: control.generation)
                        if restoration == nil { guard !role.qChanged else { throw AgentFailure.execution } }
                    }
                    _ = try retainedAdmission()
                    let replacement = changes.events.last { event in
                        if case .collectionReplaced = event.kind { return event.generation == control.generation }; return false
                    }
                    try evidence.admitSDK(origin: configuration.instanceID, control: control, replacement: replacement)
                }, failed: { [evidence] in evidence.fail() })
            }
            let production = try await NativeCollectionCloudBackend.make(workflow: workflow,
                factory: factory, ownTransport: ownTransport, mediaTransport: mediaTransport, persisted: persisted,
                observeEvent: { [evidence, callbackJournal] event in
                    await callbackJournal.record(event)
                    do {
                        let current = try await CoreSmokeInspection().cloud()
                        evidence.validate(origin: event.origin, current: current)
                    } catch { evidence.fail() }
                }, observeBatch: { [evidence, callbackJournal] origin, zone, batch in
                    do {
                        let inspection = CoreSmokeInspection()
                        let proof = zone == "meguri-media" ? nil : try await inspection.batch(sdk: origin, id: batch.id)
                        let current = try await inspection.cloud()
                        await callbackJournal.recordBatch(origin: origin, batch: batch, proof: proof, current: current)
                    } catch { evidence.fail() }
                }, observeOwn: { [evidence] in evidence.own($0) })
            try admission.revalidate()
            let wrapper = CloudKitSmokeBackend(production: production, workflow: workflow)
            backend = wrapper
            return wrapper
        }, defaults: nil, clock: .system, directory: data, observeSignals: false)
        var launch = LaunchConfiguration()
        launch.collectionLocation = AppModel.CollectionLocation(directory: data)
        let collection = ReviewCollection(configuration: launch, navigation: AppNavigation(), sync: sync)
        owner = collection
        _ = try retainedAdmission()
        try await ChangeFanout.shared.subscribe(changes)
        await collection.start()
        _ = try retainedAdmission()
        await sync.prepareConnectionAuthority()
        guard collection.state == .ready, !sync.operationFailed, let backend else { throw AgentFailure.execution }
        let local = try await inspection.cloud()
        if restoration == nil {
            guard local.publicationID == nil, !local.bindingUsable, local.engine == nil else { throw AgentFailure.execution }
            try evidence.setGeneration(collection.generation, selectedPublication: program != .smoke)
        } else {
            _ = try await validateRestoredProgress(requireScan: true)
            guard local.publicationID == nil, local.bindingUsable else { throw AgentFailure.execution }
        }
        try await resolveGeneratedRoles(collection)
        if program != .smoke {
            guard let originalRole else { throw AgentFailure.implementationMissing }
            let role = try await originalRole.validateOwned(generation: collection.generation)
            _ = try retainedAdmission()
            if restoration == nil { guard !role.qChanged else { throw AgentFailure.execution } }
        }
        if restoration == nil { try observe(.init(name: .coreOpened, count: collection.generation)) }
        try backend.checkFailure()
        return [try await integrityCheck(collection)]
    }
    private func originalExchange(owner: ReviewCollection, backend: CloudKitSmokeBackend, observe: ((AgentFact) throws -> Void)? = nil) async throws -> CloudPolicyExchangeRecord {
        guard owner.review == nil else { throw AgentFailure.execution }
        defer { backend.observeOriginalIssuance = nil }
        backend.observeOriginalIssuance = observe
        return try await AgentManualExchange.run(prepare: { try backend.prepareForSyncNow() },
            request: { owner.requestSyncNow() }, afterRequest: {
                await owner.sync.waitForInputs()
                guard owner.sync.syncNowUnavailable == nil, !owner.sync.operationFailed else { throw AgentFailure.execution }
            }, exactResult: { try await backend.requestedExchangeResult() }, afterResult: {
                await owner.sync.waitForInputs()
                guard !owner.sync.operationFailed else { throw AgentFailure.execution }
            })
    }
    private func selectedExchange(observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        guard let owner, let backend, let originalRole, owner.review == nil,
              !selectedExchangeAttempted else { throw AgentFailure.execution }
        if program == .syncNowMac {
            let current = try await originalRole.observe(generation: owner.generation)
            if let restoredEdit { try CloudKitSelectedSyncProof.requireRestoredSource(current, edited: restoredEdit) }
            else if let selectedEdit { try CloudKitSelectedSyncProof.requireSource(current, edited: selectedEdit) }
            else { throw AgentFailure.execution }
        } else if program != .syncNowPhone { throw AgentFailure.execution }
        _ = try retainedAdmission()
        try Task.checkCancellation()
        let boundary = try evidence.acknowledgementBoundary()
        selectedExchangeAttempted = true
        let exchange = try await originalExchange(owner: owner, backend: backend, observe: observe)
        selectedExchangeRecord = exchange
        var facts = try CloudKitSelectedSyncProof.exchangeFacts(exchange)
        if program == .syncNowMac {
            guard selectedEdit != nil || restoredEdit != nil else { throw AgentFailure.execution }
            await owner.sync.drainOperations()
            await owner.sync.waitForInputs()
            guard !owner.sync.operationFailed else { throw AgentFailure.execution }
            let current = try await originalRole.observe(generation: owner.generation)
            _ = try retainedAdmission()
            if let restoredEdit {
                try CloudKitSelectedSyncProof.requireRestoredSource(current, edited: restoredEdit)
                guard current.entity.acknowledged, current.entity.ackedRevision == restoredEdit.localRevision,
                      current.entity.ackedEnvelopeDigest.count == 32 else { throw AgentFailure.execution }
            } else if let selectedEdit { try CloudKitSelectedSyncProof.sourceAcknowledged(current, edited: selectedEdit) }
            else { throw AgentFailure.execution }
            facts += try evidence.requireSelectedAcknowledgement(current.entity,
                observations: try await backend.workflow.observations(), attemptBoundary: boundary)
            facts.append(.init(name: .localDurable, count: current.entity.revision,
                digest: CloudKitSmokeGeneratedInput.hex(current.entity.digest), operation: "n1"))
        }
        for fact in facts { try observe(fact) }
        if program == .syncNowMac {
            try backend.checkFailure()
            selectedCloseProof = (.syncNowMac, owner.generation)
        }
        return [.init(name: .coordinatorResult, outcome: .pass, facts: facts)]
    }
    private func requireInitialPreview(_ summary: CloudConnectionSummary) throws {
        guard summary.localCollectionPresent, !summary.initialAdoptionRequired, !summary.localEmpty,
              summary.decision == .confirmUpload, summary.manifestCounts == nil,
              summary.publishedAtMilliseconds == nil, summary.resumeBindingId == nil,
              summary.localCounts[.note] == 20, summary.localCounts[.card] == 20,
              summary.localCounts[.revlog] == 60, summary.localCounts[.deck] == 2,
              summary.localCounts[.notetype] == 1, summary.localCounts[.mediaName] == 5 else { throw AgentFailure.execution }
    }
    private func resolveGeneratedRoles(_ owner: ReviewCollection) async throws {
        let names = try await CoreDeckService().allDeckNames()
        guard names.generation == owner.generation, names.decks.count == 2,
              let parent = names.decks.first(where: { $0.id == 1 && !$0.filtered }),
              parent.fullName.hasSuffix("-D1") else { throw AgentFailure.execution }
        let marker = String(parent.fullName.dropLast(3))
        guard !marker.isEmpty, let child = names.decks.first(where: {
            !$0.filtered && $0.fullName == marker + "-D1::" + marker + "-D2"
        }) else { throw AgentFailure.execution }
        self.marker = marker; deckID = child.id
    }
    private func integrityCheck(_ owner: ReviewCollection) async throws -> AgentCheck {
        let result = try await inspection.integrity()
        guard result.generation == owner.generation, result.integrityOK, result.schemaOK,
              result.referenceFailures == 0 else { throw AgentFailure.execution }
        return .init(name: .integrity, outcome: .pass, facts: [.init(name: .integrity, count: result.seq)])
    }
    private func retain(_ rows: SmokeCloudObservation, labels: Set<String>) throws {
        for label in labels {
            guard let entity = rows.entities.first(where: { $0.label == label }),
                  entity.revision > 0, entity.digest.count == 32 else { throw AgentFailure.execution }
            retainedEntities[label] = entity
        }
    }
    private func entityFact(_ label: String) throws -> AgentFact {
        guard let entity = retainedEntities[label] else { throw AgentFailure.execution }
        return .init(name: .localDurable, count: entity.revision,
                     digest: CloudKitSmokeGeneratedInput.hex(entity.digest), operation: label)
    }
    private func requirePhysicalMedia(_ owner: ReviewCollection) async throws {
        guard let mediaName, let mediaBytes else { throw AgentFailure.execution }
        let snapshot = try await CoreMediaService().trackedNames()
        guard snapshot.generation == owner.generation,
              snapshot.root.standardizedFileURL == directory.appendingPathComponent("data/collection.media").standardizedFileURL,
              snapshot.names.filter({ $0.name == mediaName }).count == 1 else { throw AgentFailure.execution }
        let path = snapshot.root.appendingPathComponent(mediaName)
        let values = try path.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true,
              try Data(contentsOf: path) == mediaBytes else { throw AgentFailure.execution }
    }
    private func originalControlFact(_ control: SelectedCloudControlObservation) async throws -> AgentFact {
        let original = try await SelectedCloudRestorationInspection().originalControlWitness()
        guard original.generation == control.generation, original.activeSHA256 == control.activeSHA256,
              original.epochSHA256 == control.epochSHA256, original.correlation.count == 32 else { throw AgentFailure.recovery }
        return .init(name: .applied, count: original.generation,
                     digest: CloudKitSmokeGeneratedInput.hex(original.correlation), operation: "original-control-witness")
    }
    private func validateRestoredProgress(requireScan: Bool) async throws -> RestoredCloudControlObservation {
        _ = try retainedAdmission()
        guard let restoration, let originalRole else { throw AgentFailure.recovery }
        let control = try await SelectedCloudRestorationInspection().control(original: .init(
            generation: restoration.originalGeneration, activeSHA256: try CloudKitSelectedRestoration.bytes(restoration.activeDigest),
            epochSHA256: try CloudKitSelectedRestoration.bytes(restoration.epochDigest),
            correlation: try CloudKitSelectedRestoration.bytes(restoration.controlWitness)))
        guard !requireScan || control.requiredScanComplete else { throw AgentFailure.recovery }
        let role = try await originalRole.validateOwned(generation: control.generation)
        if let edit = restoration.edit {
            let actual = try await originalRole.readOriginalEdit(witness: CloudKitSelectedRestoration.bytes(edit.witness), generation: control.generation)
            guard actual.originalGeneration == edit.generation, actual.originalSeq == edit.seq,
                  actual.localRevision == edit.revision,
                  CloudKitSmokeGeneratedInput.hex(actual.noteDigest) == edit.digest else { throw AgentFailure.recovery }
            restoredEdit = actual
        } else if restoration.exchange == nil {
            guard !role.qChanged else { throw AgentFailure.recovery }
        }
        if let exchange = restoration.exchange {
            let record = try await SelectedCloudRestorationInspection().readOriginalExchange(exchange: exchange.exchange,
                witness: CloudKitSelectedRestoration.bytes(exchange.witness))
            // Exact unsuccessful results are observable but cannot recover success.
            _ = try CloudKitSelectedSyncProof.exchangeFacts(record)
            selectedExchangeAttempted = true; selectedExchangeRecord = record
        }
        _ = try retainedAdmission()
        return control
    }
    func recover(_ command: AgentCommand, started: AgentRecord, observe: @escaping (AgentFact) throws -> Void) async throws -> [AgentCheck] {
        _ = try retainedAdmission()
        guard let restoration, let pending = restoration.pending, let owner,
              pending.record_seq == started.record_seq, pending.agent_instance == started.agent_instance,
              pending.command_sha256 == started.command_sha256, command.command_seq == nextCommand,
              command.action == restoration.state.commands[nextCommand - 1].action,
              command.recovery_check == restoration.state.commands[nextCommand - 1].recovery_check else { throw AgentFailure.recovery }
        _ = try await validateRestoredProgress(requireScan: true)
        let facts: [AgentFact]
        let result: AgentCheck
        if command.action == .edit, command.recovery_check == "sync_now.edit", let edit = restoredEdit {
            facts = [.init(name: .recoveryCommitted, count: edit.localRevision,
                           digest: CloudKitSmokeGeneratedInput.hex(edit.noteDigest), operation: "n1")]
            result = .init(name: .localDurability, outcome: .pass, facts: facts)
        } else if command.action == .syncNow, command.recovery_check == "sync_now.exchange", let record = selectedExchangeRecord {
            guard let program else { throw AgentFailure.recovery }
            try AgentSelectedRestoration.requireExchangeRecovery(program)
            facts = try CloudKitSelectedSyncProof.exchangeFacts(record)
            result = .init(name: .coordinatorResult, outcome: .pass, facts: facts)
        } else { throw AgentFailure.recovery }
        for fact in facts { try observe(fact) }
        let integrity = try await integrityCheck(owner)
        // Recovery never imports the old SDK ledger or creates selectedCloseProof.
        nextCommand += 1
        return [result, integrity, .init(name: .recovery, outcome: .pass, facts: facts)]
    }
    func restore(_ state: AgentRestoration) async throws {
        _ = try retainedAdmission()
        guard try state.header.program == program, owner == nil else { throw AgentFailure.binding }
        let restored = try AgentSelectedRestoration(state)
        restoration = restored
        do {
            _ = try await openRuntime(restoration: restored, observe: { _ in throw AgentFailure.recovery })
            _ = try await validateRestoredProgress(requireScan: true)
            try evidence.check()
            nextCommand = restored.nextCommand
        } catch {
            try await cancelAndJoin()
            throw error
        }
    }
    func cancelAndJoin() async throws {
        guard let owner else { return }
        guard await owner.sync.cancelAndDrain() else { throw AgentFailure.execution }
        guard await owner.close() else { throw AgentFailure.execution }
        self.owner = nil
        changes.finish()
    }
}

// Observe existing production command completion without replacing policy,
// native transport, tracking, admission or cancellation.
@MainActor private final class CloudKitSmokeBackend: SyncCoordinatorBackend {
    let production: NativeCollectionCloudBackend
    let workflow: CoreCloudWorkflowService
    var watchPublication = false
    private(set) var publicationOperations: [SmokeCloudObservation] = []
    var connection: CoreCloudConnectionService? { production.connection }
    var own: CloudOwnExecutor? { production.own }
    private let exchanges = AgentExchangeEvents<CloudPolicyExchangeRecord>()
    private let publications = AgentExchangeEvents<SmokeCloudObservation>()
    private var requestedExchange: UInt64?
    var observeOriginalIssuance: ((AgentFact) throws -> Void)?
    init(production: NativeCollectionCloudBackend, workflow: CoreCloudWorkflowService) {
        self.production = production; self.workflow = workflow
    }
    private func observePublication() async throws {
        guard watchPublication else { return }
        let value = try await CoreSmokeInspection().cloud()
        if value.publicationID != nil {
            guard publicationOperations.count < 256 else { throw AgentFailure.execution }
            publicationOperations.append(value)
        }
        publications.yield(value)
    }
    func publicationCompleted() async throws -> SmokeCloudObservation {
        try checkFailure()
        return try await publications.next(where: { $0.publicationComplete && $0.bindingUsable && $0.engine != nil })
    }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        do { try await observePublication() }
        catch { publications.fail(.execution); exchanges.fail(.execution); throw error }
        let output = try await production.input(event)
        do { try await observePublication() }
        catch { publications.fail(.execution); exchanges.fail(.execution); throw error }
        if case .syncNowRequested(let source) = event {
            requestedExchange = nil
            // Observe only the original command issued for this input. Activity
            // records merged into the output cannot identify the requested work.
            if case .commands(let commands) = output {
                let issued = commands.filter { command in
                    guard case .runExchange(_, let issuedSource) = command.action else { return false }
                    return issuedSource == source
                }
                if issued.count == 1, case .runExchange(let exchange, _) = issued[0].action {
                    if let observeOriginalIssuance {
                        let witness = try await SelectedCloudRestorationInspection().originalExchangeWitness(command: issued[0])
                        guard witness.count == 32 else { throw AgentFailure.recovery }
                        try observeOriginalIssuance(.init(name: .syncResult, count: exchange,
                            digest: CloudKitSmokeGeneratedInput.hex(witness), operation: "original-exchange-issued"))
                    }
                    requestedExchange = exchange
                }
            }
        }
        return output
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult {
        do { try await observePublication() } catch { exchanges.fail(.execution); publications.fail(.execution) }
        let result = await production.execute(command)
        do { try await observePublication() } catch { exchanges.fail(.execution); publications.fail(.execution) }
        if result.error != nil { exchanges.fail(.execution); publications.fail(.execution) }
        if result.error == nil, !result.deferred, case .recordExchange(let record) = command.action { exchanges.yield(record) }
        return result
    }
    func checkFailure() throws { try exchanges.checkFailure(); try publications.checkFailure() }
    func prepareForSyncNow() throws {
        try checkFailure()
        requestedExchange = nil
    }
    func requestedExchangeResult() async throws -> CloudPolicyExchangeRecord {
        try checkFailure()
        guard let requestedExchange else { throw AgentFailure.execution }
        return try await exchanges.next(exchange: requestedExchange, identifiedBy: { $0.exchange })
    }
    func beginAccountRead(transition: Bool) async throws -> UInt64 {
        try await production.beginAccountRead(transition: transition)
    }
    func readAccount(_ id: UInt64) async -> SyncCommandResult {
        await production.readAccount(id)
    }
    func prepareAccountRead(_ id: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult {
        await production.prepareAccountRead(id)
    }
    func takeAccountReadRequest() async -> Bool { await production.takeAccountReadRequest() }
    func freezeForSuspend() async throws { try await production.freezeForSuspend() }
    func resumeAfterCoreResume() async {
        await production.resumeAfterCoreResume()
    }
    func quiesce() async throws { try await production.quiesce() }
    func close() async throws { try await production.close(); exchanges.finish(); publications.finish() }
}
#endif
