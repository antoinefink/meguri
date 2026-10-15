@testable import Instrumentation

/// Tests call test-only exports (Rust `test-hooks`) under names that are not
/// in the registry: a string literal is a bridge call here, and only here.
extension BridgeCall: ExpressibleByStringLiteral {
    public init(stringLiteral value: StaticString) {
        self.init(value)
    }
}
