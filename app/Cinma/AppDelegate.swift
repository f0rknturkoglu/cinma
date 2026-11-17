import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    private var home: HomeViewController?
    private var core: CinmaCore?
    private weak var browse: SliceViewController?
    private var pendingLinks: [URL] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        LaunchMetrics.report("DID_FINISH_LAUNCHING")
        // Static defaults now (no DB on the launch path); user bindings once
        // the core is open.
        NSApp.mainMenu = MainMenu.make(commands: defaultCommands(), delegate: self)

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
            try? core.subscribe(listener: CoreEventSink { [weak self] event in
                Task { @MainActor in self?.handle(event) }
            })
            rebuildMenu()
            showBrowse()
            let links = pendingLinks + [DevAutomation.openURL].compactMap { $0 }
            pendingLinks = []
            links.forEach(handleLink)
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
        self.browse = browse
        MainWindow.setContent(browse, in: window)
    }

    private func rebuildMenu() {
        guard let core, let commands = try? core.commands() else { return }
        NSApp.mainMenu = MainMenu.make(commands: commands, delegate: self)
    }

    private func handle(_ event: CoreEventKind) {
        switch event {
        case .addonsChanged: browse?.refreshAddons()
        case .keyBindingsChanged: rebuildMenu()
        }
    }

    // MARK: - Commands

    @objc func openFile(_ sender: Any?) {
        browse?.openFile()
    }

    // MARK: - Deep links (stremio://…)

    func application(_ application: NSApplication, open urls: [URL]) {
        guard core != nil else {
            pendingLinks.append(contentsOf: urls)
            return
        }
        urls.forEach(handleLink)
    }

    private func handleLink(_ url: URL) {
        guard let core, let invocation = parseDeepLink(url: url.absoluteString) else { return }
        switch invocation.command {
        case "addon.install":
            guard let manifest = invocation.argument else { return }
            if browse == nil { showBrowse() }
            browse?.showStatus(String(localized: "Installing addon…"))
            Task {
                do {
                    let addon = try await core.installAddon(url: manifest)
                    browse?.showStatus(String(localized: "Installed \(addon.name) \(addon.version)"))
                    DevAutomation.log("deep link installed \(addon.id)")
                } catch {
                    browse?.showStatus(String(localized: "Install failed: \(error.localizedDescription)"))
                }
            }
        default:
            break
        }
    }

    private func showPlayer(_ request: PlayRequest) {
        guard let core, let window else { return }
        let player = PlayerViewController(core: core, request: request) { [weak self] in
            self?.showBrowse()
        }
        MainWindow.setContent(player, in: window)
    }

    static func dataDirectory() -> URL {
        if let override = ProcessInfo.processInfo.environment["CINMA_DATA_DIR"] {
            return URL(fileURLWithPath: override, isDirectory: true)
        }
        return URL.applicationSupportDirectory.appending(path: "Cinma", directoryHint: .isDirectory)
    }
}

/// Forwards core events (any thread) to a handler.
final class CoreEventSink: CoreEventListener {
    private let handler: @Sendable (CoreEventKind) -> Void

    init(_ handler: @escaping @Sendable (CoreEventKind) -> Void) {
        self.handler = handler
    }

    func onEvent(event: CoreEventKind) {
        handler(event)
    }
}
