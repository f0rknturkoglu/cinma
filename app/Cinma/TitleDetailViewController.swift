import AppKit
import SwiftUI

/// Title detail + ranked stream picker (`Başlık detayı ve stream seçici` mockup M1).
@MainActor
final class TitleDetailViewController: NSViewController {
    private enum StreamFilter: Int {
        case all = 0
        case uhd2160
        case fhd1080
        case under10Gb
        case hdr
    }

    private let core: CinmaCore
    private let preview: MetaPreviewInfo
    private let onBack: () -> Void
    private let onPlay: (PlayRequest) -> Void
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
    private let collectionRail = CatalogRailView(title: String(localized: "Collection"))
    private var collectionRailHeight: NSLayoutConstraint?
    private var quickPlayButton: NSButton?
    private var libraryButton: NSButton?
    private var trailerButton: NSButton?

    private let seasonsStack = NSStackView()
    private let episodesStack = NSStackView()
    private let episodesSection = NSStackView()

    private let streamTitleLabel = NSTextField(labelWithString: "")
    private let streamStatusLabel = NSTextField(wrappingLabelWithString: "")
    private let streamSpinner = NSProgressIndicator()
    private lazy var streamActionButton = CinemaTheme.makePillButton(
        title: String(localized: "Open Addons…"),
        symbol: "puzzlepiece.extension",
        primary: false,
        target: self,
        action: #selector(streamRecoveryAction)
    )
    private let streamFilterStack = NSStackView()
    private let streamsListStack = NSStackView()

    private var richMeta: RichMetaInfo?
    private var selectedSeason: UInt32 = 1
    private var episodes: [MetaVideoInfo] = []
    private var selectedEpisode: MetaVideoInfo?
    private var progressByVideo: [String: WatchProgressInfo] = [:]

    private var accumulatedStreams: [StreamInfo] = []
    private var rankedWithDebrid: RankResultInfo?
    private var debridRankTask: Task<Void, Never>?
    private var respondedAddonsCount = 0
    private var lastStreamVideoId: String?
    private var lastStreamTitle: String?
    private var lastStreamSeason: Int64?
    private var lastStreamEpisode: Int64?
    private var activeFilter: StreamFilter = .all
    private var streamTask: Task<Void, Never>?
    private var backdropTask: Task<Void, Never>?
    private var logoTask: Task<Void, Never>?
    private var episodeThumbTasks: [Task<Void, Never>] = []

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
        castChipsStack.isHidden = true

        let headerStack = NSStackView(views: [topBar, logoView, titleLabel, metaLabel, overviewLabel, castChipsStack, castLabel])
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
            pickerPanel.widthAnchor.constraint(lessThanOrEqualToConstant: 580),

            pickerContent.topAnchor.constraint(equalTo: pickerPanel.topAnchor, constant: 20),
            pickerContent.leadingAnchor.constraint(equalTo: pickerPanel.leadingAnchor, constant: 20),
            pickerContent.trailingAnchor.constraint(equalTo: pickerPanel.trailingAnchor, constant: -20),
            pickerContent.bottomAnchor.constraint(equalTo: pickerPanel.bottomAnchor, constant: -20),
            streamsListStack.widthAnchor.constraint(equalTo: pickerContent.widthAnchor),
            episodesStack.widthAnchor.constraint(equalTo: episodesSection.widthAnchor),
        ])
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
            logoView.isHidden = false
            logoView.setRemoteImage(logo, pointSize: NSSize(width: 420, height: 90), taskStorage: &logoTask)
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

    private func loadProgress() {
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
                self.logoView.isHidden = false
                self.logoView.setRemoteImage(logo, pointSize: NSSize(width: 420, height: 90), taskStorage: &self.logoTask)
            }
            if !extras.watchProviders.isEmpty {
                let unique = Array(Set(extras.watchProviders.map(\.name))).sorted().prefix(5).joined(separator: ", ")
                let suffix = String(localized: "Watch on: \(unique)")
                if self.castLabel.stringValue.isEmpty {
                    self.castLabel.stringValue = suffix
                } else {
                    self.castLabel.stringValue += "\n\(suffix)"
                }
            }
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

    private func refreshDebridRanking(_ streams: [StreamInfo]) {
        debridRankTask?.cancel()
        let core = core
        debridRankTask = Task { [weak self] in
            guard let ranked = try? await core.rankStreamsWithDebrid(streams: streams, preferences: nil),
                  !Task.isCancelled,
                  let self
            else { return }
            self.rankedWithDebrid = ranked
            self.renderRankedStreams()
        }
    }

    private func rebuildSeasonTabs(_ seasons: [UInt32]) {
        for v in seasonsStack.arrangedSubviews {
            seasonsStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        for s in seasons {
            let num = Int(s)
            let btn = CinemaTheme.makePillButton(
                title: String(localized: "Season \(num)"),
                primary: s == selectedSeason,
                target: self,
                action: #selector(selectSeason(_:))
            )
            btn.tag = Int(s)
            btn.heightAnchor.constraint(equalToConstant: 32).isActive = true
            seasonsStack.addArrangedSubview(btn)
        }
        let prefs = try? core.playerPreferences()
        let sortAsc = prefs?.episodeSortAsc ?? true
        let sortBtn = CinemaTheme.makePillButton(
            title: sortAsc ? "1→N" : "N→1",
            symbol: "arrow.up.arrow.down",
            primary: false,
            target: self,
            action: #selector(toggleEpisodeSort)
        )
        sortBtn.heightAnchor.constraint(equalToConstant: 32).isActive = true
        seasonsStack.addArrangedSubview(sortBtn)

        let compact = prefs?.episodeCompactLayout ?? false
        let layoutBtn = CinemaTheme.makePillButton(
            title: compact ? String(localized: "Detailed") : String(localized: "Compact"),
            symbol: compact ? "rectangle.grid.1x2" : "list.bullet",
            primary: compact,
            target: self,
            action: #selector(toggleEpisodeLayout)
        )
        layoutBtn.heightAnchor.constraint(equalToConstant: 32).isActive = true
        seasonsStack.addArrangedSubview(layoutBtn)
    }

    private func loadSeason(_ season: UInt32) {
        let metaId = richMeta?.meta.id ?? preview.id
        let core = core
        Task { [weak self] in
            let eps = (try? await core.seasonEpisodes(mediaType: "series", metaId: metaId, season: season)) ?? []
            guard let self else { return }
            self.episodes = eps
            self.renderEpisodes()
            if let first = eps.first, self.selectedEpisode == nil {
                self.selectEpisode(first)
            }
        }
    }

    private func renderEpisodes() {
        episodeThumbTasks.forEach { $0.cancel() }
        episodeThumbTasks.removeAll()
        for v in episodesStack.arrangedSubviews {
            episodesStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }

        let prefs = try? core.playerPreferences()
        let ordered = (prefs?.episodeSortAsc ?? true) ? episodes : Array(episodes.reversed())
        let compact = prefs?.episodeCompactLayout ?? false
        let hideSpoilers = prefs?.hideSpoilers ?? false

        for ep in ordered {
            let isSelected = ep.id == selectedEpisode?.id
            let row = NSButton(title: "", target: self, action: #selector(episodeClicked(_:)))
            row.identifier = NSUserInterfaceItemIdentifier(ep.id)
            row.menu = makeEpisodeMenu(ep)
            row.isBordered = false
            row.wantsLayer = true
            row.layer?.cornerRadius = 14
            row.layer?.backgroundColor = (isSelected ? NSColor(white: 1, alpha: 0.08) : CinemaTheme.cardBackground).cgColor
            row.layer?.borderWidth = 1
            row.layer?.borderColor = (isSelected ? CinemaTheme.highlightBorder : CinemaTheme.cardBorder).cgColor

            let num = ep.episode ?? ep.number ?? 0
            let prog = progressByVideo[ep.id]
            let isWatched = prog?.watched == true
            let hideSpoiler = hideSpoilers && !isWatched

            let statusText: String
            if isWatched {
                statusText = String(localized: "Watched")
            } else if let p = prog, p.durationS > 0 {
                let leftMin = max(1, Int(((p.durationS - p.positionS) / 60).rounded()))
                statusText = String(localized: "\(leftMin) min left")
            } else {
                statusText = ""
            }
            let statusLabel = NSTextField(labelWithString: statusText)
            statusLabel.font = .monospacedDigitSystemFont(ofSize: 12, weight: .medium)
            statusLabel.textColor = isWatched ? CinemaTheme.green : CinemaTheme.accent

            if compact {
                let compactTitle = NSTextField(labelWithString: "\(num). \(ep.title ?? "")")
                compactTitle.font = .systemFont(ofSize: 13.5, weight: .semibold)
                compactTitle.textColor = CinemaTheme.textPrimary
                compactTitle.lineBreakMode = .byTruncatingTail
                compactTitle.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

                let dateLabel = NSTextField(labelWithString: ep.released ?? "")
                dateLabel.font = .monospacedDigitSystemFont(ofSize: 11.5, weight: .regular)
                dateLabel.textColor = CinemaTheme.textMuted

                let spacer = NSView()
                spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)

                let hStack = NSStackView(views: [compactTitle, spacer, dateLabel, statusLabel])
                hStack.orientation = .horizontal
                hStack.alignment = .centerY
                hStack.spacing = 10
                hStack.translatesAutoresizingMaskIntoConstraints = false
                row.addSubview(hStack)
                NSLayoutConstraint.activate([
                    hStack.topAnchor.constraint(equalTo: row.topAnchor, constant: 10),
                    hStack.bottomAnchor.constraint(equalTo: row.bottomAnchor, constant: -10),
                    hStack.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: 14),
                    hStack.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -14),
                ])
                episodesStack.addArrangedSubview(row)
                row.widthAnchor.constraint(equalTo: episodesStack.widthAnchor).isActive = true
                continue
            }

            let thumb = AspectFillImageView()
            thumb.layer?.cornerRadius = 10
            thumb.layer?.masksToBounds = true
            thumb.layer?.backgroundColor = NSColor(white: 1, alpha: 0.06).cgColor
            thumb.translatesAutoresizingMaskIntoConstraints = false
            var task: Task<Void, Never>?
            thumb.setRemoteImage(ep.thumbnail ?? richMeta?.meta.background, pointSize: NSSize(width: 176, height: 99), taskStorage: &task)
            if let task { episodeThumbTasks.append(task) }

            if hideSpoiler {
                let blur = NSVisualEffectView()
                blur.material = .hudWindow
                blur.blendingMode = .withinWindow
                blur.state = .active
                blur.translatesAutoresizingMaskIntoConstraints = false
                let eye = NSImageView(
                    image: NSImage(systemSymbolName: "eye.slash.fill", accessibilityDescription: String(localized: "Spoiler Hidden")) ?? NSImage()
                )
                eye.contentTintColor = CinemaTheme.textSecondary
                eye.translatesAutoresizingMaskIntoConstraints = false
                blur.addSubview(eye)
                thumb.addSubview(blur)
                NSLayoutConstraint.activate([
                    blur.topAnchor.constraint(equalTo: thumb.topAnchor),
                    blur.bottomAnchor.constraint(equalTo: thumb.bottomAnchor),
                    blur.leadingAnchor.constraint(equalTo: thumb.leadingAnchor),
                    blur.trailingAnchor.constraint(equalTo: thumb.trailingAnchor),
                    eye.centerXAnchor.constraint(equalTo: blur.centerXAnchor),
                    eye.centerYAnchor.constraint(equalTo: blur.centerYAnchor),
                ])
            }

            let title = NSTextField(labelWithString: "\(num). \(ep.title ?? "")")
            title.font = .systemFont(ofSize: 14.5, weight: .semibold)
            title.textColor = CinemaTheme.textPrimary
            title.lineBreakMode = .byTruncatingTail

            let sub = NSTextField(labelWithString: ep.released ?? "")
            sub.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
            sub.textColor = CinemaTheme.textMuted

            let overviewText = hideSpoiler
                ? String(localized: "Spoiler hidden until watched")
                : (ep.overview ?? "")
            let overviewLabel = NSTextField(wrappingLabelWithString: overviewText)
            overviewLabel.font = .systemFont(ofSize: 12, weight: .regular)
            overviewLabel.textColor = CinemaTheme.textSecondary
            overviewLabel.maximumNumberOfLines = 2
            overviewLabel.isHidden = overviewText.isEmpty

            let textCol = NSStackView(views: [title, sub, overviewLabel, statusLabel])
            textCol.orientation = .vertical
            textCol.alignment = .leading
            textCol.spacing = 4

            let hStack = NSStackView(views: [thumb, textCol])
            hStack.orientation = .horizontal
            hStack.alignment = .centerY
            hStack.spacing = 14
            hStack.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(hStack)

            NSLayoutConstraint.activate([
                thumb.widthAnchor.constraint(equalToConstant: 160),
                thumb.heightAnchor.constraint(equalToConstant: 90),
                hStack.topAnchor.constraint(equalTo: row.topAnchor, constant: 10),
                hStack.bottomAnchor.constraint(equalTo: row.bottomAnchor, constant: -10),
                hStack.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: 10),
                hStack.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -14),
            ])
            episodesStack.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: episodesStack.widthAnchor).isActive = true
        }
    }

    private func selectEpisode(_ ep: MetaVideoInfo) {
        selectedEpisode = ep
        renderEpisodes()
        let s = ep.season ?? selectedSeason
        let e = ep.episode ?? ep.number ?? 0
        let label = "S\(s) · B\(e) — \(ep.title ?? "")"
        fetchStreams(videoId: ep.id, title: label, season: Int64(s), episode: Int64(e))
    }

    private func fetchStreams(videoId: String, title: String, season: Int64?, episode: Int64?) {
        streamTask?.cancel()
        accumulatedStreams = []
        respondedAddonsCount = 0
        lastStreamVideoId = videoId
        lastStreamTitle = title
        lastStreamSeason = season
        lastStreamEpisode = episode
        streamTitleLabel.stringValue = title
        streamActionButton.isHidden = true
        streamSpinner.startAnimation(nil)

        let totalEnabledAddons = ((try? core.addons()) ?? []).filter(\.enabled).count
        streamStatusLabel.stringValue = totalEnabledAddons > 0
            ? String(localized: "Querying \(totalEnabledAddons) addons…")
            : String(localized: "Fetching streams…")
        renderRankedStreams()

        let mediaType = richMeta?.meta.mediaType ?? preview.mediaType
        let core = core
        let collector = StreamBatchCollector { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.respondedAddonsCount += 1
            }
        }
        streamTask = Task { [weak self] in
            // Re-wrap collector so we append incoming streams progressively per addon.
            let liveCollector = StreamBatchCollector { [weak self] batch in
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.respondedAddonsCount += 1
                    self.accumulatedStreams.append(contentsOf: batch.streams)
                    if totalEnabledAddons > 0 {
                        self.streamStatusLabel.stringValue = String(
                            localized: "\(min(self.respondedAddonsCount, totalEnabledAddons)) of \(totalEnabledAddons) addons answered · \(self.accumulatedStreams.count) streams"
                        )
                    }
                    self.renderRankedStreams()
                }
            }
            _ = collector
            do {
                let all = try await core.streamsIncremental(mediaType: mediaType, id: videoId, observer: liveCollector)
                guard !Task.isCancelled, let self else { return }
                self.streamSpinner.stopAnimation(nil)
                self.accumulatedStreams = all
                self.rankedWithDebrid = nil
                if all.isEmpty {
                    self.streamStatusLabel.stringValue = String(localized: "No playable streams found from enabled addons. Check Addons or try another source.")
                    self.streamActionButton.title = String(localized: "Open Addons…")
                    self.streamActionButton.isHidden = false
                } else {
                    self.refreshDebridRanking(all)
                }
                self.renderRankedStreams()
            } catch {
                guard !Task.isCancelled, let self else { return }
                self.streamSpinner.stopAnimation(nil)
                if self.accumulatedStreams.isEmpty {
                    self.streamStatusLabel.stringValue = FriendlyError.explain(error)
                    let hint = FriendlyError.actionHint(error)
                    self.streamActionButton.title = hint == .openAddons
                        ? String(localized: "Open Addons…")
                        : String(localized: "Retry")
                    self.streamActionButton.isHidden = false
                }
            }
        }
    }

    @objc private func streamRecoveryAction() {
        if streamActionButton.title == String(localized: "Open Addons…") {
            onOpenAddons?()
        } else if let vid = lastStreamVideoId, let title = lastStreamTitle {
            fetchStreams(videoId: vid, title: title, season: lastStreamSeason, episode: lastStreamEpisode)
        }
    }

    override func cancelOperation(_ sender: Any?) {
        goBack()
    }

    private func rebuildStreamFilters() {
        for v in streamFilterStack.arrangedSubviews {
            streamFilterStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        let filters: [(StreamFilter, String)] = [
            (.all, String(localized: "All")),
            (.uhd2160, "2160p"),
            (.fhd1080, "1080p"),
            (.under10Gb, "< 10 GB"),
            (.hdr, "HDR"),
        ]
        for (filter, title) in filters {
            let btn = CinemaTheme.makePillButton(
                title: title,
                primary: activeFilter == filter,
                target: self,
                action: #selector(selectStreamFilter(_:))
            )
            btn.tag = filter.rawValue
            btn.heightAnchor.constraint(equalToConstant: 28).isActive = true
            streamFilterStack.addArrangedSubview(btn)
        }
        let sortMode = (try? core.playerPreferences())?.streamSort ?? "score"
        let sortBtn = CinemaTheme.makePillButton(
            title: Self.sortButtonTitle(sortMode),
            symbol: "arrow.up.arrow.down",
            primary: false,
            target: self,
            action: #selector(cycleStreamSort)
        )
        sortBtn.heightAnchor.constraint(equalToConstant: 28).isActive = true
        streamFilterStack.addArrangedSubview(sortBtn)
    }

    private func renderRankedStreams() {
        for v in streamsListStack.arrangedSubviews {
            streamsListStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        guard !accumulatedStreams.isEmpty else { return }
        guard let result = rankedWithDebrid ?? (try? core.rankStreams(streams: accumulatedStreams, preferences: nil)) else { return }

        var filtered = result.ranked.filter { item in
            switch activeFilter {
            case .all: true
            case .uhd2160: item.parsed.resolution == "4K"
            case .fhd1080: item.parsed.resolution == "1080p"
            case .under10Gb: (item.parsed.sizeBytes ?? 0) <= 10 * 1024 * 1024 * 1024
            case .hdr: !item.parsed.hdrBadges.isEmpty
            }
        }
        let prefs = try? core.playerPreferences()
        if prefs?.rememberLastStream ?? true {
            let metaId = richMeta?.meta.id ?? preview.id
            if let saved = try? core.seriesSource(metaId: metaId),
               saved.bingeGroup != nil || saved.addonName != nil
            {
                let preferred = filtered.filter { Self.matchesSavedSource($0, saved: saved) }
                let rest = filtered.filter { !Self.matchesSavedSource($0, saved: saved) }
                filtered = preferred + rest
            }
        }
        let addonCount = Set(accumulatedStreams.map(\.addonName)).count
        streamStatusLabel.stringValue = String(localized: "\(filtered.count) streams from \(addonCount) addons")

        for (index, item) in filtered.enumerated() {
            streamsListStack.addArrangedSubview(makeStreamCard(item, isTop: index == 0))
            streamsListStack.arrangedSubviews.last?.widthAnchor.constraint(equalTo: streamsListStack.widthAnchor).isActive = true
        }
    }

    private func makeStreamCard(_ item: RankedStreamInfo, isTop: Bool) -> NSView {
        let card = NSView()
        card.wantsLayer = true
        card.layer?.cornerRadius = 16
        card.layer?.backgroundColor = (isTop ? CinemaTheme.highlightBackground : CinemaTheme.cardBackground).cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = (isTop ? CinemaTheme.highlightBorder : CinemaTheme.cardBorder).cgColor

        let tierText = Self.tierTitle(item.tier)
        let tierChip = CinemaTheme.makeChip(tierText, highlighted: isTop)

        let resLabel = NSTextField(labelWithString: item.parsed.resolution)
        resLabel.font = .systemFont(ofSize: 15, weight: .bold)
        resLabel.textColor = CinemaTheme.textPrimary

        var badgeViews: [NSView] = [tierChip, resLabel]
        if item.parsed.cachedDebrid {
            badgeViews.append(CinemaTheme.makeChip(String(localized: "Instant Debrid"), highlighted: true))
        }
        for b in item.parsed.hdrBadges { badgeViews.append(CinemaTheme.makeChip(b)) }
        if item.parsed.codec != "Unknown" { badgeViews.append(CinemaTheme.makeChip(item.parsed.codec)) }
        if item.parsed.audio != "Unknown" { badgeViews.append(CinemaTheme.makeChip(item.parsed.audio)) }

        let topRow = NSStackView(views: badgeViews)
        topRow.orientation = .horizontal
        topRow.spacing = 6

        var stats: [String] = []
        if let bytes = item.parsed.sizeBytes {
            stats.append(ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file))
        }
        if let seeders = item.parsed.seeders {
            stats.append("\(seeders) seed")
        }
        stats.append(item.stream.addonName)
        let statsLabel = NSTextField(labelWithString: stats.joined(separator: "  ·  "))
        statsLabel.font = .monospacedDigitSystemFont(ofSize: 12.5, weight: .regular)
        statsLabel.textColor = CinemaTheme.textSecondary

        let whyText = Self.explain(item.reasons)
        let whyLabel = NSTextField(wrappingLabelWithString: "\(String(localized: "Why:")) \(whyText)")
        whyLabel.font = .systemFont(ofSize: 12, weight: .regular)
        whyLabel.textColor = CinemaTheme.textMuted

        let playBtn = CinemaTheme.makePillButton(
            title: String(localized: "Play"),
            symbol: "play.fill",
            primary: isTop,
            target: self,
            action: #selector(playStreamClicked(_:))
        )
        playBtn.identifier = NSUserInterfaceItemIdentifier(item.stream.raw)
        playBtn.heightAnchor.constraint(equalToConstant: 32).isActive = true
        playBtn.widthAnchor.constraint(greaterThanOrEqualToConstant: 96).isActive = true
        playBtn.setAccessibilityLabel("\(String(localized: "Play")) \(item.parsed.resolution) \(item.stream.addonName)")

        let dlBtn = CinemaTheme.makePillButton(
            title: String(localized: "Download"),
            symbol: "arrow.down.circle",
            primary: false,
            target: self,
            action: #selector(downloadStreamClicked(_:))
        )
        dlBtn.identifier = NSUserInterfaceItemIdentifier(item.stream.raw)
        dlBtn.heightAnchor.constraint(equalToConstant: 32).isActive = true
        dlBtn.widthAnchor.constraint(greaterThanOrEqualToConstant: 110).isActive = true
        dlBtn.setAccessibilityLabel("\(String(localized: "Download")) \(item.parsed.resolution) \(item.stream.addonName)")

        let actionsRow = NSStackView(views: [playBtn, dlBtn])
        actionsRow.orientation = .horizontal
        actionsRow.spacing = 8

        let detailPrefs = try? core.playerPreferences()
        if detailPrefs?.externalPlayerEnabled == true,
           let extUrl = item.stream.url,
           extUrl.hasPrefix("http")
        {
            let extBtn = CinemaTheme.makePillButton(
                title: String(localized: "External"),
                symbol: "arrow.up.forward.app",
                primary: false,
                target: self,
                action: #selector(openExternalStreamClicked(_:))
            )
            extBtn.identifier = NSUserInterfaceItemIdentifier(extUrl)
            extBtn.heightAnchor.constraint(equalToConstant: 32).isActive = true
            actionsRow.addArrangedSubview(extBtn)
        }

        var cardViews: [NSView] = [topRow]
        let showFilename = detailPrefs?.pickerShowFilename ?? true
        if showFilename,
           let rawName = (item.stream.filename ?? item.stream.title)?.trimmingCharacters(in: .whitespacesAndNewlines),
           !rawName.isEmpty
        {
            let nameLabel = NSTextField(labelWithString: rawName)
            nameLabel.font = .systemFont(ofSize: 12, weight: .medium)
            nameLabel.textColor = CinemaTheme.textPrimary.withAlphaComponent(0.9)
            nameLabel.lineBreakMode = .byTruncatingMiddle
            nameLabel.maximumNumberOfLines = 2
            cardViews.append(nameLabel)
        }
        cardViews.append(contentsOf: [statsLabel, whyLabel, actionsRow])

        let vStack = NSStackView(views: cardViews)
        vStack.orientation = .vertical
        vStack.alignment = .leading
        vStack.spacing = 8
        vStack.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(vStack)

        NSLayoutConstraint.activate([
            vStack.topAnchor.constraint(equalTo: card.topAnchor, constant: 14),
            vStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -14),
            vStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 14),
            vStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -14),
        ])
        return card
    }

    private static func tierTitle(_ tier: RankTier) -> String {
        switch tier {
        case .best: String(localized: "RECOMMENDED")
        case .recommended: String(localized: "HIGH")
        case .good: String(localized: "BALANCED")
        case .fallback: String(localized: "FALLBACK")
        }
    }

    private static func explain(_ reasons: [ScoreReasonInfo]) -> String {
        reasons
            .sorted { abs($0.points) > abs($1.points) }
            .prefix(4)
            .map { r in
                let detail = r.detail ?? ""
                switch r.code {
                case "codec.av1_sw": return String(localized: "Software AV1 decode (no HW AV1 on this Mac)")
                case "seeders":
                    let count = r.detail ?? "0"
                    return String(localized: "\(count) seeders")
                case "direct": return String(localized: "Direct / cached stream")
                case "debrid.cached": return String(localized: "Cached on your Debrid account")
                case "language": return String(localized: "Language: \(detail)")
                case "release_group": return String(localized: "Group: \(detail)")
                default: return r.detail ?? r.code
                }
            }
            .joined(separator: " · ")
    }

    private func refreshLibraryButton() {
        let metaId = richMeta?.meta.id ?? preview.id
        let mediaType = richMeta?.meta.mediaType ?? preview.mediaType
        let saved = (try? core.isInLibrary(metaId: metaId, mediaType: mediaType)) ?? false
        libraryButton?.title = saved ? String(localized: "In Library") : String(localized: "Add to Library")
        libraryButton?.image = NSImage(
            systemSymbolName: saved ? "checkmark" : "plus",
            accessibilityDescription: nil
        )
    }

    @objc private func downloadStreamClicked(_ sender: NSButton) {
        guard let raw = sender.identifier?.rawValue,
              let stream = accumulatedStreams.first(where: { $0.raw == raw })
        else { return }
        let meta = richMeta?.meta
        let metaId = meta?.id ?? preview.id
        let mediaType = meta?.mediaType ?? preview.mediaType
        let baseTitle = meta?.name ?? preview.name
        let epSuffix = selectedEpisode.map { " S\($0.season ?? selectedSeason)E\($0.episode ?? $0.number ?? 0)" } ?? ""
        let title = "\(baseTitle)\(epSuffix)"
        do {
            if stream.infoHash != nil {
                _ = try core.addTorrentDownload(
                    title: title,
                    stream: stream,
                    metaId: metaId,
                    mediaType: mediaType,
                    priority: 10
                )
            } else if let url = stream.url, url.hasPrefix("http") {
                _ = try core.addHttpDownload(
                    title: title,
                    url: url,
                    metaId: metaId,
                    mediaType: mediaType,
                    priority: 10
                )
            } else {
                streamStatusLabel.stringValue = String(localized: "This stream source cannot be queued for offline download.")
                return
            }
            sender.title = String(localized: "Queued")
            sender.isEnabled = false
        } catch {
            streamStatusLabel.stringValue = FriendlyError.explain(error)
        }
    }

    @objc private func openExternalStreamClicked(_ sender: NSButton) {
        guard let urlStr = sender.identifier?.rawValue,
              let url = URL(string: urlStr)
        else { return }
        let prefs = try? core.playerPreferences()
        let preset = prefs?.externalPlayerPreset ?? "auto"
        let encoded = urlStr.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? urlStr
        if let schemeUrl: URL = {
            switch preset {
            case "iina": return URL(string: "iina://weblink?url=\(encoded)")
            case "infuse": return URL(string: "infuse://x-callback-url/play?url=\(encoded)")
            case "vlc": return URL(string: "vlc://\(urlStr)")
            default: return nil
            }
        }() {
            if NSWorkspace.shared.open(schemeUrl) {
                streamStatusLabel.stringValue = String(localized: "Opened stream in \(preset.uppercased())")
                return
            }
        }
        let customPath = prefs?.externalPlayerPath
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let candidates = [
            customPath,
            "/Applications/IINA.app/Contents/MacOS/iina-cli",
            "/opt/homebrew/bin/mpv",
            "/usr/local/bin/mpv",
        ].filter { !$0.isEmpty }
        if let binary = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) {
            let proc = Process()
            proc.executableURL = URL(fileURLWithPath: binary)
            proc.arguments = [urlStr]
            do {
                try proc.run()
                let appName = URL(fileURLWithPath: binary).lastPathComponent
                streamStatusLabel.stringValue = String(localized: "Opened stream in \(appName)")
                return
            } catch {
                streamStatusLabel.stringValue = FriendlyError.explain(error)
                return
            }
        }
        if !customPath.isEmpty && customPath.hasSuffix(".app"),
           FileManager.default.fileExists(atPath: customPath)
        {
            NSWorkspace.shared.open(
                [url],
                withApplicationAt: URL(fileURLWithPath: customPath),
                configuration: NSWorkspace.OpenConfiguration()
            )
            streamStatusLabel.stringValue = String(localized: "Opened stream in external player")
            return
        }
        if NSWorkspace.shared.open(url) {
            streamStatusLabel.stringValue = String(localized: "Opened stream in external player")
        } else {
            streamStatusLabel.stringValue = String(localized: "No external mpv/IINA binary found. Configure its path in Settings.")
        }
    }

    @objc private func goBack() { onBack() }

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
            castChipsStack.addArrangedSubview(chip)
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

    private func makeEpisodeMenu(_ ep: MetaVideoInfo) -> NSMenu {
        let menu = NSMenu()
        let metaId = richMeta?.meta.id ?? preview.id
        let showTitle = richMeta?.meta.name ?? preview.name
        let epTitle = ep.title ?? showTitle
        let isWatched = progressByVideo[ep.id]?.watched == true

        menu.addItem(ClosureMenuProxy.item(title: String(localized: "Quick Play Episode"), systemSymbol: "play.fill") { [weak self] in
            guard let self else { return }
            QuickPlayHelper.launchBest(
                core: self.core,
                item: self.preview,
                episode: ep,
                seasonOverride: self.selectedSeason,
                onPlay: self.onPlay,
                onStatus: { [weak self] msg in self?.streamStatusLabel.stringValue = msg }
            )
        })
        menu.addItem(ClosureMenuProxy.item(
            title: isWatched ? String(localized: "Mark Episode as Unwatched") : String(localized: "Mark Episode as Watched"),
            systemSymbol: isWatched ? "eye.slash" : "checkmark.circle"
        ) { [weak self] in
            guard let self else { return }
            try? self.core.setWatched(
                videoId: ep.id,
                metaId: metaId,
                mediaType: "series",
                title: epTitle,
                watched: !isWatched
            )
            self.loadProgress()
            self.renderEpisodes()
        })
        if let targetIdx = episodes.firstIndex(where: { $0.id == ep.id }) {
            let preceding = Array(episodes.prefix(targetIdx + 1))
            if preceding.count > 1 {
                menu.addItem(ClosureMenuProxy.item(
                    title: String(localized: "Mark This & Previous Episodes as Watched"),
                    systemSymbol: "checkmark.circle.badge.plus"
                ) { [weak self] in
                    guard let self else { return }
                    let batch = preceding.map { item in
                        BatchEpisodeMarkFfi(
                            videoId: item.id,
                            title: item.title ?? showTitle,
                            season: Int64(item.season ?? self.selectedSeason),
                            episode: Int64(item.episode ?? item.number ?? 0)
                        )
                    }
                    try? self.core.markEpisodesWatched(
                        metaId: metaId,
                        mediaType: "series",
                        items: batch,
                        watched: true
                    )
                    self.loadProgress()
                    self.renderEpisodes()
                })
            }
        }
        menu.addItem(.separator())
        menu.addItem(ClosureMenuProxy.item(
            title: String(localized: "Reset Remembered Source"),
            systemSymbol: "arrow.counterclockwise"
        ) { [weak self] in
            guard let self else { return }
            try? self.core.clearSeriesSource(metaId: metaId)
            self.streamStatusLabel.stringValue = String(localized: "Series source preference cleared")
        })
        return menu
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

    @objc private func selectSeason(_ sender: NSButton) {
        selectedSeason = UInt32(max(1, sender.tag))
        let seasons = richMeta?.seasonNumbers ?? []
        rebuildSeasonTabs(seasons)
        loadSeason(selectedSeason)
    }

    @objc private func episodeClicked(_ sender: NSButton) {
        guard let id = sender.identifier?.rawValue,
              let ep = episodes.first(where: { $0.id == id })
        else { return }
        selectEpisode(ep)
    }

    @objc private func selectStreamFilter(_ sender: NSButton) {
        activeFilter = StreamFilter(rawValue: sender.tag) ?? .all
        rebuildStreamFilters()
        renderRankedStreams()
    }

    @objc private func toggleEpisodeSort() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.episodeSortAsc.toggle()
        try? core.setPlayerPreferences(prefs: prefs)
        rebuildSeasonTabs(richMeta?.seasonNumbers ?? [])
        renderEpisodes()
    }

    @objc private func toggleEpisodeLayout() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.episodeCompactLayout.toggle()
        try? core.setPlayerPreferences(prefs: prefs)
        rebuildSeasonTabs(richMeta?.seasonNumbers ?? [])
        renderEpisodes()
    }

    @objc private func cycleStreamSort() {
        guard var prefs = try? core.playerPreferences() else { return }
        let order = ["score", "seeds", "size_desc", "size_asc"]
        let idx = order.firstIndex(of: prefs.streamSort) ?? 0
        prefs.streamSort = order[(idx + 1) % order.count]
        try? core.setPlayerPreferences(prefs: prefs)
        rebuildStreamFilters()
        renderRankedStreams()
    }

    private static func sortButtonTitle(_ mode: String) -> String {
        switch mode {
        case "seeds": String(localized: "Sort: Seeders")
        case "size_desc": String(localized: "Sort: Size ↓")
        case "size_asc": String(localized: "Sort: Size ↑")
        default: String(localized: "Sort: Recommended")
        }
    }

    private static func extractBingeGroup(_ raw: String) -> String? {
        QuickPlayHelper.extractBingeGroup(raw)
    }

    private static func matchesSavedSource(_ item: RankedStreamInfo, saved: PreferredSeriesSourceFfi) -> Bool {
        QuickPlayHelper.matchesSavedSource(item, saved: saved)
    }

    private static func formatTime(_ seconds: Double) -> String {
        let total = max(0, Int(seconds))
        let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%02d:%02d", m, s)
    }

    @objc private func playStreamClicked(_ sender: NSButton) {
        guard let raw = sender.identifier?.rawValue,
              let stream = accumulatedStreams.first(where: { $0.raw == raw })
        else { return }
        let meta = richMeta?.meta
        let metaId = meta?.id ?? preview.id
        let mediaType = meta?.mediaType ?? preview.mediaType
        let ep = selectedEpisode
        let videoId = ep?.id ?? metaId
        let subtitle = ep.map { "S\($0.season ?? selectedSeason) · B\($0.episode ?? $0.number ?? 0) — \($0.title ?? "")" }
        let existingProgress = try? core.getWatchProgress(videoId: videoId)
        let savedResume = (existingProgress?.watched == false && (existingProgress?.positionS ?? 0) > 10)
            ? existingProgress?.positionS
            : nil

        let rankedList = (try? core.rankStreams(streams: accumulatedStreams, preferences: nil))?.ranked ?? []
        let matched = rankedList.first(where: { $0.stream.raw == raw })
        let summary: String
        let isHdr: Bool
        if let parsed = matched?.parsed {
            let kind = parsed.isTorrent ? "Torrent" : "Direct"
            let parts = [kind, parsed.resolution, parsed.hdrBadges.joined(separator: " "), parsed.codec]
                .filter { !$0.isEmpty && $0 != "Unknown" }
            summary = parts.joined(separator: " · ")
            isHdr = !parsed.hdrBadges.isEmpty
        } else {
            summary = stream.addonName
            let lower = "\(stream.name ?? "") \(stream.title ?? "")".lowercased()
            isHdr = lower.contains("hdr") || lower.contains("dolby") || lower.contains("hlg")
        }

        let bingeGroup = Self.extractBingeGroup(stream.raw)
        let prefs = try? core.playerPreferences()
        if prefs?.rememberLastStream ?? true {
            try? core.setSeriesSource(
                metaId: metaId,
                source: PreferredSeriesSourceFfi(
                    addonName: stream.addonName,
                    bingeGroup: bingeGroup,
                    resolution: matched?.parsed.resolution
                )
            )
        }

        let launch: (Double?) -> Void = { [weak self] chosenResume in
            guard let self else { return }
            let ctx = PlaybackMetadata(
                videoId: videoId,
                metaId: metaId,
                mediaType: mediaType,
                title: meta?.name ?? self.preview.name,
                subtitle: subtitle,
                artwork: meta?.background ?? meta?.poster ?? self.preview.poster,
                season: ep?.season.map { Int64($0) },
                episode: (ep?.episode ?? ep?.number).map { Int64($0) },
                resumePosition: chosenResume,
                streamSummary: summary,
                isHdr: isHdr,
                bingeGroup: bingeGroup,
                rankedStreams: rankedList
            )
            self.onPlay(.stream(stream, metadata: ctx))
        }

        let resumeBehavior = prefs?.resumeBehavior ?? "always_resume"
        if resumeBehavior == "always_start_over" {
            launch(nil)
        } else if resumeBehavior == "ask", let savedResume {
            let alert = NSAlert()
            alert.messageText = String(localized: "Resume Playback?")
            alert.informativeText = String(localized: "Continue from \(Self.formatTime(savedResume)) or start from the beginning?")
            alert.addButton(withTitle: String(localized: "Resume (\(Self.formatTime(savedResume)))"))
            alert.addButton(withTitle: String(localized: "Start Over"))
            if let window = view.window {
                Task { @MainActor in
                    let resp = await alert.beginSheetModal(for: window)
                    launch(resp == .alertFirstButtonReturn ? savedResume : nil)
                }
            } else {
                let resp = alert.runModal()
                launch(resp == .alertFirstButtonReturn ? savedResume : nil)
            }
        } else {
            launch(savedResume)
        }
    }
}

final class StreamBatchCollector: StreamBatchObserver {
    private let handler: @Sendable (StreamBatchInfo) -> Void

    init(_ handler: @escaping @Sendable (StreamBatchInfo) -> Void) {
        self.handler = handler
    }

    func onBatch(batch: StreamBatchInfo) {
        handler(batch)
    }
}
