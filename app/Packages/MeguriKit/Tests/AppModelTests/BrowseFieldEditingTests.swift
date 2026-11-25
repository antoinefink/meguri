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

    @Test func rootInlineReturnStopsBeforeTheNextAuthoredBlock() async throws {
        let host = try Self.host("<b>abcd</b><section data-author=\"keep\">later</section>")
        defer { host.shutDown() }
        try await host.start()
        try await Self.edit(host, "select(rich.querySelector('b').firstChild, 2); key('insertParagraph');")
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == "<b>ab</b><div><b>cd</b></div><section data-author=\"keep\">later</section>")
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
        #expect(host.draft.values[0].contains("<div><br>x<b>d</b></div>"))
        _ = try await host.webView.callAsyncJavaScript("return await meguriEditorRuntime.receive({type:'undo', loadId:1, ord:0});", contentWorld: Self.world)
        _ = try await host.webView.callAsyncJavaScript("return await meguriEditorRuntime.receive({type:'undo', loadId:1, ord:0});", contentWorld: Self.world)
        _ = try await host.webView.callAsyncJavaScript("return await meguriEditorRuntime.receive({type:'undo', loadId:1, ord:0});", contentWorld: Self.world)
        try await host.flush(hold: false)
        #expect(host.draft.values[0] == "<blockquote><div data-author=\"kept\"><b>abcd</b></div></blockquote>")

        let list = try Self.host("<ul><li><div>a</div></li></ul>")
        defer { list.shutDown() }
        try await list.start()
        try await Self.edit(list, "select(rich.querySelector('li div').firstChild, 1); key('insertParagraph'); key('insertParagraph');")
        try await list.flush(hold: false)
        #expect(list.draft.values[0] == "<ul><li><div>a</div></li></ul><div><br></div>")
    }

    #if os(macOS)
    @Test(arguments: [NSPasteboard.PasteboardType.png, .tiff, .fileURL])
    func picturePasteImportsAtTheCaretAndKeepsMediaBytes(_ type: NSPasteboard.PasteboardType) async throws {
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
        else { board.setData(bytes, forType: type) }
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

    @Test(arguments: ["field", "note", "held", "source", "setField"])
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
        else { try await host.reload(fields: [0]) }
        hold.release()
        await completion.value
        if change != "note" {
            try await host.flush(hold: false)
            #expect(host.draft.values[1] == "other field")
            #expect(host.draft.values[0].contains("<img") == (change == "field"))
        } else { #expect(host.draft.values == ["abcd", "other field"]) }
        #expect(!hold.expired && media.added.count == 1)
    }

    @MainActor private final class CurrentOwner {
        var isCurrent = true
    }

    @Test func unsupportedPasteRepresentationsFailClosedAndBitmapsRespectEncoding() throws {
        let board = NSPasteboard(name: .init(UUID().uuidString))
        defer { board.clearContents() }
        let bitmap = try Self.bitmap(.png)
        for type in ["public.html", "net.meguri.field-html", "public.rtf", "com.apple.webarchive"] {
            let item = NSPasteboardItem()
            item.setData(bitmap, forType: .png)
            item.setString("<img src='https://example.invalid/image.png'>", forType: .init(type))
            board.clearContents(); board.writeObjects([item])
            #expect(try FieldPicturePaste.read(board).isEmpty, "unsupported HTML/private types never fall through to bitmap")
        }
        board.clearContents(); board.setString("https://example.invalid/image.png", forType: .URL)
        #expect(try FieldPicturePaste.read(board).isEmpty)
        board.clearContents(); board.setData(Data([0, 1, 2]), forType: .png)
        #expect(try FieldPicturePaste.read(board).isEmpty)
        board.clearContents(); board.setString("file:///unreadable/note.txt", forType: .fileURL)
        board.setData(bitmap, forType: .tiff)
        #expect(try FieldPicturePaste.read(board).isEmpty, "a copied unsupported file cannot paste its icon")
        for png in [true, false] {
            let encoded = try FieldPicturePaste.encode(bitmap, asPNG: png)
            let decoded = try #require(NSBitmapImageRep(data: encoded.bytes))
            #expect(decoded.pixelsWide == 2 && decoded.pixelsHigh == 2)
            #expect(encoded.name.hasSuffix(png ? ".png" : ".jpg"))
            #expect(encoded.name.count == 50)
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
    init(root: URL) { self.root = root }
    func addMedia(name: String, bytes: [UInt8]) async throws(BridgeError) -> String {
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
