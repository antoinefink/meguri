// Coordinator only. Offscreen Accurate Vision on the saved resident VNC frame.
// No AppKit, window/process observation, desktop capture, UI or input.
import Foundation
import Vision

do {
    guard CommandLine.arguments.count == 2 else {
        throw NSError(domain: "posture", code: 1)
    }
    let frame = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.recognitionLanguages = ["en-US"]
    request.usesLanguageCorrection = false
    try VNImageRequestHandler(data: frame).perform([request])
    let rows = (request.results ?? []).compactMap { observation -> [String: Any]? in
        guard let text = observation.topCandidates(1).first else { return nil }
        let box = observation.boundingBox
        return ["text": text.string, "confidence": text.confidence,
                "box": [box.minX, 1 - box.maxY, box.width, box.height]]
    }
    let data = try JSONSerialization.data(withJSONObject: rows, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
} catch {
    // Native errors can include private paths/content. Emit only a fixed token.
    fputs("posture offscreen text evidence unavailable\n", stderr)
    exit(1)
}
