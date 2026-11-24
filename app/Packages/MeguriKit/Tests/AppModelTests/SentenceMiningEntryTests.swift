#if os(macOS)
@testable import AppModel
import CoreBridge
import Darwin
import Foundation
import Network
import Synchronization
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
        configuration.deviceDefaults = nil
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: backups)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(directory: directory, configuration: nil, defaults: nil),
            providerDirectory: root.appending(path: "installation"))
        let receiver = MiningReceiver()
        let states = Mutex<[MiningListener.State]>([])
        let listener = MiningListener(port: 0, handler: { await receiver.handle($0) }, changed: { state in states.withLock { $0.append(state) } })
        var webSocket: URLSessionWebSocketTask?
        let webConfiguration = URLSessionConfiguration.ephemeral
        webConfiguration.connectionProxyDictionary = [:]
        let webSession = URLSession(configuration: webConfiguration)
        defer { webSession.invalidateAndCancel() }
        do {
            await owner.start(); try #require(owner.state == .noFile)
            await owner.startEmpty(); try #require(owner.state == .ready)
            #expect(!owner.sentenceMining.enabled)
            let deck = try await CoreDeckService().addDeck(name: "Synthetic Mining")
            // Represents an imported custom model; no special Meguri stock
            // model or migration is required for it to receive media.
            let imported = try await CoreNotetypeService().addNotetype(.init(name: "Imported Sentence Type", cloze: false,
                fields: ["Sentence", "Translation", "Sentence Audio", "Screenshot"], templates: [
                    .init(name: "Sentence", front: "{{Sentence}}<br>{{Sentence Audio}}<br>{{Screenshot}}", back: "{{Translation}}", browserFront: "", browserBack: "")], css: nil))
            let generation = try #require(await MiningCoreCollection().available())
            owner.sentenceMining.selectDeck(deck.id); owner.sentenceMining.selectType(imported.id)
            await receiver.configure(enabled: true, destination: .init(generation: generation, deck: deck.id, type: imported.id))
            listener.start()
            let clock = ContinuousClock(), deadline = clock.now + .seconds(5)
            var port: UInt16?
            while clock.now < deadline, port == nil {
                port = states.withLock { values in values.compactMap { if case .listening(let port) = $0 { port } else { nil } }.last }
                if port == nil { try await Task.sleep(for: .milliseconds(10)) }
            }
            let bound = try #require(port)
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
            await receiver.configure(enabled: true, destination: .init(generation: generation, deck: deck.id, type: basic.id))
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
            // Real unavailable state, rather than a UI flag or always-true stub.
            await receiver.configure(enabled: true, destination: nil)
            #expect(await owner.closeForQuit())
            let unavailable = try await miningExchange(port: bound, method: "GET", path: "/srs-check")
            #expect(unavailable.json?["ok"] as? Bool == false)
            let unavailableWrite = try await miningExchange(port: bound, method: "POST", path: "/receive", body: Data(raw.utf8))
            #expect(unavailableWrite.status == 400)
            await receiver.configure(enabled: false, destination: nil); await listener.stop(); await receiver.drain()
            #expect(states.withLock { $0.last } == .off)
            await #expect(throws: (any Error).self) { _ = try await socket.receive() }
            #expect(webSocket?.closeCode == .goingAway)
        } catch {
            await receiver.configure(enabled: false, destination: nil); await listener.stop(); await receiver.drain()
            _ = await owner.closeForQuit(); throw error
        }
    }
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
