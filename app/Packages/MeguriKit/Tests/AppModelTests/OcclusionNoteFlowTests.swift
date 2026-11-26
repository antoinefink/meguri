import CardRendering
import CoreBridge
import CoreGraphics
import Darwin
import Foundation
import ImageIO
@testable import ImageOcclusion
import Observation
import Testing
import UniformTypeIdentifiers
import WebKit
#if os(iOS)
import UIKit
#endif
@testable import AppModel
@testable import Editor

@MainActor
@Suite struct OcclusionFieldFlowTests {
    static let first = "{{c1::image-occlusion:rect:left=.10:top=.20:width=.3:height=.2:oi=1}}"
    static let second = "{{c2::image-occlusion:ellipse:left=.6:top=.5:rx=.1:ry=.1:oi=1}}"
    static let label = "{{c0::image-occlusion:text:left=.2:top=.4:text=label:scale=1:oi=1}}"
    static let deck = DeckNameEntry(id: 1, fullName: "Default", filtered: false)
    static func bitmap(width: Int = 4, height: Int = 2, type: UTType = .png) throws -> Data {
        let context = try #require(CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(gray: 0.5, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)))
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

    @Test func windowTitleUsesHeaderOnlyWhenTheFirstFieldIsHostOwned() throws {
        func title(_ tags: [UInt32?], _ values: [String], stock: Int32 = 6) throws -> String? {
            try NoteEditorDraft(generation: 1, notetype: Self.type(tags: tags, stock: stock),
                deck: Self.deck, values: values).windowTitleHTML
        }
        #expect(try title([0, 1, 2, 3], [Self.first, "image", "<b>Diagram</b>", "extra"]) == "<b>Diagram</b>")
        // Blank Header must reach Edit Note's fallback, never masks or Back Extra.
        #expect(try title([0, 1, 2, 3], [Self.first, "image", "", "extra"]) == "")
        #expect(try title([1, 3, 0, 2], ["image", "extra", Self.first, "Renamed header"]) == "Renamed header")
        #expect(try title([4, 0, 1, 2, 3], ["Custom first", Self.first, "image", "header", "extra"]) == "Custom first")
        #expect(try title([nil, nil], ["Ordinary", "back"], stock: 1) == "Ordinary")
        #expect(try title([0, 1, 2], [Self.first, "image", "header"]) == nil)
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

    @Test func remoteImageEligibilityUsesTheWrittenURLIncludingQuery() throws {
        for text in ["https://example.test/IMAGE.PNG", "https://example.test/download?name=diagram.png"] {
            #expect(OcclusionImageSession.isRemoteImageURL(try #require(URL(string: text))))
        }
        for text in ["https://example.test/diagram.png?token=signed", "https://example.test/diagram.png#fragment"] {
            #expect(!OcclusionImageSession.isRemoteImageURL(try #require(URL(string: text))))
        }
    }

    #if os(iOS)
    @Test func cameraBoundsPixelsBeforePNGAndKeepsFullResolutionWithLimitOff() throws {
        let pixels = try OcclusionImageInput.decodeBitmap(Self.bitmap(width: 1600))
        let image = UIImage(cgImage: pixels, scale: 2, orientation: .up)
        #expect(OcclusionCamera.renderSize(image: image, maximumDimension: 640) == CGSize(width: 640, height: 1))
        #expect(OcclusionCamera.renderSize(image: image, maximumDimension: nil) == CGSize(width: 1600, height: 2))
        #expect(OcclusionCamera.renderSize(image: image, maximumDimension: 2048) == CGSize(width: 1600, height: 2))
    }
    #endif

    @Test func stickyHostFieldsClearAndMappingTargetsOnlyVisibleFields() throws {
        let type = Self.type(sticky: true)
        let draft = try NoteEditorDraft(generation: 1, notetype: type, deck: Self.deck,
            values: [Self.first, "<img src=\"x.png\">", "header", "extra", "comments"], tags: ["kept"])
        let next = try draft.nextAfterAcknowledgedAdd()
        #expect(next.values == ["", "", "header", "extra", "comments"] && next.tags == ["kept"])
        let basicType = Self.type(tags: [nil, nil], stock: 1, id: 205)
        try #require(basicType.id != type.id && basicType.fields.count == 2 && type.fields.count == 5)
        let basic = try NoteEditorDraft(generation: 1, notetype: basicType, deck: Self.deck, values: ["a", "b"])
        let mapped = try basic.replacingSelection(notetype: type, deck: Self.deck)
        #expect(mapped.values == ["", "", "a", "b", ""])
        let reversed = Self.type(tags: [3, 2, 1, 0, 4])
        let remapped = try draft.replacingSelection(notetype: NotetypeRecord(id: 207, name: reversed.name,
            kind: reversed.kind, sortField: 0, css: "", base: [], fields: reversed.fields, templates: [], originalStockKind: 6), deck: Self.deck)
        #expect(remapped.values == ["header", "extra", "<img src=\"x.png\">", Self.first, "comments"])
    }

    @Test func basicToIOExpandsTheDraftAndDeckOnlyRebindingKeepsIt() throws {
        let source = Self.type(tags: [nil, nil], stock: 1, names: ["Front", "Back"], id: 205)
        let target = Self.type(names: ["Masks", "Image", "Header", "Extra", "Comments"], id: 206)
        let basic = try NoteEditorDraft(generation: 1, notetype: source, deck: Self.deck, values: ["a", "b"])
        let mapped = try basic.replacingSelection(notetype: target, deck: Self.deck)
        #expect(mapped.notetype.id == target.id && mapped.fields.count == 5)
        #expect(mapped.values == ["", "", "a", "b", ""])
        let rebound = try mapped.replacingSelection(notetype: target,
            deck: .init(id: 2, fullName: "Other", filtered: false))
        #expect(rebound.values == mapped.values && rebound.deck.id == 2)
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

    @Test func bitmapPreviewFitsAnInjectedLimitWithoutUpscaling() throws {
        let bytes = try Self.bitmap(width: 8, height: 4)
        for (limit, width, height) in [(4, 4, 2), (8, 8, 4), (9, 8, 4)] {
            let pixels = try OcclusionImageInput.decodeBitmap(bytes, maximumDimension: limit)
            #expect(pixels.width == width && pixels.height == height)
        }
    }

    @Test func largePreviewPreservesOriginalFileAndConversionOutput() async throws {
        // A thin synthetic image exercises the production ceiling with only
        // 128 KiB of source pixels, keeping the boundary fixture inexpensive.
        let bytes = try Self.bitmap(width: 8192, height: 4)
        let kept = try await OcclusionImageInput.prepare(bytes, name: "wide.png", asPNG: false)
        #expect(kept.bytes == bytes && kept.name == "wide.png")
        #expect(kept.size == CGSize(width: 4096, height: 2))
        let converted = try await OcclusionImageInput.prepare(bytes, name: nil, asPNG: true)
        #expect(converted.size == kept.size)
        let original = try OcclusionImageInput.decodeBitmap(converted.bytes, maximumDimension: nil)
        #expect(original.width == 8192 && original.height == 4)
    }

    @Test func svgPreviewFitsAnInjectedLimitAndKeepsProportions() async throws {
        let svg = Data("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"4\"><rect width=\"8\" height=\"4\"/></svg>".utf8)
        for (limit, width, height) in [(4, 4, 2), (8, 8, 4), (9, 8, 4)] {
            let pixels = try await OcclusionSVGDecoder.decode(svg, maximumDimension: limit)
            #expect(pixels.width == width && pixels.height == height)
        }
    }
}

// Runtime and actual collection checks join the existing serialized owners.
@Observable @MainActor
private final class OcclusionSourceOwner {
    var draft: UUID?
}

extension FieldEditorTests {
    @Test(arguments: [false, true])
    func pendingImageSourceKeepsMetadataEditsButRejectsChangedImageAndOwner(customRoles: Bool) async throws {
        let type = OcclusionFieldFlowTests.type(tags: customRoles ? [3, 2, 1, 0, 4] : [0, 1, 2, 3, 4])
        let roles = try #require(OcclusionFieldRoles(notetype: type))
        var values = Array(repeating: "", count: type.fields.count)
        values[roles.occlusions] = OcclusionFieldFlowTests.first
        values[roles.image] = "<img src=\"original.png\">"
        let draft = try NoteEditorDraft(editing: .init(id: 7, notetypeId: type.id,
            fields: values, tags: [], guid: "g"), notetype: type,
            deck: OcclusionFieldFlowTests.deck, generation: 1)
        let owner = OcclusionSourceOwner(); owner.draft = draft.identity
        let host = try FieldEditorHost(draft: draft, isCurrent: { owner.draft == draft.identity })
        defer { host.shutDown() }
        try await host.start()
        let session = host.occlusionImage
        #expect(host.acceptsOcclusionImageInput)
        let ticket = try #require(session.reserveSource(host: host))
        let result = AsyncStream<Void>.makeStream()
        defer { result.continuation.finish() }
        let failure = CocoaError(.fileReadCorruptFile)
        session.loadBitmap(ticket: ticket, host: host) {
            for await _ in result.stream { }
            throw failure
        }
        let completion = try #require(session.completion)
        let revision = draft.occlusionImageRevision
        // Each commit invalidates the array observation. Join its callback so
        // the next edit deterministically exercises the re-armed observation.
        for (ord, text) in [(roles.header, "edited header"), (4, "edited comments"),
                            (roles.backExtra, "edited extra"), (roles.header, "new header")] {
            try await draft.accept([.init(ord: ord, html: text, unchanged: false)], ifCurrent: { true })
            let observation = try #require(session.draftObservation)
            await observation.value
            #expect(session.busy && host.acceptsOcclusionImageInput && session.error == nil && draft.values[ord] == text)
            #expect(draft.occlusionImageRevision == revision && draft.values[roles.image] == values[roles.image])
        }
        host.setHostField(OcclusionFieldFlowTests.second, at: roles.occlusions)
        await session.draftObservation?.value
        #expect(session.busy && host.acceptsOcclusionImageInput && session.error == nil)
        result.continuation.finish()
        await completion.value
        #expect(!session.busy && session.error == failure.localizedDescription)
        #expect(draft.values[roles.image] == values[roles.image])

        for reloading in [false, true] {
            let stale = try #require(session.reserveSource(host: host))
            for image in ["<img src=\"changed.png\">", values[roles.image]] {
                if reloading {
                    var stored = draft.values; stored[roles.image] = image
                    _ = draft.reloadUnchanged(stored: stored, tags: draft.tags)
                } else { host.setHostField(image, at: roles.image) }
            }
            let obsoleteObservation = session.draftObservation
            // No actor hop: a completion must reject even an away/back edit
            // before either queued cancellation or snapshot comparison runs.
            session.receiveBitmap(Data(), ticket: stale, host: host)
            session.sourceFailed("stale failure", ticket: stale, host: host)
            #expect(!session.busy && session.completion == nil && session.error == nil)
            #expect(draft.values[roles.image] == values[roles.image])
            let newer = try #require(session.reserveSource(host: host))
            await obsoleteObservation?.value
            #expect(session.busy && session.error == nil)
            session.cancelSource(ticket: newer)
        }

        let departed = try #require(session.reserveSource(host: host))
        owner.draft = UUID(); owner.draft = draft.identity
        session.sourceFailed("departed owner", ticket: departed, host: host)
        #expect(!session.busy && session.error == nil)
        let next = try NoteEditorDraft(generation: 1, notetype: type, deck: OcclusionFieldFlowTests.deck)
        let nextHost = try FieldEditorHost(draft: next, isCurrent: { owner.draft == next.identity })
        defer { nextHost.shutDown() }
        owner.draft = next.identity
        try await nextHost.start()
        let nextTicket = try #require(nextHost.occlusionImage.reserveSource(host: nextHost))
        session.receiveFile(nil, ticket: departed, host: nextHost)
        session.receiveBitmap(Data(), ticket: departed, host: host)
        #expect(nextHost.occlusionImage.busy && !next.hasChanges && session.error == nil)
        nextHost.occlusionImage.cancelSource(ticket: nextTicket)
    }

    @Test func pendingImageSourcesRefuseFlushWithoutLosingAcquisitionOrMarkingTheDraftDirty() async throws {
        for editing in [false, true] {
            let type = OcclusionFieldFlowTests.type()
            let values = [OcclusionFieldFlowTests.first, "<img src=\"original.png\">", "header", "extra", "comments"]
            let draft: NoteEditorDraft
            if editing {
                draft = try NoteEditorDraft(editing: .init(id: 7, notetypeId: type.id, fields: values, tags: [], guid: "g"),
                    notetype: type, deck: OcclusionFieldFlowTests.deck, generation: 1)
            } else {
                draft = try NoteEditorDraft(generation: 1, notetype: type, deck: OcclusionFieldFlowTests.deck, values: values)
            }
            let host = try FieldEditorHost(draft: draft, isCurrent: { true })
            defer { host.shutDown() }
            try await host.start()
            let session = host.occlusionImage
            session.replacing = editing
            let ticket = try #require(session.reserveSource(host: host))
            for hold in [false, true] {
                await #expect(throws: (any Error).self) { try await host.flush(hold: hold) }
                try await host.release() // Save's error path must leave the source alive.
                #expect(session.busy && host.acceptsHostInput && !host.held && !draft.hostFieldsHeld)
                #expect(draft.values == values && !draft.hasChanges)
            }
            session.cancelSource(ticket: ticket)
            #expect(!session.busy && !session.replacing && draft.values == values && !draft.hasChanges)
            try await host.flush(hold: true)
            #expect(host.held)
            try await host.release()
        }
    }

    @Test func pickerCancellationAndRefusedInputReleaseOnlyTheMatchingReservation() async throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: OcclusionFieldFlowTests.type(),
            deck: OcclusionFieldFlowTests.deck, values: ["", "", "", "", ""])
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        defer { host.shutDown() }
        try await host.start()
        let session = host.occlusionImage
        let old = try #require(session.reserveSource(host: host))
        session.cancelSource(ticket: old)
        let newer = try #require(session.reserveSource(host: host))
        session.cancelSource(ticket: old)
        session.receiveFile(nil, ticket: old, host: host)
        session.receiveBitmap(nil, ticket: old, host: host)
        session.sourceFailed("obsolete error", ticket: old, host: host)
        #expect(session.busy && session.error == nil)
        session.receiveBitmap(nil, ticket: newer, host: host)
        #expect(!session.busy && !draft.hasChanges)

        for bitmap in [false, true] {
            let ticket = try #require(session.reserveSource(host: host))
            draft.hostFieldsHeld = true
            if bitmap { session.receiveBitmap(Data(), ticket: ticket, host: host) }
            else { session.receiveFile(nil, ticket: ticket, host: host) }
            #expect(!session.busy && !draft.hasChanges)
            draft.hostFieldsHeld = false
            let current = try #require(session.reserveSource(host: host))
            await Task.yield() // The obsolete observation callback cannot cancel current.
            #expect(session.busy)
            session.cancelSource(ticket: current)
        }
    }

    @Test(arguments: [false, true])
    func unreadableMasksLeaveMetadataEditableAndTheirSourceByteIdentical(browsing: Bool) async throws {
        let type = OcclusionFieldFlowTests.type()
        let malformed = "{{c1::image-occlusion:rect:left=broken}}  <br/>"
        let draft = try NoteEditorDraft(editing: .init(id: 7, notetypeId: type.id,
            fields: [malformed, "<img src=\"missing.png\">", "header", "extra", "comments"], tags: ["kept"], guid: "g"),
            notetype: type, deck: OcclusionFieldFlowTests.deck, generation: 1)
        #expect(throws: (any Error).self) { try occlusionHasNumberedMask(malformed) }
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        host.browsing = browsing
        defer { host.shutDown() }
        try await host.start()
        let masks = try #require(host.maskSession)
        let readingError = try #require(host.occlusionReadingError)
        #expect(!readingError.isEmpty && masks.readingError == readingError && !masks.isReadable)
        #expect(IONoteText.unreadableMasks(readingError, copying: false).contains(readingError))
        #if os(macOS)
        #expect(host.usesOcclusionForm == !browsing && host.maskEditorVisible == !browsing)
        #else
        #expect(!host.usesOcclusionForm && !host.maskEditorVisible)
        #endif
        #expect(draft.showsMaskEditor && !host.acceptsMaskInput)
        #expect(host.acceptsHostInput && !host.acceptsOcclusionImageInput)
        #expect(host.occlusionImage.reserveSource(host: host) == nil)
        masks.setMode(!masks.hideAll); masks.undo(); masks.redo()
        host.drawNewMasks() // Edit must never replace unreadable stored masks.
        #expect(masks.field == malformed && !masks.isDirty && !masks.canUndo && !masks.canRedo)
        #expect(!host.occlusionImage.startsNewMasks && !masks.isReadable)
        try await host.flush(hold: true)
        #expect(draft.patch() == nil && draft.values[0] == malformed)
        try await host.release()
        if !host.usesOcclusionForm { draft.showsMaskEditor = false }
        host.showFields(host.usesOcclusionForm || !host.maskEditorVisible)
        #expect(!host.webView.isHidden && host.acceptsHostInput)
        for (ord, value) in [(2, "edited header"), (3, "edited extra"), (4, "edited comments")] {
            try await host.setMode(.html, ord: ord)
            try #require(try await Self.replaceSource(host, ord: ord, value: value))
        }
        host.setNativeTags(["kept", "edited"])
        try await host.flush(hold: true)
        #expect(draft.patch()?.fields == [2: "edited header", 3: "edited extra", 4: "edited comments"])
        #expect(draft.patch()?.tags == ["kept", "edited"] && draft.tags == ["kept", "edited"])
        #expect(draft.values[0] == malformed && draft.values[1] == "<img src=\"missing.png\">")
        #expect(host.maskSession === masks && masks.field == malformed && !masks.isReadable)
        #expect(host.occlusionReadingError == readingError && !host.acceptsMaskInput)
        #expect(host.maskEditorVisible == host.usesOcclusionForm)
    }
    @Test(arguments: [false, true])
    func hostCachesStrictReadingAcrossUnrelatedEditsAndRefreshesOnActualReload(browsing: Bool) async throws {
        let type = OcclusionFieldFlowTests.type()
        let original = OcclusionFieldFlowTests.first + " \t<br/>"
        let draft = try NoteEditorDraft(editing: .init(id: 7, notetypeId: type.id,
            fields: [original, "", "header", "extra", "comments"], tags: [], guid: "g"),
            notetype: type, deck: OcclusionFieldFlowTests.deck, generation: 1)
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        host.browsing = browsing
        defer { host.shutDown() }
        try await host.start()
        let session = try #require(host.maskSession)
        for _ in 0..<20 {
            #expect(host.maskEditorVisible && host.occlusionReadingError == nil)
            host.refreshMaskSession()
            #expect(host.maskSession === session)
        }
        draft.showsMaskEditor = false
        #expect(host.maskEditorVisible == host.usesOcclusionForm)
        try await host.setMode(.html, ord: 2)
        try #require(try await Self.replaceSource(host, ord: 2, value: "new header"))
        try await host.flush(hold: false)
        #expect(host.maskSession === session && session.field == original)
        let malformed = "bad masks  <br/>"
        let pending = try #require(host.occlusionImage.reserveSource(host: host))
        var stored = draft.values; stored[0] = malformed
        let ords = draft.reloadUnchanged(stored: stored, tags: [])
        try await host.reload(fields: ords)
        #expect(host.maskSession === session && host.occlusionReadingError != nil)
        draft.showsMaskEditor = true
        #expect(host.maskEditorVisible == host.usesOcclusionForm && draft.values[0] == malformed)
        #expect(!session.isReadable && !host.acceptsMaskInput)
        host.occlusionImage.sourceFailed("obsolete source", ticket: pending, host: host)
        #expect(!host.occlusionImage.busy && host.occlusionImage.error == nil && !host.acceptsOcclusionImageInput)
        stored[0] = original
        try await host.reload(fields: draft.reloadUnchanged(stored: stored, tags: []))
        #expect(host.maskEditorVisible && host.occlusionReadingError == nil && session.field == original)
        #expect(session.isReadable)
        #expect(host.acceptsOcclusionImageInput)
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
    @Test(.timeLimit(.minutes(1)), arguments: [false, true]) @MainActor
    func occlusionNormalAddGeneratesCardsAndEditCurrentReopensExactMasksAndMetadata(customRoles: Bool) async throws {
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
        // Warm-up and review each receive their own headless surface. It
        // consumes real Core renders, without an unhosted WebKit page.
        owner.makeReviewSurface = { _ in OcclusionReviewDisplay() }
        do {
            await owner.start(); await owner.startEmpty()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            if customRoles {
                let types = CoreNotetypeService()
                let id = try await types.ensureImageOcclusionNotetype().id
                var layout = NotetypeStructureDraft(try await types.readNotetype(id: id), skipChecks: false)
                // Keep real role tags, moving both hidden roles away from field 0.
                let fields = layout.fields
                layout.fields = [fields[2], fields[3], fields[1], fields[0]] + Array(fields.dropFirst(4))
                _ = try await types.startNotetypeStructureSave(layout).value()
            }
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
            #expect(await flow.save() == nil && imageSession.busy && ioHost.acceptsHostInput)
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
            if customRoles {
                try #require(roles.header == 0 && roles.image == 2 && roles.occlusions == 3)
                let unchanged = draft.values
                #expect(await flow.save() == nil && flow.refusal == .empty && !ioHost.held)
                #expect(draft.values == unchanged && flow.missingClozeConfirmation == nil)
                // O7 keeps A5's refusal for a cloze outside the template's cloze field.
                try await ioHost.setMode(.html, ord: roles.header)
                try #require(try await FieldEditorTests.replaceSource(ioHost, ord: roles.header, value: "{{c9::header}}"))
                #expect(await flow.save() == nil && flow.refusal == .fieldNotCloze && !ioHost.held)
                #expect(draft.values[roles.occlusions] == masks && draft.values[roles.image] == unchanged[roles.image])
            }
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
            let previousMasks = try #require(ioHost.maskSession)
            previousMasks.setReferenceSizeIfNeeded(imageSize: .init(width: 800, height: 200),
                area: .init(width: 800, height: 200), displayScale: 1)
            previousMasks.setMode(false)
            let savedMasks = previousMasks.field
            ioHost.setHostField(savedMasks, at: roles.occlusions)
            #expect(!previousMasks.hideAll && previousMasks.canUndo)
            draft.showsMaskEditor = false
            try await ioHost.setMode(.html, ord: roles.header)
            try #require(try await FieldEditorTests.replaceSource(ioHost, ord: roles.header, value: "<b>Diagram</b>"))
            try await ioHost.setMode(.html, ord: roles.backExtra)
            try #require(try await FieldEditorTests.replaceSource(ioHost, ord: roles.backExtra, value: "<i>Metadata</i>"))
            draft.setTags(["io::flow", "kept"])
            let added = try #require(await flow.save())
            #expect(added.cards.count == 2)
            let note = try await CoreNoteService().readNote(id: added.id)
            #expect(note.fields[roles.occlusions] == savedMasks && note.fields[roles.header] == "<b>Diagram</b>")
            #expect(note.fields[roles.backExtra] == "<i>Metadata</i>" && note.tags == ["io::flow", "kept"])
            // O5 protects the image even when it decodes successfully. Exercise
            // both stored role layouts through the real import entry points.
            do {
                var fields = note.fields
                fields[roles.occlusions] = "{{c1::image-occlusion:rect:left=broken}}  <br/>"
                let protected = try NoteEditorDraft(editing: .init(id: note.id, notetypeId: draft.notetype.id,
                    fields: fields, tags: note.tags, guid: note.guid), notetype: draft.notetype,
                    deck: draft.deck, generation: draft.generation)
                let protectedHost = try FieldEditorHost(draft: protected, isCurrent: { true })
                defer { protectedHost.shutDown() }
                try await protectedHost.start()
                let source = protectedHost.occlusionImage
                await source.load(host: protectedHost)
                try #require(source.image != nil && protectedHost.occlusionReadingError != nil)
                #expect(protectedHost.acceptsHostInput && !protectedHost.acceptsOcclusionImageInput && !protectedHost.acceptsMaskInput)
                let mediaBefore = try await CoreMediaService().trackedNames().names
                #expect(source.reserveSource(host: protectedHost) == nil)
                source.chooseFile(tiff, host: protectedHost)
                source.chooseBitmap(try OcclusionFieldFlowTests.bitmap(), host: protectedHost)
                #expect(!source.chooseProvider([NSItemProvider(object: tiff as NSURL)], host: protectedHost))
                #expect(!source.busy && source.completion == nil && source.error == nil)
                try await protectedHost.setMode(.html, ord: roles.header)
                try #require(try await FieldEditorTests.replaceSource(protectedHost, ord: roles.header, value: "editable metadata"))
                try await protectedHost.flush(hold: false)
                protectedHost.setNativeTags(["edited"])
                #expect(protected.patch()?.fields == [roles.header: "editable metadata"] && protected.patch()?.tags == ["edited"])
                #expect(protected.values[roles.image] == fields[roles.image] && protected.values[roles.occlusions] == fields[roles.occlusions])
                #expect(try await CoreMediaService().trackedNames().names == mediaBefore)

                let copy = try NoteEditorDraft(generation: draft.generation, notetype: draft.notetype,
                    deck: draft.deck, values: fields, tags: note.tags)
                let copyHost = try FieldEditorHost(draft: copy, isCurrent: { true })
                defer { copyHost.shutDown() }
                try await copyHost.start()
                await copyHost.occlusionImage.load(host: copyHost)
                #expect(copyHost.occlusionReadingError != nil && copyHost.acceptsOcclusionImageInput)
                copyHost.drawNewMasks()
                #expect(copyHost.occlusionImage.startsNewMasks && copyHost.acceptsMaskInput)
                let replacement = try #require(copyHost.occlusionImage.reserveSource(host: copyHost))
                copyHost.occlusionImage.cancelSource(ticket: replacement)
                #expect(copy.values == fields)
            }
            let next = try #require(flow.draft)
            #expect(next.values[roles.occlusions].isEmpty && next.values[roles.image].isEmpty && next.showsMaskEditor)
            let nextHost = try #require(flow.runtime as? FieldEditorHost)
            let nextMasks = try #require(nextHost.maskSession)
            #expect(nextMasks !== previousMasks && !nextMasks.hideAll)
            #expect(nextMasks.document.shapes.isEmpty && !nextMasks.canUndo && !nextMasks.canRedo)
            #expect(nextHost.occlusionImage.image == nil && nextMasks.field.isEmpty)
            #expect(await flow.requestClose())
            await owner.startStudy(deckID: draft.deck.id)
            let review = try #require(owner.review)
            let surface = try #require(review.surface as? OcclusionReviewDisplay)
            await review.settled()
            try #require(review.item?.noteId == added.id)
            let card = try #require(review.item?.cardId)
            #expect(surface.visible?.cardID == card && review.canEditNote)
            try #require(await owner.openEditNote())
            let edit = owner.editNoteFlow
            let editDraft = try #require(edit.draft), editHost = try #require(edit.runtime as? FieldEditorHost)
            #expect(editDraft.values == note.fields && editDraft.tags == note.tags && editDraft.showsMaskEditor)
            let pendingImage = try #require(editHost.occlusionImage.reserveSource(host: editHost))
            #expect(await edit.save() == false && edit.isPresented && editHost.occlusionImage.busy)
            editHost.occlusionImage.cancelSource(ticket: pendingImage)
            #expect(!editDraft.hasChanges)
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
            let refreshing = ReviewCoordinatorTests.CancellableGate()
            defer { refreshing.release() }
            let presentations = surface.presented.count
            surface.onRender = { try await refreshing.wait() }
            try #require(await edit.save())
            await refreshing.requested()
            #expect(!review.canEditNote && !edit.canOpen, "The saved card is still being refreshed")
            #expect(!editHost.acceptsHostInput)
            let saved = try await CoreNoteService().readNote(id: added.id)
            #expect(saved.fields[roles.occlusions] == OcclusionFieldFlowTests.first + "<br>" + OcclusionFieldFlowTests.label)
            #expect(saved.fields[roles.backExtra] == note.fields[roles.backExtra] && saved.tags == ["edited"])
            #expect(try await CoreNoteService().noteCards(noteId: added.id).count == 2, "Deleting a mask keeps the generated card")
            surface.onRender = nil; refreshing.release()
            await review.settled()
            try #require(review.phase == .question && review.canEditNote)
            #expect(review.item?.cardId == card && review.shownItem?.cardId == card)
            #expect(surface.presented.count == presentations + 1 && surface.visible?.cardID == card)
            #expect(surface.visible?.question?.contains("Edited metadata") == true)
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

/// The model test confirms the actual refreshed HTML and render key. Native
/// card-page display remains covered by the VM's authoring journeys.
@MainActor private final class OcclusionReviewDisplay: ReviewCoordinatorTests.Surface {
    private var cards: [ReviewRenderKey: RenderedCard] = [:]
    private(set) var visible: RenderedCard?

    override func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws {
        #if os(macOS)
        let platform: CardRenderPlatform = .mac
        #else
        let platform: CardRenderPlatform = .ios
        #endif
        let rendered = try await Core.renderSessionHead(sessionID: sessionID, keys: items.map(\.renderKey), platform: platform)
        guard rendered.count == items.count else { throw BridgeError.internal("Incomplete occlusion review render") }
        cards = Dictionary(uniqueKeysWithValues: zip(items.map(\.renderKey), rendered))
        try await super.render(sessionID: sessionID, items: items)
    }

    override func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws {
        guard let card = cards[item.renderKey], card.cardID == item.cardId else {
            throw BridgeError.internal("Occlusion review has no matching render")
        }
        visible = card
        try await super.present(item: item, inputTime: inputTime)
    }
}
