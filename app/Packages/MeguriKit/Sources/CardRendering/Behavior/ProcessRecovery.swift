import Foundation
import WebKit

public nonisolated struct CardTypedValue: Equatable, Sendable {
    public let update: CardUpdate
    public let text: String
}

public nonisolated enum CardPageLiveness: Equatable, Sendable {
    case healthy(CardUpdate?), terminated(CardUpdate?), unavailable(CardUpdate?), cancelled
}

private final class CardLivenessProbe {
    private var continuation: CheckedContinuation<CardPageLiveness, Never>?
    private var timer: Task<Void, Never>?
    var isPending: Bool { continuation != nil }
    func start(_ continuation: CheckedContinuation<CardPageLiveness, Never>, timeout: Int, update: CardUpdate?) {
        self.continuation = continuation
        timer = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(timeout)) } catch { return }
            self?.finish(.unavailable(update))
        }
    }
    func finish(_ result: CardPageLiveness) {
        let pending = continuation
        continuation = nil
        timer?.cancel(); timer = nil
        pending?.resume(returning: result)
    }
}

extension CardPage {
    /// This tests the document/runtime, independently of U176's optional
    /// host installation. A timeout or an absent host never proves death.
    public func probeLiveness(timeoutMs: Int = 1000) async -> CardPageLiveness {
        guard !closed, !Task.isCancelled else { return .cancelled }
        let update = update, document = documentID
        if contentProcessTerminated { return .terminated(update) }
        if needsReconstruction { return .unavailable(update) }
        guard (1...2000).contains(timeoutMs) else { return .unavailable(update) }
        let probe = CardLivenessProbe()
        let id = UUID()
        let result = await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                probe.start(continuation, timeout: timeoutMs, update: update)
                livenessCancellations[id] = { probe.finish(.cancelled) }
                guard !Task.isCancelled else { probe.finish(.cancelled); return }
                let completion: @MainActor (Result<Bool, any Error>) -> Void = { [weak self] result in
                    guard probe.isPending else { return }
                    guard let self, !self.closed, self.documentID == document, self.update == update else {
                        probe.finish(.cancelled); return
                    }
                    switch result {
                    case .success(true): probe.finish(.healthy(update))
                    case .success(false): probe.finish(.unavailable(update))
                    case .failure(let error):
                        if (error as? WKError)?.code == .webContentProcessTerminated {
                            probe.finish(.terminated(update))
                            self.webViewWebContentProcessDidTerminate(self.webView)
                        } else { probe.finish(.unavailable(update)) }
                    }
                }
                if let livenessEvaluation { livenessEvaluation(completion) }
                else {
                    webView.callAsyncJavaScript("""
                        return document.readyState === 'complete' && document.getElementById('qa') !== null
                            && typeof meguriCard === 'object';
                        """, arguments: [:], in: nil, in: Self.world) { result in
                        completion(result.map { CardBridge.bool($0) == true })
                    }
                }
            }
        } onCancel: { Task { @MainActor in probe.finish(.cancelled) } }
        livenessCancellations[id] = nil
        return result
    }
}
