import CoreBridge
import CoreGraphics
import Darwin
import Foundation
import ImageIO
import ImageOcclusion
import Testing
import UniformTypeIdentifiers
import WebKit
@testable import AppModel
@testable import Editor

@MainActor
@Suite struct OcclusionFieldFlowTests {
    static let first = "{{c1::image-occlusion:rect:left=.10:top=.20:width=.3:height=.2:oi=1}}"
    static let second = "{{c2::image-occlusion:ellipse:left=.6:top=.5:rx=.1:ry=.1:oi=1}}"
    static let label = "{{c0::image-occlusion:text:left=.2:top=.4:text=label:scale=1:oi=1}}"
    static let deck = DeckNameEntry(id: 1, fullName: "Default", filtered: false)
    static func bitmap(width: Int = 4, type: UTType = .png) throws -> Data {
        let context = try #require(CGContext(data: nil, width: width, height: 2, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(gray: 0.5, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: CGFloat(width), height: 2))
        let bytes = NSMutableData()
        let destination = try #require(CGImageDestinationCreateWithData(bytes as CFMutableData, type.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, try #require(context.makeImage()), nil)
        try #require(CGImageDestinationFinalize(destination))
        return bytes as Data
    }

    static func type(tags: [UInt32?] = [0, 1, 2, 3, 4], stock: Int32 = 6, sticky: Bool = false, names: [String]? = nil, id: Int64 = 206) -> NotetypeRecord {
        let fields = tags.enumerated().map { index, tag in
            NotetypeField(ord: UInt32(index * 2), id: Int64(index + 100), name: names?[index] ?? "Field \(index)", sticky: sticky,
                rtl: false, fontName: "", fontSize: 20, description: "", plainText: false,
                collapsed: false, excludeFromSearch: false, tag: tag)
        }
        return .init(id: id, name: "IO", kind: 1, sortField: 0, css: "", base: [], fields: fields,
                     templates: [], originalStockKind: stock)
    }

    @Test func rolesUseCompleteTagsThenPositionsAndNeverNamesOrStoredOrdinals() throws {
        let reordered = try #require(OcclusionFieldRoles(notetype: Self.type(tags: [4, 3, 2, 1, 0, 0])))
        #expect(reordered.occlusions == 4 && reordered.image == 3 && reordered.header == 2 && reordered.backExtra == 1)
        let partial = try #require(OcclusionFieldRoles(notetype: Self.type(tags: [1, 0, nil, 2])))
        #expect(partial.occlusions == 0 && partial.image == 1 && partial.header == 2 && partial.backExtra == 3)
        #expect(OcclusionFieldRoles(notetype: Self.type(tags: [0, 1, 2])) == nil)
        #expect(OcclusionFieldRoles(notetype: Self.type(stock: 1)) == nil)
    }

    @Test func malformedIOTypeDoesNotHideOnDemandChoiceAndValidIOTypeDoes() async throws {
        let malformed = Self.type(tags: [0, 1, 2])
        let names = [NotetypeName(id: malformed.id, name: "Image Occlusion")]
        let offered = await AddNoteFlow.imageOcclusionChoices(names) { _ in malformed }
        #expect(offered.map(\.name) == ["Image Occlusion", "Image Occlusion+"])
        #expect(offered.last?.id == AddNoteFlow.imageOcclusionChoice)
        let usable = Self.type(tags: [0, 1, 2, 3])
        let storedOnly = await AddNoteFlow.imageOcclusionChoices(names) { _ in usable }
        #expect(storedOnly == names)
    }

    @Test func imageFieldReadsRawLastSourceInFirstMatchingImage() {
        #expect(OcclusionImageField.name(in: "<IMG src=\"no.png\"><img src='no.png'><img data-src = \"first.png\" src=\"a&amp;%20b.PNG\">") == "a&amp;%20b.PNG")
        #expect(OcclusionImageField.name(in: "<imgx src=\"no.png\"><img src=\"\">") == nil)
        #expect(OcclusionImageField.html(name: "a&b c.png") == "<img src=\"a&b c.png\">")
        #expect(OcclusionImageField.isUsableName("a.AVIF"))
        #expect(!OcclusionImageField.isUsableName("a.heic"))
    }

    @Test func stickyHostFieldsClearAndMappingTargetsOnlyVisibleFields() throws {
        let type = Self.type(sticky: true)
        let draft = try NoteEditorDraft(generation: 1, notetype: type, deck: Self.deck,
            values: [Self.first, "<img src=\"x.png\">", "header", "extra", "comments"], tags: ["kept"])
        let next = try draft.nextAfterAcknowledgedAdd()
        #expect(next.values == ["", "", "header", "extra", "comments"] && next.tags == ["kept"])
        let basic = try NoteEditorDraft(generation: 1, notetype: Self.type(tags: [nil, nil], stock: 1, id: 205),
                                        deck: Self.deck, values: ["a", "b"])
        let mapped = try basic.replacingSelection(notetype: type, deck: Self.deck)
        #expect(mapped.values == ["", "", "a", "b", ""])
        let reversed = Self.type(tags: [3, 2, 1, 0, 4])
        let remapped = try draft.replacingSelection(notetype: NotetypeRecord(id: 207, name: reversed.name,
            kind: reversed.kind, sortField: 0, css: "", base: [], fields: reversed.fields, templates: [], originalStockKind: 6), deck: Self.deck)
        #expect(remapped.values == ["header", "extra", "<img src=\"x.png\">", Self.first, "comments"])
    }

    @Test func ioMappingPreservesHiddenRolesButMatchesEveryVisibleFieldByNameThenStoredOrder() throws {
        let source = Self.type(names: ["Masks", "Image", "Header", "Extra", "Comments"])
        let draft = try NoteEditorDraft(generation: 1, notetype: source, deck: Self.deck,
            values: [Self.first, "raw image bytes", "header", "extra", "comments"])
        let fields = Self.type(names: ["Masks", "Image", "Extra", "Header", "Comments"]).fields
        let target = NotetypeRecord(id: 207, name: "Swapped", kind: 1, sortField: 0, css: "", base: [],
            fields: fields, templates: [], originalStockKind: 6)
        #expect(try draft.replacingSelection(notetype: target, deck: Self.deck).values ==
            [Self.first, "raw image bytes", "extra", "header", "comments"])
        // A visible extra field before Header gets the first unmatched value.
        let reordered = Self.type(tags: [0, 1, 4, 2, 3], names: ["Masks", "Image", "New", "Also New", "Last"])
        let ordered = NotetypeRecord(id: 208, name: "Stored order", kind: 1, sortField: 0, css: "", base: [],
            fields: reordered.fields, templates: [], originalStockKind: 6)
        #expect(try draft.replacingSelection(notetype: ordered, deck: Self.deck).values ==
            [Self.first, "raw image bytes", "header", "extra", "comments"])
    }

    @Test func scopedMediaReadRefusesTraversalLinksDirectoriesAndNamedPipes() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let data = Data("public synthetic image bytes".utf8)
        try data.write(to: root.appending(path: "image.png"))
        try FileManager.default.createSymbolicLink(at: root.appending(path: "link.png"), withDestinationURL: root.appending(path: "image.png"))
        try #require(mkfifo(root.appending(path: "pipe.png").path, 0o600) == 0)
        #expect(try MediaFolder.read("image.png", in: root) == data)
        for name in ["../image.png", "link.png", "pipe.png", ".", "..", "a/b.png", "a\\b.png"] {
            #expect(throws: (any Error).self) { try MediaFolder.read(name, in: root) }
        }
    }

    @Test func bitmapEncodingAndFileConversionValidateBeforeImport() async throws {
        let png = try Self.bitmap()
        let kept = try await OcclusionImageInput.prepare(png, name: "Diagram.PNG", asPNG: false)
        #expect(kept.name == "Diagram.PNG" && kept.bytes == png)
        let converted = try await OcclusionImageInput.prepare(png, name: "Diagram.tiff", asPNG: false)
        #expect(converted.name.hasPrefix("paste-") && converted.name.hasSuffix(".jpg"))
        #expect(converted.size == CGSize(width: 4, height: 2) && converted.bytes != png)
        await #expect(throws: (any Error).self) {
            try await OcclusionImageInput.prepare(Data("broken".utf8), name: "Diagram.png", asPNG: false)
        }
        #expect(try !occlusionHasNumberedMask(Self.label))
        #expect(try occlusionHasNumberedMask(Self.first + "<br>" + Self.label))
    }

    @Test func photoSizeLimitOffAndSystemSVGDecoder() async throws {
        let photo = try Self.bitmap(width: 1600)
        let limited = try await OcclusionImageInput.prepare(photo, name: nil, asPNG: true, maximumDimension: 640)
        #expect(limited.size == CGSize(width: 640, height: 1))
        let off = try await OcclusionImageInput.prepare(photo, name: nil, asPNG: true, maximumDimension: nil)
        #expect(off.size == CGSize(width: 1600, height: 2))
        let small = try await OcclusionImageInput.prepare(Self.bitmap(), name: nil, asPNG: true, maximumDimension: 1024)
        #expect(small.size == CGSize(width: 4, height: 2))
        let svg = Data("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"12\"><rect width=\"24\" height=\"12\" fill=\"red\"/></svg>".utf8)
        // Call the fallback directly as well as the actual file preparation:
        // ImageIO support can vary between system releases.
        let system = try await OcclusionSVGDecoder.decode(svg)
        #expect(system.width == 24 && system.height == 12)
        let kept = try await OcclusionImageInput.prepare(svg, name: "diagram.svg", asPNG: false)
        #expect(kept.bytes == svg && kept.name == "diagram.svg" && kept.size == CGSize(width: 24, height: 12))
    }
}

// Runtime and actual collection checks join the existing serialized owners.
extension FieldEditorTests {
    @Test func unreadableMasksLeaveMetadataEditableAndTheirSourceByteIdentical() async throws {
        let type = OcclusionFieldFlowTests.type()
        let malformed = "{{c1::image-occlusion:rect:left=broken}}  <br/>"
        let draft = try NoteEditorDraft(editing: .init(id: 7, notetypeId: type.id,
            fields: [malformed, "<img src=\"missing.png\">", "header", "extra", "comments"], tags: ["kept"], guid: "g"),
            notetype: type, deck: OcclusionFieldFlowTests.deck, generation: 1)
        #expect(throws: (any Error).self) { try occlusionHasNumberedMask(malformed) }
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        defer { host.shutDown() }
        try await host.start()
        try await host.flush(hold: true)
        #expect(draft.patch() == nil && draft.values[0] == malformed)
        try await host.release()
        draft.showsMaskEditor = false
        try await host.setMode(.html, ord: 2)
        try #require(try await Self.replaceSource(host, ord: 2, value: "edited header"))
        try await host.flush(hold: true)
        #expect(draft.patch()?.fields == [2: "edited header"] && draft.values[0] == malformed)
        #expect(draft.tags == ["kept"])
    }
    @Test func hostCachesStrictReadingAcrossUnrelatedEditsAndRefreshesOnActualReload() async throws {
        let type = OcclusionFieldFlowTests.type()
        let original = OcclusionFieldFlowTests.first + " \t<br/>"
        let draft = try NoteEditorDraft(editing: .init(id: 7, notetypeId: type.id,
            fields: [original, "", "header", "extra", "comments"], tags: [], guid: "g"),
            notetype: type, deck: OcclusionFieldFlowTests.deck, generation: 1)
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        defer { host.shutDown() }
        try await host.start()
        let session = try #require(host.maskSession)
        for _ in 0..<20 {
            #expect(host.maskEditorVisible && host.occlusionReadingError == nil)
            host.refreshMaskSession()
            #expect(host.maskSession === session)
        }
        draft.showsMaskEditor = false
        try await host.setMode(.html, ord: 2)
        try #require(try await Self.replaceSource(host, ord: 2, value: "new header"))
        try await host.flush(hold: false)
        #expect(host.maskSession === session && session.field == original)
        let malformed = "bad masks  <br/>"
        var stored = draft.values; stored[0] = malformed
        let ords = draft.reloadUnchanged(stored: stored, tags: [])
        try await host.reload(fields: ords)
        #expect(host.maskSession === session && host.occlusionReadingError != nil)
        draft.showsMaskEditor = true
        #expect(!host.maskEditorVisible && draft.values[0] == malformed)
        stored[0] = original
        try await host.reload(fields: draft.reloadUnchanged(stored: stored, tags: []))
        #expect(host.maskEditorVisible && host.occlusionReadingError == nil && session.field == original)
    }

    @Test func hostMasksSurviveAConversionAlreadyOutstandingInWebKit() async throws {
        let type = OcclusionFieldFlowTests.type(tags: [3, 2, 1, 0, 4])
        let original = OcclusionFieldFlowTests.first + " \t<br/>" + OcclusionFieldFlowTests.second
        let draft = try NoteEditorDraft(editing: .init(id: 7, notetypeId: type.id,
            fields: ["extra", "header", "<img src=\"raw&name.png\">", original, "comments"], tags: ["tag"], guid: "g"),
            notetype: type, deck: OcclusionFieldFlowTests.deck, generation: 1)
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        defer { host.shutDown() }
        try await host.start()
        #expect(try await host.webView.callAsyncJavaScript("""
            return document.getElementById('editor.field.2') === null &&
                document.getElementById('editor.field.3') === null &&
                document.getElementById('editor.field.1') !== null;
            """, contentWorld: .page) as? Bool == true)
        let hold = HeldResult()
        defer { hold.release(); draft.afterConvertingFields = nil }
        try await host.setMode(.html, ord: 1)
        draft.afterConvertingFields = { await hold.suspend() }
        try #require(try await Self.replaceSource(host, ord: 1, value: "<b>new header</b>"))
        let flush = Task { try await host.flush(hold: false) }
        try await hold.wait()
        host.setHostField(OcclusionFieldFlowTests.first, at: 3)
        hold.release(); try await flush.value
        #expect(draft.values[1] == "<b>new header</b>" && draft.values[3] == OcclusionFieldFlowTests.first)
        #expect(draft.values[0] == "extra" && draft.values[4] == "comments" && draft.tags == ["tag"])
        #expect(draft.patch()?.fields == [1: "<b>new header</b>", 3: OcclusionFieldFlowTests.first])
        try await host.flush(hold: true)
        host.setHostField("must be refused", at: 3)
        #expect(draft.values[3] == OcclusionFieldFlowTests.first)
        try await host.release()
    }
}

extension ReviewCollectionStartupTests {
    @Test @MainActor func occlusionNormalAddGeneratesCardsAndEditCurrentReopensExactMasksAndMetadata() async throws {
        let path = try #require(realpath(FileManager.default.temporaryDirectory.path, nil))
        let root = URL(filePath: String(cString: path)).appending(path: UUID().uuidString)
        free(path)
        let directory = root.appending(path: "collection"), backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: backups)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(directory: directory, configuration: nil, defaults: nil), providerDirectory: root.appending(path: "transfers"))
        let surface = ReviewCoordinatorTests.Surface()
        owner.makeReviewSurface = { _ in surface }
        do {
            await owner.start(); await owner.startEmpty()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            let host = try #require(await owner.prepareAddNoteEditor())
            let flow = owner.addNoteFlow
            await flow.selectNotetype(AddNoteFlow.imageOcclusionChoice)
            let ioHost = try #require(flow.runtime as? FieldEditorHost)
            let draft = try #require(flow.draft), roles = try #require(draft.occlusionRoles)
            #expect(!host.acceptsHostInput)
            let typeCount = try await CoreNotetypeService().notetypeNames().count
            #expect(try await CoreNotetypeService().ensureImageOcclusionNotetype().id == draft.notetype.id)
            #expect(try await CoreNotetypeService().notetypeNames().count == typeCount)
            ioHost.setHostField(OcclusionFieldFlowTests.label, at: roles.occlusions)
            #expect(await flow.save() == nil && draft.occlusionAddRefused && !ioHost.held)
            let imageSession = ioHost.occlusionImage
            // Cancelled photo/file representations cannot become a later image.
            let ticket = try #require(imageSession.reserveSource(host: ioHost))
            imageSession.cancel()
            imageSession.receiveBitmap(try OcclusionFieldFlowTests.bitmap(), ticket: ticket, host: ioHost)
            #expect(draft.values[roles.image].isEmpty)
            let namesBefore = try await CoreMediaService().trackedNames().names
            imageSession.chooseBitmap(Data("damaged PNG".utf8), host: ioHost)
            await imageSession.completion?.value
            #expect(draft.values[roles.image].isEmpty && imageSession.error != nil)
            #expect(try await CoreMediaService().trackedNames().names == namesBefore)
            imageSession.chooseBitmap(try OcclusionFieldFlowTests.bitmap(), host: ioHost)
            await imageSession.completion?.value
            let imageName = try #require(OcclusionImageField.name(in: draft.values[roles.image]))
            #expect(imageName.hasPrefix("paste-") && (imageName.hasSuffix(".jpg") || imageName.hasSuffix(".png")))
            #expect(imageSession.image?.size == CGSize(width: 4, height: 2))
            let media = try await CoreMediaService().trackedNames()
            #expect(try MediaFolder.read(imageName, in: media.root).isEmpty == false)
            let masks = OcclusionFieldFlowTests.first + " \t<br/>" + OcclusionFieldFlowTests.second + "<br>" + OcclusionFieldFlowTests.label
            ioHost.setHostField(masks, at: roles.occlusions)
            let oldImage = draft.values[roles.image]
            imageSession.replacing = true
            let cancelledFile = try #require(imageSession.reserveSource(host: ioHost))
            imageSession.receiveFile(nil, ticket: cancelledFile, host: ioHost)
            #expect(draft.values[roles.image] == oldImage && draft.values[roles.occlusions] == masks && !imageSession.replacing)
            let tiff = root.appending(path: "wide.tiff")
            try OcclusionFieldFlowTests.bitmap(width: 8, type: .tiff).write(to: tiff)
            imageSession.chooseFile(tiff, host: ioHost)
            await imageSession.completion?.value
            #expect(imageSession.image?.size == CGSize(width: 8, height: 2))
            #expect(draft.values[roles.occlusions] == masks && draft.values[roles.image] != oldImage)
            #expect(try MediaFolder.read(imageName, in: media.root).isEmpty == false, "Changing images leaves the old file for Check Media")
            draft.showsMaskEditor = false
            try await ioHost.setMode(.html, ord: roles.header)
            try #require(try await FieldEditorTests.replaceSource(ioHost, ord: roles.header, value: "<b>Diagram</b>"))
            try await ioHost.setMode(.html, ord: roles.backExtra)
            try #require(try await FieldEditorTests.replaceSource(ioHost, ord: roles.backExtra, value: "<i>Metadata</i>"))
            draft.setTags(["io::flow", "kept"])
            let added = try #require(await flow.save())
            #expect(added.cards.count == 2)
            let note = try await CoreNoteService().readNote(id: added.id)
            #expect(note.fields[roles.occlusions] == masks && note.fields[roles.header] == "<b>Diagram</b>")
            #expect(note.fields[roles.backExtra] == "<i>Metadata</i>" && note.tags == ["io::flow", "kept"])
            let next = try #require(flow.draft)
            #expect(next.values[roles.occlusions].isEmpty && next.values[roles.image].isEmpty && next.showsMaskEditor)
            #expect(await flow.requestClose())
            await owner.startStudy(deckID: draft.deck.id)
            let review = try #require(owner.review)
            try #require(review.item?.noteId == added.id)
            try #require(await owner.openEditNote())
            let edit = owner.editNoteFlow
            let editDraft = try #require(edit.draft), editHost = try #require(edit.runtime as? FieldEditorHost)
            #expect(editDraft.values == note.fields && editDraft.tags == note.tags && editDraft.showsMaskEditor)
            let before = try await CoreUndoService().peekUndo(scope: .global)
            #expect(await edit.save()) // no completed change: no write/undo step
            #expect(try await CoreUndoService().peekUndo(scope: .global) == before)
            try #require(await owner.openEditNote())
            let changed = try #require(edit.runtime as? FieldEditorHost)
            changed.setHostField(OcclusionFieldFlowTests.first + "<br>" + OcclusionFieldFlowTests.label, at: roles.occlusions)
            changed.draft.showsMaskEditor = false
            try await changed.setMode(.html, ord: roles.header)
            try #require(try await FieldEditorTests.replaceSource(changed, ord: roles.header, value: "Edited metadata"))
            changed.draft.setTags(["edited"])
            #expect(await edit.save())
            #expect(!editHost.acceptsHostInput)
            let saved = try await CoreNoteService().readNote(id: added.id)
            #expect(saved.fields[roles.occlusions] == OcclusionFieldFlowTests.first + "<br>" + OcclusionFieldFlowTests.label)
            #expect(saved.fields[roles.backExtra] == note.fields[roles.backExtra] && saved.tags == ["edited"])
            #expect(try await CoreNoteService().noteCards(noteId: added.id).count == 2, "Deleting a mask keeps the generated card")
            try #require(await owner.openEditNote())
            #expect(edit.draft?.values == saved.fields && edit.draft?.tags == saved.tags)
            #expect(await edit.requestCancel())
            try #require(await owner.close())
            let core = CoreCollectionService()
            try await core.open(.init(collectionDirectory: directory, backupsDirectory: backups,
                appGroupSnapshot: root.appending(path: "widget"), manualTransferDirectory: root.appending(path: "transfers")), randomSeed: 206)
            #expect(try await CoreNoteService().readNote(id: added.id).fields == saved.fields)
            await #expect(throws: (any Error).self) {
                try await CoreNotetypeService().ensureImageOcclusionNotetype(expectedGeneration: draft.generation)
            }
            try await core.close()
        } catch {
            owner.addNoteFlow.generationEnded(); owner.editNoteFlow.generationEnded()
            _ = await owner.close(); try? await CoreCollectionService().close(); throw error
        }
    }
}
