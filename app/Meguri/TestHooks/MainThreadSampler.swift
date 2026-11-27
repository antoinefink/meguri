#if MEGURI_TEST_HOOKS && os(macOS)
import Darwin
import Foundation
import QuartzCore

extension TestHooks {
    /// A sampling profiler of the main thread for `SidebarSelectionTiming`:
    /// the test VM cannot run Instruments' Time Profiler. Every millisecond a
    /// thread suspends the main thread, reads its registers and walks its
    /// frame pointers, then resumes it; nothing allocates while it is
    /// suspended. Symbols come from `dladdr` afterwards: framework frames by
    /// their nearest exported symbol, the app's own as `Meguri+0x<offset>`
    /// for `atos` against the built binary.
    nonisolated final class MainThreadSampler: @unchecked Sendable {
        static let depth = 48
        static let capacity = 12_000

        private let port: thread_act_t
        private let stackLow: UInt
        private let stackHigh: UInt
        private let frames: UnsafeMutablePointer<UInt>
        private let lengths: UnsafeMutablePointer<Int>
        private let times: UnsafeMutablePointer<Double>
        private let lock = NSLock()
        private var count = 0
        private var sampling = false
        private var threadRunning = false

        /// Call on the main thread.
        init() {
            port = mach_thread_self()
            let top = UInt(bitPattern: pthread_get_stackaddr_np(pthread_self()))
            stackHigh = top
            stackLow = top &- UInt(pthread_get_stacksize_np(pthread_self()))
            frames = .allocate(capacity: Self.capacity * Self.depth)
            lengths = .allocate(capacity: Self.capacity)
            times = .allocate(capacity: Self.capacity)
        }

        func start() {
            lock.lock()
            count = 0
            sampling = true
            // One sampling thread at a time: a stop's thread may still be sleeping.
            let spawn = !threadRunning
            threadRunning = true
            lock.unlock()
            guard spawn else { return }
            let thread = Thread { [self] in
                while true {
                    lock.lock()
                    let go = sampling && count < Self.capacity
                    if !go { threadRunning = false }
                    lock.unlock()
                    guard go else { return }
                    sample()
                    usleep(1000)
                }
            }
            thread.qualityOfService = .userInteractive
            thread.start()
        }

        func stop() {
            lock.lock()
            sampling = false
            lock.unlock()
        }

        private func sample() {
            lock.lock()
            let index = count
            lock.unlock()
            let base = frames.advanced(by: index * Self.depth)
            var state = arm_thread_state64_t()
            var stateCount = mach_msg_type_number_t(MemoryLayout<arm_thread_state64_t>.size / MemoryLayout<natural_t>.size)
            guard thread_suspend(port) == KERN_SUCCESS else { return }
            let time = CACurrentMediaTime()
            let read = withUnsafeMutablePointer(to: &state) {
                $0.withMemoryRebound(to: natural_t.self, capacity: Int(stateCount)) {
                    thread_get_state(port, ARM_THREAD_STATE64, $0, &stateCount)
                }
            }
            var length = 0
            if read == KERN_SUCCESS {
                base[0] = UInt(state.__pc)
                base[1] = UInt(state.__lr)
                length = 2
                var fp = UInt(state.__fp)
                while length < Self.depth, fp >= stackLow, fp &+ 16 <= stackHigh, fp % 8 == 0 {
                    let pointer = UnsafePointer<UInt>(bitPattern: fp)!
                    let next = pointer[0]
                    let ret = pointer[1]
                    if ret == 0 { break }
                    base[length] = ret
                    length += 1
                    guard next > fp else { break }
                    fp = next
                }
            }
            thread_resume(port)
            guard length > 0 else { return }
            lengths[index] = length
            times[index] = time
            lock.lock()
            // A restart meanwhile reset the buffer: this sample belongs to the old run.
            if count == index { count = index + 1 }
            lock.unlock()
        }

        /// The main thread's busy samples between `from` and `to`: the most
        /// frequent frames (in how many samples each appears) and leaves.
        func profile(from: Double, to: Double, top: Int = 45) -> [String: Any] {
            lock.lock()
            let total = count
            lock.unlock()
            var names: [UInt: String] = [:]
            func name(_ address: UInt) -> String {
                if let known = names[address] { return known }
                var info = Dl_info()
                var text = String(format: "0x%lx", address)
                if dladdr(UnsafeRawPointer(bitPattern: address), &info) != 0 {
                    let image = info.dli_fname.map { String(cString: $0) }.map { ($0 as NSString).lastPathComponent } ?? "?"
                    let base = UInt(bitPattern: info.dli_fbase)
                    if image == "Meguri" {
                        text = String(format: "Meguri+0x%lx", address &- base)
                    } else if let symbol = info.dli_sname {
                        text = "\(image)`\(String(cString: symbol))"
                    } else {
                        text = String(format: "\(image)+0x%lx", address &- base)
                    }
                }
                names[address] = text
                return text
            }
            var inclusive: [String: Int] = [:]
            var leaves: [String: Int] = [:]
            var busy = 0
            var idle = 0
            for index in 0..<total where times[index] >= from && times[index] <= to {
                let base = frames.advanced(by: index * Self.depth)
                let leaf = name(base[0])
                if leaf.contains("mach_msg") {
                    idle += 1
                    continue
                }
                busy += 1
                leaves[leaf, default: 0] += 1
                var seen = Set<String>()
                // The return addresses point after their call: one byte back is in it.
                for depth in 0..<lengths[index] where base[depth] > 1 {
                    let symbol = name(depth == 0 ? base[0] : base[depth] &- 1)
                    if seen.insert(symbol).inserted { inclusive[symbol, default: 0] += 1 }
                }
            }
            func ranked(_ counts: [String: Int], _ limit: Int) -> [String] {
                counts.sorted { $0.value > $1.value }.prefix(limit).map { "\($0.value) \($0.key)" }
            }
            // The event loop's own frames are in every sample: leave them out.
            let loop = ["CFRunLoop", "RunCurrentEventLoop", "NextEvent", "BlockUntilNext", "NSApplication run", "NSApplicationMain",
                        "runApp", "AppPAAE4main", "dyld`start"]
            let frameworks = inclusive.filter { entry in !entry.key.hasPrefix("Meguri+") && !loop.contains { entry.key.contains($0) } }
            let app = inclusive.filter { $0.key.hasPrefix("Meguri+") }
            return ["busySamples": busy, "idleSamples": idle, "inclusive": ranked(frameworks, top), "app": ranked(app, 80),
                    "leaves": ranked(leaves, 15)]
        }
    }
}
#endif
