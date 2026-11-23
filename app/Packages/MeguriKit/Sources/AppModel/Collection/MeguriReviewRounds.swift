import Foundation

/// `sync/meguri-provider#MP6.10`: while a review session is open, the automatic
/// rounds its local changes ask for wait for one window, so a session costs at
/// most one round a minute instead of one per answer (Antoine, 2026-11-23: the
/// phone grew warm during an ordinary review). The window starts at the first
/// local change after a round; its round runs when the window ends, or at once
/// when the session ends or the app leaves the foreground. A round started for
/// any other reason (Sync Now, foreground, network return) carries the waiting
/// changes and closes the window. Outside a session every change still asks for
/// a round at once. Durability is unchanged: changes are committed before this
/// sees them.
@MainActor final class MeguriReviewRounds {
    static let window: Duration = .seconds(60)

    /// Asks for an automatic round now.
    private let request: () -> Void
    /// Tests: the wait for the window's end.
    var sleep: (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    private var sessions: Set<UInt64> = []
    private var timer: Task<Void, Never>?
    private var timerID = UUID()
    /// Local changes committed since the last round started are waiting.
    private(set) var waiting = false

    init(request: @escaping () -> Void) { self.request = request }

    var sessionOpen: Bool { !sessions.isEmpty }
    var windowOpen: Bool { timer != nil }

    func sessionStarted(_ id: UInt64) { sessions.insert(id) }

    /// The session's last answers go with this round, now.
    func sessionEnded(_ id: UInt64) {
        sessions.remove(id)
        guard sessions.isEmpty else { return }
        closeWindow()
        request()
    }

    /// A committed local change. Returns true when its round waits for the window.
    @discardableResult func localChange() -> Bool {
        guard sessionOpen else { request(); return false }
        waiting = true
        if timer == nil { openWindow() }
        return true
    }

    /// A round begins: it carries every change committed so far.
    func roundStarting() {
        waiting = false
        closeWindow()
    }

    /// The app leaves the foreground: waiting changes go now. True when a round was asked for.
    @discardableResult func flush() -> Bool {
        guard waiting else { return false }
        closeWindow()
        request()
        return true
    }

    /// Another collection, or the coordinator closes: nothing carries over.
    func reset() {
        sessions.removeAll(); waiting = false
        closeWindow()
    }

    private func openWindow() {
        let id = UUID(), sleep = self.sleep
        timerID = id
        timer = Task { [weak self] in
            do { try await sleep(Self.window) } catch { return }
            guard let self, self.timerID == id, !Task.isCancelled else { return }
            self.timer = nil
            // `waiting` stays set until a round actually starts, so a refused
            // request leaves the changes for the next trigger.
            if self.waiting { self.request() }
        }
    }

    private func closeWindow() {
        timer?.cancel(); timer = nil
        timerID = UUID()
    }
}
