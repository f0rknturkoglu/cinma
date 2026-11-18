import AppKit
import Darwin
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
                            do {
                                try core.removeAddon(id: addon.id)
                                reload()
                            } catch {
                                status = FriendlyError.explain(error)
                            }
                        } label: {
                            Image(systemName: "trash")
                        }
                        .help(String(localized: "Remove Addon"))
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
                status = FriendlyError.explain(error)
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
                                    errorText = FriendlyError.explain(error)
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
                            do {
                                try core.setProfilePin(profileId: editing.id, pin: nil)
                                pinEditProfile = nil
                                reload()
                            } catch {
                                errorText = FriendlyError.explain(error)
                            }
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
            errorText = FriendlyError.explain(error)
        }
    }

    private func savePin(_ profile: ProfileInfo) {
        do {
            try core.setProfilePin(profileId: profile.id, pin: newPin)
            pinEditProfile = nil
            newPin = ""
            reload()
        } catch {
            errorText = FriendlyError.explain(error)
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
            errorText = FriendlyError.explain(error)
        }
    }
}

// MARK: - Onboarding (SwiftUI leaf view)

struct OnboardingView: View {
    let core: CinmaCore
    let onFinish: () -> Void

    @State private var addonUrl = ""
    @State private var tmdbKey = ""
    @State private var language = Bundle.main.preferredLocalizations.first?.hasPrefix("tr") == true ? "tr-TR" : "en-US"
    @State private var keepMenuBar = true
    @State private var addonFeedback = ""
    @State private var tmdbFeedback = ""
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 6) {
                Text(String(localized: "Welcome to Cinma"))
                    .font(.largeTitle.bold())
                Text(String(localized: "Cinma ships without built-in content sources. Add a Stremio addon, optional TMDB metadata key, and language preference below — you can skip or change everything later."))
                    .foregroundStyle(.secondary)
            }

            VStack(alignment: .leading, spacing: 6) {
                Label(String(localized: "1. Stremio Addon URL (optional)"), systemImage: "puzzlepiece.extension")
                    .font(.headline)
                HStack {
                    TextField("https://example.com/manifest.json", text: $addonUrl)
                        .textFieldStyle(.roundedBorder)
                        .accessibilityLabel(String(localized: "Stremio addon manifest URL"))
                        .onSubmit(installAddon)
                    Button(String(localized: "Install"), action: installAddon)
                        .disabled(busy || addonUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                if !addonFeedback.isEmpty {
                    Text(addonFeedback)
                        .font(.caption)
                        .foregroundStyle(addonFeedback.hasPrefix("✓") ? .green : .orange)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                Label(String(localized: "2. TMDB API Key (optional, saved to Keychain)"), systemImage: "key.fill")
                    .font(.headline)
                SecureField("TMDB v3 API key", text: $tmdbKey)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityLabel(String(localized: "TMDB API Key"))
                if !tmdbFeedback.isEmpty {
                    Text(tmdbFeedback)
                        .font(.caption)
                        .foregroundStyle(.red)
                } else {
                    Text(String(localized: "If skipped, Cinma uses installed metadata addons (such as Cinemeta) as fallback."))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                Label(String(localized: "3. Preferences"), systemImage: "slider.horizontal.3")
                    .font(.headline)
                Picker(String(localized: "Metadata Language"), selection: $language) {
                    Text("English (en-US)").tag("en-US")
                    Text("Türkçe (tr-TR)").tag("tr-TR")
                }
                .onChange(of: language) { _, val in
                    try? core.setMetadataLanguage(lang: val)
                }
                Toggle(String(localized: "Keep downloads running in Menu Bar when closed"), isOn: $keepMenuBar)
                    .onChange(of: keepMenuBar) { _, val in
                        try? core.setMenubarEnabled(enabled: val)
                    }
            }

            Spacer()

            HStack {
                Button(String(localized: "Skip")) {
                    try? core.setMetadataLanguage(lang: language)
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
        .frame(width: 560, height: 500)
        .onAppear {
            try? core.setMetadataLanguage(lang: language)
        }
    }

    private func installAddon() {
        let raw = addonUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty else { return }
        let lower = raw.lowercased()
        guard lower.hasPrefix("http://") || lower.hasPrefix("https://") || lower.hasPrefix("stremio://") else {
            addonFeedback = String(localized: "Enter a full URL starting with https:// or stremio://")
            return
        }
        busy = true
        addonFeedback = String(localized: "Checking addon manifest…")
        Task {
            defer { busy = false }
            do {
                let addon = try await core.installAddon(url: raw)
                addonFeedback = "✓ \(String(localized: "Installed \(addon.name) \(addon.version)"))"
                addonUrl = ""
            } catch {
                addonFeedback = FriendlyError.explain(error)
            }
        }
    }

    private func saveAndContinue() {
        let key = tmdbKey.trimmingCharacters(in: .whitespacesAndNewlines)
        try? core.setMetadataLanguage(lang: language)
        guard !key.isEmpty else {
            try? core.setOnboardingCompleted(completed: true)
            onFinish()
            return
        }
        busy = true
        tmdbFeedback = ""
        Task {
            defer { busy = false }
            do {
                try await core.verifyTmdbKey(key: key)
                try core.setTmdbKey(key: key)
                try core.setOnboardingCompleted(completed: true)
                onFinish()
            } catch {
                tmdbFeedback = FriendlyError.explain(error)
            }
        }
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
    @State private var osUsername = ""
    @State private var osPassword = ""
    @State private var osQuota: OpenSubtitlesQuotaInfo?
    @State private var language = "tr-TR"
    @State private var addonsEnabled = true
    @State private var torrentEnabled = true
    @State private var castingEnabled = false
    @State private var trackingEnabled = false
    @State private var skipSegmentsEnabled = false
    @State private var debridEnabled = false
    @State private var iptvEnabled = false
    @State private var discordEnabled = false
    @State private var calendarAlertsEnabled = false
    @State private var spotlightEnabled = false
    @State private var vpnBinding = VpnBindingInfo(bindDeviceName: nil, vpnKillSwitch: false)
    @State private var availableIfaces: [String] = []
    @State private var menubarEnabled = true
    @State private var diskPolicy = DiskPolicySettingsInfo(
        videoFolder: "",
        audioFolder: "",
        cacheQuotaMb: 10_240,
        keepWatchedGlobal: false,
        seedingMode: "off",
        seedingRatio: 1.0,
        seedingMinutes: 60,
        uploadLimitKbps: 0,
        downloadLimitKbps: 0
    )
    @State private var ytdlpStatus = YtdlpStatusInfo(available: false, path: nil, managed: false)
    @State private var subSettings = SubtitleSettingsFfi(
        preferredLangs: ["tur", "eng"],
        audioLangs: ["eng", "jpn", "tur"],
        dualSecondaryLang: "eng",
        srtFont: nil,
        srtFontSize: 46,
        srtColor: "#FFFFFF",
        srtBorderSize: 3,
        srtBorderColor: "#000000",
        srtBackColor: nil,
        srtBold: false,
        srtPos: 95,
        assOverride: false,
        srtOpacity: 100,
        srtBoxOpacity: 0,
        srtAlignX: "center",
        secondaryPos: 12,
        liftAboveControls: true,
        offByDefault: false,
        preferEmbedded: false,
        forcedWhenNativeAudio: false,
        hideSdh: false,
        subScalePct: 100,
        assScaleWithWindow: true,
        enableOpensubtitles: true,
        enableAddonSubtitles: true,
        subHdrPeakNits: 0,
        subUseMargins: true
    )
    @State private var playerPrefs = PlayerPreferencesFfi(
        exactSeek: true,
        hdrTargetPeak: nil,
        shaderPreset: "none",
        autoplayNext: true,
        autoplayCountdownS: 10,
        forceHdrSurface: false,
        seekBackStepS: 10,
        seekForwardStepS: 10,
        defaultSpeedPct: 100,
        volumeBoostMax: 150,
        pauseMinimized: true,
        pauseUnfocused: false,
        autoSkipIntro: false,
        autoSkipRecap: false,
        autoSkipOutro: false,
        showSkipButton: true,
        autoNextStreamOnStall: true,
        stallTimeoutS: 12,
        cropMode: "fit",
        audioNormalize: false,
        downmixStereo: false,
        hideSpoilers: false,
        rememberLastStream: true,
        keepSourceNextEpisode: true,
        trackBlockWords: [],
        streamSort: "score",
        pickerShowFilename: true,
        fullscreenClock: true,
        fullscreenClockEndTime: true,
        nextEpisodeLeadS: 35,
        stillWatchingAfterEpisodes: 0,
        hideWatchedInCatalogs: false,
        hideUnreleased: false,
        episodeSortAsc: true,
        episodeCompactLayout: false,
        confirmOnLeave: false,
        resumeBehavior: "always_resume",
        volumeHud: true,
        screenLockEnabled: true,
        hdrToneMapping: "auto",
        macEdrEnabled: true,
        forceYuv420p: false,
        mpvScalePreset: "balanced",
        mpvHwdecMode: "videotoolbox",
        mpvBufferMb: 8,
        mpvExtraOptions: "",
        debridOnlyMode: false,
        addonTimeoutS: 15,
        cwAtTop: true,
        maxAddonRails: 6,
        posterScalePct: 100,
        posterRadiusPt: 12,
        hidePosterTitles: false,
        showImdbBadge: true,
        showTypeBadge: true,
        showWatchedBadge: true,
        showLibraryBadge: false,
        seekBarChapters: true,
        playerTitleSeriesFirst: true,
        uiLanguage: "system",
        playerAlwaysOnTop: false,
        discordHideTitle: false,
        discordShowWhenPaused: true,
        discordShowWhenBrowsing: false,
        weekStartsMonday: true,
        cwHideCaughtUp: false,
        customSleepMinutes: 45,
        showP2pSpeedChip: true,
        preferOriginalTitles: false,
        tmdbImageLang: "auto",
        themeAccent: "amber",
        reduceGlassEffect: false,
        backdropDimPct: 50,
        hiddenNavTabs: [],
        externalPlayerEnabled: false,
        externalPlayerPath: "",
        confirmStremioDeeplink: true,
        pauseScrobbleOnPause: true,
        malCountRewatches: false,
        holdToBoostEnabled: true,
        holdBoostSpeedPct: 200,
        eqBrightness: 0,
        eqContrast: 0,
        eqGamma: 0,
        eqSaturation: 0,
        screenshotIncludeSubtitles: true,
        trackpadGesturesEnabled: true,
        hapticFeedbackEnabled: true,
        externalPlayerPreset: "auto",
        splitContinueWatching: false,
        pauseOnAudioOutputChange: true,
        clipboardCatcherEnabled: false,
        blackoutOtherScreens: false,
        autoPipOnOcclusion: false,
        notifyOnDownloadComplete: true,
        quickPlayEnabled: true,
        autoSkipAnimeFillers: false,
        localMediaFolders: [],
        downloadScheduleEnabled: false,
        downloadScheduleStartHour: 1,
        downloadScheduleEndHour: 7,
        localApiSocketEnabled: false,
        gamepadSupportEnabled: true
    )
    @State private var subLangsText = "tur, eng"
    @State private var audioLangsText = "eng, jpn, tur"
    @State private var dualLangText = "eng"
    @State private var srtFontText = ""
    @State private var blockWordsText = ""
    @State private var extraMpvText = ""
    @State private var externalPlayerPathText = ""
    @State private var commands: [CommandInfo] = []
    @State private var editingCommandId: String?
    @State private var keyDraft = ""
    @State private var recordingCommandId: String?
    @State private var keyMonitor: Any?
    @State private var status = ""
    @State private var settingsQuery = ""

    private enum SettingsTab: String, CaseIterable {
        case general, playback, subtitles, downloads, shortcuts, backup

        var title: String {
            switch self {
            case .general: return String(localized: "General")
            case .playback: return String(localized: "Playback")
            case .subtitles: return String(localized: "Subtitles")
            case .downloads: return String(localized: "Downloads")
            case .shortcuts: return String(localized: "Shortcuts")
            case .backup: return String(localized: "Backup")
            }
        }
    }
    @State private var selectedTab: SettingsTab = .general

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField(String(localized: "Search settings…"), text: $settingsQuery)
                    .textFieldStyle(.plain)
                if !settingsQuery.isEmpty {
                    Button {
                        settingsQuery = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(.quaternary.opacity(0.6), in: RoundedRectangle(cornerRadius: 8))
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 6)

            if settingsQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                TabView(selection: $selectedTab) {
                    generalTab
                        .tabItem { Label(String(localized: "General"), systemImage: "gearshape") }
                        .tag(SettingsTab.general)
                    playbackTab
                        .tabItem { Label(String(localized: "Playback"), systemImage: "play.rectangle") }
                        .tag(SettingsTab.playback)
                    subtitlesTab
                        .tabItem { Label(String(localized: "Subtitles"), systemImage: "captions.bubble") }
                        .tag(SettingsTab.subtitles)
                    downloadsTab
                        .tabItem { Label(String(localized: "Downloads"), systemImage: "arrow.down.circle") }
                        .tag(SettingsTab.downloads)
                    shortcutsTab
                        .tabItem { Label(String(localized: "Shortcuts"), systemImage: "keyboard") }
                        .tag(SettingsTab.shortcuts)
                    backupTab
                        .tabItem { Label(String(localized: "Backup"), systemImage: "externaldrive") }
                        .tag(SettingsTab.backup)
                }
                .padding(.top, 4)
            } else {
                settingsSearchResultsView
            }

            if !status.isEmpty {
                Divider()
                HStack {
                    Text(status)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 8)
            }
        }
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button(String(localized: "Close")) {
                    saveSubSettings()
                    savePlayerPrefs()
                    stopRecording()
                    onDismiss()
                }
            }
        }
        .frame(width: 680, height: 520)
        .onAppear(perform: reload)
        .onDisappear {
            saveSubSettings()
            savePlayerPrefs()
            stopRecording()
        }
        .onDisappear { stopRecording() }
    }

    private var generalTab: some View {
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
                    Text("English (en-US)").tag("en-US")
                    Text("Türkçe (tr-TR)").tag("tr-TR")
                    Text("Deutsch (de-DE)").tag("de-DE")
                    Text("Français (fr-FR)").tag("fr-FR")
                    Text("Español (es-ES)").tag("es-ES")
                    Text("日本語 (ja-JP)").tag("ja-JP")
                }
                .onChange(of: language) { _, newValue in
                    try? core.setMetadataLanguage(lang: newValue)
                }
                Toggle(String(localized: "Keep Original Movie / Series Titles"), isOn: $playerPrefs.preferOriginalTitles)
                    .onChange(of: playerPrefs.preferOriginalTitles) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "TMDB Poster / Logo Language"), selection: $playerPrefs.tmdbImageLang) {
                    Text(String(localized: "Auto")).tag("auto")
                    Text("English").tag("en")
                    Text("Türkçe").tag("tr")
                    Text(String(localized: "Textless (No Language)")).tag("null")
                }
                .onChange(of: playerPrefs.tmdbImageLang) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "App Interface Language"), selection: $playerPrefs.uiLanguage) {
                    Text(String(localized: "System Default")).tag("system")
                    Text("Türkçe").tag("tr")
                    Text("English").tag("en")
                }
                .onChange(of: playerPrefs.uiLanguage) { _, newValue in
                    savePlayerPrefs()
                    applyAppLanguagePreference(newValue)
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
                Toggle(String(localized: "Skip Intro / Outro Segments (AniSkip / IntroDB)"), isOn: $skipSegmentsEnabled)
                    .onChange(of: skipSegmentsEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "skip_segments", enabled: val) }
                    }
                Toggle(String(localized: "Debrid Resolvers"), isOn: $debridEnabled)
                    .onChange(of: debridEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "debrid", enabled: val) }
                    }
                Toggle(String(localized: "Live TV / IPTV"), isOn: $iptvEnabled)
                    .onChange(of: iptvEnabled) { _, val in
                        Task {
                            try? await core.setFeatureEnabled(name: "iptv", enabled: val)
                            NotificationCenter.default.post(name: .cinmaIptvVisibilityChanged, object: nil)
                        }
                    }
                Toggle(String(localized: "Discord Rich Presence"), isOn: $discordEnabled)
                    .onChange(of: discordEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "discord", enabled: val) }
                    }
            }

            Section(String(localized: "macOS Integration")) {
                Toggle(String(localized: "Calendar New Episode Notifications"), isOn: $calendarAlertsEnabled)
                    .onChange(of: calendarAlertsEnabled) { _, val in
                        Task {
                            let ok = await CalendarNotifications.setEnabled(val, core: core)
                            calendarAlertsEnabled = ok
                            status = ok ? String(localized: "Calendar episode alerts scheduled") : String(localized: "Calendar alerts disabled")
                        }
                    }
                Toggle(String(localized: "Index Library in macOS Spotlight"), isOn: $spotlightEnabled)
                    .onChange(of: spotlightEnabled) { _, val in
                        SpotlightLibraryIndexer.isEnabled = val
                        if val { SpotlightLibraryIndexer.indexLibraryIfEnabled(core: core) }
                    }
            }

            Section(String(localized: "Catalog & Episode Browsing")) {
                Toggle(String(localized: "Hide Episode Spoilers (Blur Unwatched)"), isOn: $playerPrefs.hideSpoilers)
                    .onChange(of: playerPrefs.hideSpoilers) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Enable One-Click Quick Play (Hero & Context Menu)"), isOn: $playerPrefs.quickPlayEnabled)
                    .onChange(of: playerPrefs.quickPlayEnabled) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Hide Watched Titles in Catalogs"), isOn: $playerPrefs.hideWatchedInCatalogs)
                    .onChange(of: playerPrefs.hideWatchedInCatalogs) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Hide Unreleased Titles"), isOn: $playerPrefs.hideUnreleased)
                    .onChange(of: playerPrefs.hideUnreleased) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Sort Episodes Ascending (1 → N)"), isOn: $playerPrefs.episodeSortAsc)
                    .onChange(of: playerPrefs.episodeSortAsc) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Compact Episode List"), isOn: $playerPrefs.episodeCompactLayout)
                    .onChange(of: playerPrefs.episodeCompactLayout) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "Stream Sort Order"), selection: $playerPrefs.streamSort) {
                    Text(String(localized: "Recommended (Score)")).tag("score")
                    Text(String(localized: "Most Seeders")).tag("seeds")
                    Text(String(localized: "Largest File First")).tag("size_desc")
                    Text(String(localized: "Smallest File First")).tag("size_asc")
                }
                .onChange(of: playerPrefs.streamSort) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Raw Filename in Stream Picker"), isOn: $playerPrefs.pickerShowFilename)
                    .onChange(of: playerPrefs.pickerShowFilename) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Remember Last Source per Series"), isOn: $playerPrefs.rememberLastStream)
                    .onChange(of: playerPrefs.rememberLastStream) { _, _ in savePlayerPrefs() }
                HStack {
                    Text(String(localized: "Blocked Stream Keywords"))
                    Spacer()
                    TextField("CAM, HDCAM, TS", text: $blockWordsText)
                        .frame(width: 200)
                        .onSubmit { savePlayerPrefs() }
                }
            }

            Section(String(localized: "Home, Posters & Calendar")) {
                Toggle(String(localized: "Show Continue Watching Above Trending"), isOn: $playerPrefs.cwAtTop)
                    .onChange(of: playerPrefs.cwAtTop) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Hide Nearly Finished Items in Continue Watching"), isOn: $playerPrefs.cwHideCaughtUp)
                    .onChange(of: playerPrefs.cwHideCaughtUp) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Split Continue Watching into Movies & Series"), isOn: $playerPrefs.splitContinueWatching)
                    .onChange(of: playerPrefs.splitContinueWatching) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Addon Catalog Rails on Home")): \(playerPrefs.maxAddonRails)", value: $playerPrefs.maxAddonRails, in: 0...15, step: 1)
                    .onChange(of: playerPrefs.maxAddonRails) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Poster Card Scale")): \(playerPrefs.posterScalePct)%", value: $playerPrefs.posterScalePct, in: 80...125, step: 5)
                    .onChange(of: playerPrefs.posterScalePct) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Poster Corner Radius")): \(playerPrefs.posterRadiusPt) pt", value: $playerPrefs.posterRadiusPt, in: 4...24, step: 2)
                    .onChange(of: playerPrefs.posterRadiusPt) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Hide Poster Titles"), isOn: $playerPrefs.hidePosterTitles)
                    .onChange(of: playerPrefs.hidePosterTitles) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show IMDb Rating Badge"), isOn: $playerPrefs.showImdbBadge)
                    .onChange(of: playerPrefs.showImdbBadge) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Movie / Series Type Label"), isOn: $playerPrefs.showTypeBadge)
                    .onChange(of: playerPrefs.showTypeBadge) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Watched Badge on Posters"), isOn: $playerPrefs.showWatchedBadge)
                    .onChange(of: playerPrefs.showWatchedBadge) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show In-Library Badge on Posters"), isOn: $playerPrefs.showLibraryBadge)
                    .onChange(of: playerPrefs.showLibraryBadge) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Calendar Week Starts on Monday"), isOn: $playerPrefs.weekStartsMonday)
                    .onChange(of: playerPrefs.weekStartsMonday) { _, _ in savePlayerPrefs() }
            }

            Section(String(localized: "Discord Rich Presence Privacy")) {
                Toggle(String(localized: "Hide Media Title in Discord Status"), isOn: $playerPrefs.discordHideTitle)
                    .onChange(of: playerPrefs.discordHideTitle) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Discord Status When Paused"), isOn: $playerPrefs.discordShowWhenPaused)
                    .onChange(of: playerPrefs.discordShowWhenPaused) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Discord Status While Browsing Home"), isOn: $playerPrefs.discordShowWhenBrowsing)
                    .onChange(of: playerPrefs.discordShowWhenBrowsing) { _, _ in savePlayerPrefs() }
            }

            Section(String(localized: "Theme, Glass & Navigation")) {
                Picker(String(localized: "Accent Color"), selection: $playerPrefs.themeAccent) {
                    Text(String(localized: "Amber")).tag("amber")
                    Text(String(localized: "Crimson")).tag("crimson")
                    Text(String(localized: "Cyan")).tag("cyan")
                    Text(String(localized: "Emerald")).tag("emerald")
                    Text(String(localized: "Violet")).tag("violet")
                }
                .onChange(of: playerPrefs.themeAccent) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Reduce Liquid Glass Transparency (Solid Surface)"), isOn: $playerPrefs.reduceGlassEffect)
                    .onChange(of: playerPrefs.reduceGlassEffect) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Detail Backdrop Dimming")): \(playerPrefs.backdropDimPct)%", value: $playerPrefs.backdropDimPct, in: 20...90, step: 5)
                    .onChange(of: playerPrefs.backdropDimPct) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Discover Tab"), isOn: navTabVisibleBinding("discover"))
                Toggle(String(localized: "Show Calendar Tab"), isOn: navTabVisibleBinding("calendar"))
                Toggle(String(localized: "Show Downloads Tab"), isOn: navTabVisibleBinding("downloads"))
            }

            Section(String(localized: "Integrations & Deep Links")) {
                Toggle(String(localized: "Ask Confirmation Before Installing stremio:// Addons"), isOn: $playerPrefs.confirmStremioDeeplink)
                    .onChange(of: playerPrefs.confirmStremioDeeplink) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Pause Trakt / Simkl Scrobble When Paused"), isOn: $playerPrefs.pauseScrobbleOnPause)
                    .onChange(of: playerPrefs.pauseScrobbleOnPause) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Count Rewatches in MyAnimeList / AniList"), isOn: $playerPrefs.malCountRewatches)
                    .onChange(of: playerPrefs.malCountRewatches) { _, _ in savePlayerPrefs() }
            }
        }
        .formStyle(.grouped)
    }

    private var subtitlesTab: some View {
        Form {
            Section(String(localized: "OpenSubtitles & Languages")) {
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
                            osQuota = nil
                            status = String(localized: "OpenSubtitles key removed from Keychain")
                        }
                    }
                }
                if hasOpenSubKey {
                    if let q = osQuota {
                        HStack {
                            Text(String(localized: "Account Quota: \(q.remainingDownloads) / \(q.allowedDownloads) remaining · \(q.level)"))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Spacer()
                            Button(String(localized: "Sign Out")) {
                                Task {
                                    try? await core.opensubtitlesLogout()
                                    osQuota = nil
                                }
                            }
                        }
                    } else {
                        HStack {
                            TextField(String(localized: "OpenSubtitles Username"), text: $osUsername)
                            SecureField(String(localized: "Password"), text: $osPassword)
                            Button(String(localized: "Sign In")) { signInOpenSubtitles() }
                                .disabled(osUsername.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || osPassword.isEmpty)
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
                Toggle(String(localized: "Subtitles Off by Default"), isOn: $subSettings.offByDefault)
                    .onChange(of: subSettings.offByDefault) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Prefer Embedded Subtitles"), isOn: $subSettings.preferEmbedded)
                    .onChange(of: subSettings.preferEmbedded) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Forced Subtitles Only When Audio Matches Native Language"), isOn: $subSettings.forcedWhenNativeAudio)
                    .onChange(of: subSettings.forcedWhenNativeAudio) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Hide SDH / Hearing-Impaired Cues"), isOn: $subSettings.hideSdh)
                    .onChange(of: subSettings.hideSdh) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Search Stremio Subtitle Addons"), isOn: $subSettings.enableAddonSubtitles)
                    .onChange(of: subSettings.enableAddonSubtitles) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Search OpenSubtitles API"), isOn: $subSettings.enableOpensubtitles)
                    .onChange(of: subSettings.enableOpensubtitles) { _, _ in saveSubSettings() }
            }

            Section(String(localized: "SRT Appearance")) {
                SubtitleLivePreviewCard(settings: subSettings)
                HStack {
                    Text(String(localized: "SRT Custom Font"))
                    Spacer()
                    TextField("SF Pro Text", text: $srtFontText)
                        .frame(width: 180)
                        .onSubmit { saveSubSettings() }
                }
                Stepper("\(String(localized: "SRT Font Size")): \(subSettings.srtFontSize) px", value: $subSettings.srtFontSize, in: 20...84, step: 2)
                    .onChange(of: subSettings.srtFontSize) { _, _ in saveSubSettings() }
                Picker(
                    String(localized: "Text Color"),
                    selection: Binding(
                        get: { (subSettings.srtColor ?? "#FFFFFF").uppercased() },
                        set: {
                            subSettings.srtColor = $0
                            saveSubSettings()
                        }
                    )
                ) {
                    Text(String(localized: "White")).tag("#FFFFFF")
                    Text(String(localized: "Yellow")).tag("#FFD54F")
                    Text(String(localized: "Amber")).tag("#FFC107")
                    Text(String(localized: "Cyan")).tag("#67E8F9")
                }
                Stepper("\(String(localized: "Text Opacity")): \(subSettings.srtOpacity)%", value: $subSettings.srtOpacity, in: 10...100, step: 5)
                    .onChange(of: subSettings.srtOpacity) { _, _ in saveSubSettings() }
                Stepper("\(String(localized: "Black Outline")): \(subSettings.srtBorderSize) px", value: $subSettings.srtBorderSize, in: 0...8, step: 1)
                    .onChange(of: subSettings.srtBorderSize) { _, _ in saveSubSettings() }
                Stepper(
                    "\(String(localized: "Background Box Opacity")): \(subSettings.srtBoxOpacity == 0 ? String(localized: "Off") : "\(subSettings.srtBoxOpacity)%")",
                    value: Binding(
                        get: { subSettings.srtBoxOpacity },
                        set: { val in
                            subSettings.srtBoxOpacity = val
                            subSettings.srtBackColor = val > 0 ? (subSettings.srtBackColor ?? "#000000") : nil
                            saveSubSettings()
                        }
                    ),
                    in: 0...100,
                    step: 5
                )
                Picker(String(localized: "Horizontal Alignment"), selection: $subSettings.srtAlignX) {
                    Text(String(localized: "Left")).tag("left")
                    Text(String(localized: "Center")).tag("center")
                    Text(String(localized: "Right")).tag("right")
                }
                .onChange(of: subSettings.srtAlignX) { _, _ in saveSubSettings() }
                Picker(
                    String(localized: "Secondary Subtitle Placement"),
                    selection: Binding(
                        get: { subSettings.secondaryPos <= 50 ? UInt32(12) : UInt32(88) },
                        set: {
                            subSettings.secondaryPos = $0
                            saveSubSettings()
                        }
                    )
                ) {
                    Text(String(localized: "Top")).tag(UInt32(12))
                    Text(String(localized: "Bottom")).tag(UInt32(88))
                }
                Toggle(String(localized: "Bold"), isOn: $subSettings.srtBold)
                    .onChange(of: subSettings.srtBold) { _, _ in saveSubSettings() }
                Stepper("\(String(localized: "SRT Position")): \(subSettings.srtPos)%", value: $subSettings.srtPos, in: 10...100, step: 5)
                    .onChange(of: subSettings.srtPos) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Lift Subtitles Above Player Controls"), isOn: $subSettings.liftAboveControls)
                    .onChange(of: subSettings.liftAboveControls) { _, _ in saveSubSettings() }
                Stepper("\(String(localized: "Subtitle Scale")): \(subSettings.subScalePct)%", value: $subSettings.subScalePct, in: 50...150, step: 5)
                    .onChange(of: subSettings.subScalePct) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Normalize ASS/SSA Scale with Window"), isOn: $subSettings.assScaleWithWindow)
                    .onChange(of: subSettings.assScaleWithWindow) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Override ASS/SSA Script Styling"), isOn: $subSettings.assOverride)
                    .onChange(of: subSettings.assOverride) { _, _ in saveSubSettings() }
                Toggle(String(localized: "Place Subtitles in Black Bars (Margins)"), isOn: $subSettings.subUseMargins)
                    .onChange(of: subSettings.subUseMargins) { _, _ in saveSubSettings() }
                Picker(String(localized: "HDR Subtitle Peak"), selection: $subSettings.subHdrPeakNits) {
                    Text(String(localized: "Auto")).tag(UInt32(0))
                    Text("150 nits").tag(UInt32(150))
                    Text("200 nits").tag(UInt32(200))
                    Text("250 nits").tag(UInt32(250))
                    Text("300 nits").tag(UInt32(300))
                    Text("400 nits").tag(UInt32(400))
                }
                .onChange(of: subSettings.subHdrPeakNits) { _, _ in saveSubSettings() }
            }
        }
        .formStyle(.grouped)
    }

    private var playbackTab: some View {
        Form {
            Section(String(localized: "Playback Behavior")) {
                Picker(String(localized: "Default Playback Speed"), selection: $playerPrefs.defaultSpeedPct) {
                    Text("0.5×").tag(UInt32(50))
                    Text("0.75×").tag(UInt32(75))
                    Text("1×").tag(UInt32(100))
                    Text("1.25×").tag(UInt32(125))
                    Text("1.5×").tag(UInt32(150))
                    Text("2×").tag(UInt32(200))
                }
                .onChange(of: playerPrefs.defaultSpeedPct) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Exact Relative Seeks (vs Keyframe)"), isOn: $playerPrefs.exactSeek)
                    .onChange(of: playerPrefs.exactSeek) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Seek Back Step")): \(playerPrefs.seekBackStepS)s", value: $playerPrefs.seekBackStepS, in: 3...60, step: 1)
                    .onChange(of: playerPrefs.seekBackStepS) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Seek Forward Step")): \(playerPrefs.seekForwardStepS)s", value: $playerPrefs.seekForwardStepS, in: 3...60, step: 1)
                    .onChange(of: playerPrefs.seekForwardStepS) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Pause When Minimized"), isOn: $playerPrefs.pauseMinimized)
                    .onChange(of: playerPrefs.pauseMinimized) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Pause When Window Loses Focus"), isOn: $playerPrefs.pauseUnfocused)
                    .onChange(of: playerPrefs.pauseUnfocused) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "Resume Playback"), selection: $playerPrefs.resumeBehavior) {
                    Text(String(localized: "Always Resume")).tag("always_resume")
                    Text(String(localized: "Ask Each Time")).tag("ask")
                    Text(String(localized: "Always Start Over")).tag("always_start_over")
                }
                .onChange(of: playerPrefs.resumeBehavior) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Confirm Before Leaving Playback"), isOn: $playerPrefs.confirmOnLeave)
                    .onChange(of: playerPrefs.confirmOnLeave) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Volume HUD"), isOn: $playerPrefs.volumeHud)
                    .onChange(of: playerPrefs.volumeHud) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Enable Player Screen Lock"), isOn: $playerPrefs.screenLockEnabled)
                    .onChange(of: playerPrefs.screenLockEnabled) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Clock in Player HUD"), isOn: $playerPrefs.fullscreenClock)
                    .onChange(of: playerPrefs.fullscreenClock) { _, _ in savePlayerPrefs() }
                if playerPrefs.fullscreenClock {
                    Toggle(String(localized: "Show Estimated Finish Time"), isOn: $playerPrefs.fullscreenClockEndTime)
                        .onChange(of: playerPrefs.fullscreenClockEndTime) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Keep Player Window Always on Top"), isOn: $playerPrefs.playerAlwaysOnTop)
                    .onChange(of: playerPrefs.playerAlwaysOnTop) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Chapter Markers on Seek Bar"), isOn: $playerPrefs.seekBarChapters)
                    .onChange(of: playerPrefs.seekBarChapters) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Series Title Before Episode Title"), isOn: $playerPrefs.playerTitleSeriesFirst)
                    .onChange(of: playerPrefs.playerTitleSeriesFirst) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Live P2P Speed Next to Globe"), isOn: $playerPrefs.showP2pSpeedChip)
                    .onChange(of: playerPrefs.showP2pSpeedChip) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Custom Sleep Timer")): \(playerPrefs.customSleepMinutes) min", value: $playerPrefs.customSleepMinutes, in: 5...240, step: 5)
                    .onChange(of: playerPrefs.customSleepMinutes) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show \"Open in External Player\" Button on Direct Streams"), isOn: $playerPrefs.externalPlayerEnabled)
                    .onChange(of: playerPrefs.externalPlayerEnabled) { _, _ in savePlayerPrefs() }
                if playerPrefs.externalPlayerEnabled {
                    Picker(String(localized: "External Player Mode"), selection: $playerPrefs.externalPlayerPreset) {
                        Text(String(localized: "Auto Detect")).tag("auto")
                        Text("IINA (iina://)").tag("iina")
                        Text("Infuse (infuse://)").tag("infuse")
                        Text("VLC (vlc://)").tag("vlc")
                        Text(String(localized: "Custom Binary Path")).tag("custom")
                    }
                    .onChange(of: playerPrefs.externalPlayerPreset) { _, _ in savePlayerPrefs() }
                    if playerPrefs.externalPlayerPreset == "auto" || playerPrefs.externalPlayerPreset == "custom" {
                        HStack {
                            Text(String(localized: "External Player Binary Path"))
                            Spacer()
                            TextField("/opt/homebrew/bin/mpv", text: $externalPlayerPathText)
                                .frame(width: 220)
                                .onSubmit { savePlayerPrefs() }
                        }
                    }
                }
                Toggle(String(localized: "Hold Space / Mouse for Speed Boost"), isOn: $playerPrefs.holdToBoostEnabled)
                    .onChange(of: playerPrefs.holdToBoostEnabled) { _, _ in savePlayerPrefs() }
                if playerPrefs.holdToBoostEnabled {
                    Picker(String(localized: "Hold Boost Speed"), selection: $playerPrefs.holdBoostSpeedPct) {
                        Text("1.5×").tag(UInt32(150))
                        Text("2.0×").tag(UInt32(200))
                        Text("2.5×").tag(UInt32(250))
                        Text("3.0×").tag(UInt32(300))
                    }
                    .onChange(of: playerPrefs.holdBoostSpeedPct) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Trackpad Two-Finger Seek & Volume Gestures"), isOn: $playerPrefs.trackpadGesturesEnabled)
                    .onChange(of: playerPrefs.trackpadGesturesEnabled) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Force Touch Haptic Feedback"), isOn: $playerPrefs.hapticFeedbackEnabled)
                    .onChange(of: playerPrefs.hapticFeedbackEnabled) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Include Subtitles in Screenshots"), isOn: $playerPrefs.screenshotIncludeSubtitles)
                    .onChange(of: playerPrefs.screenshotIncludeSubtitles) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Pause When Audio Output Changes (Headphone Disconnect)"), isOn: $playerPrefs.pauseOnAudioOutputChange)
                    .onChange(of: playerPrefs.pauseOnAudioOutputChange) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Black Out Other Monitors in Fullscreen"), isOn: $playerPrefs.blackoutOtherScreens)
                    .onChange(of: playerPrefs.blackoutOtherScreens) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto Mini Player (PiP) When Space/Window Hidden"), isOn: $playerPrefs.autoPipOnOcclusion)
                    .onChange(of: playerPrefs.autoPipOnOcclusion) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Enable Gamepad / Controller Navigation"), isOn: $playerPrefs.gamepadSupportEnabled)
                    .onChange(of: playerPrefs.gamepadSupportEnabled) { _, _ in savePlayerPrefs() }
            }

            Section(String(localized: "Video & Audio")) {
                Picker(String(localized: "Aspect / Crop Mode"), selection: $playerPrefs.cropMode) {
                    Text(String(localized: "Fit (Original)")).tag("fit")
                    Text(String(localized: "Fill Screen (Crop)")).tag("fill")
                    Text("16:9").tag("16:9")
                    Text("21:9").tag("21:9")
                    Text("4:3").tag("4:3")
                }
                .onChange(of: playerPrefs.cropMode) { _, _ in savePlayerPrefs() }
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
                Toggle(String(localized: "Enable macOS EDR HDR Output"), isOn: $playerPrefs.macEdrEnabled)
                    .onChange(of: playerPrefs.macEdrEnabled) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "HDR Tone Mapping"), selection: $playerPrefs.hdrToneMapping) {
                    Text(String(localized: "Auto")).tag("auto")
                    Text("BT.2390").tag("bt.2390")
                    Text("Reinhard").tag("reinhard")
                    Text("Hable").tag("hable")
                    Text("Clip").tag("clip")
                }
                .onChange(of: playerPrefs.hdrToneMapping) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Force YUV420P Pixel Format"), isOn: $playerPrefs.forceYuv420p)
                    .onChange(of: playerPrefs.forceYuv420p) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "mpv Scaler Quality"), selection: $playerPrefs.mpvScalePreset) {
                    Text(String(localized: "Fast (Bilinear)")).tag("fast")
                    Text(String(localized: "Balanced (Spline36)")).tag("balanced")
                    Text(String(localized: "High (EWA Lanczos)")).tag("high")
                }
                .onChange(of: playerPrefs.mpvScalePreset) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "Hardware Decoding"), selection: $playerPrefs.mpvHwdecMode) {
                    Text("VideoToolbox").tag("videotoolbox")
                    Text("Auto-Safe").tag("auto-safe")
                    Text(String(localized: "Software (Off)")).tag("no")
                }
                .onChange(of: playerPrefs.mpvHwdecMode) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "mpv Buffer Size")): \(playerPrefs.mpvBufferMb) MiB", value: $playerPrefs.mpvBufferMb, in: 4...128, step: 8)
                    .onChange(of: playerPrefs.mpvBufferMb) { _, _ in savePlayerPrefs() }
                HStack {
                    Text(String(localized: "Extra mpv Options"))
                    Spacer()
                    TextField("deband=yes, video-sync=display-resample", text: $extraMpvText)
                        .frame(width: 240)
                        .onSubmit { savePlayerPrefs() }
                }
                Picker(String(localized: "Max Volume Boost"), selection: $playerPrefs.volumeBoostMax) {
                    Text("100%").tag(UInt32(100))
                    Text("150%").tag(UInt32(150))
                    Text("200%").tag(UInt32(200))
                }
                .onChange(of: playerPrefs.volumeBoostMax) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Audio Normalization (Dynaudnorm)"), isOn: $playerPrefs.audioNormalize)
                    .onChange(of: playerPrefs.audioNormalize) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Downmix Audio to Stereo"), isOn: $playerPrefs.downmixStereo)
                    .onChange(of: playerPrefs.downmixStereo) { _, _ in savePlayerPrefs() }
            }

            Section(String(localized: "Automation & Streams")) {
                Toggle(String(localized: "Show Skip Intro / Recap / Outro Button"), isOn: $playerPrefs.showSkipButton)
                    .onChange(of: playerPrefs.showSkipButton) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Intro"), isOn: $playerPrefs.autoSkipIntro)
                    .onChange(of: playerPrefs.autoSkipIntro) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Recap"), isOn: $playerPrefs.autoSkipRecap)
                    .onChange(of: playerPrefs.autoSkipRecap) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Outro / Credits"), isOn: $playerPrefs.autoSkipOutro)
                    .onChange(of: playerPrefs.autoSkipOutro) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Anime Filler Episodes (MyAnimeList)"), isOn: $playerPrefs.autoSkipAnimeFillers)
                    .onChange(of: playerPrefs.autoSkipAnimeFillers) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Play Next Episode"), isOn: $playerPrefs.autoplayNext)
                    .onChange(of: playerPrefs.autoplayNext) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Keep Same Source for Next Episode"), isOn: $playerPrefs.keepSourceNextEpisode)
                    .onChange(of: playerPrefs.keepSourceNextEpisode) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Up Next Lead Time")): \(playerPrefs.nextEpisodeLeadS)s", value: $playerPrefs.nextEpisodeLeadS, in: 10...90, step: 5)
                    .onChange(of: playerPrefs.nextEpisodeLeadS) { _, _ in savePlayerPrefs() }
                Stepper(
                    "\(String(localized: "Pause \"Still Watching\" After")): \(playerPrefs.stillWatchingAfterEpisodes == 0 ? String(localized: "Off") : "\(playerPrefs.stillWatchingAfterEpisodes) ep")",
                    value: $playerPrefs.stillWatchingAfterEpisodes,
                    in: 0...10,
                    step: 1
                )
                .onChange(of: playerPrefs.stillWatchingAfterEpisodes) { _, _ in savePlayerPrefs() }
                if playerPrefs.autoplayNext {
                    Stepper("\(String(localized: "Auto-Play Countdown")): \(playerPrefs.autoplayCountdownS)s", value: $playerPrefs.autoplayCountdownS, in: 3...30)
                        .onChange(of: playerPrefs.autoplayCountdownS) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Auto-Switch Stream on Stall"), isOn: $playerPrefs.autoNextStreamOnStall)
                    .onChange(of: playerPrefs.autoNextStreamOnStall) { _, _ in savePlayerPrefs() }
                if playerPrefs.autoNextStreamOnStall {
                    Stepper("\(String(localized: "Stall Timeout")): \(playerPrefs.stallTimeoutS)s", value: $playerPrefs.stallTimeoutS, in: 5...30, step: 1)
                        .onChange(of: playerPrefs.stallTimeoutS) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Debrid / Direct Only (Hide P2P Torrents)"), isOn: $playerPrefs.debridOnlyMode)
                    .onChange(of: playerPrefs.debridOnlyMode) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Addon HTTP Timeout")): \(playerPrefs.addonTimeoutS)s", value: $playerPrefs.addonTimeoutS, in: 3...60, step: 1)
                    .onChange(of: playerPrefs.addonTimeoutS) { _, _ in savePlayerPrefs() }
            }
        }
        .formStyle(.grouped)
    }

    private var downloadsTab: some View {
        Form {
            Section(String(localized: "Folders & Menu Bar")) {
                Toggle(String(localized: "Keep Running in Menu Bar when Window Closes"), isOn: $menubarEnabled)
                    .onChange(of: menubarEnabled) { _, val in
                        try? core.setMenubarEnabled(enabled: val)
                    }
                Toggle(String(localized: "Notify When Download Completes"), isOn: $playerPrefs.notifyOnDownloadComplete)
                    .onChange(of: playerPrefs.notifyOnDownloadComplete) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Detect Magnet Links in Clipboard on App Focus"), isOn: $playerPrefs.clipboardCatcherEnabled)
                    .onChange(of: playerPrefs.clipboardCatcherEnabled) { _, _ in savePlayerPrefs() }
                folderRow(String(localized: "Video Downloads"), path: diskPolicy.videoFolder) { picked in
                    diskPolicy.videoFolder = picked
                    saveDiskPolicy()
                }
                folderRow(String(localized: "Audio Downloads"), path: diskPolicy.audioFolder) { picked in
                    diskPolicy.audioFolder = picked
                    saveDiskPolicy()
                }
            }

            Section(String(localized: "Torrent Seeding Policy")) {
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
                Stepper("\(String(localized: "Download Limit")): \(diskPolicy.downloadLimitKbps == 0 ? String(localized: "Unlimited") : "\(diskPolicy.downloadLimitKbps) KB/s")", value: $diskPolicy.downloadLimitKbps, in: 0...100_000, step: 1024)
                    .onChange(of: diskPolicy.downloadLimitKbps) { _, _ in saveDiskPolicy() }
            }

            Section(String(localized: "Download Schedule (Hour Window)")) {
                Toggle(String(localized: "Only Download During Allowed Hours"), isOn: $playerPrefs.downloadScheduleEnabled)
                    .onChange(of: playerPrefs.downloadScheduleEnabled) { _, _ in
                        savePlayerPrefs()
                        let hour = UInt8(Calendar.current.component(.hour, from: Date()))
                        try? core.enforceDownloadSchedule(currentHour: hour)
                    }
                if playerPrefs.downloadScheduleEnabled {
                    Picker(String(localized: "Start Hour"), selection: $playerPrefs.downloadScheduleStartHour) {
                        ForEach(0..<24, id: \.self) { h in
                            Text(String(format: "%02d:00", h)).tag(UInt8(h))
                        }
                    }
                    .onChange(of: playerPrefs.downloadScheduleStartHour) { _, _ in
                        savePlayerPrefs()
                        let hour = UInt8(Calendar.current.component(.hour, from: Date()))
                        try? core.enforceDownloadSchedule(currentHour: hour)
                    }
                    Picker(String(localized: "End Hour"), selection: $playerPrefs.downloadScheduleEndHour) {
                        ForEach(0..<24, id: \.self) { h in
                            Text(String(format: "%02d:00", h)).tag(UInt8(h))
                        }
                    }
                    .onChange(of: playerPrefs.downloadScheduleEndHour) { _, _ in
                        savePlayerPrefs()
                        let hour = UInt8(Calendar.current.component(.hour, from: Date()))
                        try? core.enforceDownloadSchedule(currentHour: hour)
                    }
                }
            }

            Section(String(localized: "Local Media Folders")) {
                if playerPrefs.localMediaFolders.isEmpty {
                    Text(String(localized: "No folders added. Add folders to browse local MKV/MP4 files in Library."))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(playerPrefs.localMediaFolders, id: \.self) { folder in
                        HStack {
                            Text((folder as NSString).abbreviatingWithTildeInPath)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer()
                            Button(role: .destructive) {
                                playerPrefs.localMediaFolders.removeAll { $0 == folder }
                                savePlayerPrefs()
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                }
                HStack {
                    Button(String(localized: "Add Folder…")) {
                        let panel = NSOpenPanel()
                        panel.canChooseDirectories = true
                        panel.canChooseFiles = false
                        panel.allowsMultipleSelection = true
                        if panel.runModal() == .OK {
                            for url in panel.urls where !playerPrefs.localMediaFolders.contains(url.path) {
                                playerPrefs.localMediaFolders.append(url.path)
                            }
                            savePlayerPrefs()
                        }
                    }
                    if !playerPrefs.localMediaFolders.isEmpty {
                        Button(String(localized: "Scan Now")) {
                            let found = (try? core.scanLocalFolders()) ?? []
                            status = String(localized: "Scanned \(found.count) local videos.")
                        }
                    }
                }
            }

            Section(String(localized: "Local Control Socket (Opt-In)")) {
                Toggle(String(localized: "Enable Local Unix Socket (`cinma.sock`)"), isOn: $playerPrefs.localApiSocketEnabled)
                    .onChange(of: playerPrefs.localApiSocketEnabled) { _, _ in savePlayerPrefs() }
                if playerPrefs.localApiSocketEnabled {
                    HStack {
                        Text(core.localApiSocketPath())
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer()
                        Button(String(localized: "Copy Path")) {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(core.localApiSocketPath(), forType: .string)
                            status = String(localized: "Socket path copied")
                        }
                    }
                }
            }

            Section(String(localized: "VPN Interface Binding & Kill Switch")) {
                Picker(
                    String(localized: "Bind Torrent Traffic to Interface"),
                    selection: Binding(
                        get: { vpnBinding.bindDeviceName ?? "" },
                        set: { val in
                            vpnBinding.bindDeviceName = val.isEmpty ? nil : val
                            saveVpnBinding()
                        }
                    )
                ) {
                    Text(String(localized: "Any (Default)")).tag("")
                    ForEach(availableIfaces, id: \.self) { iface in
                        Text(iface).tag(iface)
                    }
                }
                Toggle(String(localized: "VPN Kill Switch (block torrents if interface is unset/down)"), isOn: $vpnBinding.vpnKillSwitch)
                    .onChange(of: vpnBinding.vpnKillSwitch) { _, _ in saveVpnBinding() }
            }

            Section(String(localized: "Trailers & Web Downloads (yt-dlp)")) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(ytdlpStatus.available ? String(localized: "yt-dlp installed") : String(localized: "Not installed"))
                            .font(.subheadline.weight(.medium))
                        if let path = ytdlpStatus.path {
                            Text((path as NSString).abbreviatingWithTildeInPath)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    Spacer()
                    Button(ytdlpStatus.available ? String(localized: "Update yt-dlp") : String(localized: "Install yt-dlp")) {
                        status = String(localized: "Downloading yt-dlp…")
                        Task {
                            do {
                                let path = try await core.installOrUpdateYtdlp()
                                ytdlpStatus = core.ytdlpStatus()
                                status = "yt-dlp: \(path)"
                            } catch {
                                status = FriendlyError.explain(error)
                            }
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private var shortcutsTab: some View {
        Form {
            Section(String(localized: "Key Bindings")) {
                ForEach(commands, id: \.id) { cmd in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Self.commandTitle(for: cmd.id))
                                .font(.body)
                            Text(cmd.id)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
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
                            Button(String(localized: "Record")) { startRecording(cmd.id) }
                            Button(String(localized: "Edit")) {
                                editingCommandId = cmd.id
                                keyDraft = cmd.key ?? ""
                            }
                            if cmd.key != nil {
                                Button(String(localized: "Clear")) {
                                    do {
                                        try core.setKeyBinding(id: cmd.id, combo: nil)
                                        reload()
                                    } catch {
                                        status = FriendlyError.explain(error)
                                    }
                                }
                            }
                            Button(String(localized: "Reset")) {
                                do {
                                    try core.resetKeyBinding(id: cmd.id)
                                    reload()
                                } catch {
                                    status = FriendlyError.explain(error)
                                }
                            }
                        }
                    }
                }
            }

            Section(String(localized: "Game Controller Mapping")) {
                LabeledContent(String(localized: "A (Confirm / Play · Pause)"), value: String(localized: "Activate focused item or toggle pause"))
                LabeledContent(String(localized: "B (Back / Cancel)"), value: String(localized: "Close sheet, exit fullscreen, or go back"))
                LabeledContent(String(localized: "X (Skip / Play)"), value: String(localized: "Skip intro/outro, show HUD, or preview"))
                LabeledContent(String(localized: "Y (Subtitles / Settings)"), value: String(localized: "Cycle subtitle track or open Settings"))
                LabeledContent(String(localized: "LB / RB (Bumpers)"), value: String(localized: "Jump backward/forward by step or switch tabs"))
                LabeledContent(String(localized: "LT / RT (Triggers)"), value: String(localized: "Previous or next chapter"))
                LabeledContent(String(localized: "Menu"), value: String(localized: "Focus Search or toggle player HUD"))
            }
        }
        .formStyle(.grouped)
    }

    private static func commandTitle(for id: String) -> String {
        switch id {
        case "app.about": return String(localized: "About Cinma")
        case "app.hide": return String(localized: "Hide Cinma")
        case "app.hide_others": return String(localized: "Hide Others")
        case "app.show_all": return String(localized: "Show All")
        case "app.quit": return String(localized: "Quit Cinma")
        case "file.open": return String(localized: "Open File…")
        case "file.open_url": return String(localized: "Open URL or Magnet…")
        case "edit.undo": return String(localized: "Undo")
        case "edit.redo": return String(localized: "Redo")
        case "edit.cut": return String(localized: "Cut")
        case "edit.copy": return String(localized: "Copy")
        case "edit.paste": return String(localized: "Paste")
        case "edit.select_all": return String(localized: "Select All")
        case "window.minimize": return String(localized: "Minimize Window")
        case "window.close": return String(localized: "Close Window")
        case "player.toggle_pause": return String(localized: "Play / Pause")
        case "player.seek_back": return String(localized: "Seek Backward")
        case "player.seek_forward": return String(localized: "Seek Forward")
        case "player.frame_step": return String(localized: "Step Forward One Frame")
        case "player.frame_back_step": return String(localized: "Step Backward One Frame")
        case "player.close": return String(localized: "Exit Player / Fullscreen")
        case "player.fullscreen": return String(localized: "Toggle Fullscreen")
        case "player.toggle_subtitles": return String(localized: "Show / Hide Subtitles")
        case "player.cycle_subtitle": return String(localized: "Cycle Subtitle Track")
        case "player.cycle_audio": return String(localized: "Cycle Audio Track")
        case "player.screenshot": return String(localized: "Save Screenshot")
        case "player.stats": return String(localized: "Toggle Playback Stats")
        case "player.ab_loop": return String(localized: "A–B Loop")
        case "player.speed_up": return String(localized: "Increase Playback Speed")
        case "player.speed_down": return String(localized: "Decrease Playback Speed")
        case "player.volume_up": return String(localized: "Volume Up")
        case "player.volume_down": return String(localized: "Volume Down")
        case "player.mute": return String(localized: "Mute / Unmute")
        case "player.next_episode": return String(localized: "Next Episode")
        case "player.sub_seek_prev": return String(localized: "Seek to Previous Subtitle")
        case "player.sub_seek_next": return String(localized: "Seek to Next Subtitle")
        case "player.search_dialogue": return String(localized: "Search Dialogue…")
        case "player.live_text": return String(localized: "Scan Frame Text (Live Text)")
        case "player.screen_lock": return String(localized: "Lock / Unlock Player Screen")
        case "addon.install": return String(localized: "Install Addon Deep Link")
        default: return id
        }
    }

    private var backupTab: some View {
        Form {
            Section(String(localized: "Backup & Restore (SQLite)")) {
                HStack {
                    Button(String(localized: "Export Backup…")) { exportBackup() }
                    Button(String(localized: "Restore Backup…")) { restoreBackup() }
                }
                Text(String(localized: "Exports a consistent snapshot of profiles, library, progress, addons, and settings. Secrets stay in macOS Keychain."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    private func reload() {
        hasTmdbKey = (try? core.hasTmdbKey()) ?? false
        hasOpenSubKey = (try? core.hasOpensubtitlesKey()) ?? false
        language = (try? core.metadataLanguage()) ?? "tr-TR"
        addonsEnabled = (try? core.featureEnabled(name: "addons")) ?? true
        torrentEnabled = (try? core.featureEnabled(name: "torrent")) ?? true
        castingEnabled = (try? core.featureEnabled(name: "casting")) ?? false
        trackingEnabled = (try? core.featureEnabled(name: "tracking")) ?? false
        skipSegmentsEnabled = (try? core.featureEnabled(name: "skip_segments")) ?? false
        debridEnabled = (try? core.featureEnabled(name: "debrid")) ?? false
        iptvEnabled = (try? core.featureEnabled(name: "iptv")) ?? false
        discordEnabled = (try? core.featureEnabled(name: "discord")) ?? false
        calendarAlertsEnabled = CalendarNotifications.isEnabled
        spotlightEnabled = SpotlightLibraryIndexer.isEnabled
        if let v = try? core.vpnTorrentBinding() { vpnBinding = v }
        availableIfaces = Self.activeNetworkInterfaces(including: vpnBinding.bindDeviceName)
        if hasOpenSubKey {
            Task { osQuota = try? await core.opensubtitlesQuota() }
        }
        menubarEnabled = (try? core.menubarEnabled()) ?? true
        if let dp = try? core.diskPolicySettings() { diskPolicy = dp }
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
            blockWordsText = p.trackBlockWords.joined(separator: ", ")
            extraMpvText = p.mpvExtraOptions
            externalPlayerPathText = p.externalPlayerPath
        }
        commands = (try? core.commands()) ?? []
    }

    private func navTabVisibleBinding(_ key: String) -> Binding<Bool> {
        Binding(
            get: { !playerPrefs.hiddenNavTabs.contains(key) },
            set: { visible in
                var set = Set(playerPrefs.hiddenNavTabs)
                if visible {
                    set.remove(key)
                } else {
                    set.insert(key)
                }
                playerPrefs.hiddenNavTabs = set.sorted()
                savePlayerPrefs()
            }
        )
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
        playerPrefs.trackBlockWords = blockWordsText
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        playerPrefs.mpvExtraOptions = extraMpvText.trimmingCharacters(in: .whitespacesAndNewlines)
        playerPrefs.externalPlayerPath = externalPlayerPathText.trimmingCharacters(in: .whitespacesAndNewlines)
        try? core.setPlayerPreferences(prefs: playerPrefs)
        CinemaTheme.syncThemePreferences(from: playerPrefs)
        GamepadController.shared.syncEnabled(playerPrefs.gamepadSupportEnabled)
        NotificationCenter.default.post(name: .cinmaNavTabsChanged, object: nil)
    }

    private func applyAppLanguagePreference(_ code: String) {
        if code == "system" {
            UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        } else {
            UserDefaults.standard.set([code], forKey: "AppleLanguages")
        }
        status = String(localized: "App language updated. Relaunch Cinma to apply across all windows.")
    }

    private func saveVpnBinding() {
        let binding = vpnBinding
        Task {
            try? await core.setVpnTorrentBinding(
                bindDeviceName: binding.bindDeviceName,
                vpnKillSwitch: binding.vpnKillSwitch
            )
            status = binding.bindDeviceName.map { String(localized: "Torrents bound to \($0)") }
                ?? String(localized: "VPN interface binding cleared")
        }
    }

    private func signInOpenSubtitles() {
        let u = osUsername
        let p = osPassword
        osPassword = ""
        Task {
            do {
                osQuota = try await core.opensubtitlesLogin(username: u, password: p)
                status = String(localized: "Signed in to OpenSubtitles")
            } catch {
                status = FriendlyError.explain(error)
            }
        }
    }

    private static func activeNetworkInterfaces(including current: String?) -> [String] {
        var names = Set<String>()
        if let current, !current.isEmpty { names.insert(current) }
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        if getifaddrs(&ifaddr) == 0, let first = ifaddr {
            var ptr: UnsafeMutablePointer<ifaddrs>? = first
            while let cur = ptr {
                let name = String(cString: cur.pointee.ifa_name)
                if name.hasPrefix("utun") || name.hasPrefix("en") || name.hasPrefix("ipsec") || name.hasPrefix("ppp") {
                    names.insert(name)
                }
                ptr = cur.pointee.ifa_next
            }
            freeifaddrs(first)
        }
        return names.sorted()
    }

    private func folderRow(_ label: String, path: String, onPick: @escaping (String) -> Void) -> some View {
        HStack {
            Text(label)
            Spacer()
            Text((path as NSString).abbreviatingWithTildeInPath)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(path)
            Button(String(localized: "Choose…")) {
                let panel = NSOpenPanel()
                panel.canChooseDirectories = true
                panel.canChooseFiles = false
                panel.canCreateDirectories = true
                panel.allowsMultipleSelection = false
                panel.directoryURL = URL(fileURLWithPath: path)
                if panel.runModal() == .OK, let url = panel.url {
                    onPick(url.path)
                }
            }
        }
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
            status = FriendlyError.explain(error)
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
            status = FriendlyError.explain(error)
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
                status = FriendlyError.explain(error)
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
            status = FriendlyError.explain(error)
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

    private struct SettingsSearchItem: Identifiable {
        let id: String
        let title: String
        let section: String
        let tab: SettingsTab
        let keywords: String
    }

    private var searchableSettings: [SettingsSearchItem] {
        [
            SettingsSearchItem(id: "tmdb", title: String(localized: "Metadata (TMDB & Language)"), section: String(localized: "Metadata"), tab: .general, keywords: "tmdb api key language tr en"),
            SettingsSearchItem(id: "ui_lang", title: String(localized: "App Interface Language"), section: String(localized: "Metadata"), tab: .general, keywords: "language turkish english tr en"),
            SettingsSearchItem(id: "spotlight", title: String(localized: "Index Library in macOS Spotlight"), section: String(localized: "Integrations"), tab: .general, keywords: "spotlight search shortcuts siri index"),
            SettingsSearchItem(id: "calendar", title: String(localized: "Upcoming Episode Notifications"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "calendar notification release alert week"),
            SettingsSearchItem(id: "cw_split", title: String(localized: "Split Continue Watching into Movies & Series"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "continue watching movies series split home"),
            SettingsSearchItem(id: "cw_hide", title: String(localized: "Hide Nearly Finished Items in Continue Watching"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "continue watching hide caught up 85"),
            SettingsSearchItem(id: "cw_top", title: String(localized: "Show Continue Watching Above Trending"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "continue watching home order"),
            SettingsSearchItem(id: "addon_rails", title: String(localized: "Addon Catalog Rails on Home"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "home catalog rails limit pinned shelves"),
            SettingsSearchItem(id: "hide_watched", title: String(localized: "Hide Watched & Unreleased in Catalogs"), section: String(localized: "Catalog & Episode Browsing"), tab: .general, keywords: "hide watched unreleased catalog discover"),
            SettingsSearchItem(id: "poster_style", title: String(localized: "Poster Scale, Radius & Badges"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "poster size scale radius badge imdb watched library"),
            SettingsSearchItem(id: "spoilers", title: String(localized: "Hide Episode Spoilers (Blur Unwatched)"), section: String(localized: "Catalog & Episode Browsing"), tab: .general, keywords: "spoiler blur unwatched episode"),
            SettingsSearchItem(id: "quick_play", title: String(localized: "Enable One-Click Quick Play (Hero & Context Menu)"), section: String(localized: "Catalog & Episode Browsing"), tab: .general, keywords: "quick play hero one click best stream"),
            SettingsSearchItem(id: "theme", title: String(localized: "Accent Color & Liquid Glass"), section: String(localized: "Theme, Glass & Navigation"), tab: .general, keywords: "theme accent amber crimson cyan emerald violet glass tabs"),
            SettingsSearchItem(id: "hold_boost", title: String(localized: "Hold Space / Mouse for Speed Boost"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "hold 2x speed boost space mouse"),
            SettingsSearchItem(id: "trackpad", title: String(localized: "Trackpad Two-Finger Seek & Volume Gestures"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "trackpad scroll swipe gesture volume seek"),
            SettingsSearchItem(id: "haptic", title: String(localized: "Force Touch Haptic Feedback"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "haptic vibrate force touch feedback"),
            SettingsSearchItem(id: "audio_route_pause", title: String(localized: "Pause When Audio Output Changes (Headphone Disconnect)"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "audio headphone airpods disconnect pause output device"),
            SettingsSearchItem(id: "blackout", title: String(localized: "Black Out Other Monitors in Fullscreen"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "blackout dim other monitors displays fullscreen"),
            SettingsSearchItem(id: "auto_pip", title: String(localized: "Auto Mini Player (PiP) When Space/Window Hidden"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "auto pip picture in picture space hidden occlusion"),
            SettingsSearchItem(id: "gamepad", title: String(localized: "Enable Gamepad / Controller Navigation"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "gamepad controller dualsense xbox joystick navigation"),
            SettingsSearchItem(id: "screenshot_sub", title: String(localized: "Include Subtitles in Screenshots"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "screenshot png subtitle include"),
            SettingsSearchItem(id: "ext_player", title: String(localized: "External Player Mode"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "external player iina infuse vlc mpv url scheme"),
            SettingsSearchItem(id: "hdr", title: String(localized: "Enable macOS EDR HDR Output"), section: String(localized: "Video & Audio"), tab: .playback, keywords: "hdr edr tone mapping bt.2390 peak"),
            SettingsSearchItem(id: "hwdec", title: String(localized: "Hardware Decoding"), section: String(localized: "Video & Audio"), tab: .playback, keywords: "hwdec videotoolbox decoder mpv scaler buffer"),
            SettingsSearchItem(id: "audio_norm", title: String(localized: "Audio Normalization (Dynaudnorm)"), section: String(localized: "Video & Audio"), tab: .playback, keywords: "audio normalize stereo downmix volume boost"),
            SettingsSearchItem(id: "skip", title: String(localized: "Auto-Skip Intro / Outro & Countdown"), section: String(localized: "Automation & Streams"), tab: .playback, keywords: "intro outro recap skip autoplay next episode sleep timer"),
            SettingsSearchItem(id: "stream_sort", title: String(localized: "Stream Sort, Debrid Only & Blocked Keywords"), section: String(localized: "Automation & Streams"), tab: .playback, keywords: "stream sort debrid blocked keyword timeout addon"),
            SettingsSearchItem(id: "sub_langs", title: String(localized: "Preferred Languages & OpenSubtitles"), section: String(localized: "Languages & Search"), tab: .subtitles, keywords: "subtitle audio language dual sdh opensubtitles"),
            SettingsSearchItem(id: "sub_style", title: String(localized: "SRT Appearance, Font & Scale"), section: String(localized: "SRT Appearance"), tab: .subtitles, keywords: "font size color outline background opacity position bold scale"),
            SettingsSearchItem(id: "sub_hdr_peak", title: String(localized: "HDR Subtitle Peak"), section: String(localized: "SRT Appearance"), tab: .subtitles, keywords: "hdr subtitle brightness nits peak"),
            SettingsSearchItem(id: "sub_margins", title: String(localized: "Place Subtitles in Black Bars (Margins)"), section: String(localized: "SRT Appearance"), tab: .subtitles, keywords: "letterbox margin black bar subtitle placement"),
            SettingsSearchItem(id: "vpn", title: String(localized: "VPN Interface Binding"), section: String(localized: "Torrent & VPN"), tab: .downloads, keywords: "vpn interface utun kill switch torrent speed cache"),
            SettingsSearchItem(id: "dl_notify", title: String(localized: "Notify When Download Completes"), section: String(localized: "Folders & Menu Bar"), tab: .downloads, keywords: "download complete notification alert"),
            SettingsSearchItem(id: "clip_magnet", title: String(localized: "Detect Magnet Links in Clipboard on App Focus"), section: String(localized: "Folders & Menu Bar"), tab: .downloads, keywords: "clipboard magnet torrent url catcher"),
            SettingsSearchItem(id: "dl_schedule", title: String(localized: "Download Schedule (Hour Window)"), section: String(localized: "Download Schedule (Hour Window)"), tab: .downloads, keywords: "schedule night download hours pause resume"),
            SettingsSearchItem(id: "local_folders", title: String(localized: "Local Media Folders"), section: String(localized: "Local Media Folders"), tab: .downloads, keywords: "local folder scanner library mkv mp4 nas"),
            SettingsSearchItem(id: "local_socket", title: String(localized: "Local Control Socket (Opt-In)"), section: String(localized: "Local Control Socket (Opt-In)"), tab: .downloads, keywords: "unix socket cinma.sock mcp json rpc api"),
            SettingsSearchItem(id: "anime_filler", title: String(localized: "Auto-Skip Anime Filler Episodes (MyAnimeList)"), section: String(localized: "Automation & Streams"), tab: .playback, keywords: "anime filler mal jikan auto skip"),
            SettingsSearchItem(id: "shortcuts", title: String(localized: "Keyboard Shortcuts & Controller Mapping"), section: String(localized: "Shortcuts"), tab: .shortcuts, keywords: "keyboard shortcut keybinding hotkey frame step gamepad controller"),
            SettingsSearchItem(id: "backup", title: String(localized: "Backup & Sync"), section: String(localized: "Backup"), tab: .backup, keywords: "export import sqlite backup restore"),
        ]
    }

    private var settingsSearchResultsView: some View {
        let q = settingsQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let matches = searchableSettings.filter {
            $0.title.lowercased().contains(q)
                || $0.section.lowercased().contains(q)
                || $0.tab.title.lowercased().contains(q)
                || $0.keywords.lowercased().contains(q)
        }
        return List {
            if matches.isEmpty {
                Text(String(localized: "No matching settings found."))
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 12)
            } else {
                ForEach(matches) { item in
                    Button {
                        selectedTab = item.tab
                        settingsQuery = ""
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(item.title)
                                    .font(.body.weight(.medium))
                                Text("\(item.tab.title) › \(item.section)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Image(systemName: "chevron.right")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .listStyle(.inset)
    }

    private func commitBinding(_ id: String) {
        let combo = keyDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            try core.setKeyBinding(id: id, combo: combo.isEmpty ? nil : combo)
            editingCommandId = nil
            reload()
        } catch {
            status = FriendlyError.explain(error)
        }
    }
}

private struct SubtitleLivePreviewCard: View {
    let settings: SubtitleSettingsFfi

    var body: some View {
        let fontSize = CGFloat(max(11, min(30, Double(settings.srtFontSize) * 0.42)))
        let textColor = Color(hex: settings.srtColor ?? "#FFFFFF").opacity(Double(settings.srtOpacity) / 100.0)
        let boxOpacity = Double(settings.srtBoxOpacity) / 100.0
        let align: HorizontalAlignment = switch settings.srtAlignX.lowercased() {
        case "left": .leading
        case "right": .trailing
        default: .center
        }
        let textAlign: TextAlignment = switch settings.srtAlignX.lowercased() {
        case "left": .leading
        case "right": .trailing
        default: .center
        }
        let font: Font = if let custom = settings.srtFont, !custom.isEmpty {
            .custom(custom, size: fontSize).weight(settings.srtBold ? .bold : .medium)
        } else {
            .system(size: fontSize, weight: settings.srtBold ? .bold : .medium)
        }

        ZStack(alignment: Alignment(horizontal: align, vertical: settings.srtPos < 50 ? .top : .bottom)) {
            LinearGradient(
                colors: [Color(red: 0.08, green: 0.10, blue: 0.15), Color(red: 0.16, green: 0.12, blue: 0.10)],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            Text(String(localized: "Live subtitle preview line.\nSample bilingual text rendering."))
                .font(font)
                .multilineTextAlignment(textAlign)
                .foregroundStyle(textColor)
                .padding(.horizontal, boxOpacity > 0 ? 8 : 0)
                .padding(.vertical, boxOpacity > 0 ? 4 : 0)
                .background(boxOpacity > 0 ? Color.black.opacity(boxOpacity) : .clear, in: RoundedRectangle(cornerRadius: 4))
                .shadow(color: .black.opacity(0.9), radius: CGFloat(settings.srtBorderSize) * 0.45, x: 0, y: 1)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
        }
        .frame(height: 92)
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .overlay(
            HStack(spacing: 6) {
                if settings.subUseMargins {
                    Text(String(localized: "Margins"))
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.ultraThinMaterial, in: Capsule())
                }
                if settings.subHdrPeakNits > 0 {
                    Text("\(settings.subHdrPeakNits) nits")
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.ultraThinMaterial, in: Capsule())
                }
            }
            .padding(6),
            alignment: .topTrailing
        )
    }
}

private extension Color {
    init(hex: String) {
        let clean = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: clean).scanHexInt64(&int)
        let r = Double((int >> 16) & 0xFF) / 255.0
        let g = Double((int >> 8) & 0xFF) / 255.0
        let b = Double(int & 0xFF) / 255.0
        self = Color(red: r, green: g, blue: b)
    }
}
