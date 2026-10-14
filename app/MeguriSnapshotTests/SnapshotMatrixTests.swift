import Foundation
import Testing

/// The matrix definition and its pairwise reducer (R1971, R1972).
@Suite struct SnapshotMatrixTests {
    /// Every pair of values of every two axes that no row holds.
    private func uncoveredPairs(_ rows: [[Int]], _ domains: [Int]) -> [String] {
        var missing: [String] = []
        for i in domains.indices {
            for j in domains.indices where j > i {
                for a in 0..<domains[i] {
                    for b in 0..<domains[j] where !rows.contains(where: { $0[i] == a && $0[j] == b }) {
                        missing.append("axis \(i)=\(a) with axis \(j)=\(b)")
                    }
                }
            }
        }
        return missing
    }

    @Test(arguments: [
        [2, 2, 2],
        [2, 4, 2, 2, 2, 1, 3, 3],
        [2, 4, 2, 2, 2, 2, 3, 5],
        [3, 3, 3, 3, 3, 3, 3, 3, 3, 3],
        [5, 1, 4],
    ])
    func thePairwiseReducerIsDeterministicAndCoversEveryPair(domains: [Int]) {
        let first = Pairwise.cover(domains, seed: SnapshotMatrix.pairwiseSeed)
        let second = Pairwise.cover(domains, seed: SnapshotMatrix.pairwiseSeed)
        #expect(first == second, "same domains and seed gave different rows")
        #expect(uncoveredPairs(first, domains).isEmpty, "uncovered: \(uncoveredPairs(first, domains))")
        for row in first {
            #expect(row.count == domains.count)
            #expect(zip(row, domains).allSatisfy { $0 >= 0 && $0 < $1 })
        }
        // Never fewer rows than the two largest domains' product (a lower
        // bound for any pairwise set), and within a small factor of it.
        let sorted = domains.sorted(by: >)
        #expect(first.count >= sorted[0] * sorted[1])
        #expect(first.count <= 2 * sorted[0] * sorted[1], "\(first.count) rows for \(domains)")
    }

    @Test(arguments: SnapshotPlatform.allCases)
    func thePairwiseSetCoversEveryPairOfValuesASimulatorCanShowWithRenderedCells(platform: SnapshotPlatform) {
        let cells = SnapshotMatrix.cells(.pairwise, on: platform)
        #expect(cells == SnapshotMatrix.cells(.pairwise, on: platform))
        #expect(Set(cells).count == cells.count, "a cell is listed twice")
        let rendered = cells.filter { $0.deferredReason == nil }
        let axes = MatrixAxis.allCases
        for (i, first) in axes.enumerated() {
            for second in axes[(i + 1)...] {
                for a in SnapshotMatrix.values(of: first, on: platform) {
                    for b in SnapshotMatrix.values(of: second, on: platform) {
                        let anywhere = cells.contains { $0.value(of: first) == a && $0.value(of: second) == b }
                        #expect(anywhere, "\(first)=\(a) with \(second)=\(b) is in no cell")
                        let renderable = cells.contains { $0.value(of: first) == a && $0.value(of: second) == b && $0.deferredReason == nil }
                        let canRender = rendered.contains { $0.value(of: first) == a } && rendered.contains { $0.value(of: second) == b }
                        if canRender {
                            #expect(renderable, "\(first)=\(a) with \(second)=\(b) is only in deferred cells")
                        }
                    }
                }
            }
        }
        #expect(cells.allSatisfy { $0.device.platform == platform })
    }

    @Test func deviceOnlyValuesAreDeferredAndTheRestRenders() {
        let iphone = SnapshotMatrix.cells(.pairwise, on: .iphone)
        #expect(iphone.contains { $0.glassLook == .tinted })
        #expect(iphone.filter { $0.glassLook == .tinted }.allSatisfy { $0.deferredReason != nil })
        #expect(iphone.filter { $0.device == .foldableInner }.allSatisfy { $0.deferredReason?.contains("27.1") == true })
        #expect(iphone.contains { $0.transparency == .maximum && $0.deferredReason == nil }, "transparency is a simulator setting (S5)")
        let mac = SnapshotMatrix.cells(.pairwise, on: .mac)
        #expect(mac.filter { $0.transparency != .standard }.allSatisfy { $0.deferredReason != nil }, "the Mac's own settings are never changed")
        #expect(mac.contains { $0.reduceTransparency && $0.deferredReason == nil })
    }

    @Test func cellNamesAreUniqueAndReadable() {
        let cell = MatrixCell(appearance: .dark, textSize: .ax5, increaseContrast: true, reduceTransparency: true, transparency: .maximum, device: .iphoneSmall)
        #expect(cell.name == "dark-ax5-iphoneSmall+hc+rt+tmax")
        #expect(cell.systemGroup == "rt1-rm0-t100")
        #expect(MatrixCell(device: .macDefault).systemGroup == "mac")
        for platform in SnapshotPlatform.allCases {
            let cells = SnapshotMatrix.cells(.full, on: platform)
            #expect(Set(cells.map(\.name)).count == cells.count)
        }
    }

    @Test func keyVariantsAreLightAndDarkAtTheDefaultAndLargestTextSize() {
        #expect(SnapshotMatrix.cells(.keyVariants, on: .iphone).map(\.name) == [
            "light-standard-iphoneLarge", "light-ax5-iphoneLarge", "dark-standard-iphoneLarge", "dark-ax5-iphoneLarge",
        ])
        // SwiftUI on the Mac ignores Dynamic Type: one text size there.
        #expect(SnapshotMatrix.cells(.keyVariants, on: .mac).map(\.name) == ["light-standard-macDefault", "dark-standard-macDefault"])
    }
}
