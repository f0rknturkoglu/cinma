import AppKit
import SwiftUI

extension TitleDetailViewController {
    func refreshDebridRanking(_ streams: [StreamInfo]) {
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

    func rebuildSeasonTabs(_ seasons: [UInt32]) {
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

    func loadSeason(_ season: UInt32) {
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

    func renderEpisodes() {
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
            let isFiller = fillerEpisodeNumbers.contains(num)
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

                var rowViews: [NSView] = [compactTitle]
                if isFiller {
                    rowViews.append(CinemaTheme.makeChip(String(localized: "Filler"), highlighted: false))
                }
                rowViews.append(contentsOf: [spacer, dateLabel, statusLabel])
                let hStack = NSStackView(views: rowViews)
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

            let title = NSTextField(labelWithString: isFiller ? "\(num). \(ep.title ?? "") · Filler" : "\(num). \(ep.title ?? "")")
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

    func selectEpisode(_ ep: MetaVideoInfo) {
        selectedEpisode = ep
        renderEpisodes()
        let s = ep.season ?? selectedSeason
        let e = ep.episode ?? ep.number ?? 0
        let label = "\(String(localized: "S\(s) · E\(e)")) — \(ep.title ?? "")"
        fetchStreams(videoId: ep.id, title: label, season: Int64(s), episode: Int64(e))
    }

    func fetchStreams(videoId: String, title: String, season: Int64?, episode: Int64?) {
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

    @objc func streamRecoveryAction() {
        if streamActionButton.title == String(localized: "Open Addons…") {
            onOpenAddons?()
        } else if let vid = lastStreamVideoId, let title = lastStreamTitle {
            fetchStreams(videoId: vid, title: title, season: lastStreamSeason, episode: lastStreamEpisode)
        }
    }

    override func cancelOperation(_ sender: Any?) {
        goBack()
    }

    func rebuildStreamFilters() {
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

    func renderRankedStreams() {
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

    func makeStreamCard(_ item: RankedStreamInfo, isTop: Bool) -> NSView {
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

        if Dalga3StreamHelpers.isBatchTorrent(item.stream) {
            let batchBtn = CinemaTheme.makePillButton(
                title: String(localized: "Batch Files"),
                symbol: "tray.full",
                primary: false,
                target: self,
                action: #selector(inspectBatchClicked(_:))
            )
            batchBtn.identifier = NSUserInterfaceItemIdentifier(item.stream.raw)
            batchBtn.heightAnchor.constraint(equalToConstant: 32).isActive = true
            actionsRow.addArrangedSubview(batchBtn)
        }

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

    @objc func downloadStreamClicked(_ sender: NSButton) {
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

    @objc func openExternalStreamClicked(_ sender: NSButton) {
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
        // Hand the URL to another app through Launch Services. Never spawn a
        // player binary as our child process (one-process rule, §1.1).
        let customPath = prefs?.externalPlayerPath
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !customPath.isEmpty && !customPath.hasSuffix(".app") {
            streamStatusLabel.stringValue = String(localized: "Choose an app (.app) as the external player in Settings.")
            return
        }
        let appCandidates = [
            customPath,
            "/Applications/IINA.app",
            "/Applications/mpv.app",
            "/Applications/VLC.app",
            "/Applications/Infuse 7.app",
        ].filter { !$0.isEmpty && FileManager.default.fileExists(atPath: $0) }
        if let app = appCandidates.first {
            let appURL = URL(fileURLWithPath: app)
            NSWorkspace.shared.open([url], withApplicationAt: appURL, configuration: NSWorkspace.OpenConfiguration()) { [weak self] _, error in
                Task { @MainActor in
                    guard let self else { return }
                    self.streamStatusLabel.stringValue = error.map { FriendlyError.explain($0) }
                        ?? String(localized: "Opened stream in \(appURL.deletingPathExtension().lastPathComponent)")
                }
            }
            return
        }
        if NSWorkspace.shared.open(url) {
            streamStatusLabel.stringValue = String(localized: "Opened stream in external player")
        } else {
            streamStatusLabel.stringValue = String(localized: "No external mpv/IINA binary found. Configure its path in Settings.")
        }
    }

    func makeEpisodeMenu(_ ep: MetaVideoInfo) -> NSMenu {
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

    @objc func selectSeason(_ sender: NSButton) {
        selectedSeason = UInt32(max(1, sender.tag))
        let seasons = richMeta?.seasonNumbers ?? []
        rebuildSeasonTabs(seasons)
        loadSeason(selectedSeason)
    }

    @objc func episodeClicked(_ sender: NSButton) {
        guard let id = sender.identifier?.rawValue,
              let ep = episodes.first(where: { $0.id == id })
        else { return }
        selectEpisode(ep)
    }

    func loadAnimeFillers(metaId: String) {
        let core = core
        Task { [weak self] in
            let fillers = (try? await core.animeFillerEpisodes(metaId: metaId)) ?? []
            guard !fillers.isEmpty, let self else { return }
            self.fillerEpisodeNumbers = Set(fillers)
            self.renderEpisodes()
            if (try? core.playerPreferences())?.autoSkipAnimeFillers == true,
               let cur = self.selectedEpisode,
               self.fillerEpisodeNumbers.contains(cur.episode ?? cur.number ?? 0),
               let firstCanon = self.episodes.first(where: { !self.fillerEpisodeNumbers.contains($0.episode ?? $0.number ?? 0) })
            {
                self.selectEpisode(firstCanon)
            }
        }
    }

    @objc func inspectBatchClicked(_ sender: NSButton) {
        guard let raw = sender.identifier?.rawValue,
              let stream = accumulatedStreams.first(where: { $0.raw == raw }),
              let meta = Dalga3StreamHelpers.extractTorrentMeta(from: stream)
        else { return }
        let title = richMeta?.meta.name ?? preview.name
        var sheetVC: NSViewController?
        let view = BatchTorrentSheetView(
            core: core,
            infoHash: meta.infoHash,
            trackers: meta.trackers,
            title: stream.filename ?? stream.title ?? title,
            metaId: richMeta?.meta.id ?? preview.id,
            mediaType: richMeta?.meta.mediaType ?? preview.mediaType,
            onDismiss: { [weak self] in
                if let vc = sheetVC { self?.dismiss(vc) }
            },
            onQueued: { [weak self] count in
                self?.streamStatusLabel.stringValue = String(localized: "Queued \(count) episode downloads.")
            }
        )
        let host = NSHostingController(rootView: view)
        sheetVC = host
        presentAsSheet(host)
    }

    @objc func selectStreamFilter(_ sender: NSButton) {
        activeFilter = StreamFilter(rawValue: sender.tag) ?? .all
        rebuildStreamFilters()
        renderRankedStreams()
    }

    @objc func toggleEpisodeSort() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.episodeSortAsc.toggle()
        try? core.setPlayerPreferences(prefs: prefs)
        rebuildSeasonTabs(richMeta?.seasonNumbers ?? [])
        renderEpisodes()
    }

    @objc func toggleEpisodeLayout() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.episodeCompactLayout.toggle()
        try? core.setPlayerPreferences(prefs: prefs)
        rebuildSeasonTabs(richMeta?.seasonNumbers ?? [])
        renderEpisodes()
    }

    @objc func cycleStreamSort() {
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

    @objc func playStreamClicked(_ sender: NSButton) {
        guard let raw = sender.identifier?.rawValue,
              let stream = accumulatedStreams.first(where: { $0.raw == raw })
        else { return }
        let meta = richMeta?.meta
        let metaId = meta?.id ?? preview.id
        let mediaType = meta?.mediaType ?? preview.mediaType
        let ep = selectedEpisode
        let videoId = ep?.id ?? metaId
        let subtitle = ep.map {
            let s = $0.season ?? selectedSeason
            let e = $0.episode ?? $0.number ?? 0
            return "\(String(localized: "S\(s) · E\(e)")) — \($0.title ?? "")"
        }
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
