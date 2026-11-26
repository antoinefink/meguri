#if os(macOS)
import AppKit
@testable import AppModel
import Browse
import CoreBridge
import Darwin
@testable import Editor
import ImageIO
@testable import ImageOcclusion
import SwiftUI
import Testing
import UniformTypeIdentifiers

/// VM-only app-hosted checks. Mount the production Add/Browse views and use
/// their real hosts, codec, media importer and collection persistence.
@MainActor @Suite(.serialized) struct OcclusionEditorIntegrationTests {
    @Test func mountedAddReplacementResetsCanvasAndHistoryWithoutChangingMasks() async throws {
        try await withOwner { owner, root in
            _ = try #require(await owner.prepareAddNoteEditor())
            await owner.addNoteFlow.selectNotetype(AddNoteFlow.imageOcclusionChoice)
            let host = try #require(owner.addNoteFlow.runtime as? FieldEditorHost)
            let window = mount(AddNoteView(collection: owner))
            defer { window.close() }
            let wide = root.appending(path: "wide.png"), tall = root.appending(path: "tall.png")
            try bitmap(width: 800, height: 200).write(to: wide)
            try bitmap(width: 200, height: 800).write(to: tall)
            host.occlusionImage.chooseFile(wide, host: host)
            await host.occlusionImage.completion?.value
            let session = try #require(host.maskSession), roles = try #require(host.draft.occlusionRoles)
            try await wait(window) { session.referenceSize.width > 0 }
            #expect(abs(session.referenceSize.width / session.referenceSize.height - 4) < 0.02)
            session.addBox(from: .init(x: 10, y: 10), to: .init(x: 60, y: 40))
            #expect(session.canUndo && host.draft.values[roles.occlusions] == session.field)
            let document = session.document, field = session.field
            session.tool = .ellipse
            host.occlusionImage.replacing = true
            host.occlusionImage.chooseFile(tall, host: host)
            await host.occlusionImage.completion?.value
            try await wait(window) {
                session.referenceSize.height > 0 && abs(session.referenceSize.width / session.referenceSize.height - 0.25) < 0.02
            }
            #expect(host.maskSession === session && session.document == document && session.tool == .ellipse)
            #expect(!session.canUndo && !session.canRedo && host.draft.values[roles.occlusions] == field)
            session.undo(); session.redo()
            #expect(host.draft.values[roles.occlusions] == field)
            let added = try #require(await owner.addNoteFlow.save())
            #expect(try await CoreNoteService().readNote(id: added.id).fields[roles.occlusions] == field)
            #expect(await owner.addNoteFlow.requestClose())
        }
    }

    @Test func mountedBrowseLoadsConsecutiveNotesWithSameImageAndFlushesRealMaskEdits() async throws {
        try await withOwner { owner, _ in
            let types = CoreNotetypeService(), notes = CoreNoteService()
            let typeID = try await types.ensureImageOcclusionNotetype().id
            let type = try await types.readNotetype(id: typeID), roles = try #require(OcclusionFieldRoles(notetype: type))
            let image = try await CoreMediaService().addMedia(name: "shared.png", bytes: Array(bitmap(width: 800, height: 400)))
            let source = "{{c1::image-occlusion:rect:left=.1:top=.1:width=.2:height=.2:oi=1}}  <br/>"
            var ids: Set<Int64> = []
            for header in ["shared-image a", "shared-image b"] {
                var fields = Array(repeating: "", count: type.fields.count)
                fields[roles.occlusions] = source; fields[roles.image] = OcclusionImageField.html(name: image)
                fields[roles.header] = header
                ids.insert(try await notes.addNote(.init(notetypeId: typeID, fields: fields, tags: [], deckId: 1,
                    guid: nil, updateAddDefaults: false)).id)
            }
            owner.navigation.select(.section(.browse))
            let browse = owner.browse
            browse.inspector.presented = true
            let window = mount(BrowseView(model: browse))
            defer { window.close() }
            try await wait(window) { browse.opened && browse.results.list != nil }
            browse.setQuery(FilterQuery(text: "shared-image"))
            try await wait(window) { browse.results.list?.count == 2 && browse.results.row(at: 1) != nil }
            browse.results.select([0])
            try await wait(window) { (browse.inspector.note?.runtime as? FieldEditorHost)?.occlusionImage.image != nil }
            let first = try #require(browse.inspector.note?.runtime as? FieldEditorHost)
            let session = try #require(first.maskSession)
            try await wait(window) { session.referenceSize.width > 0 }
            session.addBox(from: .init(x: 10, y: 10), to: .init(x: 50, y: 40))
            let edited = session.field
            #expect(edited != source && first.draft.values[roles.occlusions] == edited)
            browse.results.select([1])
            try await wait(window) {
                guard let next = browse.inspector.note?.runtime as? FieldEditorHost else { return false }
                return next !== first && next.occlusionImage.image != nil && (next.maskSession?.referenceSize.width ?? 0) > 0
            }
            let second = try #require(browse.inspector.note?.runtime as? FieldEditorHost)
            #expect(second.webView === first.webView && second.draft.identity != first.draft.identity)
            #expect(second.draft.values[roles.image] == first.draft.values[roles.image])
            #expect(second.occlusionImage.image?.size == CGSize(width: 800, height: 400))
            #expect(second.draft.values[roles.occlusions] == source)
            #expect(ids == Set([try #require(first.draft.noteID), try #require(second.draft.noteID)]))
            #expect(try await notes.readNote(id: try #require(first.draft.noteID)).fields[roles.occlusions] == edited)
            #expect(try await notes.readNote(id: try #require(second.draft.noteID)).fields[roles.occlusions] == source)
        }
    }

    @Test func mountedCanvasHasNativeImageAndMaskFramesWithoutInterceptingDrawing() async throws {
        try await SnapshotAppLifecycle.shared.withExclusiveAccess {
            let source = "{{c1::image-occlusion:rect:left=.1:top=.2:width=.3:height=.15:oi=1}}  <br/>"
            let session = MaskEditorSession(field: source, isEditing: true)
            let pixels = try #require(NSBitmapImageRep(data: bitmap(width: 960, height: 540)))
            let image = try #require(pixels.cgImage)
            let window = mount(MaskEditorView(image: Image(decorative: image, scale: 1),
                imageSize: .init(width: image.width, height: image.height),
                occlusions: Binding(get: { session.field }, set: { _ in }), isEditing: true, session: session)
                // Match the production visible masks branch, including the
                // ancestor modifiers that used to surround a focusable leaf.
                .allowsHitTesting(true).accessibilityHidden(false))
            defer { window.close() }
            try await wait(window) { session.referenceSize.width > 0 && nativeCanvas(window.contentView) != nil }
            let native = try #require(nativeCanvas(window.contentView))
            #expect(native.hitTest(.init(x: 100, y: 100)) == nil)
            let children = try #require(native.accessibilityChildren() as? [NativeCanvasAccessibility.Element])
            let imageElement = try #require(children.first { $0.identifier == "io.editor.canvas" })
            let mask = try #require(children.first { $0.identifier == "io.mask.1" })
            #expect(imageElement.accessibilityRole() == .image)
            #expect(mask.accessibilityRole() == .button)
            func checkFrames() {
                let imageFrame = imageElement.accessibilityFrame(), maskFrame = mask.accessibilityFrame()
                #expect(abs(imageFrame.width / imageFrame.height - 960.0 / 540) < 0.002)
                #expect(abs((maskFrame.minX - imageFrame.minX) / imageFrame.width - 0.1) < 0.001)
                // AppKit screen coordinates have their origin at the bottom.
                #expect(abs((imageFrame.maxY - maskFrame.maxY) / imageFrame.height - 0.2) < 0.001)
                #expect(abs(maskFrame.width / imageFrame.width - 0.3) < 0.001)
                #expect(abs(maskFrame.height / imageFrame.height - 0.15) < 0.001)
            }
            checkFrames()
            let before = imageElement.accessibilityFrame()
            window.setFrameOrigin(window.frame.origin.applying(.init(translationX: 37, y: 23)))
            #expect(imageElement.accessibilityFrame().minX == before.minX + 37)
            #expect(imageElement.accessibilityFrame().minY == before.minY + 23)
            checkFrames()
            let reference = session.referenceSize
            window.setContentSize(.init(width: 1100, height: 750))
            try await wait(window) { imageElement.accessibilityFrame().size != before.size }
            #expect(session.referenceSize == reference)
            checkFrames()
            let currentFrame = mask.accessibilityFrame()
            let axRead = await Task.detached {
                let point = NSPoint(x: currentFrame.midX, y: currentFrame.midY)
                return (
                    mask.accessibilityFrame(),
                    (mask.accessibilityParent() as? NativeCanvasAccessibility.Container) === native,
                    (native.accessibilityHitTest(point) as? NativeCanvasAccessibility.Element) === mask,
                    (native.accessibilityHitTest(.init(x: -1_000_000, y: -1_000_000))
                        as? NativeCanvasAccessibility.Container) === native
                )
            }.value
            #expect(axRead.0 == currentFrame)
            #expect(axRead.1 && axRead.2 && axRead.3)
            let pressed = await Task.detached { mask.accessibilityPerformPress() }.value
            #expect(pressed)
            #expect(session.selection.count == 1 && session.field == source)
            let incremented = await Task.detached { mask.accessibilityPerformIncrement() }.value
            #expect(incremented)
            #expect(session.field != source)
            #expect(session.document.shapes.count == 1)
        }
    }

    private func nativeCanvas(_ view: NSView?) -> NativeCanvasAccessibility.Container? {
        guard let view else { return nil }
        if let canvas = view as? NativeCanvasAccessibility.Container { return canvas }
        return view.subviews.lazy.compactMap { self.nativeCanvas($0) }.first
    }

    private func mount(_ view: some View) -> NSWindow {
        let window = NSWindow(contentRect: .init(x: 0, y: 0, width: 1400, height: 900),
            styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = NSHostingView(rootView: view)
        window.contentView?.layoutSubtreeIfNeeded()
        return window
    }

    private func wait(_ window: NSWindow?, until condition: () -> Bool) async throws {
        for _ in 0..<1_000 {
            window?.contentView?.layoutSubtreeIfNeeded()
            if condition() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        try #require(condition())
    }

    private func bitmap(width: Int, height: Int) throws -> Data {
        let context = try #require(CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(gray: 0.5, alpha: 1)); context.fill(.init(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)))
        let bytes = NSMutableData()
        let destination = try #require(CGImageDestinationCreateWithData(bytes as CFMutableData, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, try #require(context.makeImage()), nil)
        try #require(CGImageDestinationFinalize(destination))
        return bytes as Data
    }

    private func withOwner(_ body: (ReviewCollection, URL) async throws -> Void) async throws {
        try await SnapshotAppLifecycle.shared.withExclusiveAccess {
            let path = try #require(realpath(FileManager.default.temporaryDirectory.path, nil))
            let root = URL(filePath: String(cString: path)).appending(path: UUID().uuidString)
            free(path)
            defer { try? FileManager.default.removeItem(at: root) }
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "collection"),
                backupsDirectory: root.appending(path: "backups"))
            let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
                providerDirectory: root.appending(path: "transfers"))
            do {
                await owner.start(); await owner.startEmpty()
                try #require(owner.state == .ready)
                if owner.policy != .ok { await owner.enableFsrs() }
                try await body(owner, root)
                #expect(await owner.close())
            } catch {
                owner.addNoteFlow.generationEnded(); owner.editNoteFlow.generationEnded()
                _ = await owner.close()
                throw error
            }
        }
    }
}
#endif
