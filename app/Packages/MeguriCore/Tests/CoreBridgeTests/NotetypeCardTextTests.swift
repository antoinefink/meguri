import Testing
@testable import CoreBridge

/// The Card Text row through CoreBridge (`notetypes/workspace` W10): pure
/// calls, no collection.
@Suite struct NotetypeCardTextTests {
    @Test func theRowReadsAndAnEditChangesOnlyItsValue() async throws {
        let types = CoreNotetypeService()
        let css = "/* mine */\n.card {\n  color: #FF0000 !important;\n}\n"
        let row = try #require(try await types.cardText(css: css, rightToLeft: false))
        #expect(row.textColour == CardTextColour(red: 255, green: 0, blue: 0))
        #expect(row.font == nil && row.size == nil && row.alignment == nil && row.background == nil)

        let colour = try await types.editCardText(css: css, change: .textColour(.init(red: 0, green: 0x80, blue: 0)), rightToLeft: false)
        #expect(colour.css == "/* mine */\n.card {\n  color: #008000 !important;\n}\n")
        #expect(colour.css[colour.value] == "#008000")

        let added = try await types.editCardText(css: css, change: .alignment(.leading), rightToLeft: true)
        #expect(added.css == "/* mine */\n.card {\n  color: #FF0000 !important;\n  text-align: right;\n}\n")
        #expect(added.css[added.value] == "right")

        #expect(try await types.cardText(css: ".card { color: red", rightToLeft: false) == nil)
        await #expect(throws: BridgeError.self) {
            try await types.editCardText(css: ".card {} .card {}", change: .size(18), rightToLeft: false)
        }
    }
}
