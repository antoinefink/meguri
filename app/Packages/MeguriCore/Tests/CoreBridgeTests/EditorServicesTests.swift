import Testing
@testable import CoreBridge

extension EventTests {
    @Test func editorTagCompletionRoundTripKeepsRegistryRowsLimitsAndExplicitCancellation() async throws {
        try await withCloudConnection { _ in
            // Registration with no notes leaves real unused registry rows.
            _ = try await CoreTagService().addNoteTags(notes: [], text: "Grammar::Japan Japan Japanese::Audio Straße")
            let editor: any EditorServices = CoreEditorServices()
            let cancel = try await CancelToken.make()
            #expect(try await editor.tagComplete(input: "jap", limit: 1, cancel: cancel) == ["Grammar::Japan"])
            #expect(try await editor.tagComplete(input: "jap", limit: 2, cancel: cancel) == ["Japan", "Grammar::Japan"])
            #expect(try await editor.tagComplete(input: "jap", limit: UInt64(1) << 32, cancel: cancel) == [
                "Japan", "Japanese::Audio", "Grammar::Japan",
            ])
            #expect(try await editor.tagComplete(input: "STRAẞE", limit: 500, cancel: cancel) == ["Straße"])
            #expect(try await editor.tagComplete(input: "strasse", limit: 500, cancel: cancel).isEmpty)
            #expect(try await editor.tagComplete(input: " jap", limit: 500, cancel: cancel).isEmpty)
            #expect(try await editor.tagComplete(input: "", limit: 0, cancel: cancel).isEmpty)
            try await cancel.cancel()
            do throws(BridgeError) {
                _ = try await editor.tagComplete(input: "jap", limit: 500, cancel: cancel)
                Issue.record("Expected explicit cancellation with no partial list")
            } catch { #expect(error == .cancelled) }
        }
        // The zero limit crosses FFI without looking up an open host.
        let cancel = try await CancelToken.make()
        #expect(try await CoreEditorServices().tagComplete(input: "", limit: 0, cancel: cancel).isEmpty)
    }
}
