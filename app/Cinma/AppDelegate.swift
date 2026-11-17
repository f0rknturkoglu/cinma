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
            let result = Result { () throws -> CinmaCore in
                try CinmaCore.open(dataDir: dataDir)
            }
            await self.coreDidOpen(result)
        }
    }

    private func coreDidOpen(_ result: Result<CinmaCore, any Error>) {
        switch result {
        case let .success(core):
            self.core = core
            LaunchMetrics.report("CORE_READY")
            showBrowse()
            DevAutomation.run(core: core) { [weak self] request in self?.showPlayer(request) }
        case let .failure(error):
            home?.showStatus(
                String(localized: "Core failed to start: \(error.localizedDescription)")
            )
        }
    }

    private func showBrowse() {
        guard let core, let window else { return }
        let browse = SliceViewController(core: core) { [weak self] request in
            self?.showPlayer(request)
        }
        MainWindow.setContent(browse, in: window)
    }

    private func showPlayer(_ request: PlayRequest) {
        guard let core, let window else { return }
        let player = PlayerViewController(core: core, request: request) { [weak self] in
            self?.showBrowse()
        }
        MainWindow.setContent(player, in: window)
    }

    private static func dataDirectory() -> URL {
        if let override = ProcessInfo.processInfo.environment["CINMA_DATA_DIR"] {
            return URL(fileURLWithPath: override, isDirectory: true)
        }
        return URL.applicationSupportDirectory.appending(path: "Cinma", directoryHint: .isDirectory)
    }
}
