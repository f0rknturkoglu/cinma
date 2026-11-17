import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    private var home: HomeViewController?
    private var core: CinmaCore?

    func applicationDidFinishLaunching(_ notification: Notification) {
        LaunchMetrics.report("DID_FINISH_LAUNCHING")
        NSApp.mainMenu = MainMenu.make()

        let home = HomeViewController()
        let window = MainWindow.make(content: home)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        self.home = home
        self.window = window

        // Runs after this run-loop pass commits the first frame.
        DispatchQueue.main.async {
            LaunchMetrics.report("FIRST_FRAME")
            self.openCore()
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        // Becomes `false` once the menu bar extra exists (Phase 5).
        true
    }

    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
        true
    }

    /// Opens the Rust core off the main thread (it touches disk).
    private func openCore() {
        let dataDir = Self.dataDirectory().path
        Task.detached(priority: .userInitiated) {
            let result = Result { () throws -> (CinmaCore, UInt32) in
                let core = try CinmaCore.open(dataDir: dataDir)
                return (core, try core.schemaVersion())
            }
            await self.coreDidOpen(result)
        }
    }

    private func coreDidOpen(_ result: Result<(CinmaCore, UInt32), any Error>) {
        switch result {
        case let .success((core, schema)):
            self.core = core
            home?.showStatus(
                String(localized: "Core \(coreVersion()) · schema v\(Int(schema))")
            )
            LaunchMetrics.report("CORE_READY")
        case let .failure(error):
            home?.showStatus(
                String(localized: "Core failed to start: \(error.localizedDescription)")
            )
        }
    }

    private static func dataDirectory() -> URL {
        if let override = ProcessInfo.processInfo.environment["CINMA_DATA_DIR"] {
            return URL(fileURLWithPath: override, isDirectory: true)
        }
        return URL.applicationSupportDirectory.appending(path: "Cinma", directoryHint: .isDirectory)
    }
}
