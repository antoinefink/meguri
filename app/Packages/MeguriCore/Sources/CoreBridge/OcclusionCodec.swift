import Dispatch
import Foundation
import Instrumentation
internal import MeguriCoreBindings

/// Image-normalized geometry. Source indexes and unknown properties must survive
/// every edit; only a newly drawn or duplicated shape has no source index.
public struct OcclusionPoint: Sendable, Equatable {
    public var x: Double
    public var y: Double
    public init(x: Double, y: Double) { self.x = x; self.y = y }
}

public struct OcclusionProperty: Sendable, Equatable {
    public var key: String
    public var value: String
    public init(key: String, value: String) { self.key = key; self.value = value }
}

public enum OcclusionKind: Sendable, Equatable {
    case rect(width: Double, height: Double)
    case ellipse(rx: Double, ry: Double)
    case polygon(points: [OcclusionPoint])
    case text(text: String, scale: Double, fs: Double?)
}

public struct OcclusionShapeData: Sendable, Equatable {
    public var number: UInt16
    public var left: Double
    public var top: Double
    public var angle: Double
    public var fill: String?
    public var kind: OcclusionKind
    public var unknownProperties: [OcclusionProperty]
    public var sourceIndex: UInt32?

    public init(number: UInt16, left: Double, top: Double, angle: Double = 0,
                fill: String? = nil, kind: OcclusionKind,
                unknownProperties: [OcclusionProperty] = [], sourceIndex: UInt32? = nil) {
        self.number = number; self.left = left; self.top = top; self.angle = angle
        self.fill = fill; self.kind = kind; self.unknownProperties = unknownProperties
        self.sourceIndex = sourceIndex
    }

    fileprivate init(_ value: MeguriCoreBindings.IoShape) {
        number = value.number; left = value.left; top = value.top; angle = value.angle
        fill = value.fill; sourceIndex = value.sourceIndex
        unknownProperties = value.unknownProperties.map { .init(key: $0.key, value: $0.value) }
        switch value.kind {
        case let .rect(width, height): kind = .rect(width: width, height: height)
        case let .ellipse(rx, ry): kind = .ellipse(rx: rx, ry: ry)
        case let .polygon(points): kind = .polygon(points: points.map { .init(x: $0.x, y: $0.y) })
        case let .text(text, scale, fs): kind = .text(text: text, scale: scale, fs: fs)
        }
    }

    fileprivate var ffi: MeguriCoreBindings.IoShape {
        let kind: MeguriCoreBindings.IoShapeKind
        switch self.kind {
        case let .rect(width, height): kind = .rect(width: width, height: height)
        case let .ellipse(rx, ry): kind = .ellipse(rx: rx, ry: ry)
        case let .polygon(points): kind = .polygon(points: points.map { .init(x: $0.x, y: $0.y) })
        case let .text(text, scale, fs): kind = .text(text: text, scale: scale, fs: fs)
        }
        return .init(number: number, left: left, top: top, angle: angle, fill: fill, kind: kind,
                     unknownProperties: unknownProperties.map { .init(key: $0.key, value: $0.value) },
                     sourceIndex: sourceIndex)
    }
}

public enum OcclusionNumberAction: Sendable {
    case group(selected: [UInt32])
    case ungroup(number: UInt16)
    case duplicate(selected: [UInt32])

    fileprivate var ffi: MeguriCoreBindings.IoNumberAction {
        switch self {
        case let .group(selected): .group(selected: selected)
        case let .ungroup(number): .ungroup(number: number)
        case let .duplicate(selected): .duplicate(selected: selected)
        }
    }
}

/// Synchronous at the edit boundary so a host Save cannot overtake a mask write.
/// Pure document calls use a dedicated serial executor, never the collection
/// queue or the main actor. Main-actor admission preserves completed-edit order.
@MainActor public final class OcclusionCodec {
    private let original: MeguriCoreBindings.ImageOcclusionDocument
    public let shapes: [OcclusionShapeData]
    public let hideAll: Bool

    public init(field: String) throws(BridgeError) {
        original = try occlusionCall(.imageOcclusionParse) {
            try MeguriCoreBindings.imageOcclusionParse(field: field)
        }
        let original = original
        let reading = try occlusionCall(.imageOcclusionReading) { try original.reading() }
        shapes = reading.shapes.map(OcclusionShapeData.init)
        hideAll = reading.hideAll
    }

    public func write(shapes: [OcclusionShapeData], hideAll: Bool,
                      width: Double, height: Double) throws(BridgeError) -> String {
        let original = original
        return try occlusionCall(.imageOcclusionWrite) {
            try MeguriCoreBindings.imageOcclusionWrite(original: original, shapes: shapes.map(\.ffi),
                                                       hideAll: hideAll, width: width, height: height)
        }
    }

    public func allocate(shapes: [OcclusionShapeData]) throws(BridgeError) -> UInt16 {
        try occlusionCall(.imageOcclusionAllocate) {
            try MeguriCoreBindings.imageOcclusionAllocate(shapes: shapes.map(\.ffi))
        }
    }

    public func numbers(shapes: [OcclusionShapeData], action: OcclusionNumberAction) throws(BridgeError) -> [OcclusionShapeData] {
        try occlusionCall(.imageOcclusionNumbers) {
            try MeguriCoreBindings.imageOcclusionNumbers(shapes: shapes.map(\.ffi), action: action.ffi)
                .map(OcclusionShapeData.init)
        }
    }
}

/// Isolated from collection operations: parsing a document must not wait for a
/// review/render/database call. Only this synchronous main-actor facade admits
/// work, and every call finishes before the host can save the resulting field.
private actor OcclusionLane: CoreLane {
    static let lane = Lane.interactive
    static let shared = OcclusionLane()
    nonisolated let queue = DispatchSerialQueue(label: "com.antoinefink.meguri.core.occlusion", qos: .userInteractive)
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
}

@MainActor private func occlusionCall<T: Sendable>(
    _ call: BridgeCall, _ body: @Sendable () throws -> T
) throws(BridgeError) -> T {
    let lane = OcclusionLane.shared
    // sync has no semaphore/result storage and propagates the mapped error.
    // Returning a typed Result retains typed throws across Dispatch's rethrows.
    return try lane.queue.sync {
        lane.assumeIsolated { executor in
            Result<T, BridgeError> { () throws(BridgeError) in try executor.call(call, body) }
        }
    }.get()
}

/// Uses the strict reading, including protection against unsupported input.
@MainActor public func occlusionHasNumberedMask(_ field: String) throws -> Bool {
    try OcclusionCodec(field: field).shapes.contains {
        if case .text = $0.kind { return false }
        return $0.number > 0
    }
}
