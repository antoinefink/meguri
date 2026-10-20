@testable import CardRendering
import Foundation
import Testing

@Suite struct MediaPathResolverTests {
    @Test func singleDecodeAndFlatPathDefense() {
        let cases: [(String, String?)] = [
            ("/my%20file.jpg", "/my file.jpg"), ("/x%2520y.png", "/x%20y.png"),
            ("/100%%20sure.png", "/100% sure.png"), ("/a+b.png", "/a+b.png"),
            ("/%FF.png", nil), ("/%00.png", nil), ("/%2e%2e/", nil), ("/a%2Fb", nil), ("/a%5cb", nil), ("/../secret", nil),
        ]
        for (input, expected) in cases { #expect(MediaOrigin.decode(input) == expected, "\(input)") }
        for url in ["meguri-media://other/a", "meguri-media://user@media/a", "meguri-media://media:12/a", "file:///a"] {
            #expect(MediaOrigin.path(URL(string: url)!) == nil)
        }
    }

    @Test func unicodeCaseFreshArrivalAndNoSymlinks() async throws {
        let fixture = MediaFixture()
        let volume = try fixture.root.resourceValues(forKeys: [.volumeSupportsCaseSensitiveNamesKey])
        let caseSensitive = try #require(volume.volumeSupportsCaseSensitiveNames)
        fixture.add("/cafe\u{301}.png", Data("nfd".utf8), "image/png")
        fixture.add("/photo.jpg", Data("photo".utf8), "image/jpeg")
        fixture.add("/MIXED.png", Data("winner".utf8), "image/png")
        if caseSensitive {
            fixture.add("/Mixed.png", Data("other".utf8), "image/png")
            let upper = try #require(try FileManager.default.attributesOfItem(atPath: fixture.root.appending(path: "MIXED.png").path)[.systemFileNumber] as? NSNumber)
            let mixed = try #require(try FileManager.default.attributesOfItem(atPath: fixture.root.appending(path: "Mixed.png").path)[.systemFileNumber] as? NSNumber)
            #expect(upper != mixed)
        }
        fixture.add("/x%20y.mp3", Data("percent".utf8), "audio/mpeg")
        let resolver = try await fixture.resolved()
        let cases = [("/café.png", "nfd"), ("/PHOTO.JPG", "photo"), ("/MiXeD.png", "winner")]
        for (path, expected) in cases {
            let file = try await resolver.openURLPath(path)
            #expect(try await file.read(offset: 0, count: 32) == Data(expected.utf8))
            await file.close()
        }
        let raw = try await resolver.openAudioName("folder/x%20y.mp3")
        #expect(try await raw.read(offset: 0, count: 32) == Data("percent".utf8))
        await raw.close()
        await #expect(throws: (any Error).self) { try await resolver.openURLPath("/arrived.png") }
        fixture.add("/arrived.png", Data("new".utf8), "image/png")
        let arrived = try await resolver.openURLPath("/arrived.png")
        #expect(try await arrived.read(offset: 0, count: 32) == Data("new".utf8))
        await arrived.close()
        try FileManager.default.createSymbolicLink(at: fixture.root.appending(path: "link.png"), withDestinationURL: fixture.root.appending(path: "photo.jpg"))
        for path in ["/link.png", "/../outside", "/a/b", "/.", "/.."] {
            await #expect(throws: (any Error).self) { try await resolver.openURLPath(path) }
        }
        // Atomic replacement is visible to the next secure open.
        try Data("replacement".utf8).write(to: fixture.root.appending(path: "photo.jpg"), options: .atomic)
        let replaced = try await resolver.openURLPath("/PHOTO.JPG")
        #expect(try await replaced.read(offset: 0, count: 32) == Data("replacement".utf8))
        await replaced.close()
        await resolver.close()
        await #expect(throws: (any Error).self) { try await resolver.openURLPath("/photo.jpg") }
    }

    @Test func rootReplacementRequiresOwnerRevalidation() async throws {
        let fixture = MediaFixture()
        fixture.add("/x.txt", Data("old".utf8), "text/plain")
        let resolver = try await fixture.resolved()
        let moved = fixture.root.appendingPathExtension("old")
        try FileManager.default.moveItem(at: fixture.root, to: moved)
        defer { try? FileManager.default.removeItem(at: moved) }
        try FileManager.default.createDirectory(at: fixture.root, withIntermediateDirectories: false)
        try Data("new".utf8).write(to: fixture.root.appending(path: "x.txt"))
        await #expect(throws: (any Error).self) { try await resolver.openURLPath("/x.txt") }
        await resolver.close()
    }
}
