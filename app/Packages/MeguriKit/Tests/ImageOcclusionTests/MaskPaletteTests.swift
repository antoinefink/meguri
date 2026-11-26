import Foundation
import SwiftUI
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct MaskPaletteTests {
    @Test func readsK9SlotsAndKeepsUnsupportedStringsOnWrite() throws {
        let palette = MaskPalette(json: "[\"red\",\"#12345\",\"#abc\",7,\"#FFFFFF\",\"#00FF00\"]")
        #expect(palette.colors == ["#aabbcc", "#00ff00"])
        let output = try #require(palette.applying(.add("#F0a")))
        let entries = try #require(JSONSerialization.jsonObject(with: Data(output.utf8)) as? [String])
        #expect(entries.count == 16)
        #expect(Array(entries.prefix(6)) == ["red", "#12345", "#aabbcc", "#ff00aa", "#ffffff", "#00ff00"])
        #expect(palette.applying(.add("#abc")) == nil)
        #expect(palette.applying(.add("#ffffff")) == nil)
        #expect(palette.applying(.remove("#000000")) == nil)
        let removed = try #require(MaskPalette(json: "[\"#abc\",\"#aabbcc\",\"red\"]").applying(.remove("#ABC")))
        #expect(MaskPalette(json: removed).colors.isEmpty)
        #expect(removed.contains("red"))
    }

    @Test func shiftsFullPaletteAndIgnoresEntriesBeyondSixteen() throws {
        let colors = (0..<17).map { String(format: "#%06x", $0) }
        let data = try JSONSerialization.data(withJSONObject: colors)
        let palette = MaskPalette(json: String(decoding: data, as: UTF8.self))
        #expect(palette.colors == Array(colors.prefix(16)))
        let result = try #require(palette.applying(.add("#abcdef")))
        #expect(MaskPalette(json: result).colors == Array(colors[1..<16]) + ["#abcdef"])
        for json in [nil, "{}", "null", "\"#ff0000\"", "[]"] {
            #expect(MaskPalette(json: json).colors.isEmpty)
        }
    }

    @Test func shortcutRegistrationHasNoDuplicateKeysAndIncludesAlternates() {
        let bindings = MaskEditorCommand.bindings
        for binding in bindings {
            #expect(bindings.filter { $0.character == binding.character && $0.modifiers == binding.modifiers }.count == 1)
        }
        #expect(MaskEditorCommand.context == "imageOcclusion.maskEditor")
        #expect(bindings.contains { $0.command == .redo && $0.character == "y" && $0.modifiers == .command })
        #expect(bindings.contains { $0.command == .selectAll && $0.character == "a" && $0.modifiers == .command })
        #expect(!bindings.contains { $0.character == "m" })
        var enabled = false, performed = 0
        let context = MaskEditorCommandContext(enabled: { _ in enabled }, action: { _ in performed += 1 })
        context.perform(.undo); #expect(performed == 0)
        enabled = true; context.perform(.undo); #expect(performed == 1)
    }
}
