import AppKit
import Darwin
import SwiftUI
import UniformTypeIdentifiers

// MARK: - Settings (SwiftUI leaf view)

struct SettingsView: View {
    let core: CinmaCore
    let onDismiss: () -> Void

    @State var tmdbKey = ""
    @State var hasTmdbKey = false
    @State var openSubKey = ""
    @State var hasOpenSubKey = false
    @State var osUsername = ""
    @State var osPassword = ""
    @State var osQuota: OpenSubtitlesQuotaInfo?
    @State var language = "tr-TR"
    @State var addonsEnabled = true
    @State var torrentEnabled = true
    @State var castingEnabled = false
    @State var trackingEnabled = false
    @State var skipSegmentsEnabled = false
    @State var debridEnabled = false
    @State var iptvEnabled = false
    @State var discordEnabled = false
    @State var calendarAlertsEnabled = false
    @State var spotlightEnabled = false
    @State var vpnBinding = VpnBindingInfo(bindDeviceName: nil, vpnKillSwitch: false)
    @State var availableIfaces: [String] = []
    @State var menubarEnabled = true
    @State var diskPolicy = DiskPolicySettingsInfo(
        videoFolder: "",
        audioFolder: "",
        seedingMode: "off",
        seedingRatio: 1.0,
        seedingMinutes: 60,
        uploadLimitKbps: 0,
        downloadLimitKbps: 0
    )
    @State var ytdlpStatus = YtdlpStatusInfo(available: false, path: nil, managed: false)
    @State var subSettings = SubtitleSettingsFfi(
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
    @State var playerPrefs = PlayerPreferencesFfi(
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
    @State var subLangsText = "tur, eng"
    @State var audioLangsText = "eng, jpn, tur"
    @State var dualLangText = "eng"
    @State var srtFontText = ""
    @State var blockWordsText = ""
    @State var extraMpvText = ""
    @State var externalPlayerPathText = ""
    @State var commands: [CommandInfo] = []
    @State var editingCommandId: String?
    @State var keyDraft = ""
    @State var recordingCommandId: String?
    @State var keyMonitor: Any?
    @State var status = ""
    @State var settingsQuery = ""

    enum SettingsTab: String, CaseIterable {
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
    @State var selectedTab: SettingsTab = .general

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
                    .onChange(of: playerPrefs.discordShowWhenBrowsing) { _, enabled in
                        savePlayerPrefs()
                        if !enabled {
                            Task { try? await core.clearDiscordPresence() }
                        }
                    }
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
