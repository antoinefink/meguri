#if os(macOS)
@testable import AppModel
import CoreBridge
import Foundation
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
        let raw = #"{"sentence":"A synthetic sentence.","translation":"A translation.","sentenceAudio":[{"id":"a","input":"data:audio/mp4;base64,AAAAGGZ0eXBNNEEgAAAAAE00QSBpc29t"}],"images":[{"id":"i","src":"data:image/png;base64,iVBORw0KGgo="}]}"#
        let payload = try MiningPayload(Data(raw.utf8))
        let type = Self.type(["Front", "Back"])
        let (fields, media) = try payload.fields(for: type)
        #expect(media.count == 2 && media[0].name.hasSuffix(".m4a") && media[1].name.hasSuffix(".png"))
        #expect(fields[0].contains("[sound:__meguri_mining_attachment_0__]") && fields[0].contains("<img src=\"__meguri_mining_attachment_1__\">"))
        #expect(fields[1] == "A translation.")
        for source in ["http://127.0.0.1/private", "https://example.org/audio.mp3", "file:///secret", "data:image/svg+xml;base64,PHN2Zy8+", "data:audio/mp4;base64,!"] {
            #expect(throws: (any Error).self) { try MiningPayload.media(source, key: "sentenceAudio", index: 1) }
        }
    }
    @Test func preferencesStartOffAndWaitForLaunchCompletion() {
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic")
        #expect(!model.enabled && model.status == "Waiting for collection")
        model.setEnabled(true)
        #expect(model.enabled && model.status == "Waiting for collection") // No bind before entry completes.
    }
    static func type(_ fields: [String]) -> NotetypeRecord {
        .init(id: 1, name: "Imported", kind: 0, sortField: 0, css: "", base: Array(repeating: 1, count: 32),
            fields: fields.enumerated().map { .init(ord: UInt32($0.offset), id: nil, name: $0.element, sticky: false, rtl: false, fontName: "", fontSize: 20, description: "", plainText: false, collapsed: false, excludeFromSearch: false, preventDeletion: false) }, templates: [])
    }
}
#endif
