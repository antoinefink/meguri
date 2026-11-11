#if DEBUG
import Testing
@testable import CoreBridge

struct DebugNativeFetchSequenceTests {
    private typealias Recorder = DebugCloudDiagnostics.NativeFetchSequence
    private typealias Event = DebugCloudDiagnostics.NativeFetchEvent
    private typealias Inclusion = DebugCloudDiagnostics.FetchInclusion

    @Test func boundedTailPreservesCrossInstanceOrderAndLatestConfiguration() {
        let recorder = Recorder()
        let first = recorder.register(automatic: true, restored: false)
        recorder.record(.invoke, ordinal: first)
        let second = recorder.register(automatic: false, restored: true)
        recorder.record(.will(reason: .scheduled, sameGroup: false), ordinal: first)
        recorder.record(.database, ordinal: second)
        recorder.record(.returned, ordinal: first)
        #expect(first == 1 && second == 2)
        #expect(recorder.formatted == "nf{l=2 a=0 r=1 o=0|1:C10,1:I,2:C01,1:Ws0,2:B,1:R}")
        // Exactly sixteen newer events evict all six earlier observations.
        for _ in 0..<8 {
            recorder.record(.cancel, ordinal: first)
            recorder.record(.release, ordinal: second)
        }
        #expect(recorder.formatted == "nf{l=2 a=0 r=1 o=0|" +
            Array(repeating: "1:X,2:L", count: 8).joined(separator: ",") + "}")
        recorder.record(.database, ordinal: first)
        #expect(recorder.formatted == "nf{l=2 a=0 r=1 o=0|2:L," +
            Array(repeating: "1:X,2:L", count: 7).joined(separator: ",") + ",1:B}")
    }

    @Test func freshRecordersKeepIndependentOrdinalsConfigurationAndTail() {
        let first = Recorder()
        let ordinal = first.register(automatic: true, restored: true)
        first.record(.invoke, ordinal: ordinal)
        let second = Recorder()
        #expect(second.formatted == "nf{l=0 a=0 r=0 o=0|}")
        #expect(second.register(automatic: false, restored: false) == 1)
        #expect(first.formatted == "nf{l=1 a=1 r=1 o=0|1:C11,1:I}")
        #expect(second.formatted == "nf{l=1 a=0 r=0 o=0|1:C00}")
    }

    @Test func registrationSaturatesWithoutReusingOrdinalsAndOverflowSticks() {
        let recorder = Recorder(lastAllocatedOrdinal: .max - 2)
        #expect(recorder.register(automatic: false, restored: true) == 65534)
        #expect(recorder.register(automatic: true, restored: false) == 65535)
        #expect(recorder.formatted == "nf{l=65535 a=1 r=0 o=0|65534:C01,65535:C10}")
        for _ in 0..<32 {
            #expect(recorder.register(automatic: false, restored: true) == 0)
        }
        recorder.record(.returned, ordinal: 65535)
        #expect(recorder.formatted == "nf{l=0 a=0 r=1 o=1|" +
            Array(repeating: "0:C01", count: 15).joined(separator: ",") + ",65535:R}")
        #expect(recorder.register(automatic: true, restored: false) == 0)
        #expect(recorder.formatted.hasPrefix("nf{l=0 a=1 r=0 o=1|"))
    }

    @Test func observationsKeepGenuineFieldsSeparateWithoutInferringCompletion() {
        let recorder = Recorder()
        let ordinal = recorder.register(automatic: false, restored: false)
        recorder.record(.prepared(scope: .init(root: true, collection: false),
            priority: .init(root: false, collection: true)), ordinal: ordinal)
        recorder.record(.options(reason: .manual, requested: .init(root: false, collection: true),
            offered: .init(), priority: .init(), sameGroup: true), ordinal: ordinal)
        recorder.record(.options(reason: .scheduled, requested: .init(),
            offered: .init(root: true, collection: false),
            priority: .init(root: false, collection: true), sameGroup: false), ordinal: ordinal)
        recorder.record(.fetchedZones(root: false, collection: false, other: false), ordinal: ordinal)
        recorder.record(.fetchedZones(root: true, collection: true, other: true), ordinal: ordinal)
        recorder.record(.did(reason: .manual, sameGroup: false), ordinal: ordinal)
        #expect(recorder.formatted == "nf{l=1 a=0 r=0 o=0|1:C00,1:P1001,1:Nm0100001,1:Ns0010010,1:F000,1:F111,1:Dm0}")
    }

    @Test func everyClosedEventKindAndFieldCombinationFitsWorstCaseTail() {
        let flags = [false, true]
        let inclusions = flags.flatMap { root in
            flags.map { Inclusion(root: root, collection: $0) }
        }
        var events: [Event] = [.invoke, .returned, .cancel, .release, .database,
            .willZone(.root), .willZone(.collection), .willZone(.other)]
        for automatic in flags {
            for restored in flags { events.append(.configured(automatic: automatic, restored: restored)) }
        }
        for scope in inclusions {
            for priority in inclusions { events.append(.prepared(scope: scope, priority: priority)) }
        }
        for root in flags {
            for collection in flags {
                for other in flags { events.append(.fetchedZones(root: root, collection: collection, other: other)) }
            }
        }
        for reason in DebugCloudDiagnostics.FetchReason.allCases {
            for same in flags {
                events.append(.will(reason: reason, sameGroup: same))
                events.append(.did(reason: reason, sameGroup: same))
                for requested in inclusions {
                    for offered in inclusions {
                        for priority in inclusions {
                            events.append(.options(reason: reason, requested: requested,
                                offered: offered, priority: priority, sameGroup: same))
                        }
                    }
                }
            }
        }
        for event in events {
            let recorder = Recorder(lastAllocatedOrdinal: .max - 1)
            let ordinal = recorder.register(automatic: true, restored: true)
            for _ in 0..<16 { recorder.record(event, ordinal: ordinal) }
            #expect(recorder.formatted.utf8.count <= 512)
            #expect(recorder.formatted.filter { $0 == ":" }.count == 16)
        }
    }
}
#endif
