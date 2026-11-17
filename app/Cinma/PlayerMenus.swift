import AppKit
import SwiftUI

/// SwiftUI leaf popover for online subtitles (Stremio subtitle addons +
/// OpenSubtitles REST API), timing offset, dual subtitles, and SRT styling.
struct SubtitleManagerPopoverView: View {
    let candidates: [SubtitleCandidateInfo]
    let loading: Bool
    let statusMessage: String
    let tracks: [MediaTrackInfo]
    let secondaryId: Int64?
    let subDelay: Double
    let settings: SubtitleSettingsFfi
    let onSearchOnline: () -> Void
    let onSelectOnline: (SubtitleCandidateInfo) -> Void
    let onLoadLocal: () -> Void
    let onSelectPrimary: (Int64?) -> Void
    let onSelectSecondary: (Int64?) -> Void
    let onAdjustDelay: (Double) -> Void
    let onResetDelay: () -> Void
    let onUpdateStyle: (SubtitleSettingsFfi) -> Void

    @State private var draftSettings: SubtitleSettingsFfi

    init(
        candidates: [SubtitleCandidateInfo],
        loading: Bool,
        statusMessage: String,
        tracks: [MediaTrackInfo],
        secondaryId: Int64?,
        subDelay: Double,
        settings: SubtitleSettingsFfi,
        onSearchOnline: @escaping () -> Void,
        onSelectOnline: @escaping (SubtitleCandidateInfo) -> Void,
        onLoadLocal: @escaping () -> Void,
        onSelectPrimary: @escaping (Int64?) -> Void,
        onSelectSecondary: @escaping (Int64?) -> Void,
        onAdjustDelay: @escaping (Double) -> Void,
        onResetDelay: @escaping () -> Void,
        onUpdateStyle: @escaping (SubtitleSettingsFfi) -> Void
    ) {
        self.candidates = candidates
        self.loading = loading
        self.statusMessage = statusMessage
        self.tracks = tracks
        self.secondaryId = secondaryId
        self.subDelay = subDelay
        self.settings = settings
        self.onSearchOnline = onSearchOnline
        self.onSelectOnline = onSelectOnline
        self.onLoadLocal = onLoadLocal
        self.onSelectPrimary = onSelectPrimary
        self.onSelectSecondary = onSelectSecondary
        self.onAdjustDelay = onAdjustDelay
        self.onResetDelay = onResetDelay
        self.onUpdateStyle = onUpdateStyle
        _draftSettings = State(initialValue: settings)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(String(localized: "Subtitles"))
                    .font(.headline)
                Spacer()
                Button(String(localized: "Load File…"), action: onLoadLocal)
                Button(String(localized: "Search Online"), action: onSearchOnline)
                    .disabled(loading)
            }

            if !statusMessage.isEmpty {
                Text(statusMessage)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            let subTracks = tracks.filter { $0.trackType == "sub" }
            if !subTracks.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text(String(localized: "Loaded Tracks"))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    ScrollView {
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Button(String(localized: "Off")) { onSelectPrimary(nil) }
                                    .buttonStyle(.bordered)
                                Spacer()
                                if secondaryId != nil {
                                    Button(String(localized: "Clear Dual")) { onSelectSecondary(nil) }
                                        .buttonStyle(.bordered)
                                }
                            }
                            ForEach(subTracks, id: \.id) { t in
                                HStack(spacing: 6) {
                                    Button {
                                        onSelectPrimary(t.id)
                                    } label: {
                                        HStack {
                                            Image(systemName: t.selected ? "checkmark.circle.fill" : "circle")
                                                .foregroundStyle(t.selected ? .orange : .secondary)
                                            Text(trackLabel(t))
                                                .lineLimit(1)
                                            Spacer()
                                        }
                                    }
                                    .buttonStyle(.plain)

                                    Button(secondaryId == t.id ? "2nd ✓" : "2nd") {
                                        onSelectSecondary(secondaryId == t.id ? nil : t.id)
                                    }
                                    .font(.caption)
                                    .buttonStyle(.bordered)
                                }
                            }
                        }
                    }
                    .frame(maxHeight: 110)
                }
            }

            Divider()

            HStack {
                Text(String(localized: "Timing Offset"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Button("-0.1s") { onAdjustDelay(-0.1) }
                Text(String(format: "%+.1fs", subDelay))
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 54)
                Button("+0.1s") { onAdjustDelay(0.1) }
                Button(String(localized: "Reset")) { onResetDelay() }
            }

            HStack {
                Text(String(localized: "SRT Size"))
                    .font(.caption)
                Stepper("\(draftSettings.srtFontSize) px", value: $draftSettings.srtFontSize, in: 20...84, step: 2)
                    .onChange(of: draftSettings.srtFontSize) { _, _ in onUpdateStyle(draftSettings) }
                Spacer()
                Toggle(String(localized: "Override ASS"), isOn: $draftSettings.assOverride)
                    .toggleStyle(.switch)
                    .onChange(of: draftSettings.assOverride) { _, _ in onUpdateStyle(draftSettings) }
            }

            if !candidates.isEmpty || loading {
                Divider()
                HStack {
                    Text(String(localized: "Online Results"))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    if loading {
                        ProgressView().controlSize(.small)
                    }
                }
                List(candidates, id: \.id) { c in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 4) {
                                Text(c.lang.uppercased())
                                    .font(.caption.bold())
                                    .padding(.horizontal, 5)
                                    .padding(.vertical, 1)
                                    .background(.orange.opacity(0.2))
                                    .clipShape(RoundedRectangle(cornerRadius: 4))
                                Text(c.title ?? c.provider)
                                    .font(.callout)
                                    .lineLimit(1)
                            }
                            HStack(spacing: 6) {
                                Text(c.provider)
                                if c.hashMatched { Text("Hash Match").foregroundStyle(.green) }
                                if c.isSdh { Text("SDH") }
                                if c.isForced { Text("Forced") }
                            }
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button(String(localized: "Use")) { onSelectOnline(c) }
                            .buttonStyle(.borderedProminent)
                            .tint(.orange)
                    }
                }
                .frame(height: 170)
            }
        }
        .padding(14)
        .frame(width: 430)
    }

    private func trackLabel(_ t: MediaTrackInfo) -> String {
        var parts: [String] = []
        if let lang = t.lang { parts.append(lang.uppercased()) }
        if let title = t.title { parts.append(title) }
        if t.external { parts.append(String(localized: "External")) }
        if t.forced { parts.append("Forced") }
        if t.hearingImpaired { parts.append("SDH") }
        return parts.isEmpty ? "Track \(t.id)" : parts.joined(separator: " · ")
    }
}

/// Live stream switcher popover inside the player (preserves playback position).
struct StreamSwitcherPopoverView: View {
    let streams: [RankedStreamInfo]
    let activeRaw: String?
    let onSelect: (StreamInfo) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(String(localized: "Switch Stream"))
                .font(.headline)
            if streams.isEmpty {
                Text(String(localized: "No alternative streams available for this playback."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 12)
            } else {
                List(streams, id: \.stream.raw) { item in
                    let isCurrent = item.stream.raw == activeRaw
                    HStack {
                        VStack(alignment: .leading, spacing: 3) {
                            HStack(spacing: 5) {
                                Text(item.parsed.resolution)
                                    .font(.caption.bold())
                                ForEach(item.parsed.hdrBadges, id: \.self) { badge in
                                    Text(badge)
                                        .font(.caption2.bold())
                                        .padding(.horizontal, 4)
                                        .background(.orange.opacity(0.22))
                                        .clipShape(RoundedRectangle(cornerRadius: 4))
                                }
                                Text(item.parsed.codec)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Text(summary(item))
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                        Spacer()
                        if isCurrent {
                            Text(String(localized: "Playing"))
                                .font(.caption.bold())
                                .foregroundStyle(.green)
                        } else {
                            Button(String(localized: "Switch")) { onSelect(item.stream) }
                                .buttonStyle(.borderedProminent)
                                .tint(.orange)
                        }
                    }
                }
                .frame(height: 240)
            }
        }
        .padding(14)
        .frame(width: 440)
    }

    private func summary(_ item: RankedStreamInfo) -> String {
        var parts: [String] = [item.stream.addonName]
        if let bytes = item.parsed.sizeBytes {
            parts.append(ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file))
        }
        if let seeders = item.parsed.seeders {
            parts.append("\(seeders) seed")
        }
        return parts.joined(separator: " · ")
    }
}
