import SwiftUI
import Testing

import DesignSystem

/// Every token is complete (R1992): colour tokens have a colour set with
/// light, dark and increased-contrast variants and declare the backgrounds
/// they are drawn on (R1979), spacing is a 4-pt scale, and every motion token
/// has a Reduce Motion alternative.
@Suite struct TokenCoverageTests {
    @Test func everyColourTokenHasAColourSetWithEveryVariant() throws {
        let catalog = try ColorCatalog()
        #expect(catalog.problems.isEmpty, "\(catalog.problems.joined(separator: "\n"))")
        let tokens = Set(ColorToken.allCases.map(\.rawValue))
        let sets = Set(catalog.sets.keys)
        #expect(tokens.subtracting(sets).isEmpty, "tokens without a colour set: \(tokens.subtracting(sets).sorted())")
        #expect(sets.subtracting(tokens).isEmpty, "colour sets without a token: \(sets.subtracting(tokens).sorted())")
    }

    @Test func everyColourTokenTakesPartInAPairing() {
        let problems = TokenCoverage.problems(TokenSpec.declared)
        #expect(problems.isEmpty, "\(problems.joined(separator: "\n"))")
    }

    /// A token added without pairings fails, whether it is a foreground with
    /// no background or a background nothing is drawn on.
    @Test func aTokenWithoutPairingsIsReported() {
        let specs = TokenSpec.declared + [
            TokenSpec("highlight", .foreground(on: [])),
            TokenSpec("tooltipBackground", .background),
        ]
        #expect(TokenCoverage.problems(specs) == [
            "highlight declares no background to be drawn on",
            "tooltipBackground is a background no foreground is drawn on",
        ])
    }

    @Test func onlyBackgroundsCanBeDrawnOn() {
        let specs = TokenSpec.declared + [TokenSpec("badgeText", .foreground(on: ["countNew"]))]
        #expect(TokenCoverage.problems(specs) == ["badgeText is drawn on countNew, which is not a background token"])
    }

    @Test func spacingIsAFourPointScale() {
        #expect(!Spacing.scale.isEmpty)
        for step in Spacing.scale {
            #expect(step > 0 && step.truncatingRemainder(dividingBy: 4) == 0, "\(step) is not on the 4-pt scale")
        }
        #expect(Spacing.scale == Spacing.scale.sorted() && Set(Spacing.scale).count == Spacing.scale.count, "steps must ascend without repeats")
    }

    @Test func radiiAscend() {
        #expect(!Radius.scale.isEmpty && Radius.scale.allSatisfy { $0 > 0 })
        #expect(Radius.scale == Radius.scale.sorted() && Set(Radius.scale).count == Radius.scale.count)
    }

    @Test(arguments: Motion.all)
    func everyMotionTokenHasAReduceMotionAlternative(token: MotionToken) {
        #expect(token.duration > 0)
        #expect(token.animation(reduceMotion: false) == token.animation)
        #expect(token.animation(reduceMotion: true) == token.reducedAnimation)
        switch token.reduced {
        case let .crossfade(duration):
            #expect(duration > 0 && duration <= token.duration, "\(token.name): the Reduce Motion crossfade must not outlast the motion")
            #expect(token.reducedAnimation != nil)
        case .instant:
            #expect(token.reducedAnimation == nil)
        }
    }

    @Test func motionTokenNamesAreUnique() {
        #expect(Set(Motion.all.map(\.name)).count == Motion.all.count)
    }
}
