import AppKit

// Manual AppKit entry point: no storyboard, no SwiftUI App lifecycle in the launch path.
LaunchMetrics.report("MAIN")
let delegate = AppDelegate()
let app = NSApplication.shared
app.delegate = delegate
app.run()
