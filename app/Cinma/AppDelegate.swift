import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var window: NSWindow?
    private var home: HomeViewController?
    private var core: CinmaCore?
    private var shell: MainShellViewController?
    private var menuBar: MenuBarController?
    private var miniPanel: MiniPlayerPanel?
    private var pipPlayerVC: PlayerViewController?
    private var savedBrowseState: MainShellViewController.BrowseState?
    private var pendingLinks: [URL] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        LaunchMetrics.report("DID_FINISH_LAUNCHING")
        // Static defaults now (no DB on the launch path); user bindings once
        // the core is open.
        NSApp.mainMenu = MainMenu.make(commands: defaultCommands(), delegate: self)

        let home = HomeViewController()
        let window = MainWindow.make(content: home)
        window.delegate = self
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
        !(menuBar?.isInstalled ?? false)
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { reopenMainWindow() }
        return true
    }

    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
        true
    }

    func windowWillClose(_ notification: Notification) {
        ImagePipeline.shared.clearMemoryCache()
        // When keeping Cinma in the menu bar with no open window, release heavy
        // browse/detail view hierarchy so idle closed-window footprint stays low.
        if menuBar?.isInstalled == true, (try? core?.hasPendingDownloads()) == false {
            window?.contentViewController = nil
            shell = nil
            home = nil
        }
    }

    private func reopenMainWindow() {
        if let window, window.isVisible {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate()
            return
        }
        let targetWindow: NSWindow
        if let existing = window {
            targetWindow = existing
        } else {
            let placeholder = HomeViewController()
            home = placeholder
            let w = MainWindow.make(content: placeholder)
            w.delegate = self
            window = w
            targetWindow = w
        }
        if core != nil {
            showBrowse()
        }
        targetWindow.makeKeyAndOrderFront(nil)
        NSApp.activate()
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
            if let prefs = try? core.playerPreferences() {
                CinemaTheme.syncThemePreferences(from: prefs)
            }
            LaunchMetrics.report("CORE_READY")
            try? core.subscribe(listener: CoreEventSink { [weak self] event in
                Task { @MainActor in self?.handle(event) }
            })
            let bar = MenuBarController(
                core: core,
                onShowWindow: { [weak self] in self?.reopenMainWindow() },
                onShowDownloads: { [weak self] in
                    self?.reopenMainWindow()
                    self?.shell?.openDownloadsTab()
                }
            )
            bar.syncEnabledState()
            self.menuBar = bar
            rebuildMenu()
            showBrowse()
            NotificationCenter.default.addObserver(
                forName: .cinmaExternalRoute,
                object: nil,
                queue: .main
            ) { [weak self] note in
                if let url = note.object as? URL {
                    Task { @MainActor in self?.handleLink(url) }
                }
            }
            CalendarNotifications.refreshScheduleIfEnabled(core: core)
            SpotlightLibraryIndexer.indexLibraryIfEnabled(core: core)
            let links = pendingLinks + [DevAutomation.openURL].compactMap { $0 }
            pendingLinks = []
            links.forEach(handleLink)
            DevAutomation.run(core: core) { [weak self] request in self?.showPlayer(request) }
        case let .failure(error):
            home?.showStatus(
                String(localized: "Core failed to start: \(FriendlyError.explain(error))")
            )
        }
    }

    private func showBrowse() {
        guard let core, let window else { return }
        ImagePipeline.shared.setPlaybackMode(false)
        if let shell {
            MainWindow.setContent(shell, in: window)
            shell.handleLibraryChanged()
            return
        }
        let shell = MainShellViewController(core: core) { [weak self] request in
            self?.showPlayer(request)
        }
        if let saved = savedBrowseState {
            savedBrowseState = nil
            shell.restoreBrowseState(saved)
        }
        self.shell = shell
        MainWindow.setContent(shell, in: window)
    }

    private func rebuildMenu() {
        guard let core, let commands = try? core.commands() else { return }
        NSApp.mainMenu = MainMenu.make(commands: commands, delegate: self)
    }

    private func handle(_ event: CoreEventKind) {
        switch event {
        case .addonsChanged: shell?.handleAddonsChanged()
        case .keyBindingsChanged: rebuildMenu()
        case .profileChanged: shell?.handleProfileChanged()
        case .libraryChanged:
            shell?.handleLibraryChanged()
            if let core {
                SpotlightLibraryIndexer.indexLibraryIfEnabled(core: core)
                CalendarNotifications.refreshScheduleIfEnabled(core: core)
            }
        case .downloadsChanged:
            menuBar?.syncEnabledState()
            shell?.handleDownloadsChanged()
        case .castChanged, .trackingChanged:
            break
        }
    }

    // MARK: - Commands

    @objc func openFile(_ sender: Any?) {
        shell?.openFile()
    }

    @objc func showAbout(_ sender: Any?) {
        AboutWindow.show()
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
        guard let core else { return }
        if url.scheme?.lowercased() == "cinma" {
            let host = url.host?.lowercased() ?? ""
            if host == "oauth" {
                if shell == nil { showBrowse() }
                shell?.showStatus(String(localized: "Completing sign-in…"))
                Task {
                    do {
                        let provider = try await core.completeTrackingOauthCallback(callbackUrl: url.absoluteString)
                        shell?.showStatus(String(localized: "Signed in to \(provider.capitalized)"))
                    } catch {
                        shell?.showStatus(FriendlyError.explain(error))
                    }
                }
                return
            }
            if host == "continue" {
                reopenMainWindow()
                shell?.continueMostRecent()
                return
            }
            if host == "open" {
                let parts = url.pathComponents.filter { $0 != "/" }
                if parts.count >= 2 {
                    reopenMainWindow()
                    shell?.openById(mediaType: parts[0], id: parts[1])
                }
                return
            }
        }
        guard let invocation = parseDeepLink(url: url.absoluteString) else { return }
        switch invocation.command {
        case "addon.install":
            guard let manifest = invocation.argument else { return }
            if shell == nil { showBrowse() }
            shell?.showStatus(String(localized: "Checking addon…"))
            Task {
                do {
                    let preview = try await core.inspectAddon(url: manifest)
                    let confirmNeeded = (try? core.playerPreferences())?.confirmStremioDeeplink ?? true
                    if confirmNeeded {
                        guard await confirmAddonInstall(preview) else {
                            shell?.showStatus(String(localized: "Addon installation cancelled"))
                            return
                        }
                    }
                    shell?.showStatus(String(localized: "Installing addon…"))
                    let addon = try await core.installAddon(url: preview.transportUrl)
                    shell?.showStatus(String(localized: "Installed \(addon.name) \(addon.version)"))
                    DevAutomation.log("deep link installed \(addon.id)")
                } catch {
                    shell?.showStatus(String(localized: "Install failed: \(FriendlyError.explain(error))"))
                }
            }
        default:
            break
        }
    }

    private func confirmAddonInstall(_ addon: AddonInfo) async -> Bool {
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = String(localized: "Install addon “\(addon.name)”?")
        var details = [String]()
        if let desc = addon.description?.trimmingCharacters(in: .whitespacesAndNewlines), !desc.isEmpty {
            details.append(desc)
        }
        details.append(addon.transportUrl)
        alert.informativeText = details.joined(separator: "\n\n")
        alert.addButton(withTitle: String(localized: "Install"))
        alert.addButton(withTitle: String(localized: "Cancel"))
        if let window {
            return await alert.beginSheetModal(for: window) == .alertFirstButtonReturn
        }
        return alert.runModal() == .alertFirstButtonReturn
    }

    private func showPlayer(_ request: PlayRequest) {
        guard let core, let window else { return }
        dismissMiniPlayerAndStop(closePlayer: true)
        if let shell {
            savedBrowseState = shell.currentBrowseState()
            self.shell = nil
        }
        ImagePipeline.shared.setPlaybackMode(true)
        let player = PlayerViewController(core: core, request: request) { [weak self] in
            self?.showBrowse()
        }
        player.onEnterPiP = { [weak self, weak player] in
            guard let self, let player else { return }
            self.enterMiniPlayer(player)
        }
        MainWindow.setContent(player, in: window)
    }

    private func enterMiniPlayer(_ player: PlayerViewController) {
        guard miniPanel == nil else { return }
        let title = player.playerTitleForPiP()
        let videoView = player.detachVideoViewForPiP()
        let panel = MiniPlayerPanel(
            videoView: videoView,
            title: title,
            isPaused: player.isPausedForPiP,
            onTogglePause: { [weak player] in player?.togglePauseFromPiP() },
            onExpand: { [weak self] in self?.exitMiniPlayerToWindow() },
            onClosePanel: { [weak self] in self?.dismissMiniPlayerAndStop(closePlayer: true) }
        )
        player.onPauseStateChanged = { [weak panel] paused in
            panel?.updatePaused(paused)
        }
        pipPlayerVC = player
        miniPanel = panel
        showBrowse()
        panel.orderFront(nil)
    }

    private func exitMiniPlayerToWindow() {
        guard let player = pipPlayerVC else { return }
        player.onPauseStateChanged = nil
        miniPanel?.orderOut(nil)
        miniPanel = nil
        pipPlayerVC = nil
        if let shell {
            savedBrowseState = shell.currentBrowseState()
            self.shell = nil
        }
        ImagePipeline.shared.setPlaybackMode(true)
        player.reattachVideoViewFromPiP()
        reopenMainWindow()
        if let window {
            MainWindow.setContent(player, in: window)
        }
    }

    private func dismissMiniPlayerAndStop(closePlayer: Bool) {
        if let player = pipPlayerVC {
            player.onPauseStateChanged = nil
            miniPanel?.orderOut(nil)
            miniPanel = nil
            pipPlayerVC = nil
            if closePlayer {
                player.reattachVideoViewFromPiP()
                player.closeFromPiP()
            }
        }
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
