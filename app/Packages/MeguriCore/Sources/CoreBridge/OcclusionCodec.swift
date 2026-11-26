import Dispatch
import Foundation
import Instrumentation
import Synchronization
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
/// The FFI itself executes on the interactive lane, never on the main actor.
/// This facade is main-actor-only to prevent a caller waiting on its own lane.
@MainActor public final class OcclusionCodec {
    private let original: MeguriCoreBindings.ImageOcclusionDocument
    public let shapes: [OcclusionShapeData]
    public let hideAll: Bool

    public init(field: String) throws(BridgeError) {
        original = try occlusionCall("image_occlusion_parse") {
            try MeguriCoreBindings.imageOcclusionParse(field: field)
        }
        let original = original
        let reading = try occlusionCall("ImageOcclusionDocument.reading") { try original.reading() }
        shapes = reading.shapes.map(OcclusionShapeData.init)
        hideAll = reading.hideAll
    }

    public func write(shapes: [OcclusionShapeData], hideAll: Bool,
                      width: Double, height: Double) throws(BridgeError) -> String {
        let original = original
        return try occlusionCall("image_occlusion_write") {
            try MeguriCoreBindings.imageOcclusionWrite(original: original, shapes: shapes.map(\.ffi),
                                                       hideAll: hideAll, width: width, height: height)
        }
    }

    public func allocate(shapes: [OcclusionShapeData]) throws(BridgeError) -> UInt16 {
        try occlusionCall("image_occlusion_allocate") {
            try MeguriCoreBindings.imageOcclusionAllocate(shapes: shapes.map(\.ffi))
        }
    }

    public func numbers(shapes: [OcclusionShapeData], action: OcclusionNumberAction) throws(BridgeError) -> [OcclusionShapeData] {
        try occlusionCall("image_occlusion_numbers") {
            try MeguriCoreBindings.imageOcclusionNumbers(shapes: shapes.map(\.ffi), action: action.ffi)
                .map(OcclusionShapeData.init)
        }
    }
}

/// Uses the strict reading, including protection against unsupported input.
@MainActor public func occlusionHasNumberedMask(_ field: String) throws -> Bool {
    try OcclusionCodec(field: field).shapes.contains {
        if case .text = $0.kind { return false }
        return $0.number > 0
    }
}

@MainActor private func occlusionCall<T: Sendable>(
    _ name: String, _ body: @escaping @Sendable () throws -> T
) throws(BridgeError) -> T {
    // Lookup deliberately refuses an unregistered export rather than silently
    // mislabelling it as another bridge call. The integrator owns the registry.
    guard let call = BridgeCall.all.first(where: { $0.name == name }) else {
        throw .internal("Missing bridge interval: \(name)")
    }
    let result = OcclusionCallResult<T>()
    InteractiveLane.shared.queue.async {
        let value = InteractiveLane.shared.assumeIsolated { lane in
            Result<T, BridgeError> { () throws(BridgeError) in try lane.call(call, body) }
        }
        result.value.withLock { $0 = value }
        result.finished.signal()
    }
    result.finished.wait()
    guard let value = result.value.withLock({ $0 }) else { throw .internal("Missing occlusion result") }
    return try value.get()
}

private final class OcclusionCallResult<T: Sendable>: Sendable {
    let value = Mutex<Result<T, BridgeError>?>(nil)
    let finished = DispatchSemaphore(value: 0)
}
