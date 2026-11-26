import CoreBridge
import Foundation
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct GeometryTests {
    @Test func referenceSizeIsPixelRoundedBudgetedAndFixed() {
        let size = MaskEditorSession.referenceSize(imageSize: .init(width: 4000, height: 3000),
                                                   area: .init(width: 800, height: 500), displayScale: 2)
        #expect(size == .init(width: 666.5, height: 500))
        let large = MaskEditorSession.referenceSize(imageSize: .init(width: 8000, height: 8000),
                                                    area: .init(width: 8000, height: 8000), displayScale: 2)
        #expect(large.width * large.height * 4 <= 16_777_216)
        let model = OcclusionDocumentTests().session()
        model.setReferenceSizeIfNeeded(imageSize: .init(width: 4000, height: 3000), area: .init(width: 800, height: 500), displayScale: 2)
        #expect(model.referenceSize == .init(width: 1000, height: 1000))
    }

    @Test func pointerAnchorAndZoomClamps() {
        var viewport = CanvasViewport(reference: .init(width: 1000, height: 1000), area: .init(width: 700, height: 600))
        viewport.fit()
        let pointer = CGPoint(x: 123, y: 456), before = viewport.referencePoint(CGPoint(x: 123, y: 456))
        viewport.magnify(by: pow(0.999, -60), at: pointer)
        #expect(abs(viewport.referencePoint(pointer).x - before.x) < 0.000001)
        #expect(abs(viewport.referencePoint(pointer).y - before.y) < 0.000001)
        viewport.magnify(by: 100, at: pointer); #expect(viewport.zoom == 5)
        viewport.magnify(by: 0.00001, at: pointer); #expect(viewport.zoom == 0.5)
        viewport.area = .init(width: 100, height: 100); viewport.fit()
        #expect(viewport.zoom == 0.1)
    }

    @Test(arguments: [false, true], [1.0, 2.0, 3.0])
    func extremeAspectRatioKeepsBothPixelAxesPositive(portrait: Bool, scale: Double) {
        for area in [CGSize(width: 800, height: 500), CGSize(width: 1_000_000_000, height: 1_000_000_000)] {
            let size = MaskEditorSession.referenceSize(
                imageSize: portrait ? .init(width: 1, height: 1_000_000_000) : .init(width: 1_000_000_000, height: 1),
                area: area, displayScale: scale)
            let pixels = CGSize(width: size.width * scale, height: size.height * scale)
            #expect(pixels.width >= 1 && pixels.height >= 1)
            #expect(pixels.width == floor(pixels.width) && pixels.height == floor(pixels.height))
            #expect(pixels.width * pixels.height <= 16_777_216)
            #expect(portrait ? pixels.width == 1 : pixels.height == 1)
        }
    }

    @Test func anglesSnapNormalizeAndGroupsCannotRotate() {
        #expect(OcclusionShape.snappedAngle(2.9) == 0)
        #expect(OcclusionShape.snappedAngle(357.1) == 0)
        #expect(OcclusionShape.snappedAngle(3.1) == 3.1)
        #expect(OcclusionShape.snappedAngle(-180) == 180)
    }

    @Test func resizeStopsAtFiveAndMoveAllowsFivePointOverhang() throws {
        let model = OcclusionDocumentTests().session()
        model.addBox(from: .init(x: 100, y: 100), to: .init(x: 200, y: 200))
        model.commit(model.resized(handle: .bottomRight, delta: .init(width: -200, height: -200), finishing: true))
        let shape = try #require(model.document.shapes.first)
        #expect(abs(shape.unrotatedBounds(in: model.referenceSize).width - 5) < 0.000001)
        model.commit(model.moved(dx: 5000, dy: 0, finishing: true))
        #expect(abs(model.document.shapes[0].bounds(in: model.referenceSize).maxX - 1005) < 0.000001)
        #expect(model.document.shapes[0].data.number == 1)
    }

    @Test func groupScalesUniformlyWithAnglesKept() {
        let model = OcclusionDocumentTests().session()
        model.add(.init(number: 1, left: 0.1, top: 0.1, angle: 30, kind: .rect(width: 0.1, height: 0.1)))
        model.add(.init(number: 2, left: 0.3, top: 0.3, kind: .ellipse(rx: 0.05, ry: 0.05)))
        model.selectAll(); model.group()
        let box = model.document.bounds(of: model.selection, in: model.referenceSize)!
        model.commit(model.resized(handle: .bottomRight, delta: .init(width: box.width, height: box.height), finishing: true))
        #expect(model.document.shapes[0].data.kind == .rect(width: 0.2, height: 0.2))
        #expect(model.document.shapes[0].data.angle == 30)
        #expect(model.document.shapes[1].data.kind == .ellipse(rx: 0.1, ry: 0.1))
        #expect(!model.canRotate)
    }

    @Test func polygonFirstMoveKeepsDrawnPositionAndNumber() {
        let source = "{{c6::image-occlusion:polygon:left=.19:top=.2:points=.2,.2 .4,.2 .3,.4:oi=1}}"
        let model = OcclusionDocumentTests().session(source, editing: true)
        model.selectAll()
        let before = model.document.shapes[0].bounds(in: model.referenceSize)
        model.commit(model.moved(dx: 10, dy: 20, finishing: true))
        let shape = model.document.shapes[0]
        #expect(abs(shape.bounds(in: model.referenceSize).minX - before.minX - 10) < 0.000001)
        #expect(abs(shape.bounds(in: model.referenceSize).minY - before.minY - 20) < 0.000001)
        #expect(shape.data.number == 6)
        if case let .polygon(points) = shape.data.kind {
            #expect(shape.data.left == points.map(\.x).min())
            #expect(shape.data.top == points.map(\.y).min())
        } else { Issue.record("Expected polygon") }
    }

    @Test func sixAlignmentsUseDrawnBoundingBoxes() {
        for alignment in MaskAlignment.allCases {
            let model = OcclusionDocumentTests().session()
            model.add(.init(number: 1, left: 0.1, top: 0.2, angle: 30, kind: .rect(width: 0.1, height: 0.2)))
            model.selectAll(); model.align(alignment)
            let box = model.document.shapes[0].bounds(in: model.referenceSize)
            let coordinate: Double
            let expected: Double
            switch alignment {
            case .left: coordinate = box.minX; expected = 0
            case .center: coordinate = box.midX; expected = 500
            case .right: coordinate = box.maxX; expected = 1000
            case .top: coordinate = box.minY; expected = 0
            case .middle: coordinate = box.midY; expected = 500
            case .bottom: coordinate = box.maxY; expected = 1000
            }
            #expect(abs(coordinate - expected) < 0.000001)
        }
    }
    @Test func fittedCanvasExcludesLetterboxingInBothOrientations() {
        var landscape = CanvasViewport(reference: .init(width: 800, height: 400), area: .init(width: 1000, height: 800))
        landscape.fit()
        #expect(landscape.imageRect == CGRect(x: 0, y: 150, width: 1000, height: 500))
        #expect(landscape.referencePoint(.init(x: 0, y: 150)) == .zero)
        #expect(landscape.referencePoint(.init(x: 1000, y: 650)) == .init(x: 800, y: 400))
        #expect(landscape.referencePoint(.init(x: 500, y: 100)).y < 0)
        landscape.area = .init(width: 800, height: 1000); landscape.fit()
        #expect(landscape.reference == .init(width: 800, height: 400))
        #expect(landscape.imageRect == CGRect(x: 0, y: 300, width: 800, height: 400))

        var portrait = CanvasViewport(reference: .init(width: 400, height: 800), area: .init(width: 1000, height: 800))
        portrait.fit()
        #expect(portrait.imageRect == CGRect(x: 300, y: 0, width: 400, height: 800))
        let point = CGPoint(x: 425, y: 675)
        portrait.magnify(by: 1.5, at: point); portrait.pan = .init(width: 10, height: -20)
        let imagePoint = portrait.referencePoint(point)
        #expect(abs(portrait.screenPoint(imagePoint).x - point.x) < 0.000001)
        #expect(abs(portrait.screenPoint(imagePoint).y - point.y) < 0.000001)
        portrait.area = .zero; portrait.fit()
        #expect(portrait.zoom > 0 && portrait.zoom.isFinite)
    }

    @Test func accessibilityItemsKeepNumberedGroupsAndSeparateLabelsAndZero() throws {
        let group = OcclusionShape(.init(number: 3, left: 0.1, top: 0.1, kind: .rect(width: 0.2, height: 0.1)))
        let member = OcclusionShape(.init(number: 3, left: 0.5, top: 0.4, kind: .ellipse(rx: 0.1, ry: 0.1)))
        let label = OcclusionShape(.init(number: 3, left: 0.1, top: 0.5, kind: .text(text: "Label", scale: 1, fs: 0.04)))
        let zero = OcclusionShape(.init(number: 0, left: 0.2, top: 0.3, kind: .rect(width: 0.1, height: 0.1)))
        let otherZero = OcclusionShape(.init(number: 0, left: 0.3, top: 0.4, kind: .rect(width: 0.1, height: 0.1)))
        var document = OcclusionDocument(shapes: [group, member, label, zero, otherZero])
        var viewport = CanvasViewport(reference: .init(width: 800, height: 400), area: .init(width: 1000, height: 800))
        viewport.fit()
        let items = document.accessibilityItems(in: viewport)
        #expect(items.count == 4)
        #expect(Set(items.map(\.identifier)).count == 4)
        let first = try #require(items.first)
        #expect(first.identifier == "io.mask.3")
        #expect(first.ids == [group.id, member.id])
        #expect(first.frame == CGRect(x: 100, y: 200, width: 600, height: 250))
        #expect(items[1].identifier == "io.label.\(label.id.uuidString)")
        #expect(items[2].identifier == "io.mask.zero.\(zero.id.uuidString)")
        viewport.magnify(by: 1.2, at: .init(x: 500, y: 400))
        viewport.pan = .init(width: 30, height: 20)
        let zoomed = document.accessibilityItems(in: viewport)
        #expect(zoomed.map(\.identifier) == items.map(\.identifier))
        #expect(zoomed[0].frame == CGRect(x: 50, y: 180, width: 720, height: 300))
        // A production drag preview uses the same IDs and current drawn bounds.
        document.shapes[0].move(dx: 0.1, dy: 0)
        #expect(document.accessibilityItems(in: viewport).map(\.identifier) == items.map(\.identifier))
        #expect(document.accessibilityItems(in: viewport)[0].frame.minX == 170)
    }

}
