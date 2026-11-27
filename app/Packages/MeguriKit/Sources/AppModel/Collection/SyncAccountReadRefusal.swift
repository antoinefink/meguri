import CoreBridge
import Foundation

// Only this producer can turn its own prepared-read refusal into evidence.
// The coordinator still has to prove ownership of the original task's cancel.
@MainActor final class SyncAccountReadRefusal {
    let read: UInt64
    private let preparation: CloudAppPreparedAccountRead
    private let reader: CloudAppAccountReader
    private let refusal: CloudAppAccountReadRefusal
    fileprivate init(preparation: CloudAppPreparedAccountRead, reader: CloudAppAccountReader,
                     read: UInt64, refusal: CloudAppAccountReadRefusal) {
        self.preparation = preparation; self.reader = reader; self.read = read; self.refusal = refusal
    }
    var isGeneratedCancellation: Bool {
        refusal.isGeneratedCancellation(from: preparation, reader: reader, read: read)
    }
}
