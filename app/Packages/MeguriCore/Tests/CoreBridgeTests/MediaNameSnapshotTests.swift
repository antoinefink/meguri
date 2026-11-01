import Foundation
import MeguriCoreBindings
import Testing
@testable import CoreBridge

@Suite struct MediaNameSnapshotTests {
    @Test func equivalentRootSpellingsNormalizeAcrossInitializers() {
        let expectedRoot = URL(fileURLWithPath: "/tmp/media-binding/collection.media").standardizedFileURL
        let names = [
            CoreBridge.TrackedMediaName(name: "É.PNG", caseKey: "é.png"),
            CoreBridge.TrackedMediaName(name: "E\u{301}.PNG", caseKey: "e\u{301}.png"),
        ]
        for root in [
            "/tmp/media-binding/collection.media",
            "/tmp/media-binding/./collection.media",
            "/tmp/media-binding/staging/../collection.media",
        ] {
            let explicit = CoreBridge.MediaNameSnapshot(
                generation: 19, root: URL(fileURLWithPath: root), names: names)
            let mapped = CoreBridge.MediaNameSnapshot(MeguriCoreBindings.MediaNameSnapshot(
                generation: 19, root: root,
                names: names.map { .init(name: $0.name, caseKey: $0.caseKey) }))
            #expect(explicit.root == expectedRoot)
            #expect(mapped.root == expectedRoot)
            #expect(explicit.root == mapped.root)
            for value in [explicit, mapped] {
                #expect(value.generation == 19)
                #expect(value.names.count == names.count)
                #expect(value.names.map { Data($0.name.utf8) } == names.map { Data($0.name.utf8) })
                #expect(value.names.map { Data($0.caseKey.utf8) } == names.map { Data($0.caseKey.utf8) })
            }
        }
    }

    @Test func dtoPreservesStoredNameAndCaseKeyBytesAndArrivalGeneration() {
        let raw = MeguriCoreBindings.MediaNameSnapshot(generation: 19, root: "/tmp/media-binding/collection.media", names: [
            .init(name: "É.PNG", caseKey: "é.png"),
            .init(name: "E\u{301}.PNG", caseKey: "e\u{301}.png"),
        ])
        let value = CoreBridge.MediaNameSnapshot(raw)
        #expect(value.generation == 19)
        #expect(value.root.path == raw.root)
        #expect(value.names.count == 2)
        for (mapped, original) in zip(value.names, raw.names) {
            #expect(Data(mapped.name.utf8) == Data(original.name.utf8))
            #expect(Data(mapped.caseKey.utf8) == Data(original.caseKey.utf8))
        }
        let event = CoreBridge.CoreEvent(MeguriCoreBindings.CoreEvent(seq: 23, generation: 19, causeSeq: nil,
            kind: .mediaArrived(names: raw.names.map(\.name))))
        #expect(event.generation == 19)
        guard case .mediaArrived(let names) = event.kind else { Issue.record("Missing media arrival"); return }
        #expect(names.map { Data($0.utf8) } == raw.names.map { Data($0.name.utf8) })
    }
}
