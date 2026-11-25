import CoreBridge
import Foundation
import Testing
import WebKit
@testable import AppModel
@testable import Editor
#if os(macOS)
import AppKit
#endif

/// Synthetic fields only. Exercise the actual closed-shadow editing surface,
/// its selection and beforeinput dispatcher, rather than replacing HTML source.
@MainActor
@Suite(.serialized) struct BrowseFieldEditingTests {
    @Test(arguments: ["abc", "<div>abc</div>", "<section><div>abc</div></section>", "<span block=\"true\">abc</span>"])
    func consecutiveReturnsKeepVisibleLinesAndSurviveReload(_ stored: String) async throws {
        let host = try Self.host(stored)
        defer { host.shutDown() }
        try await host.start()
        try await Self.edit(host, """
            const text = document.createTreeWalker(rich, NodeFilter.SHOW_TEXT); text.nextNode();
            select(text.currentNode, 3);
            key('insertParagraph'); key('insertParagraph'); key('insertParagraph');
            const blanks = [...rich.querySelectorAll('div')].filter(node => node.innerHTML === '<br>');
            if (blanks.length !== 3 || blanks.some(node => node.getBoundingClientRect().height <= 0))
                throw new Error('Empty lines collapsed in the actual editing surface');
            """)
        let expected: String
        if stored == "abc" { expected = "abc<div><br></div><div><br></div><div><br></div>" }
        else if stored.hasPrefix("<section>") {
            expected = "<section><div>abc</div><div><br></div><div><br></div><div><br></div></section>"
        } else if stored.hasPrefix("<span") {
            expected = "<span block=\"true\">abc</span><div><br></div><div><br></div><div><br></div>"
        } else { expected = "<div>abc</div><div><br></div><div><br></div><div><br></div>" }
        try await host.flush(hold: false)
        #expect(host.draft.values == [expected, "other field"])
        // Inserting text uses WebKit's actual current selection, not a DOM assignment.
        try await Self.edit(host, "document.execCommand('insertText', false, 'z');")
        try await host.flush(hold: false)
        #expect(host.draft.values[0].contains("<div><br></div><div><br></div><div>z"))
        let saved = host.draft.values[0]
        let reopened = try Self.host(saved)
        defer { reopened.shutDown() }
        try await reopened.start()
        try await reopened.flush(hold: true)
        #expect(reopened.draft.values == [saved, "other field"] && !reopened.draft.hasChanges)
    }

    @Test(arguments: ["", "<div><br></div>", "<div><i></i></div>"])
    func returnsInEmptyContentRetainEachCaretLine(_ stored: String) async throws {
        let host = try Self.host(stored)
        defer { host.shutDown() }
        try await host.start()
        try await Self.edit(host, "select(rich.firstChild?.localName === 'div' ? rich.firstChild : rich, 0); key('insertParagraph'); key('insertParagraph');")
        try await host.flush(hold: false)
        #expect(host.draft.values[0].contains("<div><br></div><div><br></div>"))
    }

    @Test(arguments: ["", "<br>", "<br><br>"])
    func midRootReturnsKeepOnlyAuthoredBreaksAndReload(_ trailing: String) async throws {
        let original = "<span style=\"color: red\"><b>abcd</b></span>" + trailing
        let host = try Self.host(original)
        defer { host.shutDown() }
        try await host.start()
        try await Self.edit(host, """
            select(rich.querySelector('b').firstChild, 2);
            key('insertParagraph'); key('insertParagraph');
            if (rich.querySelector('div').getBoundingClientRect().height <= 0)
                throw new Error('Empty split source lost caret height');
            """)
        try await host.flush(hold: false)
        let saved = "<span style=\"color: red\"><b>ab</b></span><div><br></div><div><span style=\"color: red\"><b>cd</b></span>" + trailing + "</div>"
        #expect(host.draft.values[0] == saved)
        try await host.reload(fields: [0])
        try await Self.edit(host, """
            const blanks = [...rich.querySelectorAll('div')].filter(node => node.innerHTML === '<br>');
            if (blanks.length !== 1 || blanks[0].getBoundingClientRect().height <= 0)
                throw new Error('Reload collapsed the consecutive split');
            """)
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == saved)
        let reopened = try Self.host(saved)
        defer { reopened.shutDown() }
        try await reopened.start()
        try await reopened.flush(hold: true)
        #expect(reopened.draft.values[0] == saved && !reopened.draft.hasChanges)
    }

    @Test func rootInlineReturnStopsBeforeTheNextAuthoredBlock() async throws {
        let host = try Self.host("<b>abcd</b><section data-author=\"keep\">later</section>")
        defer { host.shutDown() }
        try await host.start()
        try await Self.edit(host, "select(rich.querySelector('b').firstChild, 2); key('insertParagraph');")
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == "<b>ab</b><div><b>cd</b></div><section data-author=\"keep\">later</section>")
    }

    @Test(arguments: [false, true])
    func rootInlineReturnAfterAnAuthoredBlockKeepsBlankLine(_ rootBoundary: Bool) async throws {
        let original = "<div>a</div>bc"
        let host = try Self.host(original)
        defer { host.shutDown() }
        try await host.start()
        try await Self.edit(host, """
            \(rootBoundary ? "select(rich, 1);" : "select(rich.lastChild, 0);")
            key('insertParagraph');
            const lines = rich.querySelectorAll('div');
            if (lines[1].getBoundingClientRect().top <= lines[0].getBoundingClientRect().bottom)
                throw new Error('Return after an authored block lost the visible blank line');
            """)
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == "<div>a</div><br><div>bc</div>")
        _ = try await host.webView.callAsyncJavaScript("return await meguriEditorRuntime.receive({type:'undo', loadId:1, ord:0});", contentWorld: Self.world)
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == original)
    }

    @Test func returnSelectionShiftReturnListsAndUndoUseTheLiveCaret() async throws {
        let host = try Self.host("<blockquote><div data-author=\"kept\"><b>abcd</b></div></blockquote>")
        defer { host.shutDown() }
        try await host.start()
        try await Self.edit(host, "select(rich.querySelector('b').firstChild, 1, 3); key('insertParagraph');")
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == "<blockquote><div data-author=\"kept\"><b>a</b></div><div><b>d</b></div></blockquote>")
        try await Self.edit(host, "key('insertLineBreak'); document.execCommand('insertText', false, 'x');")
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == "<blockquote><div data-author=\"kept\"><b>a</b></div><div><br><b>xd</b></div></blockquote>")
        try await Self.edit(host, """
            const bold = rich.querySelector('blockquote > div:nth-child(2) > b');
            const caret = selection.getComposedRanges ? selection.getComposedRanges({shadowRoots:[root]})[0]
                : selection.getRangeAt(0);
            if (caret.startContainer !== bold.firstChild || caret.startOffset !== 1 ||
                caret.endContainer !== caret.startContainer || caret.endOffset !== 1)
                throw new Error('Typing in bold lost the live caret');
            if (rich.querySelector('[data-author="kept"]').innerHTML !== '<b>a</b>' ||
                [...rich.querySelectorAll('b')].some(node => Number(getComputedStyle(node).fontWeight) < 600))
                throw new Error('Return changed authored bold styling');
            """)
        for expected in [
            "<blockquote><div data-author=\"kept\"><b>a</b></div><div><br><b>d</b></div></blockquote>",
            "<blockquote><div data-author=\"kept\"><b>a</b></div><div><b>d</b></div></blockquote>",
            "<blockquote><div data-author=\"kept\"><b>abcd</b></div></blockquote>"
        ] {
            _ = try await host.webView.callAsyncJavaScript("return await meguriEditorRuntime.receive({type:'undo', loadId:1, ord:0});", contentWorld: Self.world)
            try await host.flush(hold: false)
            #expect(host.draft.values[0] == expected)
        }

        let list = try Self.host("<ul><li><div>a</div></li></ul>")
        defer { list.shutDown() }
        try await list.start()
        try await Self.edit(list, "select(rich.querySelector('li div').firstChild, 1); key('insertParagraph'); key('insertParagraph');")
        try await list.flush(hold: false)
        #expect(list.draft.values[0] == "<ul><li><div>a</div></li></ul><div><br></div>")
    }

    #if os(macOS)
    @Test(arguments: ["png", "tiff", "corrupt-png-tiff", "file", "html", "html-alt", "archive", "url"])
    func picturePasteImportsAtTheCaretAndKeepsMediaBytes(_ representation: String) async throws {
        let type: NSPasteboard.PasteboardType = representation == "file" ? .fileURL : representation == "tiff" ? .tiff : .png
        let host = try Self.host("<b>abcd</b>")
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { host.shutDown(); try? FileManager.default.removeItem(at: root) }
        let bytes = try Self.bitmap(type == .tiff ? .tiff : .png)
        let source = root.appending(path: "Synthetic & 100%.PNG")
        try bytes.write(to: source)
        let media = PictureTestMedia(root: root.appending(path: "media"))
        host.pictureMedia = media
        host.mediaDirectory = media.root
        try await host.start()
        try await Self.edit(host, "select(rich.querySelector('b').firstChild, 2);")
        let board = NSPasteboard(name: .init(UUID().uuidString))
        host.picturePasteboard = board
        board.clearContents()
        if type == .fileURL { board.writeObjects([source as NSURL]) }
        else if representation == "corrupt-png-tiff" {
            let item = NSPasteboardItem()
            item.setData(Data([0, 1, 2]), forType: .png)
            item.setData(try Self.bitmap(.tiff), forType: .tiff)
            board.writeObjects([item])
        } else { board.setData(bytes, forType: type) }
        let remote = "https://example.invalid/copy-image.png"
        if representation == "html" { board.setString("<a href='\(remote)'><img src='\(remote)'></a>", forType: .html) }
        if representation == "html-alt" { board.setString("<div><a href='\(remote)'><img src='\(remote)' alt='3 > 2'></a></div>", forType: .html) }
        if representation == "archive" {
            let archive: [String: Any] = ["WebMainResource": ["WebResourceMIMEType": "text/html",
                "WebResourceTextEncodingName": "UTF-8", "WebResourceData": Data("<img src='\(remote)'>".utf8)]]
            board.setData(try PropertyListSerialization.data(fromPropertyList: archive, format: .binary, options: 0),
                forType: .init("com.apple.webarchive"))
        }
        if representation == "url" { board.setString(remote, forType: .URL); board.setString(remote, forType: .string) }
        defer { board.clearContents() }
        // Exercise the runtime's real paste listener with a private native
        // pasteboard. The VM UI test supplies native Cmd-V as well.
        try await Self.edit(host, "select(rich.querySelector('b').firstChild, 2); rich.dispatchEvent(new ClipboardEvent('paste', {bubbles:true, cancelable:true}));")
        for _ in 0..<300 where media.added.isEmpty { try await Task.sleep(for: .milliseconds(10)) }
        try #require(media.added.count == 1)
        for _ in 0..<300 where !host.draft.values[0].contains("<img") { try await Task.sleep(for: .milliseconds(10)) }
        try await host.flush(hold: false)
        let saved = host.draft.values[0]
        #expect(saved == "<b>ab<img src=\"\(media.added[0].0)\">cd</b>")
        #expect(host.draft.values[1] == "other field")
        let imported = try Data(contentsOf: media.root.appending(path: media.added[0].0))
        #expect(imported == media.added[0].1)
        if type == .fileURL { #expect(imported == bytes, "file copies keep their original bytes") }
        else {
            let image = try #require(NSBitmapImageRep(data: imported))
            #expect(image.pixelsWide == 2 && image.pixelsHigh == 2)
            #expect(media.added[0].0.hasSuffix(".jpg"), "unreadable/missing encoding preference defaults to JPEG")
        }
        try await Self.edit(host, """
            const image = rich.querySelector('img');
            const caret = selection.getComposedRanges ? selection.getComposedRanges({shadowRoots:[root]})[0]
                : selection.getRangeAt(0);
            if (caret.startContainer !== image.parentNode || caret.startOffset !== [...image.parentNode.childNodes].indexOf(image) + 1)
                throw new Error('Picture insertion lost its caret');
            """)
        // The insertion is one ordinary field-history step.
        _ = try await host.webView.callAsyncJavaScript("return await meguriEditorRuntime.receive({type:'undo', loadId:1, ord:0});", contentWorld: Self.world)
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == "<b>abcd</b>")
        #expect(FileManager.default.fileExists(atPath: media.root.appending(path: media.added[0].0).path))
        let reopened = try Self.host(saved)
        defer { reopened.shutDown() }
        reopened.mediaDirectory = media.root
        try await reopened.start()
        try await reopened.flush(hold: true)
        #expect(reopened.draft.values[0] == saved)
    }

    @Test(arguments: ["field", "note", "held", "source", "setField", "typing"])
    func delayedPicturePasteCannotChangeAnotherTarget(_ change: String) async throws {
        let owner = CurrentOwner()
        let host = try Self.host("abcd", current: { owner.isCurrent })
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let media = PictureTestMedia(root: root)
        let hold = FieldEditorTests.HeldResult()
        media.afterAdd = { await hold.suspend() }
        host.pictureMedia = media
        defer { hold.release(); host.shutDown(); try? FileManager.default.removeItem(at: root) }
        try await host.start()
        let board = NSPasteboard(name: .init(UUID().uuidString))
        host.picturePasteboard = board
        board.setData(try Self.bitmap(.png), forType: .png)
        defer { board.clearContents() }
        try await Self.edit(host, "select(rich.firstChild, 2); rich.dispatchEvent(new ClipboardEvent('paste', {bubbles:true, cancelable:true}));")
        try await hold.wait()
        let completion = try #require(host.picturePasteCompletion)
        if change == "field" { try await host.setFocus(1) }
        else if change == "note" { owner.isCurrent = false; host.shutDown() }
        else if change == "held" { try await host.flush(hold: true) }
        else if change == "source" { try await host.setMode(.html, ord: 0) }
        else if change == "typing" {
            try await Self.edit(host, """
                select(rich.firstChild, 2);
                rich.dispatchEvent(new InputEvent('beforeinput', {bubbles:true, cancelable:true, inputType:'insertText', data:'X'}));
                document.execCommand('insertText', false, 'X');
                """)
        } else { try await host.reload(fields: [0]) }
        hold.release()
        await completion.value
        if change != "note" {
            try await host.flush(hold: false)
            #expect(host.draft.values[1] == "other field")
            #expect(host.draft.values[0].contains("<img") == (change == "field"))
            if change == "typing" { #expect(host.draft.values[0] == "abXcd", "a delayed paste cannot swallow or reorder subsequent typing") }
        } else { #expect(host.draft.values == ["abcd", "other field"]) }
        #expect(!hold.expired && media.added.count == 1)
    }

    @Test(arguments: [false, true])
    func filePasteWarnsInOrderContinuesFailuresAndKeepsSVG(addEmpty: Bool) async throws {
        let host = try Self.host("abcd")
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { host.shutDown(); try? FileManager.default.removeItem(at: root) }
        let empty = root.appending(path: "empty.png"), large = root.appending(path: "large.png")
        let vector = root.appending(path: "vector.svg"), last = root.appending(path: "last.png"), refused = root.appending(path: "refused.png")
        try Data().write(to: empty); try Data().write(to: large)
        let handle = try FileHandle(forWritingTo: large)
        try handle.truncate(atOffset: UInt64(FieldPicturePaste.maximumSyncSize + 1)); try handle.close()
        let svg = Data("<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><rect width='2' height='2' fill='red'/></svg>".utf8)
        try svg.write(to: vector)
        let bitmap = try Self.bitmap(.png)
        try bitmap.write(to: last); try bitmap.write(to: refused)
        let media = PictureTestMedia(root: root.appending(path: "media"))
        media.refusedNames = ["refused.png"]
        host.pictureMedia = media
        let warnings = PictureWarnings()
        host.confirmPictureSize = { name, size in warnings.values.append((name, size)); return name == "empty.png" && addEmpty }
        try await host.start()
        let board = NSPasteboard(name: .init(UUID().uuidString))
        defer { board.clearContents() }
        board.writeObjects([root.appending(path: "missing.png"), empty, large, refused, vector, last].map { $0 as NSURL })
        host.picturePasteboard = board
        try await Self.edit(host, "select(rich.firstChild, 2); rich.dispatchEvent(new ClipboardEvent('paste', {bubbles:true, cancelable:true}));")
        for _ in 0..<300 where host.picturePasteCompletion == nil { try await Task.sleep(for: .milliseconds(10)) }
        await (try #require(host.picturePasteCompletion)).value
        try await host.flush(hold: false)
        #expect(warnings.values.map(\.0) == ["empty.png", "large.png"])
        #expect(warnings.values.map(\.1) == [0, FieldPicturePaste.maximumSyncSize + 1])
        #expect(host.pictureFailures.map(\.name) == ["missing.png", "refused.png"])
        let names = (addEmpty ? ["empty.png"] : []) + ["vector.svg", "last.png"]
        #expect(media.added.map(\.0) == names)
        #expect(media.added.first(where: { $0.0 == "vector.svg" })?.1 == svg)
        #expect(host.draft.values[0] == "ab" + names.map { "<img src=\"\($0)\">" }.joined() + "cd")
        #expect(!host.draft.values[0].contains("<svg"))
    }

    @MainActor private final class PictureWarnings {
        var values: [(String, Int)] = []
    }

    @MainActor private final class CurrentOwner {
        var isCurrent = true
    }

    @Test(arguments: [
        ("<div><a href='https://example.invalid/image'><img alt='3 > 2' src='https://example.invalid/image'></a></div>", true),
        ("<!-- <img> --><img alt='3 > 2'>", true),
        ("<p>A meaningful selection<img alt='3 > 2'></p>", false),
        ("<span>3 &gt; 2</span><img>", false),
        ("<img><img>", false),
        ("<iframe src='https://example.invalid/'></iframe><img>", false)
    ])
    func copyImageClassificationUsesInertHTMLNodes(_ html: String, _ expected: Bool) async throws {
        let host = try Self.host("unchanged")
        defer { host.shutDown() }
        try await host.start()
        let result = try await host.webView.callAsyncJavaScript(
            "return await meguriEditorRuntime.receive({type:'classifyPicturePaste', loadId:1, html, urls:[]});",
            arguments: ["html": html], contentWorld: Self.world) as? [String: Any]
        #expect(result?["copyImage"] as? Bool == expected)
        try await host.flush(hold: false)
        #expect(host.draft.values == ["unchanged", "other field"])
    }

    @Test func unsupportedPasteRepresentationsFailClosedAndBitmapsRespectEncoding() throws {
        let board = NSPasteboard(name: .init(UUID().uuidString))
        defer { board.clearContents() }
        let bitmap = try Self.bitmap(.png)
        for type in ["public.html", "net.meguri.field-html", "public.rtf", "com.apple.webarchive"] {
            let item = NSPasteboardItem()
            item.setData(bitmap, forType: .png)
            let html = type == "public.html" ? "<p>A meaningful selection<img src='https://example.invalid/image.png'></p>"
                : "<img src='https://example.invalid/image.png'>"
            item.setString(html, forType: .init(type))
            board.clearContents(); board.writeObjects([item])
            #expect(FieldPicturePaste.sources(FieldPicturePaste.snapshot(board)).isEmpty, "unsupported HTML/private types never fall through to bitmap")
        }
        board.clearContents(); board.setString("https://example.invalid/image.png", forType: .URL)
        #expect(FieldPicturePaste.sources(FieldPicturePaste.snapshot(board)).isEmpty)
        board.clearContents(); board.setData(Data([0, 1, 2]), forType: .png)
        let invalid = try #require(FieldPicturePaste.sources(FieldPicturePaste.snapshot(board)).first)
        #expect(throws: (any Error).self) { try invalid.prepare(asPNG: false) }
        board.clearContents(); board.setString("file:///unreadable/note.txt", forType: .fileURL)
        board.setData(bitmap, forType: .tiff)
        #expect(FieldPicturePaste.sources(FieldPicturePaste.snapshot(board)).isEmpty, "a copied unsupported file cannot paste its icon")
        #expect(FieldPicturePaste.needsSizeWarning(0))
        #expect(!FieldPicturePaste.needsSizeWarning(FieldPicturePaste.maximumSyncSize))
        #expect(FieldPicturePaste.needsSizeWarning(FieldPicturePaste.maximumSyncSize + 1))
        for png in [true, false] {
            let encoded = try FieldPicturePaste.encode(bitmap, asPNG: png)
            let decoded = try #require(NSBitmapImageRep(data: encoded.bytes))
            #expect(decoded.pixelsWide == 2 && decoded.pixelsHigh == 2)
            #expect(encoded.name.hasSuffix(png ? ".png" : ".jpg"))
            #expect(encoded.name.count == 50)
            #expect(!FieldPicturePaste.needsSizeWarning(encoded.bytes.count))
            if !png {
                let color = try #require(decoded.colorAt(x: 0, y: 0)?.usingColorSpace(.deviceRGB))
                #expect(color.redComponent > 0.9 && color.greenComponent > 0.35 && color.blueComponent > 0.35,
                        "alpha is composited over white for JPEG")
            }
        }
    }

    static func bitmap(_ format: NSBitmapImageRep.FileType) throws -> Data {
        let image = try #require(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 2, pixelsHigh: 2,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
            bytesPerRow: 0, bitsPerPixel: 0))
        for x in 0..<2 { for y in 0..<2 { image.setColor(.red.withAlphaComponent(0.5), atX: x, y: y) } }
        return try #require(image.representation(using: format, properties: [:]))
    }
    #endif

    static let world = WKContentWorld.world(name: "Meguri.FieldEditor.v1")
    static func host(_ stored: String, current: @escaping @MainActor () -> Bool = { true }) throws -> FieldEditorHost {
        let fields = ["Front", "Back"].enumerated().map { index, name in
            NotetypeField(ord: UInt32(index), id: Int64(index), name: name, sticky: false, rtl: false, fontName: "", fontSize: 20,
                          description: "", plainText: false, collapsed: false, excludeFromSearch: false)
        }
        let type = NotetypeRecord(id: 42, name: "Synthetic pair", kind: 0, sortField: 0, css: "", base: [], fields: fields, templates: [])
        let draft = try NoteEditorDraft(editing: .init(id: 5, notetypeId: 42, fields: [stored, "other field"], tags: [], guid: "g"),
            notetype: type, deck: .init(id: 1, fullName: "Synthetic", filtered: false), generation: 1)
        let host = try FieldEditorHost(draft: draft, isCurrent: current)
        host.browsing = true
        installProbe(host)
        host.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 600)
        return host
    }

    static func installProbe(_ host: FieldEditorHost) {
        // Only this test's isolated world can reach its closed roots; production
        // and the page world expose no field-content or command test hooks.
        host.webView.configuration.userContentController.addUserScript(WKUserScript(source: """
            globalThis.testFieldRoots = [];
            const attach = Element.prototype.attachShadow;
            Element.prototype.attachShadow = function(options) {
                const root = attach.call(this, options); testFieldRoots.push(root); return root;
            };
            """, injectionTime: .atDocumentStart, forMainFrameOnly: true, in: world))
    }
    static func edit(_ host: FieldEditorHost, _ script: String, ord: Int = 0) async throws {
        _ = try await host.webView.callAsyncJavaScript("""
            const root = testFieldRoots.filter(root => root.host.isConnected)[ord], rich = root.querySelector('meguri-field-content');
            const selection = root.getSelection ? root.getSelection() : document.getSelection();
            function select(node, start, end = start) { selection.setBaseAndExtent(node, start, node, end); }
            function key(inputType) {
                if (rich.dispatchEvent(new InputEvent('beforeinput', {bubbles:true, cancelable:true, inputType})))
                    throw new Error('Return was not handled');
            }
            rich.focus();
            // A detached web view need not emit a focus event by itself.
            rich.dispatchEvent(new FocusEvent('focus'));
            \(script)
            """, arguments: ["ord": ord], contentWorld: world)
    }
}

#if os(macOS)
/// A disk-backed test importer; the separate real-core inspector regression
/// checks the production importer's tracked media and collection readback.
@MainActor final class PictureTestMedia: MediaService {
    let root: URL
    var added: [(String, Data)] = []
    var afterAdd: (@MainActor () async -> Void)?
    var refusedNames: Set<String> = []
    init(root: URL) { self.root = root }
    func addMedia(name: String, bytes: [UInt8]) async throws(BridgeError) -> String {
        if refusedNames.contains(name) { throw .unavailable(reason: "Synthetic refusal") }
        do {
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            let data = Data(bytes)
            try data.write(to: root.appending(path: name))
            added.append((name, data))
            await afterAdd?()
            return name
        } catch { throw .unavailable(reason: "Synthetic media write failed") }
    }
}
#endif
