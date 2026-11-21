import Testing

/// Throwaway: makes the hosted macOS · Swift packages job fail once, then is
/// reverted.
@Test func throwawayHostedFailure() {
    #expect(1 + 1 == 3)
}
