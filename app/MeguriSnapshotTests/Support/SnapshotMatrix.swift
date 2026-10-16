import CoreGraphics
import Foundation

// The snapshot matrix (R1971, R1972): its axes, how each value is applied on
// each platform, and the cell sets drawn from it. Spike S5
// (docs/design/decisions/s5-simulator-settings.md) decides which
// values a simulator can show: system-setting values are set on the leased
// simulator by `tools/mg snapshots` before a test run (one run per settings
// group), device-only values are deferred to the monthly device session.

/// A platform of the matrix: its axis values and cell sets. Its device
/// classes render on one or more `SnapshotTarget`s.
enum SnapshotPlatform: String, CaseIterable, Codable, Sendable {
    case iphone, ipad, mac

    /// The platform this test process renders.
    static var current: SnapshotPlatform { SnapshotTarget.current.platform }
}

/// Where a device class renders: a pool simulator kind (tools/sim-devices.json)
/// whose screen is the class's, or the Mac. A screen takes its window's safe
/// area, so a class rendered on a larger simulator would get that
/// simulator's insets, not its own.
enum SnapshotTarget: String, CaseIterable, Codable, Sendable {
    case iphone
    case iphoneSmall = "iphone-small"
    case ipad
    case mac

    /// The target this test process renders: `tools/mg snapshots` names it
    /// (`MEGURI_SNAPSHOT_TARGET`, the kind it leased); outside the job an
    /// iPhone simulator is the iphone kind.
    static let current = SnapshotTarget(environment: ProcessInfo.processInfo.environment)

    init(environment: [String: String]) {
        #if os(macOS)
        self = .mac
        #else
        if let named = environment["MEGURI_SNAPSHOT_TARGET"].flatMap(SnapshotTarget.init(rawValue:)), named != .mac {
            self = named
        } else {
            self = environment["SIMULATOR_MODEL_IDENTIFIER"]?.hasPrefix("iPad") == true ? .ipad : .iphone
        }
        #endif
    }

    var platform: SnapshotPlatform {
        switch self {
        case .iphone, .iphoneSmall: .iphone
        case .ipad: .ipad
        case .mac: .mac
        }
    }

    /// The device class this target's own checks render in (the settings
    /// probe).
    var referenceDevice: DeviceClass {
        switch self {
        case .iphone: .iphoneLarge
        case .iphoneSmall: .iphoneSmall
        case .ipad: .ipadRegular
        case .mac: .macDefault
        }
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
/// simulator of their kind (`target`), at their size, with their size
/// classes. The small iPhone class is the iPhone 17e's screen, so its cells
/// get that screen's safe area.
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

    /// The simulator kind (or the Mac) the class renders on.
    var target: SnapshotTarget {
        switch self {
        case .iphoneSmall: .iphoneSmall
        case .iphoneLarge, .iphoneResizable, .foldableOuter, .foldableInner: .iphone
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
    /// Why the screen's cell set defers this cell although it could render
    /// here: system chrome that does not render the same twice
    /// (`MatrixSet.unstableGlassKeyVariants`). Not part of the name.
    var unstableReason: String?

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
            let place = device.platform == .mac ? "in the Mac's off-screen renders (its own settings are never changed)" : "on a simulator"
            return "\(axis.rawValue) \(value(of: axis)) cannot be set \(place) (spike S5); captured at the device session"
        }
        return unstableReason
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
    /// The key variants of a screen whose system Liquid Glass renders
    /// differently from run to run (the iPad sidebar's grain), each with
    /// Reduce Transparency on (`+rt`) and off. Only the light `+rt` cells
    /// render: Reduce Transparency makes the glass opaque, and the noise it
    /// keeps (the sidebar toggle's, one level) fails the perceptual
    /// comparison on near-black pixels, so in dark cells. The others stay
    /// in the set, deferred with their reason (`MatrixCell.unstableReason`),
    /// so every run lists the gap until U252 compares them. The Mac cannot
    /// set Reduce Transparency: its `+rt` cells are deferred for that.
    case unstableGlassKeyVariants
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
            // On the Mac the user's own settings are never changed, and
            // system chrome ignores a SwiftUI environment override: a cell
            // with either setting on is captured at the device session.
            guard platform == .mac else { return .systemSetting }
            let on = axis == .reduceTransparency ? cell.reduceTransparency : cell.reduceMotion
            return on ? .deviceOnly : .inProcess
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
        case .unstableGlassKeyVariants:
            cells(.keyVariants, on: platform).map {
                var cell = $0
                cell.reduceTransparency = true
                if cell.appearance == .dark {
                    cell.unstableReason = "with Reduce Transparency on, the sidebar toggle's glass still differs by one level "
                        + "from run to run, which the perceptual comparison fails on near-black pixels (U252)"
                }
                return cell
            } + cells(.keyVariants, on: platform).map {
                var cell = $0
                cell.unstableReason = "system Liquid Glass (the sidebar's grain) differs from run to run beyond the tolerance (U252)"
                return cell
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
