import Darwin
import Foundation

/// S1(b)'s frame gate (U65, R1807, KTD19): what `CardSwapTimingSpikeTests`
/// requires of the swap timings, by where the run happens. The mode comes
/// from the environment alone, never from the samples: a run whose probe
/// fails is held to the strict gate and fails it, instead of passing under
/// weaker criteria. `CardSwapTimingGateTests` checks it.
enum CardSwapTimingGate {
    enum Mode: Equatable, CustomStringConvertible {
        /// A device or this Mac's own screen: the Metal probe gives every
        /// swap its presented frame, and the pages must land in the
        /// baseline's frame.
        case strict
        /// The iOS simulator or a virtual machine, where no drawable reports
        /// a presented time and frames are display-link estimates: function
        /// only, with one frame of slack at p95.
        case functionOnly(String)

        var description: String {
            switch self {
            case .strict: "strict"
            case .functionOnly(let why): "function only (\(why))"
            }
        }
    }

    /// The mode for a run on a simulator or not, in a virtual machine or
    /// not (`nil`: not known, which is held to the strict gate).
    static func mode(simulator: Bool, virtualMachine: Bool?) -> Mode {
        if simulator { return .functionOnly("the iOS simulator") }
        if virtualMachine == true { return .functionOnly("a virtual machine") }
        return .strict
    }

    /// The mode for this test run: the runner shares the app's machine.
    static var current: Mode {
        #if targetEnvironment(simulator)
        mode(simulator: true, virtualMachine: nil)
        #elseif os(macOS)
        mode(simulator: false, virtualMachine: hypervisorPresent())
        #else
        mode(simulator: false, virtualMachine: false)
        #endif
    }

    /// `kern.hv_vmm_present`: true inside a virtual machine, nil when the
    /// system does not say.
    static func hypervisorPresent() -> Bool? {
        var value: Int32 = 0
        var size = MemoryLayout<Int32>.size
        guard sysctlbyname("kern.hv_vmm_present", &value, &size, nil, 0) == 0 else { return nil }
        return value != 0
    }

    /// What the report fails of the gate in `mode`; empty when it passes.
    static func problems(_ report: CardSwapTimingSpikeTests.Report, mode: Mode) -> [String] {
        let web = report.web, baseline = report.baseline
        var problems: [String] = []
        switch mode {
        case .functionOnly:
            // Display-link estimates count the main-thread work after a page
            // swap as lateness: one frame of slack at p95, nothing more.
            if web.p95Frames > baseline.p95Frames + 1 {
                problems.append("p95 frames: pages \(web.p95Frames), baseline \(baseline.p95Frames)")
            }
        case .strict:
            if report.probe != "metal" {
                problems.append("the frame probe is \(report.probe), not Metal: no presented frames")
            }
            if report.unpresented > 0 {
                problems.append("\(report.unpresented) swaps had no presented time")
            }
            let resolved = web.count + baseline.count + report.unsettled
            if resolved < report.inputs {
                problems.append("\(resolved) of \(report.inputs) inputs resolved")
            }
            if web.p95Frames > baseline.p95Frames {
                problems.append("p95 frames: pages \(web.p95Frames), baseline \(baseline.p95Frames)")
            }
            if web.fromHandlerP95Frames > baseline.fromHandlerP95Frames {
                problems.append("p95 frames from the handler: pages \(web.fromHandlerP95Frames), baseline \(baseline.fromHandlerP95Frames)")
            }
            if web.p99Frames > baseline.p99Frames + 1 {
                problems.append("p99 frames: pages \(web.p99Frames), baseline \(baseline.p99Frames)")
            }
        }
        return problems
    }
}
