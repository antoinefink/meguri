import CoreGraphics
import Foundation

// The snapshot matrix (R1971, R1972): its axes, how each value is applied on
// each platform, and the cell sets drawn from it. Spike S5
// (docs/design/decisions/s5-simulator-settings.md) decides which
// values a simulator can show: system-setting values are set on the leased
// simulator by `tools/mg snapshots` before a test run (one run per settings
// group), device-only values are deferred to the monthly device session.

/// Where a snapshot is rendered: the simulator kind it needs, or the Mac.
enum SnapshotPlatform: String, CaseIterable, Codable, Sendable {
    case iphone, ipad, mac

    /// The platform this test process renders.
    static var current: SnapshotPlatform {
        #if os(macOS)
        .mac
        #else
        ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"]?.hasPrefix("iPad") == true ? .ipad : .iphone
        #endif
    }
}

enum Appearance: String, CaseIterable, Codable, Sendable { case light, dark }

/// Dynamic Type: the default, XXXL and the two largest accessibility sizes.
enum TextSize: String, CaseIterable, Codable, Sendable { case standard, xxxLarge, ax4, ax5 }

/// The Liquid Glass look.
enum GlassLook: String, CaseIterable, Codable, Sendable { case clear, tinted }

/// The Liquid Glass opacity setting: the system default and both extremes.
enum Transparency: String, CaseIterable, Codable, Sendable {
    case standard, minimum, maximum

    /// The value `devicectl device settings appearance --liquid-glass-opacity` takes.
    var opacity: Double {
        switch self {
        case .standard: 0.5
        case .minimum: 0
        case .maximum: 1
        }
    }
}

/// Device classes (R1971). Foldable cells stay deferred until a foldable
/// simulator exists (U252); the other iOS classes render on the pool's
/// simulator of their kind, at their size, with their size classes.
enum DeviceClass: String, CaseIterable, Codable, Sendable {
    case iphoneSmall, iphoneLarge, iphoneResizable, foldableOuter, foldableInner
    case ipadCompact, ipadRegular
    case macMinimum, macDefault, macLarge

    var platform: SnapshotPlatform {
        switch self {
        case .iphoneSmall, .iphoneLarge, .iphoneResizable, .foldableOuter, .foldableInner: .iphone
        case .ipadCompact, .ipadRegular: .ipad
        case .macMinimum, .macDefault, .macLarge: .mac
        }
    }

    /// The screen or window size in points.
    var size: CGSize {
        switch self {
        case .iphoneSmall: CGSize(width: 390, height: 844)
        case .iphoneLarge: CGSize(width: 440, height: 956)
        case .iphoneResizable: CGSize(width: 540, height: 720)
        case .foldableOuter: CGSize(width: 390, height: 844)
        case .foldableInner: CGSize(width: 720, height: 860)
        case .ipadCompact: CGSize(width: 507, height: 1376)
        case .ipadRegular: CGSize(width: 1032, height: 1376)
        case .macMinimum: CGSize(width: 800, height: 560)
        case .macDefault: CGSize(width: 1180, height: 780)
        case .macLarge: CGSize(width: 1600, height: 1000)
        }
    }

    /// Safe-area insets of a full-screen window of this class (status bar and
    /// home indicator on iPhone and iPad; none in a Mac window).
    var safeArea: (top: CGFloat, bottom: CGFloat) {
        switch self {
        case .iphoneSmall, .foldableOuter: (47, 34)
        case .iphoneLarge: (62, 34)
        case .iphoneResizable: (24, 20)
        case .foldableInner: (32, 20)
        case .ipadCompact, .ipadRegular: (24, 20)
        case .macMinimum, .macDefault, .macLarge: (0, 0)
        }
    }

    var horizontalSizeClassIsRegular: Bool {
        switch self {
        case .ipadRegular, .foldableInner, .macMinimum, .macDefault, .macLarge: true
        default: false
        }
    }

    /// Why no simulator can show this class, if none can.
    var deferredReason: String? {
        switch self {
        case .foldableOuter, .foldableInner: "no foldable iPhone simulator before the 27.1 SDK (U252)"
        default: nil
        }
    }
}

/// How one axis value reaches the render on a platform.
enum Application: String, Codable, Sendable {
    /// Trait, environment or size override inside the test process.
    case inProcess
    /// A simulator setting `tools/mg snapshots` applies before the run
    /// (system chrome reads the setting, not the environment; spike S5).
    case systemSetting
    /// No simulator shows it: the cell is deferred to the device session.
    case deviceOnly
}

/// The matrix axes, in a fixed order (the pairwise reducer and cell names
/// depend on it).
enum MatrixAxis: String, CaseIterable, Codable, Sendable {
    case appearance, textSize, increaseContrast, reduceTransparency, reduceMotion, glassLook, transparency, device
}

/// One combination of axis values.
struct MatrixCell: Hashable, Codable, Sendable {
    var appearance: Appearance = .light
    var textSize: TextSize = .standard
    var increaseContrast = false
    var reduceTransparency = false
    var reduceMotion = false
    var glassLook: GlassLook = .clear
    var transparency: Transparency = .standard
    var device: DeviceClass

    /// The file name stem: appearance, text size and device always, then
    /// each value that differs from the default, e.g.
    /// `dark-ax5-iphoneSmall+hc+rt+tmax`.
    var name: String {
        var name = "\(appearance.rawValue)-\(textSize.rawValue)-\(device.rawValue)"
        if increaseContrast { name += "+hc" }
        if reduceTransparency { name += "+rt" }
        if reduceMotion { name += "+rm" }
        if glassLook == .tinted { name += "+tinted" }
        if transparency == .minimum { name += "+tmin" }
        if transparency == .maximum { name += "+tmax" }
        return name
    }

    /// The simulator settings a run must have to render this cell
    /// (`rt0-rm0-t50` style); every cell of one test run shares it. The Mac
    /// and in-process values have no group of their own.
    var systemGroup: String {
        guard device.platform != .mac else { return "mac" }
        return "rt\(reduceTransparency ? 1 : 0)-rm\(reduceMotion ? 1 : 0)-t\(Int(transparency.opacity * 100))"
    }

    /// Why this cell is deferred to the device session, if it is.
    var deferredReason: String? {
        if let reason = device.deferredReason { return reason }
        for axis in MatrixAxis.allCases where SnapshotMatrix.application(of: axis, in: self) == .deviceOnly {
            return "\(axis.rawValue) \(value(of: axis)) cannot be set on a simulator (spike S5); captured at the device session"
        }
        return nil
    }

    func value(of axis: MatrixAxis) -> String {
        switch axis {
        case .appearance: appearance.rawValue
        case .textSize: textSize.rawValue
        case .increaseContrast: increaseContrast ? "on" : "off"
        case .reduceTransparency: reduceTransparency ? "on" : "off"
        case .reduceMotion: reduceMotion ? "on" : "off"
        case .glassLook: glassLook.rawValue
        case .transparency: transparency.rawValue
        case .device: device.rawValue
        }
    }
}

/// Which cells a subject is snapshotted in.
enum MatrixSet: String, Codable, Sendable {
    /// Light and dark at the default and the largest text size (the Mac has
    /// one text size) on the platform's reference device class: the T0
    /// subset (U252).
    case keyVariants
    /// A pairwise covering set (R1972, non-signature screens).
    case pairwise
    /// Every combination (signature screens use U252's counted covering set
    /// drawn from it).
    case full
}

enum SnapshotMatrix {
    /// The seed of the pairwise reducer; changing it reshuffles every
    /// non-signature screen's cells, so keep it.
    static let pairwiseSeed: UInt64 = 0x6D65_6775_7269_0001

    /// The values of each axis on a platform, in axis order.
    static func values(of axis: MatrixAxis, on platform: SnapshotPlatform) -> [String] {
        switch axis {
        case .appearance: Appearance.allCases.map(\.rawValue)
        // The Mac has no Dynamic Type: SwiftUI there ignores the text size.
        case .textSize: platform == .mac ? [TextSize.standard.rawValue] : TextSize.allCases.map(\.rawValue)
        case .increaseContrast, .reduceTransparency, .reduceMotion: ["off", "on"]
        case .glassLook: GlassLook.allCases.map(\.rawValue)
        case .transparency: Transparency.allCases.map(\.rawValue)
        case .device: DeviceClass.allCases.filter { $0.platform == platform }.map(\.rawValue)
        }
    }

    /// How `axis`'s value in `cell` is applied on the cell's platform (S5).
    static func application(of axis: MatrixAxis, in cell: MatrixCell) -> Application {
        let platform = cell.device.platform
        switch axis {
        case .appearance, .textSize, .increaseContrast:
            return .inProcess
        case .reduceTransparency, .reduceMotion:
            // On the Mac the user's own settings are never changed: the
            // SwiftUI environment carries these, and system chrome ignores it.
            return platform == .mac ? .inProcess : .systemSetting
        case .glassLook:
            // Simulators offer one look only (S5); the default renders as is.
            return cell.glassLook == .clear ? .inProcess : .deviceOnly
        case .transparency:
            if cell.transparency == .standard { return .inProcess }
            return platform == .mac ? .deviceOnly : .systemSetting
        case .device:
            return cell.device.deferredReason == nil ? .inProcess : .deviceOnly
        }
    }

    /// The reference device class of a platform (key variants, components).
    static func referenceDevice(_ platform: SnapshotPlatform) -> DeviceClass {
        switch platform {
        case .iphone: .iphoneLarge
        case .ipad: .ipadRegular
        case .mac: .macDefault
        }
    }

    static func cells(_ set: MatrixSet, on platform: SnapshotPlatform) -> [MatrixCell] {
        switch set {
        case .keyVariants:
            Appearance.allCases.flatMap { appearance in
                (platform == .mac ? [TextSize.standard] : [.standard, .ax5]).map {
                    MatrixCell(appearance: appearance, textSize: $0, device: referenceDevice(platform))
                }
            }
        case .pairwise:
            pairwiseCells(on: platform)
        case .full:
            product(on: platform)
        }
    }

    /// Every combination of every axis value on the platform.
    static func product(on platform: SnapshotPlatform) -> [MatrixCell] {
        let domains = MatrixAxis.allCases.map { values(of: $0, on: platform) }
        var rows: [[Int]] = [[]]
        for domain in domains {
            rows = rows.flatMap { row in domain.indices.map { row + [$0] } }
        }
        return rows.map { cell(fromIndices: $0, on: platform) }
    }

    /// A pairwise covering set. Pairs of values a simulator can show are
    /// covered by cells that render; each device-only value then gets the
    /// rendered cells again with that value in place, deferred, so its pairs
    /// are listed for the device session without taking a rendered pair's
    /// place.
    static func pairwiseCells(on platform: SnapshotPlatform) -> [MatrixCell] {
        let axes = MatrixAxis.allCases
        let all = axes.map { values(of: $0, on: platform) }
        // Values a simulator (or the Mac) can show, per axis.
        let capable = axes.indices.map { a in
            all[a].filter { value in
                var probe = cell(fromIndices: axes.indices.map { _ in 0 }, on: platform)
                probe = setting(axes[a], to: value, in: probe)
                return application(of: axes[a], in: probe) != .deviceOnly
            }
        }
        let rows = Pairwise.cover(capable.map(\.count), seed: pairwiseSeed)
        let rendered = rows.map { row in
            var result = cell(fromIndices: axes.indices.map { _ in 0 }, on: platform)
            for (a, index) in row.enumerated() {
                result = setting(axes[a], to: capable[a][index], in: result)
            }
            return result
        }
        var cells = rendered
        var seen = Set(cells)
        func add(_ cell: MatrixCell) {
            if seen.insert(cell).inserted { cells.append(cell) }
        }
        let deviceOnly = axes.indices.flatMap { a in all[a].filter { !capable[a].contains($0) }.map { (a, $0) } }
        for (a, value) in deviceOnly {
            for base in rendered { add(setting(axes[a], to: value, in: base)) }
        }
        // Two device-only values of different axes: one cell holds both.
        for (i, (a, first)) in deviceOnly.enumerated() {
            for (b, second) in deviceOnly[(i + 1)...] where b != a {
                add(setting(axes[b], to: second, in: setting(axes[a], to: first, in: rendered[0])))
            }
        }
        return cells
    }

    private static func cell(fromIndices indices: [Int], on platform: SnapshotPlatform) -> MatrixCell {
        var result = MatrixCell(device: referenceDevice(platform))
        for (a, axis) in MatrixAxis.allCases.enumerated() {
            result = setting(axis, to: values(of: axis, on: platform)[indices[a]], in: result)
        }
        return result
    }

    private static func setting(_ axis: MatrixAxis, to value: String, in cell: MatrixCell) -> MatrixCell {
        var cell = cell
        switch axis {
        case .appearance: cell.appearance = Appearance(rawValue: value)!
        case .textSize: cell.textSize = TextSize(rawValue: value)!
        case .increaseContrast: cell.increaseContrast = value == "on"
        case .reduceTransparency: cell.reduceTransparency = value == "on"
        case .reduceMotion: cell.reduceMotion = value == "on"
        case .glassLook: cell.glassLook = GlassLook(rawValue: value)!
        case .transparency: cell.transparency = Transparency(rawValue: value)!
        case .device: cell.device = DeviceClass(rawValue: value)!
        }
        return cell
    }
}

/// A deterministic pairwise (2-way) covering array.
enum Pairwise {
    /// Rows of value indices, one per axis, such that every pair of values
    /// of every two axes appears in at least one row. Greedy: each row starts
    /// from the first uncovered pair in a fixed scan order and fills the
    /// other axes, in an order drawn from `seed`, with the value covering
    /// the most uncovered pairs (ties drawn from `seed`); of a few candidate
    /// rows the one covering most pairs wins. Same domains and seed, same
    /// rows: nothing depends on hashing or the clock.
    static func cover(_ domains: [Int], seed: UInt64, candidates: Int = 16) -> [[Int]] {
        precondition(domains.allSatisfy { $0 > 0 }, "every axis needs at least one value")
        let n = domains.count
        guard n > 1 else { return (0..<(domains.first ?? 0)).map { [$0] } }
        // covered[i][j] for i < j: flat a * domains[j] + b.
        var covered = (0..<n).map { i in (0..<n).map { j in [Bool](repeating: j <= i, count: j > i ? domains[i] * domains[j] : 0) } }
        var remaining = 0
        for i in 0..<n { for j in (i + 1)..<n { remaining += domains[i] * domains[j] } }
        var random = SplitMix64(seed: seed)

        func isCovered(_ i: Int, _ a: Int, _ j: Int, _ b: Int) -> Bool {
            i < j ? covered[i][j][a * domains[j] + b] : covered[j][i][b * domains[i] + a]
        }

        var rows: [[Int]] = []
        while remaining > 0 {
            // The first uncovered pair in scan order.
            var start: (Int, Int, Int, Int)?
            search: for i in 0..<n {
                for j in (i + 1)..<n {
                    for a in 0..<domains[i] {
                        for b in 0..<domains[j] where !covered[i][j][a * domains[j] + b] {
                            start = (i, a, j, b)
                            break search
                        }
                    }
                }
            }
            guard let (i0, a0, j0, b0) = start else { break }
            var best: [Int] = []
            var bestGain = -1
            for _ in 0..<candidates {
                var row = [Int](repeating: -1, count: n)
                row[i0] = a0
                row[j0] = b0
                var order = (0..<n).filter { $0 != i0 && $0 != j0 }
                random.shuffle(&order)
                for k in order {
                    var bestValues: [Int] = []
                    var bestCount = -1
                    for v in 0..<domains[k] {
                        var count = 0
                        for m in 0..<n where row[m] >= 0 && !isCovered(k, v, m, row[m]) { count += 1 }
                        if count > bestCount {
                            bestCount = count
                            bestValues = [v]
                        } else if count == bestCount {
                            bestValues.append(v)
                        }
                    }
                    row[k] = bestValues[random.index(bestValues.count)]
                }
                var gain = 0
                for i in 0..<n { for j in (i + 1)..<n where !covered[i][j][row[i] * domains[j] + row[j]] { gain += 1 } }
                if gain > bestGain {
                    bestGain = gain
                    best = row
                }
            }
            for i in 0..<n {
                for j in (i + 1)..<n where !covered[i][j][best[i] * domains[j] + best[j]] {
                    covered[i][j][best[i] * domains[j] + best[j]] = true
                    remaining -= 1
                }
            }
            rows.append(best)
        }
        return rows
    }
}

/// SplitMix64: a small, fixed random sequence for the reducer.
struct SplitMix64 {
    private var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    mutating func index(_ count: Int) -> Int { Int(next() % UInt64(count)) }

    mutating func shuffle(_ values: inout [Int]) {
        guard values.count > 1 else { return }
        for i in stride(from: values.count - 1, to: 0, by: -1) {
            values.swapAt(i, index(i + 1))
        }
    }
}
