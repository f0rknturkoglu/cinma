import AppKit

/// Horizontal `NSCollectionView` rail with title and optional source badge.
@MainActor
final class CatalogRailView: NSView, NSCollectionViewDataSource, NSCollectionViewDelegate {
    enum Content {
        case continueWatching([WatchProgressInfo])
        case posters([MetaPreviewInfo])
    }

    private let headingLabel = NSTextField(labelWithString: "")
    private let sourceLabel = NSTextField(labelWithString: "")
    private let collectionView = NSCollectionView()
    private let scrollView = NSScrollView()
    private var content: Content = .posters([])

    var onSelectPoster: ((MetaPreviewInfo) -> Void)?
    var onSelectContinue: ((WatchProgressInfo) -> Void)?

    init(title: String, source: String = "") {
        super.init(frame: .zero)
        headingLabel.stringValue = title
        headingLabel.font = .systemFont(ofSize: 20, weight: .bold)
        headingLabel.textColor = CinemaTheme.textPrimary
        headingLabel.translatesAutoresizingMaskIntoConstraints = false

        sourceLabel.stringValue = source
        sourceLabel.font = .systemFont(ofSize: 13, weight: .regular)
        sourceLabel.textColor = CinemaTheme.textMuted
        sourceLabel.translatesAutoresizingMaskIntoConstraints = false

        let layout = NSCollectionViewFlowLayout()
        layout.scrollDirection = .horizontal
        layout.minimumInteritemSpacing = 18
        layout.minimumLineSpacing = 18
        layout.sectionInset = NSEdgeInsets(top: 0, left: 64, bottom: 0, right: 64)

        collectionView.collectionViewLayout = layout
        collectionView.backgroundColors = [.clear]
        collectionView.isSelectable = true
        collectionView.dataSource = self
        collectionView.delegate = self
        collectionView.register(PosterCardItem.self, forItemWithIdentifier: PosterCardItem.identifier)
        collectionView.register(ContinueWatchingCardItem.self, forItemWithIdentifier: ContinueWatchingCardItem.identifier)

        scrollView.documentView = collectionView
        scrollView.drawsBackground = false
        scrollView.hasHorizontalScroller = false
        scrollView.hasVerticalScroller = false
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        addSubview(headingLabel)
        addSubview(sourceLabel)
        addSubview(scrollView)

        NSLayoutConstraint.activate([
            headingLabel.topAnchor.constraint(equalTo: topAnchor),
            headingLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 64),

            sourceLabel.centerYAnchor.constraint(equalTo: headingLabel.centerYAnchor),
            sourceLabel.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -64),

            scrollView.topAnchor.constraint(equalTo: headingLabel.bottomAnchor, constant: 14),
            scrollView.leadingAnchor.constraint(equalTo: leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    func update(title: String? = nil, source: String? = nil, content: Content) {
        if let title { headingLabel.stringValue = title }
        if let source { sourceLabel.stringValue = source }
        self.content = content
        if let layout = collectionView.collectionViewLayout as? NSCollectionViewFlowLayout {
            switch content {
            case .continueWatching:
                layout.itemSize = ContinueWatchingCardItem.itemSize
            case .posters:
                layout.itemSize = PosterCardItem.itemSize
            }
        }
        collectionView.reloadData()
    }

    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int {
        switch content {
        case .continueWatching(let items): items.count
        case .posters(let items): items.count
        }
    }

    func collectionView(
        _ collectionView: NSCollectionView,
        itemForRepresentedObjectAt indexPath: IndexPath
    ) -> NSCollectionViewItem {
        switch content {
        case .continueWatching(let items):
            let item = collectionView.makeItem(
                withIdentifier: ContinueWatchingCardItem.identifier,
                for: indexPath
            )
            if let card = item as? ContinueWatchingCardItem, items.indices.contains(indexPath.item) {
                card.configure(with: items[indexPath.item])
            }
            return item
        case .posters(let items):
            let item = collectionView.makeItem(
                withIdentifier: PosterCardItem.identifier,
                for: indexPath
            )
            if let card = item as? PosterCardItem, items.indices.contains(indexPath.item) {
                card.configure(with: items[indexPath.item])
            }
            return item
        }
    }

    func collectionView(_ collectionView: NSCollectionView, didSelectItemsAt indexPaths: Set<IndexPath>) {
        guard let first = indexPaths.first else { return }
        collectionView.deselectItems(at: indexPaths)
        switch content {
        case .continueWatching(let items) where items.indices.contains(first.item):
            onSelectContinue?(items[first.item])
        case .posters(let items) where items.indices.contains(first.item):
            onSelectPoster?(items[first.item])
        default:
            break
        }
    }
}

/// Home screen (`Ana sayfa` mockup): rotating full-bleed hero + SQLite-first
/// Continue Watching rail + trending/catalog rails.
@MainActor
final class HomeBrowserViewController: NSViewController {
    private let core: CinmaCore
    private let onOpenDetail: (MetaPreviewInfo) -> Void
    private let onContinue: (WatchProgressInfo) -> Void

    private let scrollView = NSScrollView()
    private let contentStack = NSStackView()

    // Hero views
    private let heroContainer = NSView()
    private let heroBackdrop = NSImageView()
    private let heroFallbackGradient = CAGradientLayer()
    private let heroKicker = NSTextField(labelWithString: String(localized: "Featured"))
    private let heroTitle = NSTextField(wrappingLabelWithString: "CINMA")
    private let heroMeta = NSTextField(labelWithString: "")
    private let heroOverview = NSTextField(wrappingLabelWithString: "")
    private var heroLibraryButton: NSButton?
    private let heroDotsStack = NSStackView()
    private var heroLoadTask: Task<Void, Never>?

    private var heroItems: [MetaPreviewInfo] = []
    private var heroIndex = 0
    private var heroTimer: Task<Void, Never>?

    private let continueRail = CatalogRailView(title: String(localized: "Continue Watching"))
    private var continueHeight: NSLayoutConstraint?
    private let trendingMoviesRail = CatalogRailView(title: String(localized: "Trending Movies"))
    private let trendingSeriesRail = CatalogRailView(title: String(localized: "Trending Series"))
    private let dynamicRailsStack = NSStackView()
    private let emptyHintLabel = NSTextField(wrappingLabelWithString: "")

    init(
        core: CinmaCore,
        onOpenDetail: @escaping (MetaPreviewInfo) -> Void,
        onContinue: @escaping (WatchProgressInfo) -> Void
    ) {
        self.core = core
        self.onOpenDetail = onOpenDetail
        self.onContinue = onContinue
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = CinemaTheme.background.cgColor

        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        let doc = NSView()
        doc.translatesAutoresizingMaskIntoConstraints = false

        contentStack.orientation = .vertical
        contentStack.alignment = .leading
        contentStack.spacing = 28
        contentStack.translatesAutoresizingMaskIntoConstraints = false

        setupHero()
        setupRails()

        doc.addSubview(contentStack)
        scrollView.documentView = doc
        root.addSubview(scrollView)

        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: root.topAnchor),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            doc.topAnchor.constraint(equalTo: scrollView.contentView.topAnchor),
            doc.leadingAnchor.constraint(equalTo: scrollView.contentView.leadingAnchor),
            doc.trailingAnchor.constraint(equalTo: scrollView.contentView.trailingAnchor),
            doc.widthAnchor.constraint(equalTo: scrollView.widthAnchor),

            contentStack.topAnchor.constraint(equalTo: doc.topAnchor),
            contentStack.leadingAnchor.constraint(equalTo: doc.leadingAnchor),
            contentStack.trailingAnchor.constraint(equalTo: doc.trailingAnchor),
            contentStack.bottomAnchor.constraint(equalTo: doc.bottomAnchor, constant: -48),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        // §5.16: Draw cached SQLite rows BEFORE any network call.
        loadCachedFromSqlite()
        refreshFromNetwork()
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        startHeroRotationIfNeeded()
    }

    override func viewWillDisappear() {
        super.viewWillDisappear()
        heroTimer?.cancel()
        heroTimer = nil
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        heroFallbackGradient.frame = heroContainer.bounds
        CATransaction.commit()
    }

    private func setupHero() {
        heroContainer.wantsLayer = true
        heroContainer.layer?.insertSublayer(heroFallbackGradient, at: 0)
        heroFallbackGradient.colors = ArtworkPlaceholder.gradient(for: "cinma-hero")
        heroFallbackGradient.startPoint = CGPoint(x: 0.2, y: 0.1)
        heroFallbackGradient.endPoint = CGPoint(x: 0.9, y: 0.9)
        heroContainer.translatesAutoresizingMaskIntoConstraints = false

        heroBackdrop.imageScaling = .scaleAxesIndependently
        heroBackdrop.translatesAutoresizingMaskIntoConstraints = false

        let leftScrim = GradientLayerView(
            colors: [
                CinemaTheme.background.withAlphaComponent(0.94),
                CinemaTheme.background.withAlphaComponent(0.55),
                .clear,
            ],
            locations: [0, 0.42, 0.78],
            start: CGPoint(x: 0, y: 0.5),
            end: CGPoint(x: 1, y: 0.5)
        )
        leftScrim.translatesAutoresizingMaskIntoConstraints = false

        let bottomScrim = GradientLayerView(
            colors: [.clear, CinemaTheme.background],
            start: CGPoint(x: 0.5, y: 1),
            end: CGPoint(x: 0.5, y: 0)
        )
        bottomScrim.translatesAutoresizingMaskIntoConstraints = false

        heroKicker.font = .systemFont(ofSize: 12.5, weight: .bold)
        heroKicker.textColor = CinemaTheme.accent

        heroTitle.font = .systemFont(ofSize: 54, weight: .heavy)
        heroTitle.textColor = CinemaTheme.textPrimary
        heroTitle.maximumNumberOfLines = 2

        heroMeta.font = .monospacedDigitSystemFont(ofSize: 14, weight: .medium)
        heroMeta.textColor = CinemaTheme.textSecondary

        heroOverview.font = .systemFont(ofSize: 15, weight: .regular)
        heroOverview.textColor = CinemaTheme.textSecondary
        heroOverview.maximumNumberOfLines = 3

        let playButton = CinemaTheme.makePillButton(
            title: String(localized: "Play"),
            symbol: "play.fill",
            primary: true,
            target: self,
            action: #selector(openHeroDetail)
        )
        let detailsButton = CinemaTheme.makePillButton(
            title: String(localized: "View Sources"),
            symbol: "sparkles.tv",
            primary: false,
            target: self,
            action: #selector(openHeroDetail)
        )
        let libButton = CinemaTheme.makePillButton(
            title: "+",
            primary: false,
            target: self,
            action: #selector(toggleHeroLibrary)
        )
        heroLibraryButton = libButton

        playButton.translatesAutoresizingMaskIntoConstraints = false
        detailsButton.translatesAutoresizingMaskIntoConstraints = false
        libButton.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            playButton.heightAnchor.constraint(equalToConstant: 40),
            playButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 112),
            detailsButton.heightAnchor.constraint(equalToConstant: 40),
            detailsButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 138),
            libButton.heightAnchor.constraint(equalToConstant: 40),
            libButton.widthAnchor.constraint(equalToConstant: 40),
        ])

        let ctaRow = NSStackView(views: [playButton, detailsButton, libButton])
        ctaRow.spacing = 12

        let textStack = NSStackView(views: [heroKicker, heroTitle, heroMeta, heroOverview, ctaRow])
        textStack.orientation = .vertical
        textStack.alignment = .leading
        textStack.spacing = 14
        textStack.translatesAutoresizingMaskIntoConstraints = false

        heroDotsStack.orientation = .horizontal
        heroDotsStack.spacing = 6
        heroDotsStack.translatesAutoresizingMaskIntoConstraints = false

        heroContainer.addSubview(heroBackdrop)
        heroContainer.addSubview(leftScrim)
        heroContainer.addSubview(bottomScrim)
        heroContainer.addSubview(textStack)
        heroContainer.addSubview(heroDotsStack)

        NSLayoutConstraint.activate([
            heroContainer.heightAnchor.constraint(equalToConstant: 560),

            heroBackdrop.topAnchor.constraint(equalTo: heroContainer.topAnchor),
            heroBackdrop.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            heroBackdrop.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            heroBackdrop.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor),

            leftScrim.topAnchor.constraint(equalTo: heroContainer.topAnchor),
            leftScrim.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            leftScrim.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            leftScrim.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor),

            bottomScrim.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            bottomScrim.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            bottomScrim.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor),
            bottomScrim.heightAnchor.constraint(equalToConstant: 240),

            textStack.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor, constant: 64),
            textStack.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor, constant: -88),
            textStack.widthAnchor.constraint(lessThanOrEqualToConstant: 560),

            heroDotsStack.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor, constant: -64),
            heroDotsStack.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor, constant: -92),
        ])

        contentStack.addArrangedSubview(heroContainer)
        heroContainer.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true
    }

    private func setupRails() {
        continueRail.translatesAutoresizingMaskIntoConstraints = false
        continueRail.onSelectContinue = { [weak self] entry in self?.onContinue(entry) }
        let cwHeight = continueRail.heightAnchor.constraint(equalToConstant: 238)
        cwHeight.isActive = true
        continueHeight = cwHeight
        contentStack.addArrangedSubview(continueRail)
        continueRail.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true
        contentStack.setCustomSpacing(-44, after: heroContainer)

        for rail in [trendingMoviesRail, trendingSeriesRail] {
            rail.translatesAutoresizingMaskIntoConstraints = false
            rail.heightAnchor.constraint(equalToConstant: 344).isActive = true
            rail.onSelectPoster = { [weak self] item in self?.onOpenDetail(item) }
            contentStack.addArrangedSubview(rail)
            rail.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true
        }

        dynamicRailsStack.orientation = .vertical
        dynamicRailsStack.alignment = .leading
        dynamicRailsStack.spacing = 28
        dynamicRailsStack.translatesAutoresizingMaskIntoConstraints = false
        contentStack.addArrangedSubview(dynamicRailsStack)
        dynamicRailsStack.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true

        emptyHintLabel.font = .systemFont(ofSize: 14, weight: .regular)
        emptyHintLabel.textColor = CinemaTheme.textMuted
        emptyHintLabel.isHidden = true
        contentStack.addArrangedSubview(emptyHintLabel)
        emptyHintLabel.leadingAnchor.constraint(equalTo: contentStack.leadingAnchor, constant: 64).isActive = true
        emptyHintLabel.trailingAnchor.constraint(equalTo: contentStack.trailingAnchor, constant: -64).isActive = true
    }

    func reloadLocalState() {
        loadCachedFromSqlite()
        refreshHeroLibraryButton()
    }

    func refreshAll() {
        loadCachedFromSqlite()
        refreshFromNetwork()
    }

    private func loadCachedFromSqlite() {
        let cw = (try? core.continueWatching(limit: 12)) ?? []
        continueRail.update(content: .continueWatching(cw))
        continueRail.isHidden = cw.isEmpty
        continueHeight?.constant = cw.isEmpty ? 0 : 238

        let cachedMovies = (try? core.cachedTrending(mediaType: "movie")) ?? []
        let cachedSeries = (try? core.cachedTrending(mediaType: "series")) ?? []
        applyTrending(movies: cachedMovies, series: cachedSeries)
    }

    private func refreshFromNetwork() {
        let core = core
        Task { [weak self] in
            async let moviesTask = try? core.trending(mediaType: "movie")
            async let seriesTask = try? core.trending(mediaType: "series")
            let movies = await moviesTask ?? []
            let series = await seriesTask ?? []
            guard let self else { return }
            self.applyTrending(movies: movies, series: series)
            self.loadAddonCatalogRails()
        }
    }

    private func applyTrending(movies: [MetaPreviewInfo], series: [MetaPreviewInfo]) {
        let hasTmdb = (try? core.hasTmdbKey()) ?? false
        let src = hasTmdb ? "TMDB" : String(localized: "Addon Catalog")
        trendingMoviesRail.update(source: src, content: .posters(movies))
        trendingMoviesRail.isHidden = movies.isEmpty
        trendingSeriesRail.update(source: src, content: .posters(series))
        trendingSeriesRail.isHidden = series.isEmpty

        let candidates = Array((movies.prefix(3) + series.prefix(2)))
        if !candidates.isEmpty {
            heroItems = candidates
            heroIndex = heroIndex % candidates.count
            renderHero()
            emptyHintLabel.isHidden = true
        } else {
            heroTitle.stringValue = "CINMA"
            heroMeta.stringValue = String(localized: "Add an addon or TMDB key to explore")
            heroOverview.stringValue = String(localized: "Cinma ships without content sources. Open Profile → Addons or Settings to add a Stremio addon or TMDB API key.")
            emptyHintLabel.stringValue = String(localized: "No catalogs loaded yet. Install a catalog addon or set your TMDB key in Settings.")
            emptyHintLabel.isHidden = false
        }
    }

    private func loadAddonCatalogRails() {
        let core = core
        let collector = CatalogBatchCollector { [weak self] batch in
            Task { @MainActor [weak self] in
                self?.appendAddonRail(batch)
            }
        }
        for view in dynamicRailsStack.arrangedSubviews {
            dynamicRailsStack.removeArrangedSubview(view)
            view.removeFromSuperview()
        }
        Task {
            _ = try? await core.browseCatalogs(mediaType: nil, observer: collector)
        }
    }

    private func appendAddonRail(_ batch: CatalogBatchInfo) {
        guard batch.error == nil, !batch.items.isEmpty else { return }
        // Avoid duplicating if there are already 6 rails.
        guard dynamicRailsStack.arrangedSubviews.count < 6 else { return }
        let rail = CatalogRailView(title: batch.catalogName, source: batch.addonName)
        rail.translatesAutoresizingMaskIntoConstraints = false
        rail.heightAnchor.constraint(equalToConstant: 344).isActive = true
        rail.onSelectPoster = { [weak self] item in self?.onOpenDetail(item) }
        rail.update(content: .posters(batch.items))
        dynamicRailsStack.addArrangedSubview(rail)
        rail.widthAnchor.constraint(equalTo: dynamicRailsStack.widthAnchor).isActive = true
    }

    private func renderHero() {
        guard heroItems.indices.contains(heroIndex) else { return }
        let item = heroItems[heroIndex]
        heroFallbackGradient.colors = ArtworkPlaceholder.gradient(for: item.id)
        heroTitle.stringValue = item.name.uppercased()
        heroMeta.stringValue = [
            item.releaseInfo,
            item.mediaType == "series" ? String(localized: "Series") : String(localized: "Movie"),
            item.imdbRating.map { "IMDb \($0)" },
        ]
        .compactMap { $0 }
        .joined(separator: " · ")
        heroOverview.stringValue = item.description ?? ""
        let art = item.background ?? item.poster
        heroBackdrop.setRemoteImage(art, pointSize: NSSize(width: 1280, height: 560), taskStorage: &heroLoadTask)
        refreshHeroLibraryButton()
        renderHeroDots()
    }

    private func refreshHeroLibraryButton() {
        guard let btn = heroLibraryButton, heroItems.indices.contains(heroIndex) else { return }
        let item = heroItems[heroIndex]
        let saved = (try? core.isInLibrary(metaId: item.id, mediaType: item.mediaType)) ?? false
        let symbol = saved ? "checkmark" : "plus"
        btn.image = NSImage(systemSymbolName: symbol, accessibilityDescription: String(localized: "Library"))
        btn.title = saved ? "✓" : "+"
    }

    private func renderHeroDots() {
        for v in heroDotsStack.arrangedSubviews {
            heroDotsStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        for i in heroItems.indices {
            let dot = NSButton(title: "", target: self, action: #selector(selectHeroDot(_:)))
            dot.tag = i
            dot.isBordered = false
            dot.wantsLayer = true
            dot.layer?.cornerRadius = 2
            dot.layer?.backgroundColor = (i == heroIndex ? CinemaTheme.textPrimary : NSColor(white: 1, alpha: 0.35)).cgColor
            dot.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                dot.widthAnchor.constraint(equalToConstant: i == heroIndex ? 22 : 8),
                dot.heightAnchor.constraint(equalToConstant: 4),
            ])
            heroDotsStack.addArrangedSubview(dot)
        }
    }

    private func startHeroRotationIfNeeded() {
        heroTimer?.cancel()
        guard !CinemaTheme.reduceMotion else { return }
        heroTimer = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(10))
                guard !Task.isCancelled, let self, self.heroItems.count > 1 else { continue }
                self.heroIndex = (self.heroIndex + 1) % self.heroItems.count
                self.renderHero()
            }
        }
    }

    @objc private func selectHeroDot(_ sender: NSButton) {
        guard heroItems.indices.contains(sender.tag) else { return }
        heroIndex = sender.tag
        renderHero()
        startHeroRotationIfNeeded()
    }

    @objc private func openHeroDetail() {
        guard heroItems.indices.contains(heroIndex) else { return }
        onOpenDetail(heroItems[heroIndex])
    }

    @objc private func toggleHeroLibrary() {
        guard heroItems.indices.contains(heroIndex) else { return }
        let item = heroItems[heroIndex]
        let saved = (try? core.isInLibrary(metaId: item.id, mediaType: item.mediaType)) ?? false
        if saved {
            try? core.removeFromLibrary(metaId: item.id, mediaType: item.mediaType)
        } else {
            try? core.addToLibrary(metaId: item.id, mediaType: item.mediaType, name: item.name, poster: item.poster)
        }
        refreshHeroLibraryButton()
    }
}

/// Bridges UniFFI `CatalogBatchObserver` callbacks cleanly into Swift.
final class CatalogBatchCollector: CatalogBatchObserver {
    private let handler: @Sendable (CatalogBatchInfo) -> Void

    init(_ handler: @escaping @Sendable (CatalogBatchInfo) -> Void) {
        self.handler = handler
    }

    func onBatch(batch: CatalogBatchInfo) {
        handler(batch)
    }
}
