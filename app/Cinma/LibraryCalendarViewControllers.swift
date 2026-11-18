import AppKit
import SwiftUI

/// Library grid (`Kütüphane`) + Local Files Scanner (#N23) + WebDAV Browser (#N24).
@MainActor
final class LibraryViewController: NSViewController, NSCollectionViewDataSource, NSCollectionViewDelegate, NSSearchFieldDelegate {
    private let core: CinmaCore
    private let onOpenDetail: (MetaPreviewInfo) -> Void
    var onOpenHome: (() -> Void)?
    var onPlay: ((PlayRequest) -> Void)?
    var onOpenSettings: (() -> Void)?
    var onBuildContextMenu: ((MetaPreviewInfo) -> NSMenu?)?

    private let sourceSegment = NSSegmentedControl(labels: [
        String(localized: "Library"),
        String(localized: "Local Files"),
    ], trackingMode: .selectOne, target: nil, action: nil)
    private lazy var webdavButton = CinemaTheme.makePillButton(
        title: "WebDAV…",
        symbol: "externaldrive.connected.to.line.below",
        primary: false,
        target: self,
        action: #selector(openWebDavBrowser)
    )
    private let localFilterModel = LocalFilesFilterModel()
    private var localFilesHostView: NSHostingView<LocalFilesLibraryView>?
    private var libraryScroll = NSScrollView()

    private let filterField = NSSearchField()
    private let typeSegment = NSSegmentedControl(labels: [
        String(localized: "All"),
        String(localized: "Movies"),
        String(localized: "Series"),
    ], trackingMode: .selectOne, target: nil, action: nil)
    private let sortPopup = NSPopUpButton()
    private let collectionView = KeyboardCollectionView()
    private let emptyStack = NSStackView()
    private let emptyLabel = NSTextField(wrappingLabelWithString: String(localized: "Your library is empty. Add movies or shows from Home or Discover."))
    private lazy var exploreButton = CinemaTheme.makePillButton(
        title: String(localized: "Explore Home"),
        symbol: "house",
        primary: true,
        target: self,
        action: #selector(exploreHomeClicked)
    )
    private var allItems: [MetaPreviewInfo] = []
    private var items: [MetaPreviewInfo] = []
    private var posterStyle = PosterStyleContext()

    init(core: CinmaCore, onOpenDetail: @escaping (MetaPreviewInfo) -> Void) {
        self.core = core
        self.onOpenDetail = onOpenDetail
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = CinemaTheme.background.cgColor

        let titleLabel = NSTextField(labelWithString: String(localized: "Library"))
        titleLabel.font = .systemFont(ofSize: 32, weight: .heavy)
        titleLabel.textColor = CinemaTheme.textPrimary

        filterField.placeholderString = String(localized: "Filter library…")
        filterField.delegate = self
        filterField.target = self
        filterField.action = #selector(libraryFilterChanged)
        filterField.translatesAutoresizingMaskIntoConstraints = false
        filterField.widthAnchor.constraint(equalToConstant: 210).isActive = true

        sourceSegment.selectedSegment = 0
        sourceSegment.target = self
        sourceSegment.action = #selector(sourceSegmentChanged)

        typeSegment.selectedSegment = 0
        typeSegment.target = self
        typeSegment.action = #selector(libraryFilterChanged)

        sortPopup.target = self
        sortPopup.action = #selector(libraryFilterChanged)
        sortPopup.addItems(withTitles: [
            String(localized: "Recent"),
            String(localized: "Title A → Z"),
            String(localized: "Title Z → A"),
        ])

        webdavButton.heightAnchor.constraint(equalToConstant: 30).isActive = true

        let headerRow = NSStackView(views: [titleLabel, sourceSegment, webdavButton, NSView(), typeSegment, sortPopup, filterField])
        headerRow.orientation = .horizontal
        headerRow.alignment = .centerY
        headerRow.spacing = 12
        headerRow.translatesAutoresizingMaskIntoConstraints = false

        let layout = NSCollectionViewFlowLayout()
        layout.itemSize = PosterCardItem.itemSize
        layout.minimumInteritemSpacing = 18
        layout.minimumLineSpacing = 24
        layout.sectionInset = NSEdgeInsets(top: 12, left: 64, bottom: 32, right: 64)

        collectionView.collectionViewLayout = layout
        collectionView.backgroundColors = [.clear]
        collectionView.isSelectable = true
        collectionView.dataSource = self
        collectionView.delegate = self
        collectionView.register(PosterCardItem.self, forItemWithIdentifier: PosterCardItem.identifier)
        collectionView.onActivateSelection = { [weak self] ip in
            guard let self, self.items.indices.contains(ip.item) else { return }
            self.onOpenDetail(self.items[ip.item])
        }

        let scroll = NSScrollView()
        scroll.documentView = collectionView
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.translatesAutoresizingMaskIntoConstraints = false
        self.libraryScroll = scroll

        let localView = LocalFilesLibraryView(
            core: core,
            model: localFilterModel,
            onPlayFile: { [weak self] url in self?.onPlay?(.file(url)) },
            onOpenSettings: { [weak self] in self?.onOpenSettings?() }
        )
        let host = NSHostingView(rootView: localView)
        host.translatesAutoresizingMaskIntoConstraints = false
        host.isHidden = true
        self.localFilesHostView = host

        emptyLabel.font = .systemFont(ofSize: 15, weight: .regular)
        emptyLabel.textColor = CinemaTheme.textMuted
        emptyLabel.alignment = .center
        emptyStack.orientation = .vertical
        emptyStack.alignment = .centerX
        emptyStack.spacing = 12
        emptyStack.addArrangedSubview(emptyLabel)
        emptyStack.addArrangedSubview(exploreButton)
        emptyStack.translatesAutoresizingMaskIntoConstraints = false

        root.addSubview(headerRow)
        root.addSubview(scroll)
        root.addSubview(host)
        root.addSubview(emptyStack)

        NSLayoutConstraint.activate([
            headerRow.topAnchor.constraint(equalTo: root.topAnchor, constant: 80),
            headerRow.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 64),
            headerRow.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -64),

            scroll.topAnchor.constraint(equalTo: headerRow.bottomAnchor, constant: 14),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            host.topAnchor.constraint(equalTo: headerRow.bottomAnchor, constant: 14),
            host.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            host.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            emptyStack.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            emptyStack.centerYAnchor.constraint(equalTo: root.centerYAnchor),
            emptyLabel.widthAnchor.constraint(lessThanOrEqualToConstant: 420),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        reload()
    }

    func reload() {
        posterStyle = PosterStyleContext.load(from: core)
        if let layout = collectionView.collectionViewLayout as? NSCollectionViewFlowLayout {
            layout.itemSize = posterStyle.itemSize
        }
        let rows = (try? core.libraryItems()) ?? []
        allItems = rows.map {
            MetaPreviewInfo(
                id: $0.metaId,
                mediaType: $0.mediaType,
                name: $0.name,
                poster: $0.poster,
                posterShape: "poster",
                background: nil,
                logo: nil,
                description: nil,
                releaseInfo: nil,
                imdbRating: nil,
                genres: [],
                runtime: nil
            )
        }
        applyLibraryFilterAndSort()
        localFilterModel.refreshToken += 1
    }

    func controlTextDidChange(_ obj: Notification) {
        applyLibraryFilterAndSort()
    }

    @objc private func sourceSegmentChanged() {
        let localMode = sourceSegment.selectedSegment == 1
        sortPopup.isHidden = localMode
        libraryScroll.isHidden = localMode
        localFilesHostView?.isHidden = !localMode
        emptyStack.isHidden = localMode || !items.isEmpty
        if localMode {
            localFilterModel.refreshToken += 1
        }
    }

    @objc private func openWebDavBrowser() {
        var sheetVC: NSViewController?
        let sheet = WebDavBrowserSheetView(
            core: core,
            onPlayUrl: { [weak self] url, title in
                self?.onPlay?(.trailer(url: url, title: title))
            },
            onDismiss: { [weak self] in
                if let vc = sheetVC { self?.dismiss(vc) }
            }
        )
        let host = NSHostingController(rootView: sheet)
        sheetVC = host
        presentAsSheet(host)
    }

    @objc private func libraryFilterChanged() {
        applyLibraryFilterAndSort()
    }

    private func applyLibraryFilterAndSort() {
        localFilterModel.filterQuery = filterField.stringValue
        localFilterModel.mediaFilter = typeSegment.selectedSegment
        let query = filterField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let selectedType = typeSegment.selectedSegment
        var filtered = allItems.filter { item in
            if selectedType == 1 && item.mediaType != "movie" { return false }
            if selectedType == 2 && item.mediaType != "series" { return false }
            if !query.isEmpty && !item.name.lowercased().contains(query) { return false }
            return true
        }
        switch sortPopup.indexOfSelectedItem {
        case 1:
            filtered.sort { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        case 2:
            filtered.sort { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedDescending }
        default:
            break
        }
        items = filtered
        let localMode = sourceSegment.selectedSegment == 1
        emptyStack.isHidden = localMode || !items.isEmpty
        if !allItems.isEmpty && items.isEmpty {
            emptyLabel.stringValue = String(localized: "No matching titles in your library.")
            exploreButton.isHidden = true
        } else {
            emptyLabel.stringValue = String(localized: "Your library is empty. Add movies or shows from Home or Discover.")
            exploreButton.isHidden = false
        }
        collectionView.reloadData()
    }

    @objc private func exploreHomeClicked() {
        onOpenHome?()
    }

    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int {
        items.count
    }

    func collectionView(
        _ collectionView: NSCollectionView,
        itemForRepresentedObjectAt indexPath: IndexPath
    ) -> NSCollectionViewItem {
        let cell = collectionView.makeItem(withIdentifier: PosterCardItem.identifier, for: indexPath)
        if let card = cell as? PosterCardItem, items.indices.contains(indexPath.item) {
            card.configure(with: items[indexPath.item], style: posterStyle)
            card.onContextMenu = onBuildContextMenu
        }
        return cell
    }

    func collectionView(_ collectionView: NSCollectionView, didSelectItemsAt indexPaths: Set<IndexPath>) {
        guard let first = indexPaths.first,
              items.indices.contains(first.item),
              NSApp.currentEvent?.type == .leftMouseDown || NSApp.currentEvent?.type == .leftMouseUp
        else { return }
        collectionView.deselectItems(at: indexPaths)
        onOpenDetail(items[first.item])
    }
}

/// Upcoming episodes of library series from TMDB air dates (`Takvim`).
@MainActor
final class CalendarViewController: NSViewController {
    private let core: CinmaCore
    private let onOpenDetail: (MetaPreviewInfo) -> Void
    var onOpenSettings: (() -> Void)?

    private let listStack = NSStackView()
    private let statusLabel = NSTextField(wrappingLabelWithString: "")
    private lazy var settingsButton = CinemaTheme.makePillButton(
        title: String(localized: "Open Settings…"),
        symbol: "gearshape",
        primary: true,
        target: self,
        action: #selector(openSettingsClicked)
    )
    private var thumbTasks: [Task<Void, Never>] = []

    init(core: CinmaCore, onOpenDetail: @escaping (MetaPreviewInfo) -> Void) {
        self.core = core
        self.onOpenDetail = onOpenDetail
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = CinemaTheme.background.cgColor

        let titleLabel = NSTextField(labelWithString: String(localized: "Calendar"))
        titleLabel.font = .systemFont(ofSize: 32, weight: .heavy)
        titleLabel.textColor = CinemaTheme.textPrimary
        titleLabel.translatesAutoresizingMaskIntoConstraints = false

        statusLabel.font = .systemFont(ofSize: 13.5, weight: .regular)
        statusLabel.textColor = CinemaTheme.textMuted
        statusLabel.translatesAutoresizingMaskIntoConstraints = false

        listStack.orientation = .vertical
        listStack.alignment = .leading
        listStack.spacing = 12
        listStack.translatesAutoresizingMaskIntoConstraints = false

        let doc = NSView()
        doc.translatesAutoresizingMaskIntoConstraints = false
        doc.addSubview(listStack)

        let scroll = NSScrollView()
        scroll.documentView = doc
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.translatesAutoresizingMaskIntoConstraints = false

        settingsButton.isHidden = true
        settingsButton.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(titleLabel)
        root.addSubview(statusLabel)
        root.addSubview(settingsButton)
        root.addSubview(scroll)

        NSLayoutConstraint.activate([
            titleLabel.topAnchor.constraint(equalTo: root.topAnchor, constant: 84),
            titleLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 64),

            statusLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 6),
            statusLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 64),
            statusLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -64),

            settingsButton.topAnchor.constraint(equalTo: statusLabel.bottomAnchor, constant: 12),
            settingsButton.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 64),

            scroll.topAnchor.constraint(equalTo: settingsButton.bottomAnchor, constant: 14),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            doc.topAnchor.constraint(equalTo: scroll.contentView.topAnchor),
            doc.leadingAnchor.constraint(equalTo: scroll.contentView.leadingAnchor),
            doc.trailingAnchor.constraint(equalTo: scroll.contentView.trailingAnchor),
            doc.widthAnchor.constraint(equalTo: scroll.widthAnchor),

            listStack.topAnchor.constraint(equalTo: doc.topAnchor),
            listStack.leadingAnchor.constraint(equalTo: doc.leadingAnchor, constant: 64),
            listStack.trailingAnchor.constraint(equalTo: doc.trailingAnchor, constant: -64),
            listStack.bottomAnchor.constraint(equalTo: doc.bottomAnchor, constant: -32),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        reload()
    }

    func reload() {
        let hasTmdb = (try? core.hasTmdbKey()) ?? false
        if !hasTmdb {
            statusLabel.stringValue = String(localized: "Add a TMDB API key in Settings to track upcoming episode air dates.")
            settingsButton.isHidden = false
            renderEntries([])
            return
        }
        settingsButton.isHidden = true
        statusLabel.stringValue = String(localized: "Checking upcoming episodes for library shows…")
        let core = core
        Task { [weak self] in
            let entries = (try? await core.calendarEntries()) ?? []
            guard let self else { return }
            self.statusLabel.stringValue = entries.isEmpty
                ? String(localized: "No upcoming episodes scheduled for shows in your library.")
                : String(localized: "\(entries.count) upcoming episodes")
            self.renderEntries(entries)
        }
    }

    @objc private func openSettingsClicked() {
        onOpenSettings?()
    }

    private static let isoDateParser: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        f.locale = Locale(identifier: "en_US_POSIX")
        return f
    }()

    private static let weekdayFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "EE · yyyy-MM-dd"
        return f
    }()

    private static func formattedAirDate(_ raw: String, weekStartsMonday: Bool) -> String {
        guard let date = isoDateParser.date(from: raw) else { return raw }
        var cal = Calendar.current
        cal.firstWeekday = weekStartsMonday ? 2 : 1
        weekdayFormatter.calendar = cal
        return weekdayFormatter.string(from: date)
    }

    private func renderEntries(_ entries: [UpcomingEpisodeInfo]) {
        thumbTasks.forEach { $0.cancel() }
        thumbTasks.removeAll()
        for v in listStack.arrangedSubviews {
            listStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        let weekMonday = (try? core.playerPreferences())?.weekStartsMonday ?? true
        for ep in entries {
            let formattedDate = Self.formattedAirDate(ep.airDate, weekStartsMonday: weekMonday)
            let row = NSButton(title: "", target: self, action: #selector(openEntry(_:)))
            row.identifier = NSUserInterfaceItemIdentifier("\(ep.showId)|\(ep.showName)")
            row.isBordered = false
            row.wantsLayer = true
            row.layer?.cornerRadius = 16
            row.layer?.backgroundColor = CinemaTheme.cardBackground.cgColor
            row.layer?.borderWidth = 1
            row.layer?.borderColor = CinemaTheme.cardBorder.cgColor
            row.setAccessibilityLabel("\(ep.showName) S\(ep.season)E\(ep.episode) \(ep.episodeName), \(ep.airDate)")

            let img = AspectFillImageView()
            img.layer?.cornerRadius = 10
            img.layer?.masksToBounds = true
            img.layer?.backgroundColor = NSColor(white: 1, alpha: 0.06).cgColor
            img.translatesAutoresizingMaskIntoConstraints = false
            var task: Task<Void, Never>?
            img.setRemoteImage(ep.still ?? ep.showPoster, pointSize: NSSize(width: 160, height: 90), taskStorage: &task)
            if let task { thumbTasks.append(task) }

            let title = NSTextField(labelWithString: "\(ep.showName) — S\(ep.season) · B\(ep.episode) \(ep.episodeName)")
            title.font = .systemFont(ofSize: 15, weight: .semibold)
            title.textColor = CinemaTheme.textPrimary

            let overview = NSTextField(wrappingLabelWithString: ep.overview ?? "")
            overview.font = .systemFont(ofSize: 13, weight: .regular)
            overview.textColor = CinemaTheme.textMuted
            overview.maximumNumberOfLines = 2

            let dateChip = CinemaTheme.makeChip(formattedDate, highlighted: true)

            let textCol = NSStackView(views: [title, overview])
            textCol.orientation = .vertical
            textCol.alignment = .leading
            textCol.spacing = 4

            let hStack = NSStackView(views: [img, textCol, dateChip])
            hStack.orientation = .horizontal
            hStack.alignment = .centerY
            hStack.spacing = 16
            hStack.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(hStack)

            NSLayoutConstraint.activate([
                img.widthAnchor.constraint(equalToConstant: 140),
                img.heightAnchor.constraint(equalToConstant: 78),
                hStack.topAnchor.constraint(equalTo: row.topAnchor, constant: 12),
                hStack.bottomAnchor.constraint(equalTo: row.bottomAnchor, constant: -12),
                hStack.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: 12),
                hStack.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -16),
            ])
            listStack.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: listStack.widthAnchor).isActive = true
        }
    }

    @objc private func openEntry(_ sender: NSButton) {
        guard let raw = sender.identifier?.rawValue else { return }
        let parts = raw.split(separator: "|", maxSplits: 1).map(String.init)
        guard parts.count == 2 else { return }
        onOpenDetail(MetaPreviewInfo(
            id: parts[0],
            mediaType: "series",
            name: parts[1],
            poster: nil,
            posterShape: "poster",
            background: nil,
            logo: nil,
            description: nil,
            releaseInfo: nil,
            imdbRating: nil,
            genres: [],
            runtime: nil
        ))
    }
}
