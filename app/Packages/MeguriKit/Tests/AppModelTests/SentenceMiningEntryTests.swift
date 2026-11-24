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
        defer { defaults.removePersistentDomain(forName: suite) }
        configuration.deviceDefaults = defaults
        configuration.migakuPort = 0
        configuration.ankiConnectPort = 0
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
            #expect(!mining.enabled && mining.migaku.status == "Off · Port 0" && mining.migaku.port == 0)
            // Invoke the same production action as SentenceMiningPane's Toggle;
            // neither a substitute listener nor a separately configured receiver.
            mining.setEnabled(true, for: .migaku)
            await mining.settle()
            let initialPort = try await miningListeningPort(mining.migaku)
            let unavailable = try await miningExchange(port: initialPort, method: "GET", path: "/srs-check")
            #expect(unavailable.json?["ok"] as? Bool == false)
            #expect(defaults.bool(forKey: "mining.migaku.enabled"))
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
            let bound = try await miningListeningPort(mining.migaku)
            #expect(bound == initialPort)
            mining.setEnabled(true, for: .ankiConnect)
            await mining.settle()
            let siblingPort = try await miningListeningPort(mining.ankiConnect)
            #expect(siblingPort != bound)
            let restored = SentenceMining(defaults: defaults, collectionKey: directory.path, ports: (0, 0))
            #expect(restored.migaku.enabled && restored.ankiConnect.enabled && restored.selectedDeck == deck.id && restored.selectedType == imported.id)
            let check = try await miningExchange(port: bound, method: "GET", path: "/srs-check")
            #expect(check.status == 200 && check.json?["ok"] as? Bool == true && check.json?["version"] as? String == "4")
            let preflight = try await miningExchange(port: bound, method: "OPTIONS", path: "/receive")
            #expect(preflight.status == 204)
            var wsRequest = URLRequest(url: URL(string: "ws://127.0.0.1:\(bound)/anki-connect")!)
            wsRequest.timeoutInterval = 5
            wsRequest.setValue("chrome-extension://abcdefghijklmnopabcdefghijklmnop", forHTTPHeaderField: "Origin")
            let socket = webSession.webSocketTask(with: wsRequest)
            webSocket = socket; socket.resume()
            try await withSocketDeadline(socket, stage: "entry WebSocket hello") {
                try await socket.send(.string(#"{"id":1,"msg":"hello","data":{}}"#))
            }
            try await miningSendPing(socket, stage: "entry WebSocket ping")
            let sentence = "<span title='Study > draft' class='term emphasis' lang='en' dir='ltr'><strong>A synthetic sentence.</strong> H<sub>2</sub>O</span>"
            // Complete self-contained media: eight silent PCM samples and a gray pixel.
            let audio = "data:audio/wav;base64,UklGRiwAAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQgAAACAgICAgICAgA=="
            let raw = #"{"sentence":"\#(sentence)","translation":"Une phrase synthétique.","sentenceAudio":[{"id":"mining-audio","input":"\#(audio)"}],"images":[{"id":"mining-image","src":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNoaGgAAAMEAYFL09IQAAAAAElFTkSuQmCC"}]}"#
            let created = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.utf8))
            #expect(created.status == 200)
            let id = try #require((created.json?["id"] as? NSNumber)?.int64Value)
            let note = try await CoreNoteService().readNote(id: id)
            #expect(note.notetypeId == imported.id && note.fields[1] == "Une phrase synthétique.")
            #expect(Array(note.fields[0].utf8) == Array(sentence.utf8))
            #expect(note.fields[2].contains("[sound:") && note.fields[2].contains(".wav]") && note.fields[3].contains(".png\">"))
            let cards = try await CoreNoteService().noteCards(noteId: id)
            #expect(cards.count == 1 && cards.first?.deckId == deck.id)
            let names = try await CoreMediaService().trackedNames()
            #expect(names.names.contains { $0.name.hasSuffix(".wav") } && names.names.contains { $0.name.hasSuffix(".png") })
            let retry = try await miningExchange(port: siblingPort, method: "POST", path: "/receive", body: Data(raw.utf8))
            #expect((retry.json?["id"] as? NSNumber)?.int64Value == id)
            let conflict = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.replacingOccurrences(of: "Une phrase synthétique.", with: "Different translation.").utf8))
            #expect(conflict.status == 400 && (conflict.json?["error"] as? String)?.contains("Conflicting retry") == true)
            let untrusted = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.utf8), origin: "https://evil.example")
            #expect(untrusted.status == 403)
            let unsupported = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.replacingOccurrences(of: audio, with: "http://127.0.0.1/never-fetch").utf8))
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
            let rootAdded = try await miningExchange(port: siblingPort, method: "POST", path: "/", body: Data(rootNote.utf8))
            let rootID = try #require((rootAdded.json?["result"] as? NSNumber)?.int64Value)
            #expect(try await CoreNoteService().readNote(id: rootID).fields == ["From a local tool", "Saved through the compatibility subset"])
            let rootRetry = try await miningExchange(port: bound, method: "POST", path: "/", body: Data(rootNote.utf8))
            #expect((rootRetry.json?["result"] as? NSNumber)?.int64Value == rootID)
            let comparison = "The inequality x<y holds."
            let comparisonHTML = "The inequality x&lt;y holds."
            let comparisonBody = try JSONSerialization.data(withJSONObject: ["sentence": comparison])
            let comparisonAdded = try await miningExchange(port: bound, method: "POST", path: "/receive", body: comparisonBody)
            let comparisonID = try #require((comparisonAdded.json?["id"] as? NSNumber)?.int64Value)
            #expect(try await CoreNoteService().readNote(id: comparisonID).fields[0] == comparisonHTML)
            let rootComparison = rootNote.replacingOccurrences(of: "From a local tool", with: comparison)
            let rootComparisonAdded = try await miningExchange(port: bound, method: "POST", path: "/", body: Data(rootComparison.utf8))
            let rootComparisonID = try #require((rootComparisonAdded.json?["result"] as? NSNumber)?.int64Value)
            #expect(try await CoreNoteService().readNote(id: rootComparisonID).fields[0] == comparisonHTML)
            let discovery = try await miningExchange(port: bound, method: "POST", path: "/", body: Data(#"{"action":"deckNames","version":6}"#.utf8))
            #expect((discovery.json?["result"] as? [String])?.contains("Synthetic Mining") == true)
            // Turning off uses the production owner, closes its WebSocket and
            // persists the switch. Re-enabling creates a fresh listener.
            mining.setEnabled(false, for: .migaku)
            #expect(mining.migaku.status == "Off · Port \(bound)" && !defaults.bool(forKey: "mining.migaku.enabled"))
            await mining.settle()
            await #expect(throws: (any Error).self) {
                do {
                    try await withSocketDeadline(socket, stage: "disabled entry WebSocket receive") { _ = try await socket.receive() }
                } catch {
                    #expect(!(error is MiningSocketDeadlineExceeded))
                    throw error
                }
            }
            #expect(webSocket?.closeCode == .goingAway)
            try expectMiningPortClosed(bound)
            #expect(mining.ankiConnect.status.hasPrefix("Listening"))
            let siblingRetry = try await miningExchange(port: siblingPort, method: "POST", path: "/receive", body: Data(basicPayload.utf8))
            #expect((siblingRetry.json?["id"] as? NSNumber)?.int64Value == ordinaryID)
            mining.setEnabled(true, for: .migaku)
            await mining.settle()
            let rebound = try await miningListeningPort(mining.migaku)
            let retriedAfterEnable = try await miningExchange(port: rebound, method: "POST", path: "/receive", body: Data(basicPayload.utf8))
            #expect((retriedAfterEnable.json?["id"] as? NSNumber)?.int64Value == ordinaryID)
            try #require(await owner.closeForQuit())
            try expectMiningPortClosed(rebound)
            try expectMiningPortClosed(siblingPort)

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
                let nextPort = try await miningListeningPort(nextMining.migaku)
                let needsDestination = try await miningExchange(port: nextPort, method: "POST", path: "/receive", body: Data(raw.utf8))
                #expect(needsDestination.status == 400)
                #expect((needsDestination.json?["error"] as? String)?.contains("Choose a destination") == true)
                let nextSiblingPort = try await miningListeningPort(nextMining.ankiConnect)
                #expect(await other.closeForQuit())
                try expectMiningPortClosed(nextPort)
                try expectMiningPortClosed(nextSiblingPort)
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
private func miningListeningPort(_ model: SentenceMining.Endpoint) async throws -> UInt16 {
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
