import AppKit

/// Discover & Search screen (`Keşfet` + arama): catalog selector, genre pills,
/// pagination (`skip`), and incremental partial search results.
@MainActor
final class DiscoverViewController: NSViewController, NSCollectionViewDataSource, NSCollectionViewDelegate {
    private let core: CinmaCore
    private let onOpenDetail: (MetaPreviewInfo) -> Void

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
    private let collectionView = NSCollectionView()

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

        genreStack.orientation = .horizontal
        genreStack.spacing = 8
        let genreScroll = NSScrollView()
        genreScroll.documentView = genreStack
        genreScroll.drawsBackground = false
        genreScroll.hasHorizontalScroller = false
        genreScroll.translatesAutoresizingMaskIntoConstraints = false
        genreScroll.heightAnchor.constraint(equalToConstant: 34).isActive = true

        loadMoreButton.target = self
        loadMoreButton.isHidden = true
        loadMoreButton.translatesAutoresizingMaskIntoConstraints = false
        loadMoreButton.heightAnchor.constraint(equalToConstant: 36).isActive = true
        loadMoreButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 130).isActive = true

        let headerRow = NSStackView(views: [titleLabel, catalogPopup])
        headerRow.orientation = .horizontal
        headerRow.alignment = .centerY
        headerRow.distribution = .gravityAreas

        let layout = NSCollectionViewFlowLayout()
        layout.itemSize = PosterCardItem.itemSize
        layout.minimumInteritemSpacing = 18
        layout.minimumLineSpacing = 24
        layout.sectionInset = NSEdgeInsets(top: 12, left: 64, bottom: 24, right: 64)

        collectionView.collectionViewLayout = layout
        collectionView.backgroundColors = [.clear]
        collectionView.isSelectable = true
        collectionView.dataSource = self
        collectionView.delegate = self
        collectionView.register(PosterCardItem.self, forItemWithIdentifier: PosterCardItem.identifier)

        let gridScroll = NSScrollView()
        gridScroll.documentView = collectionView
        gridScroll.drawsBackground = false
        gridScroll.hasVerticalScroller = true
        gridScroll.translatesAutoresizingMaskIntoConstraints = false

        let topStack = NSStackView(views: [headerRow, subtitleLabel, genreScroll])
        topStack.orientation = .vertical
        topStack.alignment = .leading
        topStack.spacing = 10
        topStack.translatesAutoresizingMaskIntoConstraints = false

        root.addSubview(topStack)
        root.addSubview(gridScroll)
        root.addSubview(loadMoreButton)

        NSLayoutConstraint.activate([
            topStack.topAnchor.constraint(equalTo: root.topAnchor, constant: 84),
            topStack.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 64),
            topStack.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -64),
            headerRow.widthAnchor.constraint(equalTo: topStack.widthAnchor),
            genreScroll.widthAnchor.constraint(equalTo: topStack.widthAnchor),

            gridScroll.topAnchor.constraint(equalTo: topStack.bottomAnchor, constant: 12),
            gridScroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            gridScroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            gridScroll.bottomAnchor.constraint(equalTo: loadMoreButton.topAnchor, constant: -10),

            loadMoreButton.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            loadMoreButton.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -20),
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
            self.appendSearchItems(tmdb, prepend: true)
            self.subtitleLabel.stringValue = String(localized: "\(self.items.count) results")
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
            subtitleLabel.stringValue = String(localized: "No browsable catalog addons installed")
            loadMoreButton.isHidden = true
            return
        }
        let cat = catalogs[idx]
        if reset {
            items = []
            seenIds = []
            hasMore = false
            collectionView.reloadData()
            rebuildGenrePills(cat.genres)
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
                let fresh = page.items.filter { self.seenIds.insert("\($0.mediaType):\($0.id)").inserted }
                self.items.append(contentsOf: fresh)
                self.hasMore = page.hasMore && cat.supportsSkip && !fresh.isEmpty
                self.loadMoreButton.isHidden = !self.hasMore
                self.collectionView.reloadData()
            } catch {
                guard !Task.isCancelled, let self else { return }
                self.subtitleLabel.stringValue = error.localizedDescription
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
        guard let first = indexPaths.first, items.indices.contains(first.item) else { return }
        collectionView.deselectItems(at: indexPaths)
        onOpenDetail(items[first.item])
    }
}
