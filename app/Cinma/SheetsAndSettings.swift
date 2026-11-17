import AppKit
import SwiftUI
import UniformTypeIdentifiers

// MARK: - Addon Manager (SwiftUI leaf view)

struct AddonManagerView: View {
    let core: CinmaCore
    let onDismiss: () -> Void

    @State private var addons: [AddonInfo] = []
    @State private var newUrl = ""
    @State private var status = ""
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(String(localized: "Addons"))
                    .font(.title2.bold())
                Spacer()
                Button(String(localized: "Close"), action: onDismiss)
            }

            HStack(spacing: 8) {
                TextField(String(localized: "Addon manifest URL or stremio:// link"), text: $newUrl)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(install)
                Button(String(localized: "Install"), action: install)
                    .disabled(busy || newUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }

            if !status.isEmpty {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            List {
                ForEach(Array(addons.enumerated()), id: \.element.id) { idx, addon in
                    HStack(spacing: 12) {
                        Toggle("", isOn: Binding(
                            get: { addon.enabled },
                            set: { enabled in
                                try? core.setAddonEnabled(id: addon.id, enabled: enabled)
                                reload()
                            }
                        ))
                        .labelsHidden()

                        VStack(alignment: .leading, spacing: 2) {
                            Text("\(addon.name) \(addon.version)")
                                .font(.headline)
                            Text(addon.transportUrl)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }

                        Spacer()

                        Button { move(from: idx, delta: -1) } label: {
                            Image(systemName: "arrow.up")
                        }
                        .disabled(idx == 0)

                        Button { move(from: idx, delta: 1) } label: {
                            Image(systemName: "arrow.down")
                        }
                        .disabled(idx + 1 >= addons.count)

                        if addon.configurable, let cfgUrl = configureUrl(for: addon.transportUrl) {
                            Button(String(localized: "Configure")) {
                                NSWorkspace.shared.open(cfgUrl)
                            }
                        }

                        Button(role: .destructive) {
                            try? core.removeAddon(id: addon.id)
                            reload()
                        } label: {
                            Image(systemName: "trash")
                        }
                    }
                    .padding(.vertical, 4)
                }
            }
        }
        .padding(24)
        .frame(width: 640, height: 480)
        .onAppear(perform: reload)
    }

    private func reload() {
        addons = (try? core.addons()) ?? []
    }

    private func install() {
        let target = newUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !target.isEmpty else { return }
        busy = true
        status = String(localized: "Installing addon…")
        Task {
            defer { busy = false }
            do {
                let installed = try await core.installAddon(url: target)
                status = String(localized: "Installed \(installed.name) \(installed.version)")
                newUrl = ""
                reload()
            } catch {
                status = String(localized: "Install failed: \(error.localizedDescription)")
            }
        }
    }

    private func move(from index: Int, delta: Int) {
        let target = index + delta
        guard addons.indices.contains(index), addons.indices.contains(target) else { return }
        var ids = addons.map(\.id)
        ids.swapAt(index, target)
        try? core.reorderAddons(orderedIds: ids)
        reload()
    }

    private func configureUrl(for manifestUrl: String) -> URL? {
        guard let url = URL(string: manifestUrl) else { return nil }
        return url.deletingLastPathComponent().appending(path: "configure")
    }
}

// MARK: - Profile Picker (SwiftUI leaf view)

struct ProfilePickerView: View {
    let core: CinmaCore
    let onDismiss: () -> Void

    @State private var profiles: [ProfileInfo] = []
    @State private var activeId: Int64 = 1
    @State private var newName = ""
    @State private var errorText = ""
    @State private var pinPromptProfile: ProfileInfo?
    @State private var pinEntry = ""
    @State private var pinEditProfile: ProfileInfo?
    @State private var newPin = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(String(localized: "Profiles"))
                    .font(.title2.bold())
                Spacer()
                Button(String(localized: "Close"), action: onDismiss)
            }

            List {
                ForEach(profiles, id: \.id) { profile in
                    HStack(spacing: 12) {
                        Circle()
                            .fill(profile.id == activeId ? Color.orange : Color.gray.opacity(0.35))
                            .frame(width: 34, height: 34)
                            .overlay {
                                Text(String(profile.name.prefix(1)).uppercased())
                                    .font(.headline.bold())
                                    .foregroundStyle(profile.id == activeId ? Color.black : Color.white)
                            }
                        VStack(alignment: .leading) {
                            HStack(spacing: 6) {
                                Text(profile.name)
                                    .font(.headline)
                                if profile.hasPin {
                                    Image(systemName: "lock.fill")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            if profile.id == activeId {
                                Text(String(localized: "Active"))
                                    .font(.caption)
                                    .foregroundStyle(.orange)
                            }
                        }
                        Spacer()
                        Button(profile.hasPin ? String(localized: "PIN…") : String(localized: "Set PIN")) {
                            pinEditProfile = profile
                            newPin = ""
                            errorText = ""
                        }
                        if profile.id != activeId {
                            Button(String(localized: "Switch")) {
                                if profile.hasPin {
                                    pinPromptProfile = profile
                                    pinEntry = ""
                                    errorText = ""
                                } else {
                                    try? core.setActiveProfile(id: profile.id)
                                    reload()
                                    onDismiss()
                                }
                            }
                        }
                        if profiles.count > 1 {
                            Button(role: .destructive) {
                                do {
                                    try core.deleteProfile(id: profile.id)
                                    reload()
                                } catch {
                                    errorText = error.localizedDescription
                                }
                            } label: {
                                Image(systemName: "trash")
                            }
                            .accessibilityLabel("\(String(localized: "Delete")) \(profile.name)")
                        }
                    }
                    .padding(.vertical, 4)
                }
            }

            if let target = pinPromptProfile {
                HStack(spacing: 8) {
                    Text("\(String(localized: "Enter PIN for")) \(target.name):")
                        .font(.caption)
                    SecureField(String(localized: "PIN"), text: $pinEntry)
                        .frame(width: 120)
                        .onSubmit { verifyAndSwitch(target) }
                    Button(String(localized: "Unlock")) { verifyAndSwitch(target) }
                    Button(String(localized: "Cancel")) { pinPromptProfile = nil }
                }
            } else if let editing = pinEditProfile {
                HStack(spacing: 8) {
                    Text("\(String(localized: "PIN for")) \(editing.name):")
                        .font(.caption)
                    SecureField(String(localized: "4+ digits"), text: $newPin)
                        .frame(width: 120)
                        .onSubmit { savePin(editing) }
                    Button(String(localized: "Save PIN")) { savePin(editing) }
                    if editing.hasPin {
                        Button(String(localized: "Remove PIN"), role: .destructive) {
                            try? core.setProfilePin(profileId: editing.id, pin: nil)
                            pinEditProfile = nil
                            reload()
                        }
                    }
                    Button(String(localized: "Cancel")) { pinEditProfile = nil }
                }
            }

            HStack(spacing: 8) {
                TextField(String(localized: "New profile name"), text: $newName)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(create)
                Button(String(localized: "Create"), action: create)
                    .disabled(newName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }

            if !errorText.isEmpty {
                Text(errorText)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
        .padding(24)
        .frame(width: 500, height: 420)
        .onAppear(perform: reload)
    }

    private func verifyAndSwitch(_ profile: ProfileInfo) {
        do {
            if try core.verifyProfilePin(profileId: profile.id, pin: pinEntry) {
                try core.setActiveProfile(id: profile.id)
                pinPromptProfile = nil
                reload()
                onDismiss()
            } else {
                errorText = String(localized: "Incorrect PIN")
            }
        } catch {
            errorText = error.localizedDescription
        }
    }

    private func savePin(_ profile: ProfileInfo) {
        do {
            try core.setProfilePin(profileId: profile.id, pin: newPin)
            pinEditProfile = nil
            newPin = ""
            reload()
        } catch {
            errorText = error.localizedDescription
        }
    }

    private func reload() {
        profiles = (try? core.profiles()) ?? []
        activeId = (try? core.activeProfileId()) ?? 1
    }

    private func create() {
        let trimmed = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            let id = try core.createProfile(name: trimmed, avatar: String(trimmed.prefix(1)).uppercased())
            try core.setActiveProfile(id: id)
            newName = ""
            reload()
            onDismiss()
        } catch {
            errorText = error.localizedDescription
        }
    }
}

// MARK: - Onboarding (SwiftUI leaf view)

struct OnboardingView: View {
    let core: CinmaCore
    let onFinish: () -> Void

    @State private var addonUrl = ""
    @State private var tmdbKey = ""
    @State private var language = "tr-TR"
    @State private var keepMenuBar = true
    @State private var status = ""
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(String(localized: "Welcome to Cinma"))
                .font(.largeTitle.bold())
            Text(String(localized: "Cinma ships without built-in content sources. Set up your addons, optional TMDB metadata key, and preferences below — you can skip or change everything anytime."))
                .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 6) {
                Label(String(localized: "1. Stremio Addon URL (optional)"), systemImage: "puzzlepiece.extension")
                    .font(.headline)
                HStack {
                    TextField("https://…/manifest.json", text: $addonUrl)
                        .textFieldStyle(.roundedBorder)
                        .accessibilityLabel(String(localized: "Stremio addon manifest URL"))
                    Button(String(localized: "Install"), action: installAddon)
                        .disabled(busy || addonUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                Label(String(localized: "2. TMDB API Key (optional, saved to Keychain)"), systemImage: "key.fill")
                    .font(.headline)
                SecureField("TMDB v3 API key", text: $tmdbKey)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityLabel(String(localized: "TMDB API Key"))
                Text(String(localized: "If skipped, Cinma uses installed metadata addons (such as Cinemeta) as fallback."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            VStack(alignment: .leading, spacing: 8) {
                Label(String(localized: "3. Preferences"), systemImage: "slider.horizontal.3")
                    .font(.headline)
                Picker(String(localized: "Metadata Language"), selection: $language) {
                    Text("Türkçe (tr-TR)").tag("tr-TR")
                    Text("English (en-US)").tag("en-US")
                }
                .onChange(of: language) { _, val in
                    try? core.setMetadataLanguage(lang: val)
                }
                Toggle(String(localized: "Keep downloads running in Menu Bar when closed"), isOn: $keepMenuBar)
                    .onChange(of: keepMenuBar) { _, val in
                        try? core.setMenubarEnabled(enabled: val)
                    }
            }

            if !status.isEmpty {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            HStack {
                Button(String(localized: "Skip")) {
                    try? core.setOnboardingCompleted(completed: true)
                    onFinish()
                }
                .accessibilityLabel(String(localized: "Skip setup"))
                Spacer()
                Button(String(localized: "Continue")) {
                    saveAndContinue()
                }
                .buttonStyle(.borderedProminent)
                .tint(.orange)
                .disabled(busy)
                .accessibilityLabel(String(localized: "Save and continue to Cinma"))
            }
        }
        .padding(28)
        .frame(width: 560, height: 470)
    }

    private func installAddon() {
        let url = addonUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty else { return }
        busy = true
        status = String(localized: "Installing addon…")
        Task {
            defer { busy = false }
            do {
                let addon = try await core.installAddon(url: url)
                status = String(localized: "Installed \(addon.name) \(addon.version)")
                addonUrl = ""
            } catch {
                status = String(localized: "Install failed: \(error.localizedDescription)")
            }
        }
    }

    private func saveAndContinue() {
        let key = tmdbKey.trimmingCharacters(in: .whitespacesAndNewlines)
        if !key.isEmpty {
            try? core.setTmdbKey(key: key)
        }
        try? core.setOnboardingCompleted(completed: true)
        onFinish()
    }
}

// MARK: - Settings (SwiftUI leaf view)

struct SettingsView: View {
    let core: CinmaCore
    let onDismiss: () -> Void

    @State private var tmdbKey = ""
    @State private var hasTmdbKey = false
    @State private var openSubKey = ""
    @State private var hasOpenSubKey = false
    @State private var language = "tr-TR"
    @State private var addonsEnabled = true
    @State private var torrentEnabled = true
    @State private var castingEnabled = false
    @State private var trackingEnabled = false
    @State private var menubarEnabled = true
    @State private var diskPolicy = DiskPolicySettingsInfo(
        cacheQuotaMb: 10_240,
        keepWatchedGlobal: false,
        seedingMode: "off",
        seedingRatio: 1.0,
        seedingMinutes: 60,
        uploadLimitKbps: 0
    )
    @State private var diskUsage = DiskUsageSummaryInfo(
        totalBytes: 0,
        keptBytes: 0,
        evictableBytes: 0,
        entryCount: 0
    )
    @State private var ytdlpStatus = YtdlpStatusInfo(available: false, path: nil, managed: false)
    @State private var subSettings = SubtitleSettingsFfi(
        preferredLangs: ["tur", "eng"],
        audioLangs: ["eng", "jpn", "tur"],
        dualSecondaryLang: "eng",
        srtFont: nil,
        srtFontSize: 46,
        srtColor: nil,
        srtPos: 95,
        assOverride: false
    )
    @State private var playerPrefs = PlayerPreferencesFfi(
        exactSeek: true,
        hdrTargetPeak: nil,
        shaderPreset: "none",
        autoplayNext: true,
        autoplayCountdownS: 10,
        forceHdrSurface: false
    )
    @State private var subLangsText = "tur, eng"
    @State private var audioLangsText = "eng, jpn, tur"
    @State private var dualLangText = "eng"
    @State private var srtFontText = ""
    @State private var commands: [CommandInfo] = []
    @State private var editingCommandId: String?
    @State private var keyDraft = ""
    @State private var recordingCommandId: String?
    @State private var keyMonitor: Any?
    @State private var status = ""

    var body: some View {
        Form {
            Section(String(localized: "Metadata (TMDB & Language)")) {
                HStack {
                    SecureField(
                        hasTmdbKey ? String(localized: "TMDB key saved in Keychain") : "TMDB v3 API key",
                        text: $tmdbKey
                    )
                    Button(String(localized: "Save")) { saveTmdbKey() }
                        .disabled(tmdbKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    if hasTmdbKey {
                        Button(String(localized: "Clear")) {
                            try? core.setTmdbKey(key: nil)
                            tmdbKey = ""
                            hasTmdbKey = false
                            status = String(localized: "TMDB key removed from Keychain")
                        }
                    }
                }
                Picker(String(localized: "Metadata Language"), selection: $language) {
                    Text("Türkçe (tr-TR)").tag("tr-TR")
                    Text("English (en-US)").tag("en-US")
                }
                .onChange(of: language) { _, newValue in
                    try? core.setMetadataLanguage(lang: newValue)
                }
            }

            Section(String(localized: "Subtitles & Audio (OpenSubtitles + Style)")) {
                HStack {
                    SecureField(
                        hasOpenSubKey ? String(localized: "OpenSubtitles key saved in Keychain") : "OpenSubtitles API key",
                        text: $openSubKey
                    )
                    Button(String(localized: "Save")) { saveOpenSubKey() }
                        .disabled(openSubKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    if hasOpenSubKey {
                        Button(String(localized: "Clear")) {
                            try? core.setOpensubtitlesCredentials(apiKey: nil, token: nil)
                            openSubKey = ""
                            hasOpenSubKey = false
                            status = String(localized: "OpenSubtitles key removed from Keychain")
                        }
                    }
                }
                HStack {
                    Text(String(localized: "Subtitle Priority"))
                    Spacer()
                    TextField("tur, eng", text: $subLangsText)
                        .frame(width: 180)
                        .onSubmit { saveSubSettings() }
                }
                HStack {
                    Text(String(localized: "Audio Priority"))
                    Spacer()
                    TextField("eng, jpn, tur", text: $audioLangsText)
                        .frame(width: 180)
                        .onSubmit { saveSubSettings() }
                }
                HStack {
                    Text(String(localized: "Dual Subtitle Language"))
                    Spacer()
                    TextField("eng", text: $dualLangText)
                        .frame(width: 120)
                        .onSubmit { saveSubSettings() }
                }
                HStack {
                    Text(String(localized: "SRT Custom Font"))
                    Spacer()
                    TextField("SF Pro Text", text: $srtFontText)
                        .frame(width: 180)
                        .onSubmit { saveSubSettings() }
                }
                Stepper("\(String(localized: "SRT Font Size")): \(subSettings.srtFontSize) px", value: $subSettings.srtFontSize, in: 20...84, step: 2)
                    .onChange(of: subSettings.srtFontSize) { _, _ in saveSubSettings() }
                Stepper("\(String(localized: "SRT Position")): \(subSettings.srtPos)%", value: $subSettings.srtPos, in: 10...100, step: 5)
                    .onChange(of: subSettings.srtPos) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Override ASS/SSA Script Styling"), isOn: $subSettings.assOverride)
                    .onChange(of: subSettings.assOverride) { _, _ in saveSubSettings() }
            }

            Section(String(localized: "Playback & Video")) {
                Toggle(String(localized: "Exact Relative Seeks (vs Keyframe)"), isOn: $playerPrefs.exactSeek)
                    .onChange(of: playerPrefs.exactSeek) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "Upscaling / Shader"), selection: $playerPrefs.shaderPreset) {
                    Text(String(localized: "Off")).tag("none")
                    Text(String(localized: "Adaptive Sharpen (CAS)")).tag("sharpen")
                }
                .onChange(of: playerPrefs.shaderPreset) { _, _ in savePlayerPrefs() }
                Picker(
                    String(localized: "HDR Target Peak"),
                    selection: Binding(
                        get: { playerPrefs.hdrTargetPeak ?? 0 },
                        set: { val in
                            playerPrefs.hdrTargetPeak = val == 0 ? nil : val
                            savePlayerPrefs()
                        }
                    )
                ) {
                    Text(String(localized: "Auto")).tag(UInt32(0))
                    Text("400 nits").tag(UInt32(400))
                    Text("600 nits").tag(UInt32(600))
                    Text("1000 nits").tag(UInt32(1000))
                    Text("1600 nits").tag(UInt32(1600))
                }
                Toggle(String(localized: "Force 16-bit HDR Surface on SDR"), isOn: $playerPrefs.forceHdrSurface)
                    .onChange(of: playerPrefs.forceHdrSurface) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Play Next Episode"), isOn: $playerPrefs.autoplayNext)
                    .onChange(of: playerPrefs.autoplayNext) { _, _ in savePlayerPrefs() }
                if playerPrefs.autoplayNext {
                    Stepper("\(String(localized: "Auto-Play Countdown")): \(playerPrefs.autoplayCountdownS)s", value: $playerPrefs.autoplayCountdownS, in: 3...30)
                        .onChange(of: playerPrefs.autoplayCountdownS) { _, _ in savePlayerPrefs() }
                }
            }

            Section(String(localized: "Features")) {
                Toggle(String(localized: "Stremio Addons"), isOn: $addonsEnabled)
                    .onChange(of: addonsEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "addons", enabled: val) }
                    }
                Toggle(String(localized: "BitTorrent Engine"), isOn: $torrentEnabled)
                    .onChange(of: torrentEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "torrent", enabled: val) }
                    }
                Toggle(String(localized: "LAN Casting (Chromecast / DLNA)"), isOn: $castingEnabled)
                    .onChange(of: castingEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "casting", enabled: val) }
                    }
                Toggle(String(localized: "Watch Tracking (Trakt / Simkl / AniList / MAL)"), isOn: $trackingEnabled)
                    .onChange(of: trackingEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "tracking", enabled: val) }
                    }
            }

            Section(String(localized: "Downloads, Disk Policy & Menu Bar")) {
                Toggle(String(localized: "Keep Running in Menu Bar when Window Closes"), isOn: $menubarEnabled)
                    .onChange(of: menubarEnabled) { _, val in
                        try? core.setMenubarEnabled(enabled: val)
                    }
                Picker(String(localized: "Cache & Download Quota"), selection: $diskPolicy.cacheQuotaMb) {
                    Text("5 GB").tag(UInt64(5_120))
                    Text("10 GB").tag(UInt64(10_240))
                    Text("25 GB").tag(UInt64(25_600))
                    Text("50 GB").tag(UInt64(51_200))
                    Text(String(localized: "Unlimited")).tag(UInt64(0))
                }
                .onChange(of: diskPolicy.cacheQuotaMb) { _, _ in saveDiskPolicy() }

                Toggle(String(localized: "Keep Watched Titles Offline by Default"), isOn: $diskPolicy.keepWatchedGlobal)
                    .onChange(of: diskPolicy.keepWatchedGlobal) { _, _ in saveDiskPolicy() }

                Picker(String(localized: "Torrent Seeding Policy"), selection: $diskPolicy.seedingMode) {
                    Text(String(localized: "Off (stop after finish)")).tag("off")
                    Text(String(localized: "Target Ratio")).tag("ratio")
                    Text(String(localized: "Time Limit (minutes)")).tag("minutes")
                }
                .onChange(of: diskPolicy.seedingMode) { _, _ in saveDiskPolicy() }

                if diskPolicy.seedingMode == "ratio" {
                    Stepper("\(String(localized: "Seed Ratio")): \(String(format: "%.1f", diskPolicy.seedingRatio))", value: $diskPolicy.seedingRatio, in: 0.2...5.0, step: 0.2)
                        .onChange(of: diskPolicy.seedingRatio) { _, _ in saveDiskPolicy() }
                } else if diskPolicy.seedingMode == "minutes" {
                    Stepper("\(String(localized: "Seed Duration")): \(diskPolicy.seedingMinutes) min", value: $diskPolicy.seedingMinutes, in: 5...720, step: 15)
                        .onChange(of: diskPolicy.seedingMinutes) { _, _ in saveDiskPolicy() }
                }

                Stepper("\(String(localized: "Upload Limit")): \(diskPolicy.uploadLimitKbps == 0 ? String(localized: "Unlimited") : "\(diskPolicy.uploadLimitKbps) KB/s")", value: $diskPolicy.uploadLimitKbps, in: 0...50_000, step: 512)
                    .onChange(of: diskPolicy.uploadLimitKbps) { _, _ in saveDiskPolicy() }

                HStack {
                    let totalStr = ByteCountFormatter.string(fromByteCount: Int64(diskUsage.totalBytes), countStyle: .file)
                    let keptStr = ByteCountFormatter.string(fromByteCount: Int64(diskUsage.keptBytes), countStyle: .file)
                    Text("\(String(localized: "Tracked")): \(totalStr) · \(String(localized: "Kept")): \(keptStr)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button(String(localized: "Trim Cache Now")) {
                        let freed = (try? core.enforceDiskQuota()) ?? 0
                        diskUsage = (try? core.diskUsageSummary()) ?? diskUsage
                        let freedStr = ByteCountFormatter.string(fromByteCount: Int64(freed), countStyle: .file)
                        status = "\(String(localized: "Freed")) \(freedStr)"
                    }
                }
            }

            Section(String(localized: "Backup & Restore (SQLite)")) {
                HStack {
                    Button(String(localized: "Export Backup…")) { exportBackup() }
                    Button(String(localized: "Restore Backup…")) { restoreBackup() }
                }
                Text(String(localized: "Exports a consistent snapshot of profiles, library, progress, addons, and settings. Secrets stay in macOS Keychain."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section(String(localized: "Trailers (yt-dlp)")) {
                HStack {
                    Text(ytdlpStatus.path ?? String(localized: "Not installed"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button(ytdlpStatus.available ? String(localized: "Update yt-dlp") : String(localized: "Install yt-dlp")) {
                        status = String(localized: "Downloading yt-dlp…")
                        Task {
                            do {
                                let path = try await core.installOrUpdateYtdlp()
                                ytdlpStatus = core.ytdlpStatus()
                                status = "yt-dlp: \(path)"
                            } catch {
                                status = error.localizedDescription
                            }
                        }
                    }
                }
            }

            Section(String(localized: "Key Bindings")) {
                ForEach(commands, id: \.id) { cmd in
                    HStack {
                        Text(cmd.id)
                            .font(.system(.body, design: .monospaced))
                        Spacer()
                        if recordingCommandId == cmd.id {
                            Text(String(localized: "Press shortcut… (Esc cancels)"))
                                .font(.caption)
                                .foregroundStyle(.orange)
                            Button(String(localized: "Cancel")) { stopRecording() }
                        } else if editingCommandId == cmd.id {
                            TextField("cmd+k", text: $keyDraft)
                                .frame(width: 110)
                                .onSubmit { commitBinding(cmd.id) }
                            Button(String(localized: "Save")) { commitBinding(cmd.id) }
                        } else {
                            Text(cmd.key ?? "—")
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(.secondary)
                            Button(String(localized: "Record")) {
                                startRecording(cmd.id)
                            }
                            Button(String(localized: "Edit")) {
                                editingCommandId = cmd.id
                                keyDraft = cmd.key ?? ""
                            }
                            Button(String(localized: "Reset")) {
                                try? core.resetKeyBinding(id: cmd.id)
                                reload()
                            }
                        }
                    }
                }
            }

            if !status.isEmpty {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button(String(localized: "Close")) {
                    saveSubSettings()
                    stopRecording()
                    onDismiss()
                }
            }
        }
        .frame(width: 640, height: 600)
        .onAppear(perform: reload)
        .onDisappear { stopRecording() }
    }

    private func reload() {
        hasTmdbKey = (try? core.hasTmdbKey()) ?? false
        hasOpenSubKey = (try? core.hasOpensubtitlesKey()) ?? false
        language = (try? core.metadataLanguage()) ?? "tr-TR"
        addonsEnabled = (try? core.featureEnabled(name: "addons")) ?? true
        torrentEnabled = (try? core.featureEnabled(name: "torrent")) ?? true
        castingEnabled = (try? core.featureEnabled(name: "casting")) ?? false
        trackingEnabled = (try? core.featureEnabled(name: "tracking")) ?? false
        menubarEnabled = (try? core.menubarEnabled()) ?? true
        if let dp = try? core.diskPolicySettings() { diskPolicy = dp }
        if let du = try? core.diskUsageSummary() { diskUsage = du }
        ytdlpStatus = core.ytdlpStatus()
        if let s = try? core.subtitleSettings() {
            subSettings = s
            subLangsText = s.preferredLangs.joined(separator: ", ")
            audioLangsText = s.audioLangs.joined(separator: ", ")
            dualLangText = s.dualSecondaryLang ?? ""
            srtFontText = s.srtFont ?? ""
        }
        if let p = try? core.playerPreferences() {
            playerPrefs = p
        }
        commands = (try? core.commands()) ?? []
    }

    private func saveSubSettings() {
        let parseList: (String) -> [String] = { text in
            text.split(separator: ",").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        }
        subSettings.preferredLangs = parseList(subLangsText)
        subSettings.audioLangs = parseList(audioLangsText)
        let dual = dualLangText.trimmingCharacters(in: .whitespacesAndNewlines)
        subSettings.dualSecondaryLang = dual.isEmpty ? nil : dual
        let font = srtFontText.trimmingCharacters(in: .whitespacesAndNewlines)
        subSettings.srtFont = font.isEmpty ? nil : font
        try? core.setSubtitleSettings(settings: subSettings)
    }

    private func savePlayerPrefs() {
        try? core.setPlayerPreferences(prefs: playerPrefs)
    }

    private func saveDiskPolicy() {
        try? core.setDiskPolicySettings(s: diskPolicy)
    }

    private func exportBackup() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "cinma-backup.sqlite3"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try core.exportBackup(destPath: url.path)
            status = "\(String(localized: "Backup saved to")) \(url.lastPathComponent)"
        } catch {
            status = error.localizedDescription
        }
    }

    private func restoreBackup() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let schema = try core.importBackup(srcPath: url.path)
            reload()
            status = "\(String(localized: "Restored backup (schema")) v\(schema))"
        } catch {
            status = error.localizedDescription
        }
    }

    private func saveTmdbKey() {
        let trimmed = tmdbKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        status = String(localized: "Verifying TMDB key…")
        Task {
            do {
                try await core.verifyTmdbKey(key: trimmed)
                try core.setTmdbKey(key: trimmed)
                hasTmdbKey = true
                tmdbKey = ""
                status = String(localized: "TMDB key verified and saved to Keychain")
            } catch {
                status = error.localizedDescription
            }
        }
    }

    private func saveOpenSubKey() {
        let trimmed = openSubKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            try core.setOpensubtitlesCredentials(apiKey: trimmed, token: nil)
            hasOpenSubKey = true
            openSubKey = ""
            status = String(localized: "OpenSubtitles key saved to Keychain")
        } catch {
            status = error.localizedDescription
        }
    }

    private func startRecording(_ id: String) {
        stopRecording()
        editingCommandId = nil
        recordingCommandId = id
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.keyCode == 53 { // Escape cancels recording unless binding escape
                stopRecording()
                return nil
            }
            if let combo = KeyCombo.from(event) {
                keyDraft = combo
                commitBinding(id)
                stopRecording()
                return nil
            }
            return event
        }
    }

    private func stopRecording() {
        recordingCommandId = nil
        if let monitor = keyMonitor {
            NSEvent.removeMonitor(monitor)
            keyMonitor = nil
        }
    }

    private func commitBinding(_ id: String) {
        let combo = keyDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            try core.setKeyBinding(id: id, combo: combo.isEmpty ? nil : combo)
            editingCommandId = nil
            reload()
        } catch {
            status = error.localizedDescription
        }
    }
}
