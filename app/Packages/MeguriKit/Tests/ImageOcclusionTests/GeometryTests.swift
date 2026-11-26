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
}
