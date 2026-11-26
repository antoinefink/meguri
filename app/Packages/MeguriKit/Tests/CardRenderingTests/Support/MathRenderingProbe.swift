import Foundation
import Testing
import WebKit

/// Count expressions, not SVG fragments: the pinned renderer splits inline
/// breaks and nests SVGs for stretches/tables. Also usable in onShownHook.
@MainActor
enum MathRenderingProbe {
    static let snapshot = #"""
        (function () {
          var qa = document.getElementById('qa');
          var nodes = Array.from(qa.querySelectorAll('mjx-container'));
          var containers = nodes.map(function (node) {
            var math = node.querySelector('mjx-assistive-mml math');
            return {
              parent: node.parentElement.id || node.parentElement.tagName,
              display: node.getAttribute('display') === 'true',
              paths: Array.from(node.querySelectorAll(':scope > svg path')).filter(function (path) {
                return path.namespaceURI === 'http://www.w3.org/2000/svg' &&
                  !!(path.getAttribute('d') || '').trim();
              }).length,
              accessible: !!math && math.namespaceURI === 'http://www.w3.org/1998/Math/MathML' &&
                math.children.length > 0,
              mathML: math ? math.outerHTML : null
            };
          });
          var errors = qa.querySelectorAll('[data-mml-node="merror"], merror, [data-meguri-math-error]').length;
          return {
            count: nodes.length, errors: errors, containers: containers,
            valid: errors === 0 && containers.every(function (item) {
              return item.paths > 0 && item.accessible;
            })
          };
        })()
        """#

    static func expect(
        in webView: WKWebView, count: Int, label: String, display: Bool? = nil,
        sourceLocation: SourceLocation = #_sourceLocation
    ) async throws {
        let result = try await webView.callAsyncJavaScript(
            "return \(snapshot);", arguments: [:], in: nil, contentWorld: .page)
        let observed = try #require(result as? [String: Any], sourceLocation: sourceLocation)
        let details = "\(label): \(observed)"
        #expect(observed["count"] as? Int == count, "\(details)", sourceLocation: sourceLocation)
        #expect(observed["valid"] as? Bool == true, "\(details)", sourceLocation: sourceLocation)
        if let display {
            let containers = try #require(observed["containers"] as? [[String: Any]], sourceLocation: sourceLocation)
            #expect(containers.allSatisfy { $0["display"] as? Bool == display }, "\(details)", sourceLocation: sourceLocation)
        }
    }
}

extension WebTestHarness {
    var mathRequestDetails: String {
        "bundled requests: \(media.requests.filter { $0.path.hasPrefix("/_anki/mathjax/") }.map(\.path))"
    }
}
