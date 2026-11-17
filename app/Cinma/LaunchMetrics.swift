import Foundation

/// Prints launch milestones to stderr when `CINMA_MEASURE=1` (read by tools/measure.sh).
/// Times are measured from the kernel's process start time, so they include dyld.
enum LaunchMetrics {
    static let isEnabled = ProcessInfo.processInfo.environment["CINMA_MEASURE"] == "1"

    static func report(_ milestone: String) {
        guard isEnabled, let ms = millisecondsSinceProcessStart() else { return }
        let line = "CINMA_\(milestone)_MS=\(String(format: "%.1f", ms))\n"
        FileHandle.standardError.write(Data(line.utf8))
    }

    private static func millisecondsSinceProcessStart() -> Double? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&mib, u_int(mib.count), &info, &size, nil, 0) == 0 else { return nil }
        let start = info.kp_proc.p_starttime
        var now = timeval()
        gettimeofday(&now, nil)
        return Double(now.tv_sec - start.tv_sec) * 1000 + Double(now.tv_usec - start.tv_usec) / 1000
    }
}
