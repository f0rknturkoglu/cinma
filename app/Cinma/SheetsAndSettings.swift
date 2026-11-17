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
        uploadLimitKbps: 0
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

    private enum SettingsTab: String, CaseIterable {
        case general, playback, subtitles, downloads, shortcuts, backup
    }
    @State private var selectedTab: SettingsTab = .general

    var body: some View {
        VStack(spacing: 0) {
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
            .padding(.top, 12)

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
                    stopRecording()
                    onDismiss()
                }
            }
        }
        .frame(width: 680, height: 520)
        .onAppear(perform: reload)
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
                }
                .onChange(of: language) { _, newValue in
                    try? core.setMetadataLanguage(lang: newValue)
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
            }

            Section(String(localized: "SRT Appearance")) {
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
        }
        .formStyle(.grouped)
    }

    private var playbackTab: some View {
        Form {
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
                            Button(String(localized: "Record")) { startRecording(cmd.id) }
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
        }
        .formStyle(.grouped)
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
