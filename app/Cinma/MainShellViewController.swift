import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// Main application shell: floating Liquid Glass top navigation pill, search
/// bar, profile menu, tab container, and pushed detail screen (mockup M0).
@MainActor
final class MainShellViewController: NSViewController, NSTextFieldDelegate {
    private enum Tab: Int {
        case home = 0
        case discover = 1
        case library = 2
        case calendar = 3
        case downloads = 4
    }

    private let core: CinmaCore
    private let onPlay: (PlayRequest) -> Void

    private let contentContainer = NSView()
    private let navStack = NSStackView()
    private var tabButtons: [NSButton] = []
    private let searchField = NSTextField()
    private let profileButton = NSButton(title: "D", target: nil, action: nil)
    private let statusToast = NSTextField(labelWithString: "")
    private var statusToastContainer: NSView?

    private lazy var homeVC = HomeBrowserViewController(
        core: core,
        onOpenDetail: { [weak self] item in self?.openDetail(item) },
        onContinue: { [weak self] entry in self?.resumeContinueWatching(entry) }
    )
    private lazy var discoverVC = DiscoverViewController(core: core) { [weak self] item in
        self?.openDetail(item)
    }
    private lazy var libraryVC = LibraryViewController(core: core) { [weak self] item in
        self?.openDetail(item)
    }
    private lazy var calendarVC = CalendarViewController(core: core) { [weak self] item in
        self?.openDetail(item)
    }
    private lazy var downloadsVC = DownloadsViewController(core: core, onPlay: onPlay)

    private var activeTab: Tab = .home
    private var detailVC: TitleDetailViewController?
    private var currentVC: NSViewController?
    private var statusClearTask: Task<Void, Never>?
    private var onboardingChecked = false

    init(core: CinmaCore, onPlay: @escaping (PlayRequest) -> Void) {
        self.core = core
        self.onPlay = onPlay
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = CinemaTheme.background.cgColor

        contentContainer.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(contentContainer)

        let navPill = buildTopNavBar()
        let searchPill = buildSearchPill()
        setupProfileButton()
        let toast = buildStatusToast()

        root.addSubview(navPill)
        root.addSubview(searchPill)
        root.addSubview(profileButton)
        root.addSubview(toast)

        NSLayoutConstraint.activate([
            contentContainer.topAnchor.constraint(equalTo: root.topAnchor),
            contentContainer.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            contentContainer.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            contentContainer.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            navPill.topAnchor.constraint(equalTo: root.topAnchor, constant: 18),
            navPill.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 88),

            profileButton.centerYAnchor.constraint(equalTo: navPill.centerYAnchor),
            profileButton.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -28),
            profileButton.widthAnchor.constraint(equalToConstant: 38),
            profileButton.heightAnchor.constraint(equalToConstant: 38),

            searchPill.centerYAnchor.constraint(equalTo: navPill.centerYAnchor),
            searchPill.trailingAnchor.constraint(equalTo: profileButton.leadingAnchor, constant: -12),
            searchPill.widthAnchor.constraint(equalToConstant: 240),
            searchPill.heightAnchor.constraint(equalToConstant: 38),

            toast.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            toast.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -24),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        refreshProfileBadge()
        switchToTab(.home)
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        // A sheet needs a window to attach to; in viewDidLoad the view has none yet,
        // so AppKit shows it as a stray window and the content swap never finishes.
        guard !onboardingChecked, view.window != nil else { return }
        onboardingChecked = true
        presentOnboardingIfNeeded()
    }

    private func buildTopNavBar() -> NSView {
        let glass = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 22)
        glass.translatesAutoresizingMaskIntoConstraints = false

        navStack.orientation = .horizontal
        navStack.spacing = 4
        navStack.translatesAutoresizingMaskIntoConstraints = false

        let titles: [(Tab, String)] = [
            (.home, String(localized: "Home")),
            (.discover, String(localized: "Discover")),
            (.library, String(localized: "Library")),
            (.calendar, String(localized: "Calendar")),
            (.downloads, String(localized: "Downloads")),
        ]
        tabButtons = titles.map { tab, title in
            let btn = NSButton(title: title, target: self, action: #selector(tabButtonClicked(_:)))
            btn.tag = tab.rawValue
            btn.isBordered = false
            btn.wantsLayer = true
            btn.layer?.cornerRadius = 17
            btn.setAccessibilityLabel(title)
            btn.translatesAutoresizingMaskIntoConstraints = false
            btn.heightAnchor.constraint(equalToConstant: 34).isActive = true
            btn.widthAnchor.constraint(greaterThanOrEqualToConstant: 78).isActive = true
            navStack.addArrangedSubview(btn)
            return btn
        }

        glass.addSubview(navStack)
        NSLayoutConstraint.activate([
            navStack.topAnchor.constraint(equalTo: glass.topAnchor, constant: 4),
            navStack.bottomAnchor.constraint(equalTo: glass.bottomAnchor, constant: -4),
            navStack.leadingAnchor.constraint(equalTo: glass.leadingAnchor, constant: 5),
            navStack.trailingAnchor.constraint(equalTo: glass.trailingAnchor, constant: -5),
        ])
        return glass
    }

    private func buildSearchPill() -> NSView {
        let glass = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 19)
        glass.translatesAutoresizingMaskIntoConstraints = false

        let icon = NSImageView()
        icon.image = NSImage(systemSymbolName: "magnifyingglass", accessibilityDescription: String(localized: "Search"))
        icon.contentTintColor = CinemaTheme.textSecondary
        icon.translatesAutoresizingMaskIntoConstraints = false

        searchField.isBordered = false
        searchField.drawsBackground = false
        searchField.focusRingType = .none
        searchField.font = .systemFont(ofSize: 13.5)
        searchField.textColor = CinemaTheme.textPrimary
        searchField.placeholderString = String(localized: "Search movies, series…")
        searchField.delegate = self
        searchField.target = self
        searchField.action = #selector(submitSearch)
        searchField.translatesAutoresizingMaskIntoConstraints = false

        glass.addSubview(icon)
        glass.addSubview(searchField)
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: glass.leadingAnchor, constant: 13),
            icon.centerYAnchor.constraint(equalTo: glass.centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 15),
            icon.heightAnchor.constraint(equalToConstant: 15),

            searchField.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 8),
            searchField.trailingAnchor.constraint(equalTo: glass.trailingAnchor, constant: -12),
            searchField.centerYAnchor.constraint(equalTo: glass.centerYAnchor),
        ])
        return glass
    }

    private func setupProfileButton() {
        profileButton.target = self
        profileButton.action = #selector(showProfileMenu(_:))
        profileButton.isBordered = false
        profileButton.wantsLayer = true
        profileButton.layer?.cornerRadius = 19
        profileButton.layer?.borderWidth = 1
        profileButton.layer?.borderColor = NSColor(white: 1, alpha: 0.24).cgColor
        profileButton.layer?.backgroundColor = CinemaTheme.accent.cgColor
        profileButton.translatesAutoresizingMaskIntoConstraints = false
    }

    private func buildStatusToast() -> NSView {
        let glass = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 16)
        glass.translatesAutoresizingMaskIntoConstraints = false
        glass.isHidden = true
        statusToast.font = .systemFont(ofSize: 13, weight: .medium)
        statusToast.textColor = CinemaTheme.textPrimary
        statusToast.translatesAutoresizingMaskIntoConstraints = false
        glass.addSubview(statusToast)
        NSLayoutConstraint.activate([
            statusToast.topAnchor.constraint(equalTo: glass.topAnchor, constant: 8),
            statusToast.bottomAnchor.constraint(equalTo: glass.bottomAnchor, constant: -8),
            statusToast.leadingAnchor.constraint(equalTo: glass.leadingAnchor, constant: 16),
            statusToast.trailingAnchor.constraint(equalTo: glass.trailingAnchor, constant: -16),
        ])
        statusToastContainer = glass
        return glass
    }

    private func switchToTab(_ tab: Tab) {
        activeTab = tab
        popDetailIfNeeded()
        updateTabButtonStyles()
        let target: NSViewController
        switch tab {
        case .home:
            target = homeVC
        case .discover:
            target = discoverVC
        case .library:
            libraryVC.reload()
            target = libraryVC
        case .calendar:
            calendarVC.reload()
            target = calendarVC
        case .downloads:
            downloadsVC.reload()
            target = downloadsVC
        }
        showChild(target)
    }

    private func showChild(_ vc: NSViewController) {
        if currentVC === vc { return }
        currentVC?.view.removeFromSuperview()
        currentVC?.removeFromParent()

        addChild(vc)
        vc.view.translatesAutoresizingMaskIntoConstraints = false
        contentContainer.addSubview(vc.view)
        NSLayoutConstraint.activate([
            vc.view.topAnchor.constraint(equalTo: contentContainer.topAnchor),
            vc.view.leadingAnchor.constraint(equalTo: contentContainer.leadingAnchor),
            vc.view.trailingAnchor.constraint(equalTo: contentContainer.trailingAnchor),
            vc.view.bottomAnchor.constraint(equalTo: contentContainer.bottomAnchor),
        ])
        currentVC = vc
    }

    private func updateTabButtonStyles() {
        for btn in tabButtons {
            let selected = btn.tag == activeTab.rawValue && detailVC == nil
            btn.layer?.backgroundColor = selected ? NSColor(white: 1, alpha: 0.16).cgColor : NSColor.clear.cgColor
            btn.attributedTitle = NSAttributedString(
                string: btn.title,
                attributes: [
                    .foregroundColor: selected ? CinemaTheme.textPrimary : CinemaTheme.textSecondary,
                    .font: NSFont.systemFont(ofSize: 13.5, weight: selected ? .semibold : .medium),
                ]
            )
        }
    }

    private func openDetail(_ item: MetaPreviewInfo) {
        popDetailIfNeeded()
        let vc = TitleDetailViewController(
            core: core,
            preview: item,
            onBack: { [weak self] in
                guard let self else { return }
                self.popDetailIfNeeded()
                self.switchToTab(self.activeTab)
            },
            onPlay: onPlay
        )
        detailVC = vc
        updateTabButtonStyles()
        showChild(vc)
    }

    private func popDetailIfNeeded() {
        detailVC = nil
    }

    private func resumeContinueWatching(_ entry: WatchProgressInfo) {
        let preview = MetaPreviewInfo(
            id: entry.metaId,
            mediaType: entry.mediaType,
            name: entry.title,
            poster: entry.artwork,
            posterShape: "poster",
            background: entry.artwork,
            logo: nil,
            description: nil,
            releaseInfo: nil,
            imdbRating: nil,
            genres: [],
            runtime: nil
        )
        openDetail(preview)
    }

    // MARK: - Core Events & Status

    func showStatus(_ text: String) {
        statusToast.stringValue = text
        statusToastContainer?.isHidden = text.isEmpty
        statusClearTask?.cancel()
        guard !text.isEmpty else { return }
        statusClearTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(4))
            guard !Task.isCancelled else { return }
            self?.statusToastContainer?.isHidden = true
        }
    }

    func handleAddonsChanged() {
        homeVC.refreshAll()
        discoverVC.reloadCatalogList()
    }

    func handleProfileChanged() {
        refreshProfileBadge()
        homeVC.refreshAll()
        discoverVC.reloadCatalogList()
        libraryVC.reload()
        calendarVC.reload()
    }

    func handleLibraryChanged() {
        homeVC.reloadLocalState()
        libraryVC.reload()
    }

    func handleDownloadsChanged() {
        downloadsVC.reload()
    }

    func openDownloadsTab() {
        switchToTab(.downloads)
    }

    private func refreshProfileBadge() {
        let profiles = (try? core.profiles()) ?? []
        let activeId = (try? core.activeProfileId()) ?? 1
        let active = profiles.first(where: { $0.id == activeId }) ?? profiles.first
        let letter = String((active?.avatar ?? active?.name ?? "C").prefix(1)).uppercased()
        profileButton.attributedTitle = NSAttributedString(
            string: letter,
            attributes: [
                .foregroundColor: CinemaTheme.accentInk,
                .font: NSFont.systemFont(ofSize: 15, weight: .heavy),
            ]
        )
        profileButton.toolTip = active?.name
    }

    // MARK: - Actions & Sheets

    @objc private func tabButtonClicked(_ sender: NSButton) {
        guard let tab = Tab(rawValue: sender.tag) else { return }
        switchToTab(tab)
    }

    @objc private func submitSearch() {
        let query = searchField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if query.isEmpty {
            discoverVC.clearSearch()
            return
        }
        switchToTab(.discover)
        discoverVC.showSearch(query)
    }

    @objc private func showProfileMenu(_ sender: NSButton) {
        let menu = NSMenu()
        menu.addItem(withTitle: String(localized: "Addons…"), action: #selector(openAddons), keyEquivalent: "")
        menu.addItem(withTitle: String(localized: "Integrations…"), action: #selector(openIntegrations), keyEquivalent: "")
        menu.addItem(withTitle: String(localized: "Profiles…"), action: #selector(openProfiles), keyEquivalent: "")
        menu.addItem(withTitle: String(localized: "Settings…"), action: #selector(openSettings), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: String(localized: "Open File…"), action: #selector(openFile), keyEquivalent: "")
        for item in menu.items { item.target = self }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc func openIntegrations() {
        var holder: NSHostingController<IntegrationsSheetView>?
        let vc = NSHostingController(rootView: IntegrationsSheetView(core: core) { [weak self] in
            if let vc = holder { self?.dismiss(vc) }
        })
        holder = vc
        presentAsSheet(vc)
    }

    @objc func openAddons() {
        var holder: NSHostingController<AddonManagerView>?
        let vc = NSHostingController(rootView: AddonManagerView(core: core) { [weak self] in
            if let vc = holder { self?.dismiss(vc) }
        })
        holder = vc
        presentAsSheet(vc)
    }

    @objc func openProfiles() {
        var holder: NSHostingController<ProfilePickerView>?
        let vc = NSHostingController(rootView: ProfilePickerView(core: core) { [weak self] in
            if let vc = holder { self?.dismiss(vc) }
        })
        holder = vc
        presentAsSheet(vc)
    }

    @objc func openSettings() {
        var holder: NSHostingController<SettingsView>?
        let vc = NSHostingController(rootView: SettingsView(core: core) { [weak self] in
            if let vc = holder { self?.dismiss(vc) }
        })
        holder = vc
        presentAsSheet(vc)
    }

    private func presentOnboardingIfNeeded() {
        guard ProcessInfo.processInfo.environment["CINMA_AUTOPLAY"] == nil,
              ProcessInfo.processInfo.environment["CINMA_MEASURE"] == nil
        else { return }
        let completed = (try? core.onboardingCompleted()) ?? false
        let hasAddons = !((try? core.addons()) ?? []).isEmpty
        let hasKey = (try? core.hasTmdbKey()) ?? false
        guard !completed && !hasAddons && !hasKey else { return }

        var holder: NSHostingController<OnboardingView>?
        let vc = NSHostingController(rootView: OnboardingView(core: core) { [weak self] in
            if let vc = holder { self?.dismiss(vc) }
            self?.homeVC.refreshAll()
        })
        holder = vc
        presentAsSheet(vc)
    }

    @objc func openFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.movie, .video, .audiovisualContent]
        panel.allowsOtherFileTypes = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        onPlay(.file(url))
    }
}
