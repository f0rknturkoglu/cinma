import AppKit

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

    private let scrollView = NSScrollView()
    private let backdropView = NSImageView()
    private let fallbackGradient = CAGradientLayer()
    private let logoView = NSImageView()
    private let titleLabel = NSTextField(wrappingLabelWithString: "")
    private let metaLabel = NSTextField(labelWithString: "")
    private let overviewLabel = NSTextField(wrappingLabelWithString: "")
    private let castLabel = NSTextField(wrappingLabelWithString: "")
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
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        let doc = NSView()
        doc.translatesAutoresizingMaskIntoConstraints = false

        let heroBox = NSView()
        heroBox.wantsLayer = true
        heroBox.layer?.masksToBounds = true
        fallbackGradient.colors = ArtworkPlaceholder.gradient(for: preview.id)
        heroBox.layer?.insertSublayer(fallbackGradient, at: 0)
        heroBox.translatesAutoresizingMaskIntoConstraints = false

        backdropView.imageScaling = .scaleProportionallyUpOrDown
        backdropView.wantsLayer = true
        backdropView.layer?.contentsGravity = .resizeAspectFill
        backdropView.layer?.masksToBounds = true
        backdropView.translatesAutoresizingMaskIntoConstraints = false

        let leftScrim = GradientLayerView(
            colors: [
                CinemaTheme.background.withAlphaComponent(0.95),
                CinemaTheme.background.withAlphaComponent(0.68),
                CinemaTheme.background.withAlphaComponent(0.28),
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
                CinemaTheme.background.withAlphaComponent(0.45),
                CinemaTheme.background.withAlphaComponent(0.88),
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
        libraryButton = libBtn
        trailerButton = trBtn
        for b in [backBtn, libBtn, trBtn] {
            b.translatesAutoresizingMaskIntoConstraints = false
            b.heightAnchor.constraint(equalToConstant: 36).isActive = true
            b.widthAnchor.constraint(greaterThanOrEqualToConstant: 100).isActive = true
        }
        let topBar = NSStackView(views: [backBtn, libBtn, trBtn])
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

        let headerStack = NSStackView(views: [topBar, logoView, titleLabel, metaLabel, overviewLabel, castLabel])
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

        doc.addSubview(heroBox)
        doc.addSubview(headerStack)
        doc.addSubview(columns)
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
            columns.bottomAnchor.constraint(equalTo: doc.bottomAnchor, constant: -48),

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
        if !m.cast.isEmpty {
            let names = m.cast.prefix(5).joined(separator: ", ")
            castLabel.stringValue = String(localized: "Cast: \(names)")
        }
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

        for ep in episodes {
            let isSelected = ep.id == selectedEpisode?.id
            let row = NSButton(title: "", target: self, action: #selector(episodeClicked(_:)))
            row.identifier = NSUserInterfaceItemIdentifier(ep.id)
            row.isBordered = false
            row.wantsLayer = true
            row.layer?.cornerRadius = 14
            row.layer?.backgroundColor = (isSelected ? NSColor(white: 1, alpha: 0.08) : CinemaTheme.cardBackground).cgColor
            row.layer?.borderWidth = 1
            row.layer?.borderColor = (isSelected ? CinemaTheme.highlightBorder : CinemaTheme.cardBorder).cgColor

            let thumb = NSImageView()
            thumb.imageScaling = .scaleAxesIndependently
            thumb.wantsLayer = true
            thumb.layer?.cornerRadius = 10
            thumb.layer?.masksToBounds = true
            thumb.layer?.backgroundColor = NSColor(white: 1, alpha: 0.06).cgColor
            thumb.translatesAutoresizingMaskIntoConstraints = false
            var task: Task<Void, Never>?
            thumb.setRemoteImage(ep.thumbnail ?? richMeta?.meta.background, pointSize: NSSize(width: 176, height: 99), taskStorage: &task)
            if let task { episodeThumbTasks.append(task) }

            let num = ep.episode ?? ep.number ?? 0
            let title = NSTextField(labelWithString: "\(num). \(ep.title ?? "")")
            title.font = .systemFont(ofSize: 14.5, weight: .semibold)
            title.textColor = CinemaTheme.textPrimary
            title.lineBreakMode = .byTruncatingTail

            let sub = NSTextField(labelWithString: ep.released ?? "")
            sub.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
            sub.textColor = CinemaTheme.textMuted

            let prog = progressByVideo[ep.id]
            let statusText: String
            if prog?.watched == true {
                statusText = String(localized: "Watched")
            } else if let p = prog, p.durationS > 0 {
                let leftMin = max(1, Int(((p.durationS - p.positionS) / 60).rounded()))
                statusText = String(localized: "\(leftMin) min left")
            } else {
                statusText = ""
            }
            let statusLabel = NSTextField(labelWithString: statusText)
            statusLabel.font = .monospacedDigitSystemFont(ofSize: 12, weight: .medium)
            statusLabel.textColor = prog?.watched == true ? CinemaTheme.green : CinemaTheme.accent

            let textCol = NSStackView(views: [title, sub, statusLabel])
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
    }

    private func renderRankedStreams() {
        for v in streamsListStack.arrangedSubviews {
            streamsListStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        guard !accumulatedStreams.isEmpty else { return }
        guard let result = rankedWithDebrid ?? (try? core.rankStreams(streams: accumulatedStreams, preferences: nil)) else { return }

        let filtered = result.ranked.filter { item in
            switch activeFilter {
            case .all: true
            case .uhd2160: item.parsed.resolution == "4K"
            case .fhd1080: item.parsed.resolution == "1080p"
            case .under10Gb: (item.parsed.sizeBytes ?? 0) <= 10 * 1024 * 1024 * 1024
            case .hdr: !item.parsed.hdrBadges.isEmpty
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

        let vStack = NSStackView(views: [topRow, statsLabel, whyLabel, actionsRow])
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

    @objc private func goBack() { onBack() }

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
        let resume = (existingProgress?.watched == false && (existingProgress?.positionS ?? 0) > 10)
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

        let bingeGroup: String? = {
            guard let data = stream.raw.data(using: .utf8),
                  let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let hints = obj["behaviorHints"] as? [String: Any]
            else { return nil }
            return hints["bingeGroup"] as? String
        }()

        let ctx = PlaybackMetadata(
            videoId: videoId,
            metaId: metaId,
            mediaType: mediaType,
            title: meta?.name ?? preview.name,
            subtitle: subtitle,
            artwork: meta?.background ?? meta?.poster ?? preview.poster,
            season: ep?.season.map { Int64($0) },
            episode: (ep?.episode ?? ep?.number).map { Int64($0) },
            resumePosition: resume,
            streamSummary: summary,
            isHdr: isHdr,
            bingeGroup: bingeGroup,
            rankedStreams: rankedList
        )
        onPlay(.stream(stream, metadata: ctx))
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
