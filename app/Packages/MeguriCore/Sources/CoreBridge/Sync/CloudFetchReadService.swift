import Foundation
import Instrumentation
internal import MeguriCoreBindings

struct CoreCloudFetchRead: Sendable {
    private let raw: MeguriCoreBindings.CloudFetchRead
    init(_ raw: MeguriCoreBindings.CloudFetchRead) { self.raw = raw }
    func target() async throws -> CloudControlTarget {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudFetchReadTarget) { CloudControlTarget(try raw.target()) }
    }
    func finalAuthorization() -> @Sendable () throws -> Void {
        let raw = self.raw
        return { try cloudFinalCoreCall(.cloudFetchReadValidate) { try raw.validate() } }
    }
    func complete(_ result: CloudControlRead, discard: Bool = false,
                  evidence: CloudZoneDeletionRead? = nil, admission: CloudOwnWorkAdmission) async throws -> CoreCloudFetchRead? {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudFetchReadComplete) {
            try admission.withPermission { allowed in
                try raw.complete(result: result.raw, discard: discard || !allowed, failure: evidence?.failure?.raw,
                                 terminal: evidence?.terminal?.raw).map(CoreCloudFetchRead.init)
            }
        }
    }
}
struct CloudFetchReadPlan: Sendable {
    let qualified: Bool
    let reads: [CoreCloudFetchRead]
}
