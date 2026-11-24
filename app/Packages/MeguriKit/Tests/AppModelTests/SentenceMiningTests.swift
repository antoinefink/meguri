#if os(macOS)
@testable import AppModel
import CoreBridge
import Foundation
import Synchronization
import Testing

@MainActor @Suite struct SentenceMiningTests {
    func head(_ value: String) throws -> MiningHTTPRequest {
        try #require(MiningHTTP.head(Data(value.utf8))?.0)
    }
    @Test func actualHandshakeAndNoDataGreeting() throws {
        let request = try head("GET /anki-connect HTTP/1.1\r\nHost: localhost:44432\r\nOrigin: chrome-extension://abcdefghijklmnopabcdefghijklmnop\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n")
        let response = try #require(MiningHTTP.preliminary(request, port: 44432, sockets: 0))
        #expect(response.status == 101 && response.upgrade && response.body.isEmpty)
        #expect(response.headers.contains { $0.0 == "Sec-WebSocket-Accept" && $0.1 == "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=" })
        var socket = MiningWebSocket()
        // The public mining wire contract has no server data greeting. A
        // masked client JSON greeting is ignored; the socket stays alive.
        let greeting = Data(#"{"id":1,"msg":"hello","data":{}}"#.utf8)
        let mask: [UInt8] = [1, 2, 3, 4]
        let frame = Data([0x81, 0x80 | UInt8(greeting.count)] + mask) + Data(greeting.enumerated().map { $0.element ^ mask[$0.offset % 4] })
        #expect(socket.receive(frame).isEmpty)
        let ping = Data([0x89, 0x81, 1, 2, 3, 4, 1 ^ 65])
        let events = socket.receive(ping)
        #expect(events.count == 1)
        if case .some(.send(let pong)) = events.first { #expect(pong == Data([0x8a, 1, 65])) }
        else { Issue.record("expected pong") }
    }
    @Test func hostAndOriginChecksPrecedeBodyOrUpgradeAndPreflightIsExact() throws {
        for headers in ["Host: evil.example:44432", "Host: localhost:044432", "Host: localhost:44432\r\nHost: localhost:44432", "Origin: null", "Origin: https://evil.example", "Origin: http://127.0.0.1:44432", "Origin: https://migaku.com.evil.example"] {
            let request = try head("POST /receive HTTP/1.1\r\n\(headers)\r\nContent-Length: 999999999\r\n\r\n")
            let response = try #require(MiningHTTP.preliminary(request, port: 44432, sockets: 0))
            #expect(response.status == 403 && response.body.isEmpty && response.headers.isEmpty)
        }
        let preflight = try head("OPTIONS /receive HTTP/1.1\r\nHost: localhost:44432\r\nOrigin: https://app.migaku.com\r\nAccess-Control-Request-Private-Network: true\r\n\r\n")
        let response = try #require(MiningHTTP.preliminary(preflight, port: 44432, sockets: 0))
        #expect(response.status == 204)
        #expect(response.headers.contains { $0.0 == "Access-Control-Allow-Private-Network" && $0.1 == "true" })
        let root = try head("POST / HTTP/1.1\r\nHost: localhost:44432\r\nOrigin: https://app.migaku.com\r\n\r\n")
        #expect(MiningHTTP.preliminary(root, port: 44432, sockets: 0)?.status == 403)
    }
    @Test func malformedFramingAndChunkedLimits() throws {
        for extra in ["Content-Length: 1\r\nTransfer-Encoding: chunked", "Content-Length: -1", "Content-Length: 1\r\nContent-Length: 2", "X Bad: value"] {
            #expect(throws: (any Error).self) { try MiningHTTP.head(Data("POST /receive HTTP/1.1\r\n\(extra)\r\n\r\n".utf8)) }
        }
        let request = try head("POST /receive HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
        #expect(try MiningHTTP.body(Data("2\r\n{}\r\n0\r\n\r\n".utf8), request: request) == Data("{}".utf8))
        #expect(throws: (any Error).self) { try MiningHTTP.body(Data("8000001\r\n".utf8), request: request) }
        #expect(throws: (any Error).self) { try MiningHTTP.body(Data("z\r\n".utf8), request: request) }
    }
    @Test func importedAndOrdinaryTypesMapMediaWithoutFetching() throws {
        #expect(MiningFieldMap.infer(["Sentence", "Translation", "Target Word", "Definitions", "Screenshot", "Sentence Audio", "Word Audio", "Images", "Example Sentences", "Is Vocabulary Card", "Is Audio Card"]) == ["sentence", "translation", "targetWord", "definitions", "firstImage", "sentenceAudio", "wordAudio", "restImages", "exampleSentences", nil, nil])
        #expect(MiningFieldMap.infer(["Front", "Back"]) == ["sentence", "translation"])
        #expect(MiningFieldMap.defaults(typeName: "Migaku Japanese Custom", fields: ["Is Vocabulary Card", "Is Audio Card"]) == ["", "x"])
        #expect(MiningFieldMap.defaults(typeName: "Migaku German Custom", fields: ["Is Vocabulary Card", "Is Audio Card"]) == ["x", "x"])
        #expect(MiningFieldMap.defaults(typeName: "Unspecified", fields: ["Is Vocabulary Card", "Is Audio Card"]) == ["", ""])
        // Complete synthetic media: eight silent PCM samples and one gray pixel.
        let raw = #"{"sentence":"A synthetic sentence.","translation":"A translation.","sentenceAudio":[{"id":"a","input":"data:audio/wav;base64,UklGRiwAAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQgAAACAgICAgICAgA=="}],"images":[{"id":"i","src":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNoaGgAAAMEAYFL09IQAAAAAElFTkSuQmCC"}]}"#
        let payload = try MiningPayload(Data(raw.utf8))
        let type = Self.type(["Front", "Back"])
        let (fields, media) = try payload.fields(for: type)
        #expect(media.count == 2 && media[0].name.hasSuffix(".wav") && media[1].name.hasSuffix(".png"))
        #expect(fields[0].contains("[sound:__meguri_mining_attachment_0__]") && fields[0].contains("<img src=\"__meguri_mining_attachment_1__\">"))
        #expect(fields[1] == "A translation.")
        for source in ["http://127.0.0.1/private", "https://example.org/audio.mp3", "file:///secret", "data:audio/mp4;base64,!"] {
            #expect(throws: (any Error).self) { try MiningPayload.media(source, key: "sentenceAudio", index: 1) }
        }
    }
    @Test func proseAndSafeStudyFormattingArePreserved() throws {
        for sentence in [
            "CSS vocabulary: url(), @import, src= and background=.",
            "A comparison: 2 < 3 and 5 > 4.",
            #"<span title="study">word</span>"#,
            #"<SPAN title = "Study > draft < final; url(), @import, src=" CLASS='term emphasis' lang=ja dir="auto">語</SPAN>"#,
            "<p lang='en'><strong class='focus'>Useful</strong> <em title='a phrase'>formatting</em><br />H<sub>2</sub>O</p>",
            "<span\nclass=term\tlang='fr' dir=auto title=''>café &amp; thé</span>",
            "<abbr title='for example'>e.g.</abbr> <kbd>Return</kbd> <bdi lang=ar>كلمة</bdi>",
            "<p>A <strong>useful</strong> <em>sentence</em>.<br>H<sub>2</sub>O</p>",
            "<ruby>語<rt>ご</rt></ruby><ul><li><code>url()</code></li></ul>"
        ] {
            let data = try JSONSerialization.data(withJSONObject: ["sentence": sentence])
            let payload = try MiningPayload(data)
            let (fields, media) = try payload.fields(for: Self.type(["Front", "Back"]))
            #expect(fields == [sentence, ""])
            #expect(Array(fields[0].utf8) == Array(sentence.utf8))
            #expect(media.isEmpty)
        }
        // Empty unsupported elements/attributes and incomplete tags exercise
        // refusal without executable content or an external resource.
        for sentence in ["<img>", "<audio></audio>", "<script></script>", "<style></style>",
                         "<span style=''>word</span>", "<span src=''>word</span>",
                         "<span title='study>word</span>", "<span title=study", "</span title='study'>"] {
            let data = try JSONSerialization.data(withJSONObject: ["sentence": sentence])
            #expect(throws: MiningFailure.self) { try MiningPayload(data) }
        }
    }

    @Test func unspacedComparisonsRemainVisibleBesideStudyMarkup() throws {
        for sentence in ["The inequality x<y holds.", "x<y", "<b>x<y</b>", "x<y <span title='Study > draft'>holds</span>."] {
            let expected = sentence.replacingOccurrences(of: "x<y", with: "x&lt;y")
            let payload = try MiningPayload(JSONSerialization.data(withJSONObject: ["sentence": sentence]))
            let (fields, media) = try payload.fields(for: Self.type(["Front", "Back"]))
            #expect(fields == [expected, ""])
            #expect(media.isEmpty)
        }
    }

    @Test func embeddedMediaAliasesAndGenericSignatures() throws {
        let aliases = [
            "audio/mpeg3": "mp3", "audio/x-mpeg": "mp3", "audio/x-mp3": "mp3",
            "audio/vorbis": "ogg", "audio/wave": "wav", "audio/vnd.wave": "wav",
            "audio/x-flac": "flac", "audio/x-aac": "aac", "audio/aacp": "aac", "image/pjpeg": "jpg"
        ]
        let signatures: [(Data, String, String)] = [
            (Data("ID3".utf8), "mp3", "sentenceAudio"), (Data("OggS".utf8), "ogg", "wordAudio"),
            (Data("fLaC".utf8), "flac", "sentenceAudio"), (Data("RIFF0000WAVE".utf8), "wav", "wordAudio"),
            (Data("RIFF0000WEBP".utf8), "webp", "images"), (Data("0000ftypavif".utf8), "avif", "images"),
            (Data("0000ftypheic".utf8), "heic", "images"), (Data("0000ftypM4A ".utf8), "m4a", "sentenceAudio"),
            (Data([0x1a, 0x45, 0xdf, 0xa3]), "webm", "wordAudio"),
            (Data([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), "png", "images"),
            (Data([0xff, 0xd8, 0xff]), "jpg", "images"), (Data("GIF89a".utf8), "gif", "images"),
            (Data("BM".utf8), "bmp", "images"),
            (Data([0xff, 0xf1]), "aac", "sentenceAudio"), (Data([0xff, 0xfb]), "mp3", "wordAudio")
        ]
        for (mime, ext) in aliases {
            let (bytes, _, key) = try #require(signatures.first { $0.1 == ext })
            let source = "data:\(mime);base64," + bytes.base64EncodedString()
            let media = try MiningPayload.media(source, key: key, index: 1)
            #expect(media.name.hasSuffix("." + ext) && media.bytes == bytes)
            let wrongKey = key == "images" ? "sentenceAudio" : "images"
            #expect(throws: MiningFailure.self) { try MiningPayload.media(source, key: wrongKey, index: 1) }
        }
        for (bytes, ext, key) in signatures {
            for mime in ["", "application/octet-stream", "binary/octet-stream"] {
                let source = "DATA:\(mime);BASE64," + bytes.base64EncodedString()
                let media = try MiningPayload.media(source, key: key, index: 1)
                #expect(media.name.hasSuffix("." + ext) && media.bytes == bytes)
                let wrongKey = key == "images" ? "wordAudio" : "images"
                #expect(throws: MiningFailure.self) { try MiningPayload.media(source, key: wrongKey, index: 1) }
            }
        }
        let parameterized = try MiningPayload.media("data:audio/vorbis;codecs=opus;base64,T2dnUw", key: "sentenceAudio", index: 1)
        #expect(parameterized.name.hasSuffix(".ogg"))
    }

    @Test func imageRouteRejectsBenignInvalidFormatsAndMismatchedMIME() {
        for (mime, bytes) in [
            ("image/png", Data("Not an image.".utf8)),
            ("image/png", Data("ID3".utf8)),
            ("image/svg+xml", Data("<svg></svg>".utf8)),
            ("application/octet-stream", Data("<svg></svg>".utf8))
        ] {
            #expect(throws: MiningFailure.self) {
                try MiningPayload.media("data:\(mime);base64," + bytes.base64EncodedString(), key: "images", index: 1)
            }
        }
    }

    @Test func fragmentedDataIsConsumedAndControlFramesStillWork() {
        var socket = MiningWebSocket()
        func masked(_ opcode: UInt8, _ bytes: [UInt8], final: Bool) -> Data {
            let mask: [UInt8] = [1, 2, 3, 4]
            return Data([(final ? 0x80 : 0) | opcode, 0x80 | UInt8(bytes.count)] + mask)
                + Data(bytes.enumerated().map { $0.element ^ mask[$0.offset % 4] })
        }
        // A UTF-8 scalar may span frames, with a ping between fragments.
        let first = masked(1, [0xf0, 0x9f], final: false)
        for byte in first { #expect(socket.receive(Data([byte])).isEmpty) }
        #expect(socket.retainedBytes == 0)
        #expect(socket.receive(masked(9, [65], final: true)).count == 1)
        #expect(socket.receive(masked(0, [0x98, 0x80], final: true)).isEmpty)
        for _ in 0..<100 {
            #expect(socket.receive(masked(2, Array(repeating: 7, count: 120), final: false)).isEmpty)
            #expect(socket.receive(masked(0, Array(repeating: 8, count: 120), final: true)).isEmpty)
            #expect(socket.retainedBytes == 0)
        }
        let closing = socket.receive(masked(8, [3, 232], final: true))
        if case .some(.close(let reply)) = closing.first { #expect(reply == MiningWebSocket.close(1000)) }
        else { Issue.record("Expected the ordinary close handshake") }
    }

    @Test(.timeLimit(.minutes(1))) func stoppingListenerDeliversAnAdmittedTerminalReply() async throws {
        let gate = MiningReplyGate()
        let states = Mutex<[MiningListener.State]>([])
        let listener = MiningListener(port: 0, handler: { _, _ in
            await gate.hold()
            return .json(200, ["finished": true])
        }, changed: { state in states.withLock { $0.append(state) } })
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        listener.start()
        do {
            let clock = ContinuousClock()
            let deadline = clock.now + .seconds(5)
            var port: UInt16?
            while port == nil, clock.now < deadline {
                port = states.withLock { values in
                    values.compactMap { state -> UInt16? in
                        if case .listening(let port) = state { return port }
                        return nil
                    }.last
                }
                if port == nil { try await Task.sleep(for: .milliseconds(10)) }
            }
            let bound = try #require(port)
            let url = try #require(URL(string: "http://127.0.0.1:\(bound)/srs-check"))
            let reply = Task { try await session.data(from: url) }
            let entryDeadline = clock.now + .seconds(5)
            while !(await gate.entered), clock.now < entryDeadline {
                try await Task.sleep(for: .milliseconds(10))
            }
            try #require(await gate.entered)
            listener.requestStop()
            await gate.release()
            await listener.stop()
            #expect(states.withLock { $0.last } == .off)
            let (data, response) = try await reply.value
            #expect((response as? HTTPURLResponse)?.statusCode == 200)
            #expect((try JSONSerialization.jsonObject(with: data) as? [String: Bool])?["finished"] == true)
        } catch {
            await gate.release()
            await listener.stop()
            throw error
        }
    }

    @Test func preferencesStartOffAndWaitForLaunchCompletion() {
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic")
        #expect(!model.enabled && model.migaku.status == "Off · Port 44432")
        model.setEnabled(true, for: .migaku)
        #expect(model.enabled && model.migaku.status == "Waiting for collection · Port 44432") // No bind before entry completes.
    }
    static func type(_ fields: [String]) -> NotetypeRecord {
        .init(id: 1, name: "Imported", kind: 0, sortField: 0, css: "", base: Array(repeating: 1, count: 32),
            fields: fields.enumerated().map { .init(ord: UInt32($0.offset), id: nil, name: $0.element, sticky: false, rtl: false, fontName: "", fontSize: 20, description: "", plainText: false, collapsed: false, excludeFromSearch: false, preventDeletion: false) }, templates: [])
    }
}
private actor MiningReplyGate {
    private(set) var entered = false
    private var released = false
    private var continuation: CheckedContinuation<Void, Never>?

    func hold() async {
        entered = true
        await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                if released || Task.isCancelled { continuation.resume() }
                else { self.continuation = continuation }
            }
        } onCancel: {
            Task { await self.release() }
        }
    }

    func release() {
        released = true
        continuation?.resume()
        continuation = nil
    }
}
#endif
