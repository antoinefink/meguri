#if os(macOS)
@testable import AppModel
import CoreBridge
import Darwin
import Foundation
import Testing

// Shares the normal app-entry suite's process-wide Core ownership. Select
// this method alone in the VM; no production defaults or collection is read.
extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(2))) func sentenceMiningWireFromNormalCollectionEntry() async throws {
        let temporary = try #require(realpath(FileManager.default.temporaryDirectory.path, nil))
        let root = URL(filePath: String(cString: temporary)).appending(path: "mining-\(UUID().uuidString)")
        free(temporary)
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appending(path: "collection"), backups = root.appending(path: "backups")
        var configuration = LaunchConfiguration()
        let suite = "SentenceMiningEntryTests." + UUID().uuidString
        let defaults = try #require(UserDefaults(suiteName: suite))
        defaults.set(false, forKey: "mining.enabled")
        defer { defaults.removePersistentDomain(forName: suite) }
        configuration.deviceDefaults = defaults
        configuration.migakuPort = 0
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: backups)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(directory: directory, configuration: nil, defaults: nil),
            providerDirectory: root.appending(path: "installation"))
        let mining = owner.sentenceMining
        var webSocket: URLSessionWebSocketTask?
        let webConfiguration = URLSessionConfiguration.ephemeral
        webConfiguration.connectionProxyDictionary = [:]
        let webSession = URLSession(configuration: webConfiguration)
        defer { webSession.invalidateAndCancel() }
        do {
            await owner.start()
            try #require(owner.state == .noFile)
            #expect(!mining.enabled && mining.status == "Off" && mining.port == 0)
            // Invoke the same production action as SentenceMiningPane's Toggle;
            // neither a substitute listener nor a separately configured receiver.
            mining.setEnabled(true)
            await mining.settle()
            let initialPort = try await miningListeningPort(mining)
            let unavailable = try await miningExchange(port: initialPort, method: "GET", path: "/srs-check")
            #expect(unavailable.json?["ok"] as? Bool == false)
            #expect(defaults.bool(forKey: "mining.enabled"))
            await owner.startEmpty()
            try #require(owner.state == .ready)
            let deck = try await CoreDeckService().addDeck(name: "Synthetic Mining")
            // Represents an imported custom model; no special Meguri stock
            // model or migration is required for it to receive media.
            let imported = try await CoreNotetypeService().addNotetype(.init(name: "Imported Sentence Type", cloze: false,
                fields: ["Sentence", "Translation", "Sentence Audio", "Screenshot"], templates: [
                    .init(name: "Sentence", front: "{{Sentence}}<br>{{Sentence Audio}}<br>{{Screenshot}}", back: "{{Translation}}", browserFront: "", browserBack: "")], css: nil))
            mining.selectDeck(deck.id)
            mining.selectType(imported.id)
            await mining.settle()
            let bound = try await miningListeningPort(mining)
            #expect(bound == initialPort)
            let restored = SentenceMining(defaults: defaults, collectionKey: directory.path, port: 0)
            #expect(restored.enabled && restored.selectedDeck == deck.id && restored.selectedType == imported.id)
            let check = try await miningExchange(port: bound, method: "GET", path: "/srs-check")
            #expect(check.status == 200 && check.json?["ok"] as? Bool == true && check.json?["version"] as? String == "4")
            let preflight = try await miningExchange(port: bound, method: "OPTIONS", path: "/receive")
            #expect(preflight.status == 204)
            var wsRequest = URLRequest(url: URL(string: "ws://127.0.0.1:\(bound)/anki-connect")!)
            wsRequest.timeoutInterval = 5
            wsRequest.setValue("chrome-extension://abcdefghijklmnopabcdefghijklmnop", forHTTPHeaderField: "Origin")
            let socket = webSession.webSocketTask(with: wsRequest)
            webSocket = socket; socket.resume()
            try await socket.send(.string(#"{"id":1,"msg":"hello","data":{}}"#))
            try await socket.sendPing()
            let raw = #"{"sentence":"A synthetic sentence.","translation":"Une phrase synthétique.","sentenceAudio":[{"id":"mining-audio","input":"data:audio/mp4;base64,AAAAGGZ0eXBNNEEgAAAAAE00QSBpc29t"}],"images":[{"id":"mining-image","src":"data:image/png;base64,iVBORw0KGgo="}]}"#
            let created = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.utf8))
            #expect(created.status == 200)
            let id = try #require((created.json?["id"] as? NSNumber)?.int64Value)
            let note = try await CoreNoteService().readNote(id: id)
            #expect(note.notetypeId == imported.id && note.fields[0] == "A synthetic sentence." && note.fields[1] == "Une phrase synthétique.")
            #expect(note.fields[2].contains("[sound:") && note.fields[2].contains(".m4a]") && note.fields[3].contains(".png\">"))
            let cards = try await CoreNoteService().noteCards(noteId: id)
            #expect(cards.count == 1 && cards.first?.deckId == deck.id)
            let names = try await CoreMediaService().trackedNames()
            #expect(names.names.contains { $0.name.hasSuffix(".m4a") } && names.names.contains { $0.name.hasSuffix(".png") })
            let retry = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.utf8))
            #expect((retry.json?["id"] as? NSNumber)?.int64Value == id)
            let conflict = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.replacingOccurrences(of: "Une phrase synthétique.", with: "Different translation.").utf8))
            #expect(conflict.status == 400 && (conflict.json?["error"] as? String)?.contains("Conflicting retry") == true)
            let untrusted = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.utf8), origin: "https://evil.example")
            #expect(untrusted.status == 403)
            let unsupported = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.replacingOccurrences(of: "data:audio/mp4;base64,AAAAGGZ0eXBNNEEgAAAAAE00QSBpc29t", with: "http://127.0.0.1/never-fetch").utf8))
            #expect(unsupported.status == 400 && (unsupported.json?["error"] as? String)?.contains("remote URLs are not fetched") == true)
            // Compatibility envelope uses selected destination, never a
            // general collection-management or unrestricted write API.
            let version = try await miningExchange(port: bound, method: "POST", path: "/", body: Data(#"{"action":"version","version":6}"#.utf8))
            #expect(version.json?["result"] as? Int == 6)
            let denied = try await miningExchange(port: bound, method: "POST", path: "/", body: Data(#"{"action":"deleteNotes","version":6,"params":{"notes":[1]}}"#.utf8))
            #expect(denied.json?["error"] as? String == "unsupported action")
            let basic = try await CoreNotetypeService().addNotetype(.init(name: "Ordinary Front Back", cloze: false,
                fields: ["Front", "Back"], templates: [.init(name: "Card", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: "")], css: nil))
            mining.selectType(basic.id)
            await mining.settle()
            let basicPayload = raw.replacingOccurrences(of: "A synthetic sentence.", with: "An ordinary front.")
            let ordinary = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(basicPayload.utf8))
            let ordinaryID = try #require((ordinary.json?["id"] as? NSNumber)?.int64Value)
            let ordinaryNote = try await CoreNoteService().readNote(id: ordinaryID)
            #expect(ordinaryNote.fields[0].contains("An ordinary front.") && ordinaryNote.fields[0].contains("[sound:") && ordinaryNote.fields[0].contains("<img src="))
            let rootNote = #"{"action":"addNote","version":6,"params":{"note":{"deckName":"Synthetic Mining","modelName":"Ordinary Front Back","fields":{"Front":"From a local tool","Back":"Saved through the compatibility subset"},"tags":["Synthetic"]}}}"#
            let rootAdded = try await miningExchange(port: bound, method: "POST", path: "/", body: Data(rootNote.utf8))
            let rootID = try #require((rootAdded.json?["result"] as? NSNumber)?.int64Value)
            #expect(try await CoreNoteService().readNote(id: rootID).fields == ["From a local tool", "Saved through the compatibility subset"])
            let discovery = try await miningExchange(port: bound, method: "POST", path: "/", body: Data(#"{"action":"deckNames","version":6}"#.utf8))
            #expect((discovery.json?["result"] as? [String])?.contains("Synthetic Mining") == true)
            // Turning off uses the production owner, closes its WebSocket and
            // persists the switch. Re-enabling creates a fresh listener.
            mining.setEnabled(false)
            #expect(mining.status == "Off" && !defaults.bool(forKey: "mining.enabled"))
            await mining.settle()
            await #expect(throws: (any Error).self) { _ = try await socket.receive() }
            #expect(webSocket?.closeCode == .goingAway)
            await #expect(throws: (any Error).self) {
                _ = try await miningExchange(port: bound, method: "GET", path: "/srs-check")
            }
            mining.setEnabled(true)
            await mining.settle()
            let rebound = try await miningListeningPort(mining)
            let retriedAfterEnable = try await miningExchange(port: rebound, method: "POST", path: "/receive", body: Data(basicPayload.utf8))
            #expect((retriedAfterEnable.json?["id"] as? NSNumber)?.int64Value == ordinaryID)
            #expect(await owner.closeForQuit())

            // A different real synthetic collection shares the device switch,
            // but must never inherit the first collection's destination IDs.
            let otherDirectory = root.appending(path: "other-collection")
            configuration.collectionLocation = AppModel.CollectionLocation(directory: otherDirectory,
                backupsDirectory: root.appending(path: "other-backups"))
            let other = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
                sync: SyncCoordinator(directory: otherDirectory, configuration: nil, defaults: nil),
                providerDirectory: root.appending(path: "other-installation"))
            do {
                await other.start()
                await other.startEmpty()
                try #require(other.state == .ready)
                let nextMining = other.sentenceMining
                await nextMining.settle()
                #expect(nextMining.enabled && nextMining.selectedDeck == 0 && nextMining.selectedType == 0)
                let nextPort = try await miningListeningPort(nextMining)
                let needsDestination = try await miningExchange(port: nextPort, method: "POST", path: "/receive", body: Data(raw.utf8))
                #expect(needsDestination.status == 400)
                #expect((needsDestination.json?["error"] as? String)?.contains("Choose a destination") == true)
                #expect(await other.closeForQuit())
            } catch {
                _ = await other.closeForQuit()
                throw error
            }
        } catch {
            _ = await owner.closeForQuit()
            throw error
        }
    }
}

@MainActor
private func miningListeningPort(_ model: SentenceMining) async throws -> UInt16 {
    let clock = ContinuousClock()
    let deadline = clock.now + .seconds(5)
    while clock.now < deadline {
        if model.port != 0, model.status.hasPrefix("Listening") { return model.port }
        try await Task.sleep(for: .milliseconds(10))
    }
    Issue.record("Mining did not listen: \(model.status)")
    throw MiningFailure("Mining listener did not become ready.")
}

private struct MiningWireReply {
    let status: Int
    let data: Data
    var json: [String: Any]? { (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] }
}
private func miningExchange(port: UInt16, method: String, path: String, body: Data = Data(), origin: String = "chrome-extension://abcdefghijklmnopabcdefghijklmnop") async throws -> MiningWireReply {
    var request = URLRequest(url: URL(string: "http://127.0.0.1:\(port)\(path)")!)
    request.httpMethod = method; request.timeoutInterval = 5
    request.setValue(origin, forHTTPHeaderField: "Origin")
    if !body.isEmpty { request.httpBody = body; request.setValue("application/json", forHTTPHeaderField: "Content-Type") }
    let configuration = URLSessionConfiguration.ephemeral
    configuration.connectionProxyDictionary = [:]
    let session = URLSession(configuration: configuration)
    defer { session.invalidateAndCancel() }
    let (data, response) = try await session.data(for: request)
    return MiningWireReply(status: try #require((response as? HTTPURLResponse)?.statusCode), data: data)
}
#endif
