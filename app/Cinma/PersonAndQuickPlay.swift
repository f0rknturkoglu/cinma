import AppKit
import SwiftUI

/// Retains a closure handler directly via `NSMenuItem.representedObject` for dynamic context menus.
@MainActor
final class ClosureMenuProxy: NSObject {
    private let handler: () -> Void

    init(_ handler: @escaping () -> Void) {
        self.handler = handler
    }

    @objc func invoke() {
        handler()
    }

    static func item(title: String, systemSymbol: String? = nil, handler: @escaping () -> Void) -> NSMenuItem {
        let proxy = ClosureMenuProxy(handler)
        let entry = NSMenuItem(title: title, action: #selector(invoke), keyEquivalent: "")
        entry.target = proxy
        entry.representedObject = proxy
        if let systemSymbol {
            entry.image = NSImage(systemSymbolName: systemSymbol, accessibilityDescription: title)
        }
        return entry
    }
}

/// Shared One-Click Quick Play + Poster right-click context menu builder (#N12).
@MainActor
enum QuickPlayHelper {
    static func launchBest(
        core: CinmaCore,
        item: MetaPreviewInfo,
        episode: MetaVideoInfo? = nil,
        seasonOverride: UInt32? = nil,
        onPlay: @escaping (PlayRequest) -> Void,
        onStatus: @escaping (String) -> Void,
        onFallbackOpenDetail: (() -> Void)? = nil
    ) {
        onStatus(String(localized: "Finding best stream for \(item.name)…"))
        Task {
            do {
                var targetEpisode = episode
                var resolvedMeta: MetaInfo?
                if item.mediaType == "series", targetEpisode == nil {
                    let progressRows = (try? core.progressForMeta(metaId: item.id)) ?? []
                    if let resumeRow = progressRows.first(where: { !$0.watched && $0.positionS > 10 }) {
                        targetEpisode = MetaVideoInfo(
                            id: resumeRow.videoId,
                            title: resumeRow.title,
                            season: resumeRow.season.map { UInt32(max(0, $0)) },
                            episode: resumeRow.episode.map { UInt32(max(0, $0)) },
                            number: resumeRow.episode.map { UInt32(max(0, $0)) },
                            released: nil,
                            overview: nil,
                            thumbnail: resumeRow.artwork
                        )
                    } else if let rich = try? await core.richMeta(mediaType: item.mediaType, id: item.id) {
                        resolvedMeta = rich.meta
                        let watchedIds = Set(progressRows.filter(\.watched).map(\.videoId))
                        let orderedVideos = rich.meta.videos
                            .filter { ($0.season ?? 1) > 0 }
                            .sorted {
                                let sa = $0.season ?? 1
                                let sb = $1.season ?? 1
                                if sa != sb { return sa < sb }
                                return ($0.episode ?? $0.number ?? 0) < ($1.episode ?? $1.number ?? 0)
                            }
                        if let nextUnwatched = orderedVideos.first(where: { !watchedIds.contains($0.id) }) {
                            targetEpisode = nextUnwatched
                        } else {
                            let firstSeason = rich.seasonNumbers.sorted().first ?? 1
                            if let first = rich.meta.videos.first(where: { $0.season == firstSeason }) ?? rich.meta.videos.first {
                                targetEpisode = first
                            } else if let eps = try? await core.seasonEpisodes(mediaType: "series", metaId: rich.meta.id, season: firstSeason),
                                      let first = eps.first
                            {
                                targetEpisode = first
                            }
                        }
                    }
                }

                let metaId = resolvedMeta?.id ?? item.id
                let mediaType = resolvedMeta?.mediaType ?? item.mediaType
                let videoId = targetEpisode?.id ?? metaId
                let rawStreams = try await core.streams(mediaType: mediaType, id: videoId)
                guard !rawStreams.isEmpty else {
                    onStatus(String(localized: "No streams found for \(item.name) — opening details"))
                    onFallbackOpenDetail?()
                    return
                }
                let ranked = try await core.rankStreamsWithDebrid(streams: rawStreams, preferences: nil)
                let prefs = try? core.playerPreferences()
                var sorted = ranked.ranked
                if prefs?.rememberLastStream ?? true,
                   let saved = try? core.seriesSource(metaId: metaId),
                   saved.bingeGroup != nil || saved.addonName != nil
                {
                    let preferred = sorted.filter { matchesSavedSource($0, saved: saved) }
                    let rest = sorted.filter { !matchesSavedSource($0, saved: saved) }
                    sorted = preferred + rest
                }
                guard let best = sorted.first else {
                    onStatus(String(localized: "No matching streams after filters — opening details"))
                    onFallbackOpenDetail?()
                    return
                }

                let existing = try? core.getWatchProgress(videoId: videoId)
                let savedResume = (existing?.watched == false && (existing?.positionS ?? 0) > 10)
                    ? existing?.positionS
                    : nil
                let resumePos: Double? = (prefs?.resumeBehavior == "always_start_over") ? nil : savedResume

                let epNum = targetEpisode?.episode ?? targetEpisode?.number
                let epSeason = targetEpisode?.season ?? seasonOverride
                let subtitle = targetEpisode.map {
                    "S\(epSeason ?? 1) · B\(epNum ?? 0) — \($0.title ?? "")"
                }

                let parsed = best.parsed
                let kind = parsed.isTorrent ? "Torrent" : "Direct"
                let parts = [kind, parsed.resolution, parsed.hdrBadges.joined(separator: " "), parsed.codec]
                    .filter { !$0.isEmpty && $0 != "Unknown" }
                let summary = parts.isEmpty ? best.stream.addonName : parts.joined(separator: " · ")
                let bingeGroup = extractBingeGroup(best.stream.raw)
                if prefs?.rememberLastStream ?? true {
                    try? core.setSeriesSource(
                        metaId: metaId,
                        source: PreferredSeriesSourceFfi(
                            addonName: best.stream.addonName,
                            bingeGroup: bingeGroup,
                            resolution: parsed.resolution
                        )
                    )
                }

                let ctx = PlaybackMetadata(
                    videoId: videoId,
                    metaId: metaId,
                    mediaType: mediaType,
                    title: resolvedMeta?.name ?? item.name,
                    subtitle: subtitle,
                    artwork: resolvedMeta?.background ?? resolvedMeta?.poster ?? item.poster,
                    season: epSeason.map { Int64($0) },
                    episode: epNum.map { Int64($0) },
                    resumePosition: resumePos,
                    streamSummary: summary,
                    isHdr: !parsed.hdrBadges.isEmpty,
                    bingeGroup: bingeGroup,
                    rankedStreams: sorted
                )
                onStatus("")
                onPlay(.stream(best.stream, metadata: ctx))
            } catch {
                onStatus(FriendlyError.explain(error))
                onFallbackOpenDetail?()
            }
        }
    }

    static func makePosterMenu(
        core: CinmaCore,
        item: MetaPreviewInfo,
        onOpenDetail: @escaping (MetaPreviewInfo) -> Void,
        onQuickPlay: @escaping (MetaPreviewInfo) -> Void,
        onStatus: ((String) -> Void)? = nil
    ) -> NSMenu {
        let menu = NSMenu()
        menu.addItem(ClosureMenuProxy.item(title: String(localized: "Quick Play Best Stream"), systemSymbol: "play.fill") {
            onQuickPlay(item)
        })
        menu.addItem(ClosureMenuProxy.item(title: String(localized: "Open Details"), systemSymbol: "info.circle") {
            onOpenDetail(item)
        })
        menu.addItem(.separator())

        let inLibrary = (try? core.isInLibrary(metaId: item.id, mediaType: item.mediaType)) ?? false
        menu.addItem(ClosureMenuProxy.item(
            title: inLibrary ? String(localized: "Remove from Library") : String(localized: "Add to Library"),
            systemSymbol: inLibrary ? "heart.slash" : "heart"
        ) {
            if inLibrary {
                try? core.removeFromLibrary(metaId: item.id, mediaType: item.mediaType)
                onStatus?(String(localized: "Removed from Library"))
            } else {
                try? core.addToLibrary(metaId: item.id, mediaType: item.mediaType, name: item.name, poster: item.poster)
                onStatus?(String(localized: "Added to Library"))
            }
        })

        let watchedIds = Set((try? core.watchedMetaIds()) ?? [])
        let isWatched = watchedIds.contains(item.id)
        menu.addItem(ClosureMenuProxy.item(
            title: isWatched ? String(localized: "Mark as Unwatched") : String(localized: "Mark as Watched"),
            systemSymbol: isWatched ? "eye.slash" : "checkmark.circle"
        ) {
            try? core.setWatched(
                videoId: item.id,
                metaId: item.id,
                mediaType: item.mediaType,
                title: item.name,
                watched: !isWatched
            )
            onStatus?(isWatched ? String(localized: "Marked as unwatched") : String(localized: "Marked as watched"))
        })

        if item.mediaType == "series" {
            let saved = try? core.seriesSource(metaId: item.id)
            if saved?.addonName != nil || saved?.bingeGroup != nil {
                menu.addItem(ClosureMenuProxy.item(
                    title: String(localized: "Reset Remembered Source"),
                    systemSymbol: "arrow.counterclockwise"
                ) {
                    try? core.clearSeriesSource(metaId: item.id)
                    onStatus?(String(localized: "Series source preference cleared"))
                })
            }
        }

        if item.id.hasPrefix("tt") {
            menu.addItem(.separator())
            menu.addItem(ClosureMenuProxy.item(title: String(localized: "Copy IMDb ID"), systemSymbol: "doc.on.doc") {
                let pb = NSPasteboard.general
                pb.clearContents()
                pb.setString(item.id, forType: .string)
                onStatus?(String(localized: "Copied \(item.id)"))
            })
        }
        return menu
    }

    static func extractBingeGroup(_ raw: String) -> String? {
        guard let data = raw.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let hints = obj["behaviorHints"] as? [String: Any]
        else { return nil }
        return hints["bingeGroup"] as? String
    }

    static func matchesSavedSource(_ item: RankedStreamInfo, saved: PreferredSeriesSourceFfi) -> Bool {
        if let bg = saved.bingeGroup, extractBingeGroup(item.stream.raw) == bg { return true }
        if let addon = saved.addonName, item.stream.addonName == addon {
            if let res = saved.resolution { return item.parsed.resolution == res }
            return true
        }
        return false
    }
}

/// Actor / Director filmography & credits sheet loaded from TMDB (#N19).
struct PersonCreditsSheetView: View {
    let core: CinmaCore
    let personName: String
    let onSelectTitle: (MetaPreviewInfo) -> Void
    let onDismiss: () -> Void

    @State private var profile: PersonProfileInfo?
    @State private var portraitImage: NSImage?
    @State private var loading = true
    @State private var errorMessage = ""

    private let columns = [GridItem(.adaptive(minimum: 118, maximum: 140), spacing: 14)]

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 14) {
                Group {
                    if let portraitImage {
                        Image(nsImage: portraitImage)
                            .resizable()
                            .scaledToFill()
                    } else {
                        ZStack {
                            Color.white.opacity(0.08)
                            Image(systemName: "person.fill")
                                .font(.title)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .frame(width: 78, height: 102)
                .clipShape(RoundedRectangle(cornerRadius: 12))

                VStack(alignment: .leading, spacing: 5) {
                    Text(profile?.name ?? personName)
                        .font(.title2.bold())
                    let subtitle = [profile?.knownFor, profile?.birthday, profile?.placeOfBirth]
                        .compactMap { $0 }
                        .joined(separator: " · ")
                    if !subtitle.isEmpty {
                        Text(subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    if let bio = profile?.biography, !bio.isEmpty {
                        Text(bio)
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .lineLimit(3)
                    }
                }
                Spacer()
            }
            .padding(20)

            Divider()

            if loading {
                Spacer()
                ProgressView()
                Spacer()
            } else if !errorMessage.isEmpty {
                Spacer()
                Text(errorMessage)
                    .foregroundStyle(.secondary)
                    .padding()
                Spacer()
            } else if let credits = profile?.credits, !credits.isEmpty {
                ScrollView {
                    LazyVGrid(columns: columns, spacing: 16) {
                        ForEach(Array(credits.enumerated()), id: \.offset) { _, item in
                            PersonCreditCard(item: item) {
                                onDismiss()
                                onSelectTitle(item)
                            }
                        }
                    }
                    .padding(20)
                }
            } else {
                Spacer()
                Text(String(localized: "No credits found on TMDB for this person."))
                    .foregroundStyle(.secondary)
                Spacer()
            }
        }
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button(String(localized: "Close"), action: onDismiss)
            }
        }
        .frame(width: 680, height: 540)
        .task { await loadPerson() }
    }

    private func loadPerson() async {
        loading = true
        defer { loading = false }
        do {
            let fetched = try await core.personCredits(personName: personName)
            profile = fetched
            if let imgUrl = fetched?.profileImage {
                portraitImage = await ImagePipeline.shared.load(urlString: imgUrl, pointSize: NSSize(width: 78, height: 102))
            }
        } catch {
            errorMessage = FriendlyError.explain(error)
        }
    }
}

private struct PersonCreditCard: View {
    let item: MetaPreviewInfo
    let onTap: () -> Void

    @State private var posterImage: NSImage?

    var body: some View {
        Button(action: onTap) {
            VStack(alignment: .leading, spacing: 5) {
                Group {
                    if let posterImage {
                        Image(nsImage: posterImage)
                            .resizable()
                            .scaledToFill()
                    } else {
                        ZStack {
                            Color.white.opacity(0.06)
                            Text(item.name)
                                .font(.caption.bold())
                                .multilineTextAlignment(.center)
                                .padding(6)
                        }
                    }
                }
                .frame(width: 118, height: 177)
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .stroke(Color.white.opacity(0.10), lineWidth: 1)
                )

                Text(item.name)
                    .font(.caption.weight(.semibold))
                    .lineLimit(1)
                    .foregroundStyle(.primary)
                let meta = [item.releaseInfo, item.mediaType == "series" ? String(localized: "Series") : String(localized: "Movie")]
                    .compactMap { $0 }
                    .joined(separator: " · ")
                if !meta.isEmpty {
                    Text(meta)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            .frame(width: 118, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .task(id: item.poster) {
            if let url = item.poster {
                posterImage = await ImagePipeline.shared.load(urlString: url, pointSize: NSSize(width: 118, height: 177))
            }
        }
    }
}
