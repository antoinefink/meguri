import SwiftUI
import Testing

import DesignSystem

/// Every declared text/background token pairing meets WCAG contrast in light,
/// dark and increased contrast (R1979): 4.5:1 for text, 3:1 for large text
/// and non-text UI.
@Suite struct ContrastTests {
    @Test func theRatioMatchesPublishedWCAGValues() {
        let white = RGB(hex: "#FFFFFF"), black = RGB(hex: "#000000")
        #expect(abs(WCAG.contrastRatio(black, white) - 21) < 0.0001)
        #expect(abs(WCAG.contrastRatio(white, black) - 21) < 0.0001)
        #expect(WCAG.contrastRatio(white, white) == 1)
        // The usual examples: #777 just fails 4.5:1 on white, #767676 just passes.
        #expect(String(format: "%.2f", WCAG.contrastRatio(RGB(hex: "#777777"), white)) == "4.48")
        #expect(String(format: "%.2f", WCAG.contrastRatio(RGB(hex: "#767676"), white)) == "4.54")
    }

    @Test func everyDeclaredPairingPassesInEveryMode() {
        let pairs = ContrastAudit.declaredPairs
        #expect(pairs.count >= 30, "only \(pairs.count) pairings declared")
        let failures = ContrastAudit.failures(pairs, color: ContrastAudit.resolved)
        #expect(failures.isEmpty, "\(failures.map(\.description).joined(separator: "\n"))")
    }

    /// The audit names the pair, the mode and the ratio of a pair that fails.
    @Test func aLowContrastPairFailsNamingThePairModeAndRatio() {
        let fixture: [String: RGB] = [
            "grayText": RGB(hex: "#808080"), "grayBackground": RGB(hex: "#888888"),
            "inkText": RGB(hex: "#000000"), "paper": RGB(hex: "#FFFFFF"),
        ]
        let pairs = [
            AuditPair(foreground: "inkText", background: "paper", minimumRatio: 4.5),
            AuditPair(foreground: "grayText", background: "grayBackground", minimumRatio: 4.5),
        ]
        let failures = ContrastAudit.failures(pairs) { name, _ in fixture[name]! }
        // #808080 on #888888: luminances 0.2159 and 0.2462, (0.2962 / 0.2659) = 1.11.
        #expect(failures.map(\.description) == AppearanceMode.allCases.map { "grayText on grayBackground in \($0): 1.11:1, needs 4.5:1" })
    }

    /// The modes really differ when SwiftUI resolves the tokens: each token
    /// resolves to what its colour set declares for that mode. Without this,
    /// a resolver that ignored dark or increased contrast would make the
    /// audit above check one mode four times.
    @Test(arguments: AppearanceMode.allCases)
    func tokensResolveToTheirCatalogValues(mode: AppearanceMode) throws {
        let catalog = try ColorCatalog()
        for token in ColorToken.allCases {
            let declared = try #require(catalog.sets[token.rawValue]?[mode], "\(token) has no \(mode) colour")
            let resolved = ContrastAudit.resolved(token.rawValue, mode)
            #expect(resolved.isClose(to: declared), "\(token) in \(mode) resolves to \(resolved), declared \(declared)")
        }
    }
}
