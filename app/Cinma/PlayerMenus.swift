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
    @State private var activeSection: PopoverSection = .tracks

    private enum PopoverSection: String, CaseIterable {
        case tracks
        case style
    }

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
                Picker("", selection: $activeSection) {
                    Text(String(localized: "Tracks")).tag(PopoverSection.tracks)
                    Text(String(localized: "Appearance")).tag(PopoverSection.style)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 170)
            }

            if activeSection == .tracks {
                tracksSection
            } else {
                appearanceSection
            }
        }
        .padding(14)
        .frame(width: 410)
        .preferredColorScheme(.dark)
    }

    @ViewBuilder
    private var tracksSection: some View {
        HStack {
            Button(String(localized: "Load File…"), action: onLoadLocal)
            Spacer()
            Toggle(String(localized: "Hide SDH"), isOn: Binding(
                get: { draftSettings.hideSdh },
                set: { val in
                    draftSettings.hideSdh = val
                    onUpdateStyle(draftSettings)
                }
            ))
            .toggleStyle(.switch)
            .controlSize(.small)
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
            .frame(height: 160)
            .scrollContentBackground(.hidden)
        }
    }

    @ViewBuilder
    private var appearanceSection: some View {
        let colorPresets: [(label: String, hex: String, color: Color)] = [
            (String(localized: "White"), "#FFFFFF", .white),
            (String(localized: "Yellow"), "#FFD54F", Color(red: 1.0, green: 0.84, blue: 0.31)),
            (String(localized: "Amber"), "#FFC107", .orange),
            (String(localized: "Cyan"), "#67E8F9", .cyan),
        ]
        let selectedColor = (draftSettings.srtColor ?? "#FFFFFF").uppercased()
        let selectedBorder = (draftSettings.srtBorderColor ?? "#000000").uppercased()

        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(String(localized: "Font Size"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Slider(
                    value: Binding(
                        get: { Double(draftSettings.srtFontSize) },
                        set: {
                            draftSettings.srtFontSize = UInt32($0.rounded())
                            onUpdateStyle(draftSettings)
                        }
                    ),
                    in: 20...84,
                    step: 2
                )
                .frame(width: 165)
                Text("\(draftSettings.srtFontSize) px")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 44, alignment: .trailing)
            }

            HStack {
                Text(String(localized: "Position"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Slider(
                    value: Binding(
                        get: { Double(draftSettings.srtPos) },
                        set: {
                            draftSettings.srtPos = UInt32($0.rounded())
                            onUpdateStyle(draftSettings)
                        }
                    ),
                    in: 10...100,
                    step: 5
                )
                .frame(width: 165)
                Text("\(draftSettings.srtPos)%")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 44, alignment: .trailing)
            }

            HStack {
                Text(String(localized: "Text Opacity"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Slider(
                    value: Binding(
                        get: { Double(draftSettings.srtOpacity) },
                        set: {
                            draftSettings.srtOpacity = UInt32($0.rounded())
                            onUpdateStyle(draftSettings)
                        }
                    ),
                    in: 10...100,
                    step: 5
                )
                .frame(width: 165)
                Text("\(draftSettings.srtOpacity)%")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 44, alignment: .trailing)
            }

            HStack {
                Text(String(localized: "Background Box"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Slider(
                    value: Binding(
                        get: { Double(draftSettings.srtBoxOpacity) },
                        set: {
                            let val = UInt32($0.rounded())
                            draftSettings.srtBoxOpacity = val
                            if val > 0, (draftSettings.srtBackColor ?? "").isEmpty {
                                draftSettings.srtBackColor = "#000000"
                            }
                            onUpdateStyle(draftSettings)
                        }
                    ),
                    in: 0...100,
                    step: 5
                )
                .frame(width: 165)
                Text(draftSettings.srtBoxOpacity == 0 ? String(localized: "Off") : "\(draftSettings.srtBoxOpacity)%")
                    .font(.system(.caption, design: .monospaced))
                    .frame(width: 44, alignment: .trailing)
            }

            HStack {
                Text(String(localized: "Text Color"))
                    .font(.caption.weight(.semibold))
                Spacer()
                ForEach(colorPresets, id: \.hex) { preset in
                    let isSelected = selectedColor == preset.hex
                    Button {
                        draftSettings.srtColor = preset.hex
                        onUpdateStyle(draftSettings)
                    } label: {
                        HStack(spacing: 4) {
                            Circle()
                                .fill(preset.color)
                                .frame(width: 12, height: 12)
                                .overlay(Circle().stroke(.black.opacity(0.6), lineWidth: 1))
                            Text(preset.label)
                                .font(.caption)
                        }
                        .padding(.horizontal, 7)
                        .padding(.vertical, 4)
                        .background(isSelected ? Color.orange.opacity(0.28) : Color.white.opacity(0.08))
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                        .overlay(
                            RoundedRectangle(cornerRadius: 6)
                                .stroke(isSelected ? Color.orange : Color.clear, lineWidth: 1)
                        )
                    }
                    .buttonStyle(.plain)
                }
            }

            HStack {
                Text(String(localized: "Alignment"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Picker("", selection: Binding(
                    get: { draftSettings.srtAlignX },
                    set: {
                        draftSettings.srtAlignX = $0
                        onUpdateStyle(draftSettings)
                    }
                )) {
                    Text(String(localized: "Left")).tag("left")
                    Text(String(localized: "Center")).tag("center")
                    Text(String(localized: "Right")).tag("right")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 210)
            }

            HStack {
                Text(String(localized: "2nd Subtitle"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Picker("", selection: Binding(
                    get: { draftSettings.secondaryPos <= 50 ? UInt32(12) : UInt32(88) },
                    set: {
                        draftSettings.secondaryPos = $0
                        onUpdateStyle(draftSettings)
                    }
                )) {
                    Text(String(localized: "Top")).tag(UInt32(12))
                    Text(String(localized: "Bottom")).tag(UInt32(88))
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 140)
            }

            HStack {
                Text(String(localized: "Black Outline"))
                    .font(.caption.weight(.semibold))
                Spacer()
                Stepper("\(draftSettings.srtBorderSize) px", value: $draftSettings.srtBorderSize, in: 0...8, step: 1)
                    .onChange(of: draftSettings.srtBorderSize) { _, _ in
                        if draftSettings.srtBorderSize > 0 && (draftSettings.srtBorderColor ?? "").isEmpty {
                            draftSettings.srtBorderColor = "#000000"
                        }
                        onUpdateStyle(draftSettings)
                    }
                Button {
                    draftSettings.srtBorderColor = selectedBorder == "#000000" ? "#262626" : "#000000"
                    if draftSettings.srtBorderSize == 0 { draftSettings.srtBorderSize = 3 }
                    onUpdateStyle(draftSettings)
                } label: {
                    Circle()
                        .fill(selectedBorder == "#000000" ? Color.black : Color(white: 0.2))
                        .frame(width: 16, height: 16)
                        .overlay(Circle().stroke(Color.white.opacity(0.45), lineWidth: 1))
                }
                .buttonStyle(.plain)
                .help(String(localized: "Outline color"))
            }

            HStack(spacing: 14) {
                Toggle(String(localized: "Bold"), isOn: $draftSettings.srtBold)
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .onChange(of: draftSettings.srtBold) { _, _ in onUpdateStyle(draftSettings) }

                Toggle(String(localized: "Override ASS"), isOn: $draftSettings.assOverride)
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .onChange(of: draftSettings.assOverride) { _, _ in onUpdateStyle(draftSettings) }
            }

            Toggle(String(localized: "Lift Above Controls"), isOn: $draftSettings.liftAboveControls)
                .toggleStyle(.switch)
                .controlSize(.small)
                .onChange(of: draftSettings.liftAboveControls) { _, _ in onUpdateStyle(draftSettings) }
        }
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
                            if let fileTitle = item.stream.filename ?? item.stream.title, !fileTitle.isEmpty {
                                Text(fileTitle)
                                    .font(.caption2)
                                    .foregroundStyle(.primary.opacity(0.85))
                                    .lineLimit(1)
                                    .truncationMode(.middle)
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
                .frame(height: 220)
                .scrollContentBackground(.hidden)
            }
        }
        .padding(14)
        .frame(width: 400)
        .preferredColorScheme(.dark)
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

/// Live streaming & P2P transfer details popover (`globe` button in bottom bar).
struct StreamDetailsPopoverView: View {
    let session: PlaybackSession?
    let addonName: String?
    let streamSummary: String?

    @State private var streamStats: StreamStatsInfo?
    @State private var playerStats: PlayerStatsInfo?

    var body: some View {
        VStack(alignment: .leading, spacing: 11) {
            HStack {
                Image(systemName: "globe")
                    .foregroundStyle(.orange)
                Text(String(localized: "Streaming Details"))
                    .font(.headline)
                Spacer()
                Text(sourceTypeBadge)
                    .font(.caption2.bold())
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(.orange.opacity(0.22))
                    .clipShape(RoundedRectangle(cornerRadius: 5))
            }

            if let addonName, !addonName.isEmpty {
                detailRow(String(localized: "Addon"), value: addonName)
            }
            if let streamSummary, !streamSummary.isEmpty {
                detailRow(String(localized: "Quality"), value: streamSummary)
            }

            if let st = streamStats {
                if st.isTorrent {
                    Divider()
                    HStack(spacing: 16) {
                        metricBox(
                            title: String(localized: "Download"),
                            value: formatSpeed(st.downloadSpeedBps)
                        )
                        metricBox(
                            title: String(localized: "Upload"),
                            value: formatSpeed(st.uploadSpeedBps)
                        )
                        metricBox(
                            title: String(localized: "Peers"),
                            value: "\(st.peersConnected) / \(st.peersSeen)"
                        )
                    }

                    if st.totalBytes > 0 {
                        let ratio = min(max(Double(st.downloadedBytes) / Double(st.totalBytes), 0), 1)
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text(String(localized: "Buffered"))
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.secondary)
                                Spacer()
                                Text(
                                    "\(ByteCountFormatter.string(fromByteCount: Int64(st.downloadedBytes), countStyle: .file)) / \(ByteCountFormatter.string(fromByteCount: Int64(st.totalBytes), countStyle: .file)) (\(Int((ratio * 100).rounded()))%)"
                                )
                                .font(.system(.caption2, design: .monospaced))
                            }
                            ProgressView(value: ratio)
                                .tint(.orange)
                        }
                    }

                    if let hash = st.infoHash {
                        detailRow("Hash", value: String(hash.prefix(12)) + "…")
                    }
                } else if let url = st.sourceUrl, let host = URL(string: url)?.host {
                    Divider()
                    detailRow(String(localized: "Host"), value: host)
                }

                if let file = st.filename, !file.isEmpty {
                    detailRow(String(localized: "File"), value: file)
                }
            }

            if let ps = playerStats {
                Divider()
                HStack(spacing: 16) {
                    metricBox(
                        title: String(localized: "Cache"),
                        value: String(format: "%.1fs", ps.cacheSeconds)
                    )
                    metricBox(
                        title: String(localized: "Decoder"),
                        value: ps.hwdec.isEmpty ? "sw" : ps.hwdec
                    )
                    metricBox(
                        title: String(localized: "Drops"),
                        value: "\(ps.droppedFrames)"
                    )
                }
            }
        }
        .padding(14)
        .frame(width: 360)
        .preferredColorScheme(.dark)
        .task {
            while !Task.isCancelled {
                streamStats = session?.streamStats()
                playerStats = session?.stats()
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    private var sourceTypeBadge: String {
        guard let st = streamStats else { return "—" }
        if st.isTorrent { return "BitTorrent P2P" }
        if st.sourceUrl != nil { return "HTTP / CDN" }
        return String(localized: "Local File")
    }

    private func formatSpeed(_ bps: UInt64) -> String {
        let kb = Double(bps) / 1024.0
        if kb >= 1024.0 {
            return String(format: "%.1f MB/s", kb / 1024.0)
        }
        return String(format: "%.0f KB/s", kb)
    }

    private func detailRow(_ label: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            Spacer()
            Text(value)
                .font(.caption)
                .lineLimit(1)
                .truncationMode(.middle)
        }
    }

    private func metricBox(title: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(value)
                .font(.system(.callout, design: .monospaced).weight(.semibold))
        }
    }
}
