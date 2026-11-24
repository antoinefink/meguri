// Guest only. Reads AppKit/CG geometry, or Vision text in a saved VNC image.
// No Accessibility, AppleEvents, screen capture, permission requests or input.
import AppKit
import Vision

func emit(_ value: Any) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}

do {
    if CommandLine.arguments.count == 2 && CommandLine.arguments[1] == "window" {
        guard let app = NSWorkspace.shared.frontmostApplication,
              let bundle = app.bundleURL, let executable = app.executableURL,
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
        let owned = normal.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == app.processIdentifier }
        // Refuse the library plus device, multiple devices, and an occluding app.
        guard owned.count == 1, let window = owned.first, let first = normal.first,
              (first[kCGWindowNumber as String] as? Int) == (window[kCGWindowNumber as String] as? Int),
              let bounds = window[kCGWindowBounds as String] as? [String: Any],
              let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary), screen.contains(rect),
              rect.width > 100, rect.height > 150 else {
            throw NSError(domain: "posture", code: 4)
        }
        guard let pointer = CGEvent(source: nil)?.location else {
            throw NSError(domain: "posture", code: 6)
        }
        try emit(["pid": app.processIdentifier, "bundle": bundle.resolvingSymlinksInPath().path,
                  "executable": executable.resolvingSymlinksInPath().path,
                  "id": window[kCGWindowNumber as String] as? Int ?? -1,
                  "title": window[kCGWindowName as String] as? String ?? "",
                  "pointer": [pointer.x, pointer.y],
                  "rect": [rect.minX, rect.minY, rect.width, rect.height],
                  "screen": [screen.minX, screen.minY, screen.width, screen.height]])
    } else if CommandLine.arguments.count == 3 && CommandLine.arguments[1] == "text" {
        let handler = VNImageRequestHandler(url: URL(fileURLWithPath: CommandLine.arguments[2]))
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = ["en-US"]
        request.usesLanguageCorrection = false
        try handler.perform([request])
        try emit((request.results ?? []).compactMap { observation -> [String: Any]? in
            guard let text = observation.topCandidates(1).first else { return nil }
            let box = observation.boundingBox
            return ["text": text.string, "confidence": text.confidence,
                    "box": [box.minX, 1 - box.maxY, box.width, box.height]]
        })
    } else {
        throw NSError(domain: "posture", code: 5)
    }
} catch {
    // Never emit native descriptions, window titles or recognized user content.
    fputs("posture visual evidence unavailable\n", stderr)
    exit(1)
}
