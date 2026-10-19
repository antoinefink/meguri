import Foundation
import Testing
import WebKit

extension WebKitSuites.CardRuntimeSuites {
    /// C13 metadata must not own callbacks or detached old card graphs.
    @Suite struct ListenerOwnershipTests {
        private struct Reachability: Decodable {
            let alive: Bool
            let finalized: Bool
        }

        private static let installProbe = #"""
            const frame = document.createElement("iframe");
            frame.hidden = true;
            frame.src = "about:blank";
            await new Promise(resolve => { frame.onload = resolve; document.body.appendChild(frame); });
            const native = frame.contentWindow.EventTarget.prototype;
            const state = window.__listenerOwnership = {
              refs: Object.create(null), finalized: Object.create(null),
              controller: new AbortController(), targets: Object.create(null),
              removes: Object.create(null)
            };
            state.registry = new FinalizationRegistry(label => { state.finalized[label] = true; });
            function observe(value, label) {
              state.refs[label] = new WeakRef(value);
              state.registry.register(value, label);
            }
            // A separate scope genuinely captures the old root and 64 KiB
            // payload. No external variable in that closure is nulled later.
            function makeCallback(root) {
              const payload = new Uint8Array(64 * 1024);
              return function () { root.__calls = (root.__calls || 0) + payload.byteLength; };
            }
            function install(mode) {
              const root = document.createElement("section");
              const element = document.createElement("button");
              root.appendChild(element);
              document.getElementById("qa").appendChild(root);
              const target = scenario.endsWith("element") ? element :
                scenario === "once-window" ? window : document;
              const callback = makeCallback(root);
              const api = mode === "native" ? native : EventTarget.prototype;
              const options = { once: scenario.startsWith("once-") };
              if (scenario.endsWith("element") || scenario === "remove-document" || scenario === "active-abort") {
                // One global controller stays live, even after removal/once.
                options.signal = state.controller.signal;
              }
              observe(callback, mode + "-callback");
              observe(root, mode + "-root");
              if (scenario.endsWith("element")) observe(target, mode + "-target");
              state.targets[mode] = new WeakRef(target);
              state.removes[mode] = api.removeEventListener;
              Reflect.apply(api.addEventListener, target, ["keydown", callback, options]);
              if (options.once) target.dispatchEvent(new Event("keydown"));
              else if (scenario.startsWith("remove-")) {
                Reflect.apply(api.removeEventListener, target, ["keydown", callback]);
              }
            }
            install("native");
            install("wrapped");
            // This helper captures state only, never an install activation.
            state.release = function (abort) {
              if (abort) { state.controller.abort(); return; }
              for (const mode of ["native", "wrapped"]) {
                Reflect.apply(state.removes[mode], state.targets[mode].deref(),
                  ["keydown", state.refs[mode + "-callback"].deref()]);
              }
            };
            frame.remove();
            """#

        private static let snapshot = #"""
            const state = window.__listenerOwnership;
            const result = Object.create(null);
            for (const label of Object.keys(state.refs)) {
              result[label] = { alive: state.refs[label].deref() !== undefined,
                finalized: state.finalized[label] === true };
            }
            return JSON.stringify(result);
            """#

        /// Same test-only pool GC route used by the existing retention suite.
        /// Fail explicitly if unavailable; allocation pressure is no substitute.
        @MainActor
        private static func collect(_ page: TestPage) async throws {
            // End the job that last dereferenced anything before requesting GC.
            try await page.run("await new Promise(resolve => setTimeout(resolve, 0));")
            let configuration = page.webView.configuration
            let poolGetter = NSSelectorFromString("processPool")
            guard configuration.responds(to: poolGetter),
                  let pool = configuration.perform(poolGetter)?.takeUnretainedValue() as? NSObject else {
                throw HarnessFailure("listener ownership probe lacks the test-only processPool GC route")
            }
            let selector = NSSelectorFromString("_garbageCollectJavaScriptObjectsForTesting")
            guard pool.responds(to: selector) else {
                throw HarnessFailure("listener ownership probe lacks _garbageCollectJavaScriptObjectsForTesting")
            }
            let collect = unsafeBitCast(pool.method(for: selector), to: (@convention(c) (AnyObject, Selector) -> Void).self)
            collect(pool, selector)
            // Ordered IPC follows GC; finalizers still need event-loop tasks.
            try await page.run("for (let i = 0; i < 4; i++) await new Promise(resolve => setTimeout(resolve, 0));")
        }

        @Test(arguments: [
            "once-window", "once-document", "remove-document", "once-element", "remove-element",
            "active-remove", "active-abort"
        ])
        func oldGraphsFollowNativeListenerOwnership(_ scenario: String) async throws {
            try await WebTestHarness.withPage { _, page async throws in
                try await page.showPresentedQuestion("")
                try await page.run(Self.installProbe, ["scenario": scenario])
                try await page.showPresentedAnswer("")

                if scenario.starts(with: "active-") {
                    try await Self.collect(page)
                    let active = try await page.json([String: Reachability].self, Self.snapshot)
                    let native = active.filter { $0.key.hasPrefix("native-") }.map(\.value)
                    try #require(native.count == 2 && native.allSatisfy { $0.alive && !$0.finalized },
                                 "pristine native active listener did not retain its callback/root")
                    #expect(active.values.allSatisfy { $0.alive && !$0.finalized })
                    #expect(try await page.hasCardListener("keydown"), "active wrapped registration must remain detectable")
                    try await page.run("window.__listenerOwnership.release(abort);", ["abort": scenario == "active-abort"])
                    #expect(try await !page.hasCardListener("keydown"))
                }

                var last: [String: Reachability] = [:]
                for _ in 0..<12 {
                    try await Self.collect(page)
                    // Only booleans/labels cross IPC; no deref result survives
                    // into the next task or GC request.
                    last = try await page.json([String: Reachability].self, Self.snapshot)
                    if last.values.allSatisfy({ !$0.alive && $0.finalized }) { break }
                }
                let native = last.filter { $0.key.hasPrefix("native-") }.map(\.value)
                let expected = scenario.hasSuffix("element") ? 3 : 2
                try #require(native.count == expected && native.allSatisfy { !$0.alive && $0.finalized },
                             "\(scenario): native collection control failed; cannot judge wrapped result")
                let wrapped = last.filter { $0.key.hasPrefix("wrapped-") }.map(\.value)
                #expect(wrapped.count == expected && wrapped.allSatisfy { !$0.alive && $0.finalized },
                        "\(scenario): wrapped callback/root/target retained after native control collected")
                #expect(try await !page.hasCardListener("keydown"), "dead callback metadata should be pruned")
                if scenario == "remove-document" || scenario.hasSuffix("element") {
                    #expect(try await page.bool("!window.__listenerOwnership.controller.signal.aborted"))
                }
            }
        }
    }
}
