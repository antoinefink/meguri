// Guest only. Reads AppKit/CG window, process and pointer geometry.
// No Accessibility, AppleEvents, screen capture, permission requests or input.
import AppKit

func emit(_ value: Any) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}

func identity(_ app: NSRunningApplication?) -> [String: Any] {
    guard let app = app else { return [:] }
    return ["pid": app.processIdentifier, "bundle_id": app.bundleIdentifier ?? "",
            "active": app.isActive, "terminated": app.isTerminated,
            "finished_launching": app.isFinishedLaunching,
            "bundle": app.bundleURL?.resolvingSymlinksInPath().path ?? "",
            "executable": app.executableURL?.resolvingSymlinksInPath().path ?? ""]
}

do {
    if CommandLine.arguments.count == 2 && CommandLine.arguments[1] == "window" {
        guard let app = NSWorkspace.shared.frontmostApplication,
              let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements],
                                                       kCGNullWindowID) as? [[String: Any]] else {
            throw NSError(domain: "posture", code: 1)
        }
        var count: UInt32 = 0
        guard CGGetActiveDisplayList(0, nil, &count) == .success, count == 1 else {
            throw NSError(domain: "posture", code: 2)
        }
        var display = CGDirectDisplayID()
        guard CGGetActiveDisplayList(1, &display, &count) == .success else {
            throw NSError(domain: "posture", code: 3)
        }
        let screen = CGDisplayBounds(display)
        let normal = windows.filter { ($0[kCGWindowLayer as String] as? Int) == 0 }
        // Preserve CG front-to-back order. Admission checks the selected signed
        // installation across PIDs; the launcher need not own the device window.
        let evidence = try normal.map { window -> [String: Any] in
            guard let pid = window[kCGWindowOwnerPID as String] as? Int32,
                  let bounds = window[kCGWindowBounds as String] as? [String: Any],
                  let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary) else {
                throw NSError(domain: "posture", code: 4)
            }
            var row: [String: Any] = [:]
            if pid > 0, let owner = NSRunningApplication(processIdentifier: pid) {
                row = identity(owner)
                // AppKit may return -1 even for the object looked up by CG PID.
                // Preserve its identity/flags and report that PID separately.
                let lookupPID = row.removeValue(forKey: "pid")
                row["lookup_pid"] = lookupPID
            }
            row["pid"] = pid
            row["id"] = window[kCGWindowNumber as String] as? Int ?? -1
            row["title"] = window[kCGWindowName as String] as? String ?? ""
            row["rect"] = [rect.minX, rect.minY, rect.width, rect.height]
            return row
        }
        guard let pointer = CGEvent(source: nil)?.location else {
            throw NSError(domain: "posture", code: 6)
        }
        try emit(["front": identity(app), "normal": evidence,
                  "pointer": [pointer.x, pointer.y],
                  "screen": [screen.minX, screen.minY, screen.width, screen.height]])
    } else {
        throw NSError(domain: "posture", code: 5)
    }
} catch {
    // Never emit native descriptions, window titles or recognized user content.
    fputs("posture visual evidence unavailable\n", stderr)
    exit(1)
}
