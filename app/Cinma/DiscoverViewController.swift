import AppKit

/// Discover & Search screen (`Keşfet` + arama): catalog selector, genre pills,
/// pagination (`skip`), and incremental partial search results.
@MainActor
final class DiscoverViewController: NSViewController, NSCollectionViewDataSource, NSCollectionViewDelegate {
    private let core: CinmaCore
    private let onOpenDetail: (MetaPreviewInfo) -> Void
    var onOpenAddons: (() -> Void)?

    private let titleLabel = NSTextField(labelWithString: String(localized: "Discover"))
    private let subtitleLabel = NSTextField(labelWithString: "")
    private let catalogPopup = NSPopUpButton()
    private let genreStack = NSStackView()
    private let loadMoreButton = CinemaTheme.makePillButton(
        title: String(localized: "Load More"),
        primary: false,
        target: nil,
        action: #selector(loadMore)
    )
    private let emptyStack = NSStackView()
    private let emptyMessageLabel = NSTextField(wrappingLabelWithString: "")
    private let emptyActionButton = CinemaTheme.makePillButton(
        title: String(localized: "Open Addons…"),
        symbol: "puzzlepiece.extension",
        primary: true,
        target: nil,
        action: #selector(emptyActionClicked)
    )
    private let spinner = NSProgressIndicator()
    private let collectionView = KeyboardCollectionView()

    private var catalogs: [CatalogInfo] = []
    private var selectedGenre: String?
    private var items: [MetaPreviewInfo] = []
    private var seenIds: Set<String> = []
    private var hasMore = false
    private var searchModeQuery: String?
    private var activeTask: Task<Void, Never>?

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

        titleLabel.font = .systemFont(ofSize: 32, weight: .heavy)
        titleLabel.textColor = CinemaTheme.textPrimary

        subtitleLabel.font = .systemFont(ofSize: 13.5, weight: .regular)
        subtitleLabel.textColor = CinemaTheme.textMuted

        catalogPopup.target = self
        catalogPopup.action = #selector(catalogChanged)
        catalogPopup.bezelStyle = .rounded
        catalogPopup.translatesAutoresizingMaskIntoConstraints = false
        catalogPopup.widthAnchor.constraint(lessThanOrEqualToConstant: 360).isActive = true

        genreStack.orientation = .horizontal
        genreStack.alignment = .centerY
        genreStack.spacing = 8
        genreStack.translatesAutoresizingMaskIntoConstraints = false

        let genreDoc = NSView()
        genreDoc.translatesAutoresizingMaskIntoConstraints = false
        genreDoc.addSubview(genreStack)

        let genreScroll = NSScrollView()
        genreScroll.documentView = genreDoc
        genreScroll.drawsBackground = false
        genreScroll.hasHorizontalScroller = false
        genreScroll.hasVerticalScroller = false
        genreScroll.automaticallyAdjustsContentInsets = false
        genreScroll.translatesAutoresizingMaskIntoConstraints = false

        NSLayoutConstraint.activate([
            genreScroll.heightAnchor.constraint(equalToConstant: 36),
            genreDoc.topAnchor.constraint(equalTo: genreScroll.contentView.topAnchor),
            genreDoc.bottomAnchor.constraint(equalTo: genreScroll.contentView.bottomAnchor),
            genreDoc.leadingAnchor.constraint(equalTo: genreScroll.contentView.leadingAnchor),
            genreDoc.heightAnchor.constraint(equalTo: genreScroll.heightAnchor),
            genreStack.topAnchor.constraint(equalTo: genreDoc.topAnchor),
            genreStack.bottomAnchor.constraint(equalTo: genreDoc.bottomAnchor),
            genreStack.leadingAnchor.constraint(equalTo: genreDoc.leadingAnchor),
            genreStack.trailingAnchor.constraint(equalTo: genreDoc.trailingAnchor),
        ])

        loadMoreButton.target = self
        loadMoreButton.isHidden = true
        loadMoreButton.translatesAutoresizingMaskIntoConstraints = false
        loadMoreButton.heightAnchor.constraint(equalToConstant: 36).isActive = true
        loadMoreButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 140).isActive = true

        let titleCol = NSStackView(views: [titleLabel, subtitleLabel])
        titleCol.orientation = .vertical
        titleCol.alignment = .leading
        titleCol.spacing = 4

        let headerRow = NSStackView(views: [titleCol, NSView(), catalogPopup])
        headerRow.orientation = .horizontal
        headerRow.alignment = .centerY
        headerRow.spacing = 16

        let layout = NSCollectionViewFlowLayout()
        layout.itemSize = PosterCardItem.itemSize
        layout.minimumInteritemSpacing = 18
        layout.minimumLineSpacing = 24
        layout.sectionInset = NSEdgeInsets(top: 14, left: 64, bottom: 72, right: 64)

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

        spinner.style = .spinning
        spinner.controlSize = .regular
        spinner.isDisplayedWhenStopped = false
        spinner.translatesAutoresizingMaskIntoConstraints = false

        emptyMessageLabel.font = .systemFont(ofSize: 15, weight: .regular)
        emptyMessageLabel.textColor = CinemaTheme.textMuted
        emptyMessageLabel.alignment = .center
        emptyActionButton.target = self
        emptyStack.orientation = .vertical
        emptyStack.alignment = .centerX
        emptyStack.spacing = 12
        emptyStack.addArrangedSubview(emptyMessageLabel)
        emptyStack.addArrangedSubview(emptyActionButton)
        emptyStack.isHidden = true
        emptyStack.translatesAutoresizingMaskIntoConstraints = false

        let gridScroll = NSScrollView()
        gridScroll.documentView = collectionView
        gridScroll.drawsBackground = false
        gridScroll.hasVerticalScroller = true
        gridScroll.automaticallyAdjustsContentInsets = false
        gridScroll.translatesAutoresizingMaskIntoConstraints = false

        let topStack = NSStackView(views: [headerRow, genreScroll])
        topStack.orientation = .vertical
        topStack.alignment = .leading
        topStack.spacing = 12
        topStack.translatesAutoresizingMaskIntoConstraints = false

        root.addSubview(topStack)
        root.addSubview(gridScroll)
        root.addSubview(emptyStack)
        root.addSubview(spinner)
        root.addSubview(loadMoreButton)

        NSLayoutConstraint.activate([
            topStack.topAnchor.constraint(equalTo: root.topAnchor, constant: 80),
            topStack.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 64),
            topStack.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -64),
            headerRow.widthAnchor.constraint(equalTo: topStack.widthAnchor),
            genreScroll.widthAnchor.constraint(equalTo: topStack.widthAnchor),

            gridScroll.topAnchor.constraint(equalTo: topStack.bottomAnchor, constant: 12),
            gridScroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            gridScroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            gridScroll.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            emptyStack.centerXAnchor.constraint(equalTo: gridScroll.centerXAnchor),
            emptyStack.centerYAnchor.constraint(equalTo: gridScroll.centerYAnchor),
            emptyMessageLabel.widthAnchor.constraint(lessThanOrEqualToConstant: 440),

            spinner.centerXAnchor.constraint(equalTo: gridScroll.centerXAnchor),
            spinner.centerYAnchor.constraint(equalTo: gridScroll.centerYAnchor),

            loadMoreButton.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            loadMoreButton.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -18),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        reloadCatalogList()
    }

    func reloadCatalogList() {
        catalogs = ((try? core.catalogs()) ?? []).filter(\.isBrowsable)
        catalogPopup.removeAllItems()
        for c in catalogs {
            let typeLabel = c.mediaType == "series" ? String(localized: "Series") : String(localized: "Movie")
            catalogPopup.addItem(withTitle: "\(c.name) · \(typeLabel) (\(c.addonName))")
        }
        catalogPopup.isHidden = catalogs.isEmpty || searchModeQuery != nil
        if searchModeQuery == nil {
            loadSelectedCatalog(reset: true)
        }
    }

    func showSearch(_ query: String) {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            clearSearch()
            return
        }
        searchModeQuery = trimmed
        catalogPopup.isHidden = true
        genreStack.isHidden = true
        loadMoreButton.isHidden = true
        titleLabel.stringValue = String(localized: "Search: “\(trimmed)”")
        subtitleLabel.stringValue = String(localized: "Searching…")
        items = []
        seenIds = []
        emptyStack.isHidden = true
        spinner.startAnimation(nil)
        collectionView.reloadData()

        activeTask?.cancel()
        let core = core
        let collector = CatalogBatchCollector { [weak self] batch in
            Task { @MainActor [weak self] in
                self?.appendSearchItems(batch.items)
            }
        }
        activeTask = Task { [weak self] in
            async let tmdbResults = (try? core.searchTmdb(query: trimmed, mediaType: nil)) ?? []
            async let addonPages = try? core.searchCatalogs(query: trimmed, mediaType: nil, observer: collector)
            let tmdb = await tmdbResults
            _ = await addonPages
            guard !Task.isCancelled, let self else { return }
            self.spinner.stopAnimation(nil)
            self.appendSearchItems(tmdb, prepend: true)
            self.subtitleLabel.stringValue = String(localized: "\(self.items.count) results")
            if self.items.isEmpty {
                self.emptyMessageLabel.stringValue = String(localized: "No matches found for “\(trimmed)”.")
                self.emptyActionButton.title = String(localized: "Clear Search")
                self.emptyStack.isHidden = false
            }
        }
    }

    func clearSearch() {
        searchModeQuery = nil
        titleLabel.stringValue = String(localized: "Discover")
        genreStack.isHidden = false
        reloadCatalogList()
    }

    private func appendSearchItems(_ incoming: [MetaPreviewInfo], prepend: Bool = false) {
        let fresh = incoming.filter { seenIds.insert("\($0.mediaType):\($0.id)").inserted }
        guard !fresh.isEmpty else { return }
        if prepend {
            items.insert(contentsOf: fresh, at: 0)
        } else {
            items.append(contentsOf: fresh)
        }
        subtitleLabel.stringValue = String(localized: "\(items.count) results")
        collectionView.reloadData()
    }

    private func loadSelectedCatalog(reset: Bool) {
        let idx = catalogPopup.indexOfSelectedItem
        guard catalogs.indices.contains(idx) else {
            items = []
            collectionView.reloadData()
            spinner.stopAnimation(nil)
            subtitleLabel.stringValue = String(localized: "No browsable catalog addons installed")
            emptyMessageLabel.stringValue = String(localized: "Install a catalog addon (such as Cinemeta) or add your TMDB key to browse titles.")
            emptyActionButton.title = String(localized: "Open Addons…")
            emptyStack.isHidden = false
            loadMoreButton.isHidden = true
            return
        }
        emptyStack.isHidden = true
        let cat = catalogs[idx]
        if reset {
            items = []
            seenIds = []
            hasMore = false
            collectionView.reloadData()
            rebuildGenrePills(cat.genres)
            spinner.startAnimation(nil)
        }
        subtitleLabel.stringValue = "\(cat.addonName) · \(cat.name)"
        let skip: UInt32? = reset || items.isEmpty ? nil : UInt32(items.count)

        activeTask?.cancel()
        let core = core
        let genre = selectedGenre
        activeTask = Task { [weak self] in
            do {
                let page = try await core.catalog(
                    addonId: cat.addonId,
                    mediaType: cat.mediaType,
                    catalogId: cat.id,
                    query: CatalogQueryInput(search: nil, genre: genre, skip: skip)
                )
                guard !Task.isCancelled, let self else { return }
                self.spinner.stopAnimation(nil)
                let fresh = page.items.filter { self.seenIds.insert("\($0.mediaType):\($0.id)").inserted }
                self.items.append(contentsOf: fresh)
                self.hasMore = page.hasMore && cat.supportsSkip && !fresh.isEmpty
                self.loadMoreButton.isHidden = !self.hasMore
                self.emptyStack.isHidden = !self.items.isEmpty
                if self.items.isEmpty {
                    self.emptyMessageLabel.stringValue = String(localized: "This catalog returned no titles.")
                    self.emptyActionButton.title = String(localized: "Retry")
                }
                self.collectionView.reloadData()
            } catch {
                guard !Task.isCancelled, let self else { return }
                self.spinner.stopAnimation(nil)
                let friendly = FriendlyError.explain(error)
                self.subtitleLabel.stringValue = friendly
                if self.items.isEmpty {
                    self.emptyMessageLabel.stringValue = friendly
                    self.emptyActionButton.title = String(localized: "Retry")
                    self.emptyStack.isHidden = false
                }
            }
        }
    }

    private func rebuildGenrePills(_ genres: [String]) {
        for v in genreStack.arrangedSubviews {
            genreStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        let allTitle = String(localized: "All")
        let allBtn = CinemaTheme.makePillButton(
            title: allTitle,
            primary: selectedGenre == nil,
            target: self,
            action: #selector(selectGenre(_:))
        )
        allBtn.identifier = nil
        allBtn.heightAnchor.constraint(equalToConstant: 30).isActive = true
        genreStack.addArrangedSubview(allBtn)

        for g in genres.prefix(16) {
            let btn = CinemaTheme.makePillButton(
                title: g,
                primary: selectedGenre == g,
                target: self,
                action: #selector(selectGenre(_:))
            )
            btn.identifier = NSUserInterfaceItemIdentifier(g)
            btn.heightAnchor.constraint(equalToConstant: 30).isActive = true
            genreStack.addArrangedSubview(btn)
        }
    }

    @objc private func catalogChanged() {
        selectedGenre = nil
        loadSelectedCatalog(reset: true)
    }

    @objc private func selectGenre(_ sender: NSButton) {
        selectedGenre = sender.identifier?.rawValue
        loadSelectedCatalog(reset: true)
    }

    @objc private func emptyActionClicked() {
        if searchModeQuery != nil {
            clearSearch()
        } else if catalogs.isEmpty {
            onOpenAddons?()
        } else {
            loadSelectedCatalog(reset: true)
        }
    }

    @objc private func loadMore() {
        loadSelectedCatalog(reset: false)
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
            card.configure(with: items[indexPath.item])
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
