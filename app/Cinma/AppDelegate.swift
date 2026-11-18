import AppKit
@preconcurrency import CoreSpotlight
import UserNotifications

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
    private var knownIncompleteDownloads: Set<Int64> = []
    private var lastClipboardChangeCount: Int = NSPasteboard.general.changeCount
    private var clipboardPromptOpen = false
    private var downloadScheduleTimer: Timer?

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

        NotificationCenter.default.addObserver(
            forName: .cinmaExternalRoute,
            object: nil,
            queue: .main
        ) { [weak self] note in
            if let url = note.object as? URL {
                Task { @MainActor in
                    guard let self else { return }
                    if self.core != nil {
                        self.handleLink(url)
                    } else {
                        self.pendingLinks.append(url)
                    }
                }
            }
        }

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
            for row in ((try? core.downloads()) ?? []) where row.state != "completed" && row.state != "failed" {
                knownIncompleteDownloads.insert(row.id)
            }
            syncDownloadScheduleEnforcement(core: core)
            GamepadController.shared.activePlayer = { [weak self] in
                (self?.window?.contentViewController as? PlayerViewController) ?? self?.pipPlayerVC
            }
            GamepadController.shared.activeShell = { [weak self] in
                guard self?.window?.contentViewController is MainShellViewController else { return nil }
                return self?.shell
            }
            GamepadController.shared.onStatus = { [weak self] msg in
                if let player = (self?.window?.contentViewController as? PlayerViewController) ?? self?.pipPlayerVC {
                    player.gamepadShowStatus(msg)
                } else {
                    self?.shell?.showStatus(msg)
                }
            }
            GamepadController.shared.configure(core: core)
            rebuildMenu()
            showBrowse()
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
            if let core, !(window?.contentViewController is PlayerViewController) {
                SpotlightLibraryIndexer.indexLibraryIfEnabled(core: core)
                CalendarNotifications.refreshScheduleIfEnabled(core: core)
            }
        case .downloadsChanged:
            menuBar?.syncEnabledState()
            shell?.handleDownloadsChanged()
            if let core {
                let rows = (try? core.downloads()) ?? []
                let notify = (try? core.playerPreferences())?.notifyOnDownloadComplete ?? true
                for row in rows {
                    if row.state == "completed" {
                        if knownIncompleteDownloads.remove(row.id) != nil && notify {
                            postDownloadCompletedNotification(title: row.title)
                        }
                    } else if row.state != "failed" {
                        knownIncompleteDownloads.insert(row.id)
                    }
                }
                syncDownloadScheduleEnforcement(core: core)
            }
        case .castChanged, .trackingChanged:
            break
        }
    }

    // MARK: - Commands

    @objc func openFile(_ sender: Any?) {
        shell?.openFile()
    }

    @objc func openMagnetOrUrl(_ sender: Any?) {
        let clip = NSPasteboard.general.string(forType: .string)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let prefill = (clip.hasPrefix("magnet:?") || clip.hasPrefix("http://") || clip.hasPrefix("https://")) ? clip : ""
        promptMagnetOrUrl(prefill: prefill)
    }

    func applicationDidBecomeActive(_ notification: Notification) {
        guard let core, !clipboardPromptOpen,
              (try? core.playerPreferences())?.clipboardCatcherEnabled == true
        else { return }
        let pb = NSPasteboard.general
        guard pb.changeCount != lastClipboardChangeCount else { return }
        lastClipboardChangeCount = pb.changeCount
        guard let text = pb.string(forType: .string)?.trimmingCharacters(in: .whitespacesAndNewlines),
              text.hasPrefix("magnet:?xt=")
        else { return }
        promptMagnetOrUrl(prefill: text)
    }

    private func promptMagnetOrUrl(prefill: String) {
        guard let core else { return }
        reopenMainWindow()
        clipboardPromptOpen = true
        defer { clipboardPromptOpen = false }
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = String(localized: "Open Magnet or Stream URL")
        alert.informativeText = String(localized: "Paste a magnet:? link or direct HTTP/HTTPS media URL to stream immediately or queue for download.")
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 420, height: 24))
        field.stringValue = prefill
        field.placeholderString = "magnet:?xt=urn:btih:... or https://..."
        alert.accessoryView = field
        alert.addButton(withTitle: String(localized: "Stream Now"))
        alert.addButton(withTitle: String(localized: "Download"))
        alert.addButton(withTitle: String(localized: "Cancel"))
        let resp = alert.runModal()
        let raw = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty, resp != .alertThirdButtonReturn else { return }

        if raw.hasPrefix("magnet:") {
            do {
                let stream = try core.parseMagnetStream(uri: raw)
                if resp == .alertFirstButtonReturn {
                    showPlayer(.stream(stream, metadata: nil))
                } else {
                    let title = stream.name ?? String(localized: "Magnet Download")
                    _ = try core.addTorrentDownload(title: title, stream: stream, metaId: nil, mediaType: nil, priority: 10)
                    shell?.showStatus(String(localized: "Queued magnet download: \(title)"))
                }
            } catch {
                shell?.showStatus(FriendlyError.explain(error))
            }
        } else if let url = URL(string: raw), url.scheme?.hasPrefix("http") == true {
            let name = url.lastPathComponent.isEmpty ? "Stream" : url.lastPathComponent
            if resp == .alertFirstButtonReturn {
                showPlayer(.trailer(url: raw, title: name))
            } else {
                do {
                    _ = try core.addHttpDownload(title: name, url: raw, metaId: nil, mediaType: nil, priority: 10)
                    shell?.showStatus(String(localized: "Queued download: \(name)"))
                } catch {
                    shell?.showStatus(FriendlyError.explain(error))
                }
            }
        }
    }

    private func postDownloadCompletedNotification(title: String) {
        let center = UNUserNotificationCenter.current()
        Task {
            let settings = await center.notificationSettings()
            if settings.authorizationStatus == .notDetermined {
                let granted = (try? await center.requestAuthorization(options: [.alert, .sound])) ?? false
                guard granted else { return }
            } else if settings.authorizationStatus != .authorized && settings.authorizationStatus != .provisional {
                return
            }
            let content = UNMutableNotificationContent()
            content.title = String(localized: "Download Complete")
            content.body = title
            content.sound = .default
            let req = UNNotificationRequest(identifier: "dl-\(UUID().uuidString)", content: content, trigger: nil)
            try? await center.add(req)
        }
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

    func application(
        _ application: NSApplication,
        continue userActivity: NSUserActivity,
        restorationHandler: @escaping ([any NSUserActivityRestoring]) -> Void
    ) -> Bool {
        guard userActivity.activityType == CSSearchableItemActionType,
              let identifier = userActivity.userInfo?[CSSearchableItemActivityIdentifier] as? String
        else {
            return false
        }
        let parts = identifier.split(separator: ":", maxSplits: 2)
        guard parts.count == 3, parts[0] == "cinma",
              let url = URL(string: "cinma://open/\(parts[1])/\(parts[2])")
        else {
            return false
        }
        if core != nil {
            handleLink(url)
        } else {
            pendingLinks.append(url)
        }
        return true
    }

    private func handleLink(_ url: URL) {
        guard let core else { return }
        if url.isFileURL {
            let ext = url.pathExtension.lowercased()
            if ext == "torrent" {
                reopenMainWindow()
                do {
                    let stream = try core.parseTorrentFileStream(path: url.path)
                    let title = stream.name ?? url.deletingPathExtension().lastPathComponent
                    let alert = NSAlert()
                    alert.messageText = String(localized: "Open Torrent File")
                    alert.informativeText = title
                    alert.addButton(withTitle: String(localized: "Stream Now"))
                    alert.addButton(withTitle: String(localized: "Download"))
                    alert.addButton(withTitle: String(localized: "Cancel"))
                    let resp = alert.runModal()
                    if resp == .alertFirstButtonReturn {
                        showPlayer(.stream(stream, metadata: nil))
                    } else if resp == .alertSecondButtonReturn {
                        _ = try core.addTorrentDownload(title: title, stream: stream, metaId: nil, mediaType: nil, priority: 10)
                        shell?.showStatus(String(localized: "Queued torrent download: \(title)"))
                    }
                } catch {
                    shell?.showStatus(FriendlyError.explain(error))
                }
                return
            }
            let videoExts: Set<String> = ["mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "wmv", "flv"]
            if videoExts.contains(ext) {
                reopenMainWindow()
                showPlayer(.file(url))
                return
            }
        }
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

    private func syncDownloadScheduleEnforcement(core: CinmaCore) {
        let enabled = (try? core.playerPreferences())?.downloadScheduleEnabled ?? false
        let hour = UInt8(Calendar.current.component(.hour, from: Date()))
        try? core.enforceDownloadSchedule(currentHour: hour)
        if enabled && !knownIncompleteDownloads.isEmpty {
            if downloadScheduleTimer == nil {
                downloadScheduleTimer = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in
                    Task { @MainActor [weak self] in
                        guard let self, let core = self.core else { return }
                        let h = UInt8(Calendar.current.component(.hour, from: Date()))
                        try? core.enforceDownloadSchedule(currentHour: h)
                    }
                }
            }
        } else {
            downloadScheduleTimer?.invalidate()
            downloadScheduleTimer = nil
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
