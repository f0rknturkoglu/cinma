import AppKit
import SwiftUI

/// Title detail + ranked stream picker (`Başlık detayı ve stream seçici` mockup M1).
@MainActor
final class TitleDetailViewController: NSViewController {
    enum StreamFilter: Int {
        case all = 0
        case uhd2160
        case fhd1080
        case under10Gb
        case hdr
    }

    let core: CinmaCore
    let preview: MetaPreviewInfo
    private let onBack: () -> Void
    let onPlay: (PlayRequest) -> Void
    var onOpenAddons: (() -> Void)?
    var onOpenRelated: ((MetaPreviewInfo) -> Void)?

    private let scrollView = NSScrollView()
    private let backdropView = AspectFillImageView()
    private let fallbackGradient = CAGradientLayer()
    private let logoView = NSImageView()
    private let titleLabel = NSTextField(wrappingLabelWithString: "")
    private let metaLabel = NSTextField(labelWithString: "")
    private let overviewLabel = NSTextField(wrappingLabelWithString: "")
    private let castLabel = NSTextField(wrappingLabelWithString: "")
    private let castChipsStack = NSStackView()
    /// "Watch on" provider logos (TMDB watch providers).
    private let providersStack = NSStackView()
    private var providerLogoTasks: [Task<Void, Never>?] = []
    /// Movies have no episode column, so the stream list may use the width.
    private var pickerMaxWidth: NSLayoutConstraint?
    private let collectionRail = CatalogRailView(title: String(localized: "Collection"))
    private var collectionRailHeight: NSLayoutConstraint?
    private var quickPlayButton: NSButton?
    private var libraryButton: NSButton?
    private var trailerButton: NSButton?

    let seasonsStack = NSStackView()
    let episodesStack = NSStackView()
    let episodesSection = NSStackView()

    let streamTitleLabel = NSTextField(labelWithString: "")
    let streamStatusLabel = NSTextField(wrappingLabelWithString: "")
    let streamSpinner = NSProgressIndicator()
    lazy var streamActionButton = CinemaTheme.makePillButton(
        title: String(localized: "Open Addons…"),
        symbol: "puzzlepiece.extension",
        primary: false,
        target: self,
        action: #selector(streamRecoveryAction)
    )
    let streamFilterStack = NSStackView()
    let streamsListStack = NSStackView()

    var richMeta: RichMetaInfo?
    var selectedSeason: UInt32 = 1
    var episodes: [MetaVideoInfo] = []
    var selectedEpisode: MetaVideoInfo?
    var progressByVideo: [String: WatchProgressInfo] = [:]
    var fillerEpisodeNumbers: Set<UInt32> = []

    var accumulatedStreams: [StreamInfo] = []
    var rankedWithDebrid: RankResultInfo?
    var debridRankTask: Task<Void, Never>?
    var respondedAddonsCount = 0
    var lastStreamVideoId: String?
    var lastStreamTitle: String?
    var lastStreamSeason: Int64?
    var lastStreamEpisode: Int64?
    var activeFilter: StreamFilter = .all
    var streamTask: Task<Void, Never>?
    private var backdropTask: Task<Void, Never>?
    private var logoTask: Task<Void, Never>?
    var episodeThumbTasks: [Task<Void, Never>] = []

    init(
        core: CinmaCore,
        preview: MetaPreviewInfo,
        onBack: @escaping () -> Void,
        onPlay: @escaping (PlayRequest) -> Void
    ) {
        self.core = core
        self.preview = preview
        self.onBack = onBack
        self.onPlay = onPlay
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

        let heroBox = NSView()
        heroBox.wantsLayer = true
        heroBox.layer?.masksToBounds = true
        fallbackGradient.colors = ArtworkPlaceholder.gradient(for: preview.id)
        heroBox.layer?.insertSublayer(fallbackGradient, at: 0)
        heroBox.translatesAutoresizingMaskIntoConstraints = false

        backdropView.translatesAutoresizingMaskIntoConstraints = false

        let dimPct = Double((try? core.playerPreferences())?.backdropDimPct ?? 50)
        let dimScale = CGFloat(min(1.8, max(0.35, dimPct / 50.0)))
        let leftScrim = GradientLayerView(
            colors: [
                CinemaTheme.background.withAlphaComponent(min(0.98, 0.95 * dimScale)),
                CinemaTheme.background.withAlphaComponent(min(0.95, 0.68 * dimScale)),
                CinemaTheme.background.withAlphaComponent(min(0.85, 0.28 * dimScale)),
                .clear,
            ],
            locations: [0, 0.25, 0.55, 0.92],
            start: CGPoint(x: 0, y: 0.5),
            end: CGPoint(x: 1, y: 0.5)
        )
        leftScrim.translatesAutoresizingMaskIntoConstraints = false
        let bottomScrim = GradientLayerView(
            colors: [
                .clear,
                CinemaTheme.background.withAlphaComponent(min(0.92, 0.45 * dimScale)),
                CinemaTheme.background.withAlphaComponent(min(0.98, 0.88 * dimScale)),
                CinemaTheme.background,
            ],
            locations: [0, 0.35, 0.78, 1],
            start: CGPoint(x: 0.5, y: 1),
            end: CGPoint(x: 0.5, y: 0)
        )
        bottomScrim.translatesAutoresizingMaskIntoConstraints = false

        heroBox.addSubview(backdropView)
        heroBox.addSubview(leftScrim)
        heroBox.addSubview(bottomScrim)

        let backBtn = CinemaTheme.makePillButton(
            title: String(localized: "Back"),
            symbol: "chevron.left",
            primary: false,
            target: self,
            action: #selector(goBack)
        )
        let playBtn = CinemaTheme.makePillButton(
            title: String(localized: "Quick Play"),
            symbol: "play.fill",
            primary: true,
            target: self,
            action: #selector(quickPlayClicked)
        )
        let libBtn = CinemaTheme.makePillButton(
            title: String(localized: "Add to Library"),
            symbol: "plus",
            primary: false,
            target: self,
            action: #selector(toggleLibrary)
        )
        let trBtn = CinemaTheme.makePillButton(
            title: String(localized: "Trailer"),
            symbol: "film",
            primary: false,
            target: self,
            action: #selector(playTrailer)
        )
        trBtn.isHidden = true
        quickPlayButton = playBtn
        libraryButton = libBtn
        trailerButton = trBtn
        for b in [backBtn, playBtn, libBtn, trBtn] {
            b.translatesAutoresizingMaskIntoConstraints = false
            b.heightAnchor.constraint(equalToConstant: 36).isActive = true
            b.widthAnchor.constraint(greaterThanOrEqualToConstant: 100).isActive = true
        }
        let topBar = NSStackView(views: [backBtn, playBtn, libBtn, trBtn])
        topBar.spacing = 10
        topBar.translatesAutoresizingMaskIntoConstraints = false

        logoView.imageScaling = .scaleProportionallyDown
        logoView.isHidden = true
        logoView.translatesAutoresizingMaskIntoConstraints = false
        logoView.heightAnchor.constraint(lessThanOrEqualToConstant: 90).isActive = true
        logoView.widthAnchor.constraint(lessThanOrEqualToConstant: 420).isActive = true

        titleLabel.stringValue = preview.name.uppercased()
        titleLabel.font = .systemFont(ofSize: 52, weight: .heavy)
        titleLabel.textColor = CinemaTheme.textPrimary

        metaLabel.font = .monospacedDigitSystemFont(ofSize: 14, weight: .medium)
        metaLabel.textColor = CinemaTheme.textSecondary

        overviewLabel.stringValue = preview.description ?? ""
        overviewLabel.font = .systemFont(ofSize: 15, weight: .regular)
        overviewLabel.textColor = CinemaTheme.textSecondary

        castLabel.font = .systemFont(ofSize: 13, weight: .regular)
        castLabel.textColor = CinemaTheme.textMuted

        castChipsStack.orientation = .horizontal
        castChipsStack.alignment = .centerY
        castChipsStack.spacing = 6
        // Let the row be narrower than all chips: it then detaches the
        // lowest-priority chips instead of squashing one (see rebuildCastChips).
        castChipsStack.setClippingResistancePriority(.defaultLow, for: .horizontal)
        castChipsStack.detachesHiddenViews = true
        castChipsStack.isHidden = true

        providersStack.orientation = .horizontal
        providersStack.alignment = .centerY
        providersStack.spacing = 8
        providersStack.isHidden = true

        let headerStack = NSStackView(views: [topBar, logoView, titleLabel, metaLabel, overviewLabel, castChipsStack, providersStack, castLabel])
        headerStack.orientation = .vertical
        headerStack.alignment = .leading
        headerStack.spacing = 12
        headerStack.translatesAutoresizingMaskIntoConstraints = false

        // Left column (episodes for series, or quick info for movie)
        seasonsStack.orientation = .horizontal
        seasonsStack.spacing = 6
        episodesStack.orientation = .vertical
        episodesStack.alignment = .leading
        episodesStack.spacing = 10

        episodesSection.orientation = .vertical
        episodesSection.alignment = .leading
        episodesSection.spacing = 14
        episodesSection.addArrangedSubview(seasonsStack)
        episodesSection.addArrangedSubview(episodesStack)
        episodesSection.translatesAutoresizingMaskIntoConstraints = false

        // Right column: Liquid Glass stream picker
        let pickerPanel = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 22)
        pickerPanel.translatesAutoresizingMaskIntoConstraints = false

        streamTitleLabel.font = .systemFont(ofSize: 18, weight: .bold)
        streamTitleLabel.textColor = CinemaTheme.textPrimary
        streamTitleLabel.stringValue = preview.name

        streamSpinner.style = .spinning
        streamSpinner.controlSize = .small
        streamSpinner.isDisplayedWhenStopped = false

        streamStatusLabel.font = .monospacedDigitSystemFont(ofSize: 12.5, weight: .regular)
        streamStatusLabel.textColor = CinemaTheme.textMuted
        streamStatusLabel.stringValue = String(localized: "Fetching streams…")

        streamActionButton.isHidden = true

        let statusRow = NSStackView(views: [streamSpinner, streamStatusLabel])
        statusRow.orientation = .horizontal
        statusRow.alignment = .centerY
        statusRow.spacing = 8

        streamFilterStack.orientation = .horizontal
        streamFilterStack.spacing = 6
        rebuildStreamFilters()

        streamsListStack.orientation = .vertical
        streamsListStack.alignment = .leading
        streamsListStack.spacing = 10
        streamsListStack.translatesAutoresizingMaskIntoConstraints = false

        let pickerContent = NSStackView(views: [streamTitleLabel, statusRow, streamActionButton, streamFilterStack, streamsListStack])
        pickerContent.orientation = .vertical
        pickerContent.alignment = .leading
        pickerContent.spacing = 12
        pickerContent.translatesAutoresizingMaskIntoConstraints = false
        pickerPanel.addSubview(pickerContent)

        let columns = NSStackView(views: [episodesSection, pickerPanel])
        columns.orientation = .horizontal
        columns.alignment = .top
        columns.spacing = 28
        columns.translatesAutoresizingMaskIntoConstraints = false

        collectionRail.translatesAutoresizingMaskIntoConstraints = false
        collectionRail.isHidden = true
        collectionRail.onSelectPoster = { [weak self] item in
            self?.onOpenRelated?(item)
        }
        collectionRail.onBuildContextMenu = { [weak self] item in
            guard let self else { return nil }
            return QuickPlayHelper.makePosterMenu(
                core: self.core,
                item: item,
                onOpenDetail: { [weak self] target in self?.onOpenRelated?(target) },
                onQuickPlay: { [weak self] target in
                    guard let self else { return }
                    QuickPlayHelper.launchBest(
                        core: self.core,
                        item: target,
                        onPlay: self.onPlay,
                        onStatus: { [weak self] msg in self?.streamStatusLabel.stringValue = msg },
                        onFallbackOpenDetail: { [weak self] in self?.onOpenRelated?(target) }
                    )
                },
                onStatus: { [weak self] msg in self?.streamStatusLabel.stringValue = msg }
            )
        }
        let colRailH = collectionRail.heightAnchor.constraint(equalToConstant: 0)
        colRailH.isActive = true
        collectionRailHeight = colRailH

        doc.addSubview(heroBox)
        doc.addSubview(headerStack)
        doc.addSubview(columns)
        doc.addSubview(collectionRail)
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

            heroBox.topAnchor.constraint(equalTo: doc.topAnchor),
            heroBox.leadingAnchor.constraint(equalTo: doc.leadingAnchor),
            heroBox.trailingAnchor.constraint(equalTo: doc.trailingAnchor),
            heroBox.heightAnchor.constraint(equalToConstant: 460),

            backdropView.topAnchor.constraint(equalTo: heroBox.topAnchor),
            backdropView.leadingAnchor.constraint(equalTo: heroBox.leadingAnchor),
            backdropView.trailingAnchor.constraint(equalTo: heroBox.trailingAnchor),
            backdropView.bottomAnchor.constraint(equalTo: heroBox.bottomAnchor),

            leftScrim.topAnchor.constraint(equalTo: heroBox.topAnchor),
            leftScrim.leadingAnchor.constraint(equalTo: heroBox.leadingAnchor),
            leftScrim.trailingAnchor.constraint(equalTo: heroBox.trailingAnchor),
            leftScrim.bottomAnchor.constraint(equalTo: heroBox.bottomAnchor),

            bottomScrim.leadingAnchor.constraint(equalTo: heroBox.leadingAnchor),
            bottomScrim.trailingAnchor.constraint(equalTo: heroBox.trailingAnchor),
            bottomScrim.bottomAnchor.constraint(equalTo: heroBox.bottomAnchor),
            bottomScrim.heightAnchor.constraint(equalToConstant: 220),

            headerStack.topAnchor.constraint(equalTo: doc.topAnchor, constant: 86),
            headerStack.leadingAnchor.constraint(equalTo: doc.leadingAnchor, constant: 64),
            headerStack.trailingAnchor.constraint(lessThanOrEqualTo: doc.trailingAnchor, constant: -64),
            headerStack.widthAnchor.constraint(lessThanOrEqualToConstant: 700),

            columns.topAnchor.constraint(equalTo: headerStack.bottomAnchor, constant: 32),
            columns.leadingAnchor.constraint(equalTo: doc.leadingAnchor, constant: 64),
            columns.trailingAnchor.constraint(equalTo: doc.trailingAnchor, constant: -64),

            collectionRail.topAnchor.constraint(equalTo: columns.bottomAnchor, constant: 28),
            collectionRail.leadingAnchor.constraint(equalTo: doc.leadingAnchor),
            collectionRail.trailingAnchor.constraint(equalTo: doc.trailingAnchor),
            collectionRail.bottomAnchor.constraint(equalTo: doc.bottomAnchor, constant: -48),

            episodesSection.widthAnchor.constraint(greaterThanOrEqualToConstant: 420),
            pickerPanel.widthAnchor.constraint(greaterThanOrEqualToConstant: 460),

            pickerContent.topAnchor.constraint(equalTo: pickerPanel.topAnchor, constant: 20),
            pickerContent.leadingAnchor.constraint(equalTo: pickerPanel.leadingAnchor, constant: 20),
            pickerContent.trailingAnchor.constraint(equalTo: pickerPanel.trailingAnchor, constant: -20),
            pickerContent.bottomAnchor.constraint(equalTo: pickerPanel.bottomAnchor, constant: -20),
            streamsListStack.widthAnchor.constraint(equalTo: pickerContent.widthAnchor),
            episodesStack.widthAnchor.constraint(equalTo: episodesSection.widthAnchor),
        ])
        let maxWidth = pickerPanel.widthAnchor.constraint(lessThanOrEqualToConstant: 580)
        maxWidth.isActive = true
        pickerMaxWidth = maxWidth
        // Stretch toward the available width (up to the cap) instead of hugging.
        let fill = pickerPanel.widthAnchor.constraint(equalTo: columns.widthAnchor)
        fill.priority = .defaultLow
        fill.isActive = true
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        backdropView.setRemoteImage(
            preview.background ?? preview.poster,
            pointSize: NSSize(width: 1280, height: 460),
            taskStorage: &backdropTask
        )
        refreshLibraryButton()
        loadRichMetadata()
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        fallbackGradient.frame = backdropView.superview?.bounds ?? .zero
        CATransaction.commit()
    }

    private func loadRichMetadata() {
        let core = core
        let preview = preview
        Task { [weak self] in
            do {
                let rich = try await core.richMeta(mediaType: preview.mediaType, id: preview.id)
                guard let self else { return }
                self.applyRichMeta(rich)
            } catch {
                guard let self else { return }
                // Even if metadata failed, movies can still query streams by preview.id.
                if preview.mediaType == "movie" {
                    self.fetchStreams(videoId: preview.id, title: preview.name, season: nil, episode: nil)
                } else {
                    self.streamSpinner.stopAnimation(nil)
                    self.streamStatusLabel.stringValue = FriendlyError.explain(error)
                }
            }
        }
    }

    private func applyRichMeta(_ rich: RichMetaInfo) {
        self.richMeta = rich
        let m = rich.meta
        titleLabel.stringValue = m.name.uppercased()
        if let logo = m.logo {
            showTitleLogo(logo)
        }
        backdropView.setRemoteImage(
            m.background ?? m.poster ?? preview.poster,
            pointSize: NSSize(width: 960, height: 380),
            taskStorage: &backdropTask
        )
        let genres = m.genres.prefix(3).joined(separator: ", ")
        metaLabel.stringValue = [
            m.releaseInfo,
            genres.isEmpty ? nil : genres,
            m.runtime,
            m.imdbRating.map { "IMDb \($0)" },
        ]
        .compactMap { $0 }
        .joined(separator: " · ")
        overviewLabel.stringValue = m.description ?? ""
        rebuildCastChips(cast: m.cast, directors: m.director)
        trailerButton?.isHidden = m.trailers.isEmpty
        refreshLibraryButton()
        loadProgress()
        loadTitleExtras(mediaType: m.mediaType, id: m.id, hasLogo: m.logo != nil)
        if m.mediaType == "series" {
            loadAnimeFillers(metaId: m.id)
        }

        pickerMaxWidth?.constant = m.mediaType == "series" ? 580 : 1100
        if m.mediaType == "series" {
            let seasons = rich.seasonNumbers.isEmpty
                ? Array(Set(m.videos.compactMap(\.season))).sorted()
                : rich.seasonNumbers
            selectedSeason = seasons.first ?? 1
            rebuildSeasonTabs(seasons)
            episodes = m.videos.filter { $0.season == selectedSeason }
            if episodes.isEmpty, let first = seasons.first {
                loadSeason(first)
            } else {
                renderEpisodes()
                if let firstEp = episodes.first {
                    selectEpisode(firstEp)
                }
            }
        } else {
            episodesSection.isHidden = true
            fetchStreams(videoId: m.id, title: m.name, season: nil, episode: nil)
        }
    }

    func loadProgress() {
        let metaId = richMeta?.meta.id ?? preview.id
        let rows = (try? core.progressForMeta(metaId: metaId)) ?? []
        progressByVideo = Dictionary(rows.map { ($0.videoId, $0) }, uniquingKeysWith: { a, _ in a })
    }

    private func loadTitleExtras(mediaType: String, id: String, hasLogo: Bool) {
        let core = core
        Task { [weak self] in
            guard let extras = try? await core.titleExtras(mediaType: mediaType, id: id),
                  let self
            else { return }
            if !hasLogo, let logo = extras.clearlogo {
                self.showTitleLogo(logo)
            }
            self.renderProviders(extras.watchProviders)
        }
        if mediaType == "movie" {
            Task { [weak self] in
                guard let self,
                      let col = try? await core.titleCollection(mediaType: mediaType, id: id),
                      col.parts.count > 1
                else { return }
                let style = PosterStyleContext.load(from: core)
                self.collectionRailHeight?.constant = style.itemSize.height + 42
                self.collectionRail.update(title: col.name, source: "TMDB", content: .posters(col.parts), style: style)
                self.collectionRail.isHidden = false
            }
        }
    }


    func refreshLibraryButton() {
        let metaId = richMeta?.meta.id ?? preview.id
        let mediaType = richMeta?.meta.mediaType ?? preview.mediaType
        let saved = (try? core.isInLibrary(metaId: metaId, mediaType: mediaType)) ?? false
        libraryButton?.title = saved ? String(localized: "In Library") : String(localized: "Add to Library")
        libraryButton?.image = NSImage(
            systemSymbolName: saved ? "checkmark" : "plus",
            accessibilityDescription: nil
        )
    }

    @objc func goBack() { onBack() }

    @objc private func quickPlayClicked() {
        if NSEvent.modifierFlags.contains(.option) {
            scrollView.contentView.scroll(to: NSPoint(x: 0, y: max(0, episodesSection.frame.minY - 90)))
            scrollView.reflectScrolledClipView(scrollView.contentView)
            return
        }
        if let top = (rankedWithDebrid ?? (try? core.rankStreams(streams: accumulatedStreams, preferences: nil)))?.ranked.first {
            let trigger = NSButton()
            trigger.identifier = NSUserInterfaceItemIdentifier(top.stream.raw)
            playStreamClicked(trigger)
            return
        }
        let targetItem = MetaPreviewInfo(
            id: richMeta?.meta.id ?? preview.id,
            mediaType: richMeta?.meta.mediaType ?? preview.mediaType,
            name: richMeta?.meta.name ?? preview.name,
            poster: richMeta?.meta.poster ?? preview.poster,
            posterShape: preview.posterShape,
            background: richMeta?.meta.background ?? preview.background,
            logo: richMeta?.meta.logo,
            description: richMeta?.meta.description ?? preview.description,
            releaseInfo: richMeta?.meta.releaseInfo ?? preview.releaseInfo,
            imdbRating: richMeta?.meta.imdbRating ?? preview.imdbRating,
            genres: richMeta?.meta.genres ?? preview.genres,
            runtime: richMeta?.meta.runtime ?? preview.runtime
        )
        QuickPlayHelper.launchBest(
            core: core,
            item: targetItem,
            episode: selectedEpisode,
            seasonOverride: selectedSeason,
            onPlay: onPlay,
            onStatus: { [weak self] msg in self?.streamStatusLabel.stringValue = msg }
        )
    }

    /// Shows the title's logo in place of the text title, but only once the
    /// image has loaded (a failed logo must not leave the page untitled).
    private func showTitleLogo(_ url: String) {
        logoTask?.cancel()
        let scale = view.window?.backingScaleFactor ?? 2.0
        logoTask = Task { [weak self] in
            let image = await ImagePipeline.shared.load(urlString: url, pointSize: NSSize(width: 420, height: 90), scale: scale)
            guard !Task.isCancelled, let self, let image else { return }
            self.logoView.image = image
            self.logoView.isHidden = false
            self.logoView.setAccessibilityLabel(self.titleLabel.stringValue)
            self.titleLabel.isHidden = true
        }
    }

    /// Provider logos with their names as tooltips; subscription first.
    private func renderProviders(_ providers: [WatchProviderInfo]) {
        for v in providersStack.arrangedSubviews {
            providersStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        providerLogoTasks.forEach { $0?.cancel() }
        providerLogoTasks = []
        let rank = ["flatrate": 0, "free": 1, "ads": 2, "rent": 3, "buy": 4]
        var seen = Set<String>()
        let unique = providers
            .sorted { (rank[$0.kind] ?? 9, $0.name) < (rank[$1.kind] ?? 9, $1.name) }
            .filter { seen.insert($0.name).inserted }
            .prefix(8)
        guard !unique.isEmpty else {
            providersStack.isHidden = true
            return
        }
        let caption = NSTextField(labelWithString: String(localized: "Watch on"))
        caption.font = .systemFont(ofSize: 12, weight: .semibold)
        caption.textColor = CinemaTheme.textMuted
        providersStack.addArrangedSubview(caption)
        for p in unique {
            let logo = NSImageView()
            logo.translatesAutoresizingMaskIntoConstraints = false
            logo.imageScaling = .scaleProportionallyUpOrDown
            logo.wantsLayer = true
            logo.layer?.cornerRadius = 8
            logo.layer?.masksToBounds = true
            logo.layer?.backgroundColor = NSColor(white: 1, alpha: 0.08).cgColor
            logo.toolTip = p.name
            logo.setAccessibilityLabel(p.name)
            NSLayoutConstraint.activate([
                logo.widthAnchor.constraint(equalToConstant: 34),
                logo.heightAnchor.constraint(equalToConstant: 34),
            ])
            var task: Task<Void, Never>?
            logo.setRemoteImage(p.logoUrl, pointSize: NSSize(width: 34, height: 34), taskStorage: &task)
            providerLogoTasks.append(task)
            providersStack.addArrangedSubview(logo)
        }
        providersStack.isHidden = false
    }

    private func rebuildCastChips(cast: [String], directors: [String]) {
        for v in castChipsStack.arrangedSubviews {
            castChipsStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        var names: [(String, Bool)] = directors.prefix(1).map { ($0, true) }
        for actor in cast.prefix(5) where !names.contains(where: { $0.0 == actor }) {
            names.append((actor, false))
        }
        castChipsStack.isHidden = names.isEmpty
        for (name, isDirector) in names {
            let title = isDirector ? "\(name) (\(String(localized: "Director")))" : name
            let chip = CinemaTheme.makePillButton(
                title: title,
                symbol: isDirector ? "film.stack" : "person.fill",
                primary: false,
                target: self,
                action: #selector(personChipClicked(_:))
            )
            chip.identifier = NSUserInterfaceItemIdentifier(name)
            chip.heightAnchor.constraint(equalToConstant: 26).isActive = true
            chip.setContentCompressionResistancePriority(.required, for: .horizontal)
            castChipsStack.addArrangedSubview(chip)
            // When the row is too narrow, drop trailing names instead of
            // squashing a chip to a sliver (the director stays).
            let order = castChipsStack.arrangedSubviews.count
            castChipsStack.setVisibilityPriority(
                isDirector ? .mustHold : NSStackView.VisibilityPriority(rawValue: Float(900 - order * 10)),
                for: chip
            )
        }
    }

    @objc private func personChipClicked(_ sender: NSButton) {
        guard let name = sender.identifier?.rawValue, !name.isEmpty else { return }
        let hosting = NSHostingController(
            rootView: PersonCreditsSheetView(
                core: core,
                personName: name,
                onSelectTitle: { [weak self] item in
                    self?.onOpenRelated?(item)
                },
                onDismiss: { [weak self] in
                    self?.dismiss(nil)
                }
            )
        )
        presentAsSheet(hosting)
    }

    @objc private func toggleLibrary() {
        let meta = richMeta?.meta
        let metaId = meta?.id ?? preview.id
        let mediaType = meta?.mediaType ?? preview.mediaType
        let name = meta?.name ?? preview.name
        let poster = meta?.poster ?? preview.poster
        let saved = (try? core.isInLibrary(metaId: metaId, mediaType: mediaType)) ?? false
        if saved {
            try? core.removeFromLibrary(metaId: metaId, mediaType: mediaType)
        } else {
            try? core.addToLibrary(metaId: metaId, mediaType: mediaType, name: name, poster: poster)
        }
        refreshLibraryButton()
    }

    @objc private func playTrailer() {
        guard let trailer = richMeta?.meta.trailers.first else { return }
        trailerButton?.isEnabled = false
        let core = core
        let title = richMeta?.meta.name ?? preview.name
        Task { [weak self] in
            defer { self?.trailerButton?.isEnabled = true }
            do {
                let url = try await core.extractTrailerUrl(source: trailer.source)
                self?.onPlay(.trailer(url: url, title: "\(title) — \(String(localized: "Trailer"))"))
            } catch {
                let alert = NSAlert()
                alert.messageText = String(localized: "Trailer unavailable")
                alert.informativeText = FriendlyError.explain(error)
                if let window = self?.view.window {
                    await alert.beginSheetModal(for: window)
                } else {
                    alert.runModal()
                }
            }
        }
    }

    static func formatTime(_ seconds: Double) -> String {
        let total = max(0, Int(seconds))
        let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%02d:%02d", m, s)
    }
}
