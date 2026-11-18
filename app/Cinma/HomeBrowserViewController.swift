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
    private let unpinButton = NSButton()
    private let collectionView = KeyboardCollectionView()
    private let scrollView = NSScrollView()
    private var content: Content = .posters([])
    private var posterStyle = PosterStyleContext()

    var onSelectPoster: ((MetaPreviewInfo) -> Void)?
    var onSelectContinue: ((WatchProgressInfo) -> Void)?
    var onBuildContextMenu: ((MetaPreviewInfo) -> NSMenu?)?
    var onUnpin: (() -> Void)? {
        didSet {
            unpinButton.isHidden = onUnpin == nil
        }
    }

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

        unpinButton.title = String(localized: "Unpin")
        unpinButton.image = NSImage(systemSymbolName: "pin.slash", accessibilityDescription: String(localized: "Unpin"))
        unpinButton.imagePosition = .imageLeading
        unpinButton.bezelStyle = .rounded
        unpinButton.controlSize = .small
        unpinButton.target = self
        unpinButton.action = #selector(unpinClicked)
        unpinButton.isHidden = true
        unpinButton.translatesAutoresizingMaskIntoConstraints = false

        let layout = NSCollectionViewFlowLayout()
        layout.scrollDirection = .horizontal
        layout.minimumInteritemSpacing = 18
        layout.minimumLineSpacing = 18
        layout.sectionInset = NSEdgeInsets(top: 4, left: 64, bottom: 4, right: 64)

        collectionView.collectionViewLayout = layout
        collectionView.backgroundColors = [.clear]
        collectionView.isSelectable = true
        collectionView.dataSource = self
        collectionView.delegate = self
        collectionView.register(PosterCardItem.self, forItemWithIdentifier: PosterCardItem.identifier)
        collectionView.register(ContinueWatchingCardItem.self, forItemWithIdentifier: ContinueWatchingCardItem.identifier)
        collectionView.onActivateSelection = { [weak self] ip in
            self?.activateItem(at: ip)
        }

        scrollView.documentView = collectionView
        scrollView.drawsBackground = false
        scrollView.hasHorizontalScroller = false
        scrollView.hasVerticalScroller = false
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        addSubview(headingLabel)
        addSubview(sourceLabel)
        addSubview(unpinButton)
        addSubview(scrollView)

        NSLayoutConstraint.activate([
            headingLabel.topAnchor.constraint(equalTo: topAnchor),
            headingLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 64),

            unpinButton.centerYAnchor.constraint(equalTo: headingLabel.centerYAnchor),
            unpinButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -64),

            sourceLabel.centerYAnchor.constraint(equalTo: headingLabel.centerYAnchor),
            sourceLabel.trailingAnchor.constraint(equalTo: unpinButton.leadingAnchor, constant: -10),

            scrollView.topAnchor.constraint(equalTo: headingLabel.bottomAnchor, constant: 14),
            scrollView.leadingAnchor.constraint(equalTo: leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    @objc private func unpinClicked() {
        onUnpin?()
    }

    func update(title: String? = nil, source: String? = nil, content: Content, style: PosterStyleContext? = nil) {
        if let title { headingLabel.stringValue = title }
        if let source { sourceLabel.stringValue = source }
        if let style { posterStyle = style }
        self.content = content
        if let layout = collectionView.collectionViewLayout as? NSCollectionViewFlowLayout {
            switch content {
            case .continueWatching:
                layout.itemSize = ContinueWatchingCardItem.itemSize
            case .posters:
                layout.itemSize = posterStyle.itemSize
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
                card.configure(with: items[indexPath.item], style: posterStyle)
                card.onContextMenu = onBuildContextMenu
            }
            return item
        }
    }

    func collectionView(_ collectionView: NSCollectionView, didSelectItemsAt indexPaths: Set<IndexPath>) {
        guard let first = indexPaths.first,
              NSApp.currentEvent?.type == .leftMouseDown || NSApp.currentEvent?.type == .leftMouseUp
        else { return }
        collectionView.deselectItems(at: indexPaths)
        activateItem(at: first)
    }

    private func activateItem(at indexPath: IndexPath) {
        switch content {
        case .continueWatching(let items) where items.indices.contains(indexPath.item):
            onSelectContinue?(items[indexPath.item])
        case .posters(let items) where items.indices.contains(indexPath.item):
            onSelectPoster?(items[indexPath.item])
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
    var onQuickPlay: ((MetaPreviewInfo) -> Void)?
    var onBuildContextMenu: ((MetaPreviewInfo) -> NSMenu?)?

    private let scrollView = NSScrollView()
    private let contentStack = NSStackView()

    // Hero views
    private let heroContainer = NSView()
    private let heroBackdrop = AspectFillImageView()
    private let heroFallbackView: GradientLayerView = {
        let colors = ArtworkPlaceholder.gradient(for: "cinma-hero").compactMap { NSColor(cgColor: $0) }
        return GradientLayerView(
            colors: colors,
            start: CGPoint(x: 0.2, y: 0.1),
            end: CGPoint(x: 0.9, y: 0.9)
        )
    }()
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
    private var windowObservers: [NSObjectProtocol] = []

    private let continueRail = CatalogRailView(title: String(localized: "Continue Watching"))
    private let continueSeriesRail = CatalogRailView(title: String(localized: "Continue Watching — Series"))
    private var continueHeight: NSLayoutConstraint?
    private var continueSeriesHeight: NSLayoutConstraint?
    private var moviesRailHeight: NSLayoutConstraint?
    private var seriesRailHeight: NSLayoutConstraint?
    private let trendingMoviesRail = CatalogRailView(title: String(localized: "Trending Movies"))
    private let trendingSeriesRail = CatalogRailView(title: String(localized: "Trending Series"))
    private let pinnedRailsStack = NSStackView()
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
        scrollView.automaticallyAdjustsContentInsets = false
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        let doc = FlippedDocumentView()
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
            doc.widthAnchor.constraint(equalTo: scrollView.contentView.widthAnchor),

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
        scrollView.contentView.scroll(to: .zero)
        scrollView.reflectScrolledClipView(scrollView.contentView)
        installWindowObserversIfNeeded()
        startHeroRotationIfNeeded()
        if (try? core.playerPreferences())?.discordShowWhenBrowsing == true {
            let core = core
            Task {
                try? await core.updateDiscordPresence(
                    title: "Browsing Catalogs",
                    subtitle: nil,
                    durationS: nil,
                    positionS: 0,
                    paused: false
                )
            }
        }
    }

    override func viewWillDisappear() {
        super.viewWillDisappear()
        heroTimer?.cancel()
        heroTimer = nil
        removeWindowObservers()
    }

    private func installWindowObserversIfNeeded() {
        guard windowObservers.isEmpty else { return }
        let nc = NotificationCenter.default
        if let window = view.window {
            windowObservers.append(
                nc.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
                    Task { @MainActor in self?.syncHeroRotationWithVisibility() }
                }
            )
        }
        windowObservers.append(
            nc.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in
                    self?.heroTimer?.cancel()
                    self?.heroTimer = nil
                }
            }
        )
        windowObservers.append(
            nc.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.syncHeroRotationWithVisibility() }
            }
        )
    }

    private func removeWindowObservers() {
        for obs in windowObservers {
            NotificationCenter.default.removeObserver(obs)
        }
        windowObservers.removeAll()
    }

    private func syncHeroRotationWithVisibility() {
        let visible = view.window?.occlusionState.contains(.visible) == true && NSApp.isActive
        if visible {
            startHeroRotationIfNeeded()
        } else {
            heroTimer?.cancel()
            heroTimer = nil
        }
    }

    private func setupHero() {
        heroContainer.wantsLayer = true
        heroContainer.layer?.masksToBounds = true
        heroContainer.translatesAutoresizingMaskIntoConstraints = false
        heroFallbackView.translatesAutoresizingMaskIntoConstraints = false

        heroBackdrop.translatesAutoresizingMaskIntoConstraints = false

        let topScrim = GradientLayerView(
            colors: [
                CinemaTheme.background.withAlphaComponent(0.76),
                CinemaTheme.background.withAlphaComponent(0.26),
                .clear,
            ],
            locations: [0, 0.55, 1],
            start: CGPoint(x: 0.5, y: 1),
            end: CGPoint(x: 0.5, y: 0)
        )
        topScrim.translatesAutoresizingMaskIntoConstraints = false

        let leftScrim = GradientLayerView(
            colors: [
                CinemaTheme.background.withAlphaComponent(0.95),
                CinemaTheme.background.withAlphaComponent(0.70),
                CinemaTheme.background.withAlphaComponent(0.32),
                CinemaTheme.background.withAlphaComponent(0.08),
                .clear,
            ],
            locations: [0, 0.24, 0.48, 0.72, 0.96],
            start: CGPoint(x: 0, y: 0.5),
            end: CGPoint(x: 1, y: 0.5)
        )
        leftScrim.translatesAutoresizingMaskIntoConstraints = false

        let bottomScrim = GradientLayerView(
            colors: [
                .clear,
                CinemaTheme.background.withAlphaComponent(0.45),
                CinemaTheme.background.withAlphaComponent(0.88),
                CinemaTheme.background,
            ],
            locations: [0, 0.38, 0.78, 1],
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
            action: #selector(playHeroClicked)
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
            detailsButton.heightAnchor.constraint(equalToConstant: 40),
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

        heroContainer.addSubview(heroFallbackView)
        heroContainer.addSubview(heroBackdrop)
        heroContainer.addSubview(topScrim)
        heroContainer.addSubview(leftScrim)
        heroContainer.addSubview(bottomScrim)
        heroContainer.addSubview(textStack)
        heroContainer.addSubview(heroDotsStack)

        NSLayoutConstraint.activate([
            heroContainer.heightAnchor.constraint(equalToConstant: 560),

            heroFallbackView.topAnchor.constraint(equalTo: heroContainer.topAnchor),
            heroFallbackView.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            heroFallbackView.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            heroFallbackView.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor),

            heroBackdrop.topAnchor.constraint(equalTo: heroContainer.topAnchor),
            heroBackdrop.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            heroBackdrop.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            heroBackdrop.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor),

            topScrim.topAnchor.constraint(equalTo: heroContainer.topAnchor),
            topScrim.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            topScrim.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            topScrim.heightAnchor.constraint(equalToConstant: 112),

            leftScrim.topAnchor.constraint(equalTo: heroContainer.topAnchor),
            leftScrim.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            leftScrim.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            leftScrim.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor),

            bottomScrim.leadingAnchor.constraint(equalTo: heroContainer.leadingAnchor),
            bottomScrim.trailingAnchor.constraint(equalTo: heroContainer.trailingAnchor),
            bottomScrim.bottomAnchor.constraint(equalTo: heroContainer.bottomAnchor),
            bottomScrim.heightAnchor.constraint(equalToConstant: 280),

            textStack.topAnchor.constraint(greaterThanOrEqualTo: heroContainer.topAnchor, constant: 108),
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
        contentStack.setCustomSpacing(28, after: heroContainer)

        continueSeriesRail.translatesAutoresizingMaskIntoConstraints = false
        continueSeriesRail.isHidden = true
        continueSeriesRail.onSelectContinue = { [weak self] entry in self?.onContinue(entry) }
        let cwSeriesHeight = continueSeriesRail.heightAnchor.constraint(equalToConstant: 0)
        cwSeriesHeight.isActive = true
        continueSeriesHeight = cwSeriesHeight
        contentStack.addArrangedSubview(continueSeriesRail)
        continueSeriesRail.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true

        for (idx, rail) in [trendingMoviesRail, trendingSeriesRail].enumerated() {
            rail.translatesAutoresizingMaskIntoConstraints = false
            let h = rail.heightAnchor.constraint(equalToConstant: 344)
            h.isActive = true
            if idx == 0 { moviesRailHeight = h } else { seriesRailHeight = h }
            rail.onSelectPoster = { [weak self] item in self?.onOpenDetail(item) }
            rail.onBuildContextMenu = { [weak self] item in self?.onBuildContextMenu?(item) }
            contentStack.addArrangedSubview(rail)
            rail.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true
        }

        for stack in [pinnedRailsStack, dynamicRailsStack] {
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 28
            stack.translatesAutoresizingMaskIntoConstraints = false
            contentStack.addArrangedSubview(stack)
            stack.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true
        }

        emptyHintLabel.font = .systemFont(ofSize: 14, weight: .regular)
        emptyHintLabel.textColor = CinemaTheme.textMuted
        emptyHintLabel.isHidden = true
        contentStack.addArrangedSubview(emptyHintLabel)
        emptyHintLabel.leadingAnchor.constraint(equalTo: contentStack.leadingAnchor, constant: 64).isActive = true
        emptyHintLabel.trailingAnchor.constraint(equalTo: contentStack.trailingAnchor, constant: -64).isActive = true

        NotificationCenter.default.addObserver(
            forName: .cinmaPinnedShelvesChanged,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in self?.loadAddonCatalogRails() }
        }
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
        let cw = (try? core.continueWatching(limit: 20)) ?? []
        let splitCw = (try? core.playerPreferences())?.splitContinueWatching ?? false
        if splitCw {
            let cwMovies = cw.filter { $0.mediaType == "movie" }
            let cwSeries = cw.filter { $0.mediaType != "movie" }
            continueRail.update(title: String(localized: "Continue Watching — Movies"), content: .continueWatching(cwMovies))
            continueRail.isHidden = cwMovies.isEmpty
            continueHeight?.constant = cwMovies.isEmpty ? 0 : 238
            continueSeriesRail.update(title: String(localized: "Continue Watching — Series"), content: .continueWatching(cwSeries))
            continueSeriesRail.isHidden = cwSeries.isEmpty
            continueSeriesHeight?.constant = cwSeries.isEmpty ? 0 : 238
        } else {
            continueRail.update(title: String(localized: "Continue Watching"), content: .continueWatching(Array(cw.prefix(12))))
            continueRail.isHidden = cw.isEmpty
            continueHeight?.constant = cw.isEmpty ? 0 : 238
            continueSeriesRail.isHidden = true
            continueSeriesHeight?.constant = 0
        }
        syncContinueRailPosition(cwAtTop: (try? core.playerPreferences())?.cwAtTop ?? true)

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
        let prefs = try? core.playerPreferences()
        syncContinueRailPosition(cwAtTop: prefs?.cwAtTop ?? true)
        let style = PosterStyleContext.load(from: core)
        let railH = style.itemSize.height + 42
        moviesRailHeight?.constant = railH
        seriesRailHeight?.constant = railH

        let visMovies = CatalogVisibilityFilter.apply(movies, core: core)
        let visSeries = CatalogVisibilityFilter.apply(series, core: core)
        let hasTmdb = (try? core.hasTmdbKey()) ?? false
        let src = hasTmdb ? "TMDB" : String(localized: "Addon Catalog")
        trendingMoviesRail.update(source: src, content: .posters(visMovies), style: style)
        trendingMoviesRail.isHidden = visMovies.isEmpty
        trendingSeriesRail.update(source: src, content: .posters(visSeries), style: style)
        trendingSeriesRail.isHidden = visSeries.isEmpty

        let candidates = Array((visMovies.prefix(3) + visSeries.prefix(2)))
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

    private func syncContinueRailPosition(cwAtTop: Bool) {
        let targetIdx: Int
        if cwAtTop {
            guard let heroIdx = contentStack.arrangedSubviews.firstIndex(of: heroContainer) else { return }
            targetIdx = heroIdx + 1
        } else {
            guard let seriesIdx = contentStack.arrangedSubviews.firstIndex(of: trendingSeriesRail) else { return }
            targetIdx = seriesIdx + 1
        }
        if contentStack.arrangedSubviews.firstIndex(of: continueRail) != targetIdx {
            contentStack.removeArrangedSubview(continueRail)
            let safeIdx = min(targetIdx, contentStack.arrangedSubviews.count)
            contentStack.insertArrangedSubview(continueRail, at: safeIdx)
            continueRail.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true
        }
        contentStack.setCustomSpacing((cwAtTop && !continueRail.isHidden) ? -44 : 28, after: heroContainer)
        let targetSeriesIdx = min((contentStack.arrangedSubviews.firstIndex(of: continueRail) ?? 0) + 1, contentStack.arrangedSubviews.count)
        if contentStack.arrangedSubviews.firstIndex(of: continueSeriesRail) != targetSeriesIdx {
            contentStack.removeArrangedSubview(continueSeriesRail)
            let safeIdx = min(targetSeriesIdx, contentStack.arrangedSubviews.count)
            contentStack.insertArrangedSubview(continueSeriesRail, at: safeIdx)
            continueSeriesRail.widthAnchor.constraint(equalTo: contentStack.widthAnchor).isActive = true
        }
    }

    private func loadAddonCatalogRails() {
        let core = core
        let collector = CatalogBatchCollector { [weak self] batch in
            Task { @MainActor [weak self] in
                self?.appendAddonRail(batch)
            }
        }
        for stack in [pinnedRailsStack, dynamicRailsStack] {
            for view in stack.arrangedSubviews {
                stack.removeArrangedSubview(view)
                view.removeFromSuperview()
            }
        }
        let pinned = (try? core.pinnedShelves()) ?? []
        Task {
            for shelf in pinned {
                if let page = try? await core.catalog(
                    addonId: shelf.addonId,
                    mediaType: shelf.mediaType,
                    catalogId: shelf.catalogId,
                    query: CatalogQueryInput(search: nil, genre: shelf.genre, skip: nil)
                ) {
                    let visItems = CatalogVisibilityFilter.apply(page.items, core: core)
                    if !visItems.isEmpty {
                        await MainActor.run { [weak self] in
                            self?.appendPinnedShelfRail(shelf, items: visItems)
                        }
                    }
                }
            }
            _ = try? await core.browseCatalogs(mediaType: nil, observer: collector)
            ImagePipeline.releaseMallocPressure()
        }
    }

    private func appendPinnedShelfRail(_ shelf: PinnedShelfFfi, items: [MetaPreviewInfo]) {
        let style = PosterStyleContext.load(from: core)
        let rail = CatalogRailView(title: "📌 \(shelf.title)", source: String(localized: "Pinned Shelf"))
        rail.translatesAutoresizingMaskIntoConstraints = false
        rail.heightAnchor.constraint(equalToConstant: style.itemSize.height + 42).isActive = true
        rail.onSelectPoster = { [weak self] item in self?.onOpenDetail(item) }
        rail.onBuildContextMenu = { [weak self] item in self?.onBuildContextMenu?(item) }
        rail.onUnpin = { [weak self] in
            try? self?.core.unpinShelf(id: shelf.id)
            self?.loadAddonCatalogRails()
        }
        rail.update(content: .posters(items), style: style)
        pinnedRailsStack.addArrangedSubview(rail)
        rail.widthAnchor.constraint(equalTo: pinnedRailsStack.widthAnchor).isActive = true
    }

    private func appendAddonRail(_ batch: CatalogBatchInfo) {
        let visItems = CatalogVisibilityFilter.apply(batch.items, core: core)
        let maxRails = Int(min(15, (try? core.playerPreferences())?.maxAddonRails ?? 6))
        guard batch.error == nil, !visItems.isEmpty, dynamicRailsStack.arrangedSubviews.count < maxRails else { return }
        let style = PosterStyleContext.load(from: core)
        let rail = CatalogRailView(title: batch.catalogName, source: batch.addonName)
        rail.translatesAutoresizingMaskIntoConstraints = false
        rail.heightAnchor.constraint(equalToConstant: style.itemSize.height + 42).isActive = true
        rail.onSelectPoster = { [weak self] item in self?.onOpenDetail(item) }
        rail.onBuildContextMenu = { [weak self] item in self?.onBuildContextMenu?(item) }
        rail.update(content: .posters(visItems), style: style)
        dynamicRailsStack.addArrangedSubview(rail)
        rail.widthAnchor.constraint(equalTo: dynamicRailsStack.widthAnchor).isActive = true
    }

    private func renderHero() {
        guard heroItems.indices.contains(heroIndex) else { return }
        let item = heroItems[heroIndex]
        heroFallbackView.updateColors(ArtworkPlaceholder.gradient(for: item.id).compactMap { NSColor(cgColor: $0) })
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
        heroBackdrop.setRemoteImage(art, pointSize: NSSize(width: 960, height: 420), taskStorage: &heroLoadTask)
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
            dot.setAccessibilityLabel(String(localized: "Featured slide \(i + 1)"))
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
        heroTimer = nil
        guard !CinemaTheme.reduceMotion else { return }
        if let window = view.window, !window.occlusionState.contains(.visible) || !NSApp.isActive {
            return
        }
        heroTimer = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(10))
                guard !Task.isCancelled, let self, self.heroItems.count > 1 else { continue }
                guard self.view.window?.occlusionState.contains(.visible) == true, NSApp.isActive else {
                    self.heroTimer = nil
                    return
                }
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

    @objc private func playHeroClicked() {
        guard heroItems.indices.contains(heroIndex) else { return }
        let item = heroItems[heroIndex]
        let quickPlay = (try? core.playerPreferences())?.quickPlayEnabled ?? true
        if quickPlay && !NSEvent.modifierFlags.contains(.option), let onQuickPlay {
            onQuickPlay(item)
        } else {
            onOpenDetail(item)
        }
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

enum CatalogVisibilityFilter {
    static func apply(_ items: [MetaPreviewInfo], core: CinmaCore) -> [MetaPreviewInfo] {
        guard let prefs = try? core.playerPreferences() else { return items }
        guard prefs.hideWatchedInCatalogs || prefs.hideUnreleased else { return items }
        let watched = prefs.hideWatchedInCatalogs ? Set((try? core.watchedMetaIds()) ?? []) : []
        let currentYear = Calendar.current.component(.year, from: Date())
        return items.filter { item in
            if prefs.hideWatchedInCatalogs && watched.contains(item.id) { return false }
            if prefs.hideUnreleased, let year = extractYear(item.releaseInfo), year > currentYear { return false }
            return true
        }
    }

    private static func extractYear(_ text: String?) -> Int? {
        guard let text, let range = text.range(of: #"\b(19|20)\d{2}\b"#, options: .regularExpression) else { return nil }
        return Int(text[range])
    }
}
