import Foundation
import Security
import Synchronization
import Testing
@testable import CoreBridge

private final class SecureDrawCounter: Sendable {
    let value = Mutex(0)
}

struct CloudKeyByteProviderTests {
    @Test func secureProviderRequestsExactly32BytesAndPreservesTheirOrder() throws {
        let calls = SecureDrawCounter()
        let provider = SystemCloudKeyByteProvider { buffer in
            calls.value.withLock { $0 += 1 }
            #expect(buffer.count == 32)
            for index in buffer.indices { buffer[index] = UInt8(index * 7) }
            return errSecSuccess
        }
        #expect(calls.value.withLock { $0 } == 0)
        let bytes = try #require(provider.drawKey())
        let correct = bytes == Data((0..<32).map { UInt8($0 * 7) })
        #expect(correct)
        #expect(calls.value.withLock { $0 } == 1)
    }

    @Test func osFailureDiscardsEvenPartiallyWrittenBytes() {
        let provider = SystemCloudKeyByteProvider { buffer in
            buffer[0] = 0xff
            return errSecNotAvailable
        }
        let failed = provider.drawKey() == nil
        #expect(failed)
    }

    @Test func actualSecurityProviderReturnsFullWidthIndependentDraws() throws {
        let provider = SystemCloudKeyByteProvider()
        let first = try #require(provider.drawKey())
        let second = try #require(provider.drawKey())
        #expect(first.count == 32 && second.count == 32)
        let distinct = first != second
        #expect(distinct)
    }
}
