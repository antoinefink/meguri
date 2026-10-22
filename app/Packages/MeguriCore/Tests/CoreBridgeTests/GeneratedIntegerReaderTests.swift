import Foundation
import Testing
import MeguriCoreBindings
@testable import CoreBridge

// Public generated record readers exercise the shared integer helper, including
// bool bytes, without exposing or duplicating its private implementation.
@Suite struct GeneratedIntegerReaderTests {
    @Test func unalignedCountedTreeRoundtripAndMalformedReaders() async throws {
        try await QueryLane.shared.checkGeneratedIntegerReader()
    }
}

private extension QueryLane {
    func checkGeneratedIntegerReader() throws {
        try call("test.generatedIntegerReader") {
            // One prefix byte makes the UInt64/Int64 fields unaligned. These
            // are the record's normal big-endian wire bytes, not a fast path.
            let data = Data([
                0xA5,
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // generation: UInt64.max
                0x80, 0, 0, 0, 0, 0, 0, 0, // nextChangeAt: Int64.min
                0, 0, 0, 1, // one node
                0x80, 0, 0, 0, 0, 0, 0, 0, // id: Int64.min
                0, 0, 0, 5, 0xC3, 0xA9, 0xE7, 0x8C, 0xAB, // "é猫", five UTF-8 bytes
                1, 0, 1, // filtered, studyCollapsed, browserCollapsed
                0, 0, 0, 0, // new: 0
                0xFF, 0xFF, 0xFF, 0xFF, // learning: UInt32.max
                0x80, 0, 0, 0, // review: 2^31
                0, 0, 0, 0, // no children
            ])
            let expected = CoreBridge.CountedDeckTree(generation: .max, nextChangeAt: .min, nodes: [
                .init(id: .min, name: "é猫", filtered: true, studyCollapsed: false, browserCollapsed: true,
                      counts: .init(new: 0, learning: 0xFFFF_FFFF, review: 0x8000_0000), children: [])
            ])
            var reader = (data: data, offset: 1)
            let raw = try MeguriCoreBindings.FfiConverterTypeCountedDeckTree.read(from: &reader)
            #expect(reader.offset == data.count)
            #expect(CoreBridge.CountedDeckTree(raw) == expected)
            let buffer = FfiConverterTypeCountedDeckTree_lower(raw)
            let roundtrip = try FfiConverterTypeCountedDeckTree_lift(buffer)
            #expect(CoreBridge.CountedDeckTree(roundtrip) == expected)

            // Every truncated prefix must throw, including truncation within
            // strings, single-byte flags, and unaligned multibyte fields.
            for end in 1..<data.count {
                var truncated = (data: Data(data.prefix(end)), offset: 1)
                do {
                    _ = try MeguriCoreBindings.FfiConverterTypeCountedDeckTree.read(from: &truncated)
                    Issue.record("Accepted truncated counted tree at byte \(end)")
                } catch {
                    #expect(String(describing: error) == "bufferOverflow")
                }
            }
            for offset in [Int.min, -1, data.count, data.count + 1, Int.max] {
                var invalid = (data: data, offset: offset)
                do {
                    _ = try MeguriCoreBindings.FfiConverterTypeCountedDeckTree.read(from: &invalid)
                    Issue.record("Accepted invalid reader offset \(offset)")
                } catch {
                    #expect(String(describing: error) == "bufferOverflow")
                    #expect(invalid.offset == offset)
                }
            }
            // Reject a slice with a nonzero start index.
            var offsetReader = (data: data.dropFirst(), offset: 1)
            #expect(offsetReader.data.startIndex == 1)
            do {
                _ = try MeguriCoreBindings.FfiConverterTypeCountedDeckTree.read(from: &offsetReader)
                Issue.record("Unexpected read success")
            } catch {
                #expect(String(describing: error) == "bufferOverflow")
                #expect(offsetReader.offset == 1)
            }
            var empty = (data: Data(), offset: 0)
            do {
                _ = try MeguriCoreBindings.FfiConverterTypeCountedDeckTree.read(from: &empty)
                Issue.record("Accepted an empty counted tree")
            } catch {
                #expect(String(describing: error) == "bufferOverflow")
                #expect(empty.offset == 0)
            }
        }
    }
}
