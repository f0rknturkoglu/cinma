import AppKit

/// Optional in-process macOS Menu Bar extra (`NSStatusItem`, ADR 0013):
/// - Zero background daemon process
/// - Shows active download count and quick controls
/// - Holds `ProcessInfo` App Nap activity *only* while downloads are active
@MainActor
final class MenuBarController: NSObject {
    private let core: CinmaCore
    private let onShowWindow: () -> Void
    private let onShowDownloads: () -> Void

    private var statusItem: NSStatusItem?
    private var appNapActivity: NSObjectProtocol?

    var isInstalled: Bool { statusItem != nil }

    init(
        core: CinmaCore,
        onShowWindow: @escaping () -> Void,
        onShowDownloads: @escaping () -> Void
    ) {
        self.core = core
        self.onShowWindow = onShowWindow
        self.onShowDownloads = onShowDownloads
        super.init()
    }

    func syncEnabledState() {
        let enabled = (try? core.menubarEnabled()) ?? true
        if enabled && statusItem == nil {
            install()
        } else if !enabled && statusItem != nil {
            uninstall()
        }
        refreshStatus()
    }

    private func install() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.image = NSImage(
            systemSymbolName: "film",
            accessibilityDescription: String(localized: "Cinma Menu")
        )
        item.button?.imagePosition = .imageLeading
        statusItem = item
    }

    func uninstall() {
        if let item = statusItem {
            NSStatusBar.system.removeStatusItem(item)
            statusItem = nil
        }
        endAppNapActivity()
    }

    func refreshStatus() {
        let downloads = (try? core.downloads()) ?? []
        let active = downloads.filter { $0.state == "active" || $0.state == "queued" }

        if active.isEmpty {
            endAppNapActivity()
        } else {
            beginAppNapActivity()
        }

        guard let item = statusItem else { return }
        if active.isEmpty {
            item.button?.title = ""
            item.button?.image = NSImage(
                systemSymbolName: "film",
                accessibilityDescription: String(localized: "Cinma Menu")
            )
        } else {
            item.button?.title = " \(active.count)"
            item.button?.image = NSImage(
                systemSymbolName: "arrow.down.circle",
                accessibilityDescription: String(localized: "Cinma Active Downloads")
            )
        }
        item.menu = buildMenu(downloads: downloads, active: active)
    }

    private func buildMenu(downloads: [DownloadInfo], active: [DownloadInfo]) -> NSMenu {
        let menu = NSMenu()

        let showItem = NSMenuItem(
            title: String(localized: "Show Cinma"),
            action: #selector(showWindowClicked),
            keyEquivalent: ""
        )
        showItem.target = self
        menu.addItem(showItem)

        let dlItem = NSMenuItem(
            title: String(localized: "Downloads"),
            action: #selector(showDownloadsClicked),
            keyEquivalent: ""
        )
        dlItem.target = self
        menu.addItem(dlItem)

        menu.addItem(.separator())

        if active.isEmpty {
            let idle = NSMenuItem(title: String(localized: "No active downloads"), action: nil, keyEquivalent: "")
            idle.isEnabled = false
            menu.addItem(idle)
        } else {
            for row in active.prefix(4) {
                let pct: String = if let total = row.bytesTotal, total > 0 {
                    "\(Int((Double(row.bytesDone) / Double(total)) * 100))%"
                } else {
                    "…"
                }
                let entry = NSMenuItem(title: "\(row.title) (\(pct))", action: nil, keyEquivalent: "")
                entry.isEnabled = false
                menu.addItem(entry)
            }
            let pauseAll = NSMenuItem(
                title: String(localized: "Pause All Downloads"),
                action: #selector(pauseAllClicked),
                keyEquivalent: ""
            )
            pauseAll.target = self
            menu.addItem(pauseAll)
        }

        if downloads.contains(where: { $0.state == "paused" }) {
            let resumeAll = NSMenuItem(
                title: String(localized: "Resume Paused Downloads"),
                action: #selector(resumeAllClicked),
                keyEquivalent: ""
            )
            resumeAll.target = self
            menu.addItem(resumeAll)
        }

        menu.addItem(.separator())
        let quit = NSMenuItem(
            title: String(localized: "Quit Cinma"),
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        )
        menu.addItem(quit)
        return menu
    }

    private func beginAppNapActivity() {
        guard appNapActivity == nil else { return }
        appNapActivity = ProcessInfo.processInfo.beginActivity(
            options: [.userInitiatedAllowingIdleSystemSleep, .suddenTerminationDisabled],
            reason: "Cinma active media downloads"
        )
    }

    private func endAppNapActivity() {
        guard let activity = appNapActivity else { return }
        ProcessInfo.processInfo.endActivity(activity)
        appNapActivity = nil
    }

    @objc private func showWindowClicked() { onShowWindow() }
    @objc private func showDownloadsClicked() { onShowDownloads() }

    @objc private func pauseAllClicked() {
        let rows = (try? core.downloads()) ?? []
        for row in rows where row.state == "active" || row.state == "queued" {
            try? core.pauseDownload(id: row.id)
        }
        refreshStatus()
    }

    @objc private func resumeAllClicked() {
        let rows = (try? core.downloads()) ?? []
        for row in rows where row.state == "paused" {
            try? core.resumeDownload(id: row.id)
        }
        refreshStatus()
    }
}
