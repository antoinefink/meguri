@testable import CardRendering
import Foundation
import PDFKit
import Testing
import WebKit

extension WebKitSuites {
    @Suite struct CardAttachmentPDFTests {
        static let name = "meguri-" + String(repeating: "7b", count: 32) + ".pdf"

        @Test func nativePDFRetainsOfflinePolicyAndPreservesLocalDestinations() throws {
            weak var policy: (any PDFViewDelegate)?
            let pdf = autoreleasepool {
                let view = CardPDFView(frame: CGRect(x: 0, y: 0, width: 600, height: 800))
                policy = view.delegate
                return view
            }
            // Only the view owns the delegate after the creating scope has ended.
            #expect(policy != nil)
            #expect(pdf.delegate === policy)
            try #require(pdf.delegate?.responds(to: #selector(PDFViewDelegate.pdfViewWillClick(onLink:with:))) == true)
            try #require(pdf.delegate?.responds(to: #selector(PDFViewDelegate.pdfViewOpenPDF(_:forRemoteGoToAction:))) == true)

            let document = PDFDocument()
            let first = PDFPage()
            let second = PDFPage()
            for (index, page) in [first, second].enumerated() {
                page.setBounds(CGRect(x: 0, y: 0, width: 600, height: 800), for: .mediaBox)
                document.insert(page, at: index)
            }
            pdf.document = document
            pdf.go(to: first)
            for address in ["https://example.invalid/", "mailto:reader@example.invalid",
                            "meguri://review", "file:///meguri-pdf-policy-test.pdf"] {
                let url = try #require(URL(string: address))
                pdf.perform(PDFActionURL(url: url))
                #expect(pdf.document === document && pdf.currentPage === first)
            }
            let remote = try #require(URL(string: "file:///meguri-pdf-policy-remote.pdf"))
            pdf.perform(PDFActionRemoteGoTo(pageIndex: 0, at: .zero, fileURL: remote))
            #expect(pdf.document === document && pdf.currentPage === first)

            pdf.perform(PDFActionGoTo(destination: PDFDestination(page: second, at: .zero)))
            #expect(pdf.currentPage === second)
            pdf.document = nil
            #expect(policy != nil && pdf.delegate === policy)
        }

        @Test func exactNameAndNavigationRefusal() throws {
            #expect(CardAttachmentPDF.accepts(Self.name))
            for name in ["/" + Self.name, "./" + Self.name, Self.name + "?x", Self.name + "#",
                         Self.name.uppercased(), "MeGuRi-" + String(repeating: "7B", count: 32) + ".pDf",
                         Self.name.replacingOccurrences(of: "7", with: "%37"),
                         "meguri-" + String(repeating: "a", count: 63) + ".pdf", "other.pdf"] {
                #expect(!CardAttachmentPDF.accepts(name))
            }
            for prefix in ["meguri-media://media/", "meguri-media://foreign/", "meguri-media://media:1/",
                           "meguri-media://user@media/"] {
                let url = try #require(URL(string: prefix + Self.name))
                #expect(LinkPolicy.route(url, root: MediaOrigin.url, platform: .mac, preferExternal: false) == .refuse)
            }
        }

        @Test func boundedReadAndShortInput() throws {
            var reads = 0
            for length in [UInt64(0), UInt64(CardAttachmentPDF.maximumBytes + 1), UInt64.max] {
                #expect(throws: (any Error).self) {
                    try CardAttachmentPDF.read(length: length) { _, _ in reads += 1; return Data([1]) }
                }
            }
            #expect(reads == 0)
            var maximum = 0
            let bytes = try CardAttachmentPDF.read(length: UInt64(CardAttachmentPDF.maximumBytes)) { offset, count in
                maximum = max(maximum, count)
                // Short nonempty reads must advance by the bytes actually read.
                return Data(repeating: UInt8(offset % 251), count: min(count, 30_001))
            }
            #expect(bytes.count == CardAttachmentPDF.maximumBytes && maximum <= 65_536)
            #expect(bytes[30_001] == UInt8(30_001 % 251))
            #expect(throws: (any Error).self) {
                try CardAttachmentPDF.read(length: 9) { _, _ in Data() }
            }
        }

        @Test func cancelledReadNeverReads() async {
            let task = Task {
                withUnsafeCurrentTask { $0?.cancel() }
                var reads = 0
                #expect(throws: CancellationError.self) {
                    try CardAttachmentPDF.read(length: 1) { _, _ in reads += 1; return Data([1]) }
                }
                #expect(reads == 0)
            }
            await task.value
        }

        @Test func trackedFileOnlyAndEndpointRefusal() async throws {
            let media = MediaFixture()
            let bytes = Data("synthetic attachment bytes".utf8)
            media.add("/" + Self.name, bytes, "")
            let resolver = try await media.resolved()
            defer { Task { await resolver.close() } }
            let file = try await resolver.openAttachmentPDF(Self.name)
            #expect(try await file.attachmentPDFBytes() == bytes)
            await file.close()
            let spellings = [Self.name, Self.name.uppercased(),
                             "MeGuRi-" + String(repeating: "7B", count: 32) + ".pDf"]
            for spelling in spellings {
                // Prove that each refused endpoint could otherwise serve the tracked file.
                let aliased = try await resolver.openURLPath("/" + spelling)
                #expect(try await aliased.attachmentPDFBytes() == bytes)
                await aliased.close()
                if spelling != Self.name {
                    #expect(!CardAttachmentPDF.accepts(spelling))
                    await #expect(throws: (any Error).self) { try await resolver.openAttachmentPDF(spelling) }
                }
                let escaped = spelling.utf8.map { String(format: "%%%02X", $0) }.joined()
                for path in [spelling, escaped] {
                    for suffix in ["", "?cache=1", "#page=2", "?cache=%37#page=2"] {
                        let request = URLRequest(url: try #require(URL(string: MediaOrigin.url.absoluteString + path + suffix)))
                        let response = try await MediaSchemeHandlerTests.answer(request, fixture: media)
                        #expect(response.response?.statusCode == 404 && response.body.isEmpty)
                    }
                }
            }
            await resolver.update(trackedNames: [])
            await #expect(throws: (any Error).self) { try await resolver.openAttachmentPDF(Self.name) }
        }

        @Test func zeroWidthPDFAliasesAreRefusedOnlyInReservedNamespace() async throws {
            let media = MediaFixture()
            let bytes = Data("synthetic attachment alias bytes".utf8)
            var spellings: [String] = []
            for scalar in ["\u{200B}", "\u{200C}", "\u{200D}", "\u{2060}", "\u{FEFF}", "\u{FE0F}"] {
                for offset in [3, 32, Self.name.count] {
                    var spelling = Self.name.uppercased()
                    spelling.insert(contentsOf: scalar, at: spelling.index(spelling.startIndex, offsetBy: offset))
                    spellings.append(spelling)
                    // Real generated files make refusal independent of filesystem collation.
                    media.add("/" + spelling, bytes, "")
                }
            }
            let ordinary = "ordinary\u{200D}.pdf"
            media.add("/" + ordinary, bytes, "")
            let resolver = try await media.resolved()
            defer { Task { await resolver.close() } }
            for spelling in spellings {
                let file = try await resolver.openURLPath("/" + spelling)
                #expect(try await file.attachmentPDFBytes() == bytes)
                await file.close()
                #expect(!CardAttachmentPDF.accepts(spelling))
                await #expect(throws: (any Error).self) { try await resolver.openAttachmentPDF(spelling) }
                let escaped = spelling.utf8.map { String(format: "%%%02X", $0) }.joined()
                for path in [spelling, escaped] {
                    let request = URLRequest(url: try #require(URL(string: MediaOrigin.url.absoluteString + path + "?cache=1#page=2")))
                    let response = try await MediaSchemeHandlerTests.answer(request, fixture: media)
                    #expect(response.response?.statusCode == 404 && response.body.isEmpty)
                }
            }
            let request = URLRequest(url: MediaOrigin.url.appending(path: ordinary))
            let response = try await MediaSchemeHandlerTests.answer(request, fixture: media)
            #expect(response.response?.statusCode == 200 && response.body == bytes)
        }

        @Test func supportedMIMEAndNativeAudioSuffixes() {
            for (suffix, mime) in [("heic", "image/heic"), ("heif", "image/heif"), ("aif", "audio/aiff"),
                                   ("aiff", "audio/aiff"), ("aifc", "audio/aiff"), ("caf", "audio/x-caf"),
                                   ("ogg", "audio/ogg"), ("oga", "audio/ogg"), ("opus", "audio/ogg")] {
                #expect(MediaMIMETypes.type(for: "attachment." + suffix.uppercased()) == mime)
            }
            for suffix in ["aif", "aiff", "aifc", "caf"] { #expect(NativePlaybackCopy.extensions.contains(suffix)) }
            for suffix in ["ogg", "oga", "opus", "pdf"] { #expect(!NativePlaybackCopy.extensions.contains(suffix)) }
        }

        @Test func presentationHandlesAreBoundedAndCancelled() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            fixture.media.add("/" + Self.name, Data("synthetic PDF presenter input".utf8), "")
            let resolver = try await fixture.media.resolved()
            await resolver.update(trackedNames: [.init(name: Self.name, caseKey: Self.name)])
            try await fixture.present(.init(cardID: 77, question: "question", answer: "answer"))
            for count in 1...17 {
                fixture.pair.current.behavior.openAttachmentPDF(Self.name)
                try await CardBehaviorFixture.until("PDF receipt not presented") { fixture.presenter.attachments.count == count }
                #expect(fixture.presenter.dismissals == max(0, count - 16))
            }
            fixture.pair.current.behavior.cancel()
            #expect(fixture.presenter.dismissals == 17)
            fixture.pair.current.behavior.cancel()
            #expect(fixture.presenter.dismissals == 17)
        }

        @Test func syntheticAndTunneledClicksCannotOpenAndBindingCancelsPresentation() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let bytes = Data("synthetic in-memory PDF presenter input".utf8)
            fixture.media.add("/" + Self.name, bytes, "")
            let resolver = try await fixture.media.resolved()
            await resolver.update(trackedNames: [.init(name: Self.name, caseKey: Self.name)])
            try await fixture.present(.init(cardID: 77, question: "<a id=pdf href='\(Self.name)'><span>Open PDF</span></a>", answer: "answer"))
            let page = fixture.pair.current
            let update = try #require(page.update)
            let receipt: [String: Any] = ["signal": "openAttachmentPDF", "filename": Self.name,
                "card": update.card, "generation": update.generation, "side": update.sideName]
            let json = try String(decoding: JSONSerialization.data(withJSONObject: receipt), as: UTF8.self)
            try await page.run("""
                document.getElementById('pdf').click();
                document.getElementById('pdf').firstChild.dispatchEvent(new MouseEvent('click', {bubbles:true}));
                document.dispatchEvent(new CustomEvent('meguri:signal', {detail: receipt}));
                location.href = filename;
                await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
                """, arguments: ["receipt": json, "filename": Self.name])
            #expect(fixture.presenter.attachments.isEmpty && fixture.presenter.opens.isEmpty)
            #expect(page.webView.url == MediaOrigin.url)
            // The bridge seam stands in for an admitted isolated engine click.
            func send(_ body: [String: Any], world: String? = CardRuntime.interactionWorldName) {
                fixture.configuration.bridge.receive(name: CardRuntime.signalHandlerName, world: world, body: body, page: page)
            }
            send(receipt, world: WKContentWorld.page.name)
            for name in [Self.name.uppercased(), "MeGuRi-" + String(repeating: "7B", count: 32) + ".pDf",
                         Self.name.replacingOccurrences(of: "7", with: "%37"), Self.name + "?x", Self.name + "#page=2"] {
                send(receipt.merging(["filename": name], uniquingKeysWith: { _, new in new }))
            }
            send(receipt.merging(["generation": update.generation + 1], uniquingKeysWith: { _, new in new }))
            #expect(fixture.presenter.attachments.isEmpty)
            send(receipt)
            try await CardBehaviorFixture.until("PDF receipt not presented") { fixture.presenter.attachments.count == 1 }
            #expect(fixture.presenter.attachments == [bytes])
            await fixture.pair.suspend()
            #expect(fixture.presenter.dismissals == 1)
            send(receipt)
            await fixture.pair.resume()
            // Cancel synchronously before the read task can run.
            send(receipt)
            page.behavior.cancel()
            try await fixture.present(.init(cardID: 78, question: "replacement", answer: "answer"))
            send(receipt)
            try await fixture.script("await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));")
            #expect(fixture.presenter.attachments.count == 1)
            #expect(fixture.pair.current.update?.card == 78 && fixture.pair.current.update?.side == .question)
        }
    }
}
