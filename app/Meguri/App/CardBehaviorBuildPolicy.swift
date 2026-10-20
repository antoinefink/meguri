import CardRendering
import Foundation

/// Native configurations declare distribution explicitly. SwiftPM never
/// sees app compilation flags. U160 supplies the device setting override.
enum CardBehaviorBuildPolicy {
    static func inspectorPolicy(enabled: Bool? = nil, bundle: Bundle = .main) -> WebInspectorPolicy {
        let value = bundle.object(forInfoDictionaryKey: "MeguriCardInspectionBuild") as? String
        return WebInspectorPolicy(build: value.flatMap(CardInspectionBuild.init(rawValue:)) ?? .unknown, enabled: enabled)
    }
}
