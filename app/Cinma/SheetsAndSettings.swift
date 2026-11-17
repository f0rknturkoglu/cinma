import AppKit
import SwiftUI

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
                            Text(profile.name)
                                .font(.headline)
                            if profile.id == activeId {
                                Text(String(localized: "Active"))
                                    .font(.caption)
                                    .foregroundStyle(.orange)
                            }
                        }
                        Spacer()
                        if profile.id != activeId {
                            Button(String(localized: "Switch")) {
                                try? core.setActiveProfile(id: profile.id)
                                reload()
                                onDismiss()
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
                        }
                    }
                    .padding(.vertical, 4)
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
        .frame(width: 440, height: 380)
        .onAppear(perform: reload)
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
    @State private var status = ""
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text(String(localized: "Welcome to Cinma"))
                .font(.largeTitle.bold())
            Text(String(localized: "Cinma ships without built-in content sources. Add a Stremio addon by URL and optionally your free TMDB API key for rich Turkish/English artwork and calendars."))
                .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 8) {
                Text(String(localized: "Stremio Addon URL (optional)"))
                    .font(.headline)
                HStack {
                    TextField("https://…/manifest.json", text: $addonUrl)
                        .textFieldStyle(.roundedBorder)
                    Button(String(localized: "Install"), action: installAddon)
                        .disabled(busy || addonUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                Text(String(localized: "TMDB API Key (optional, stored in Keychain)"))
                    .font(.headline)
                SecureField("TMDB v3 API key", text: $tmdbKey)
                    .textFieldStyle(.roundedBorder)
                Text(String(localized: "If skipped, Cinma uses installed metadata addons (such as Cinemeta) as fallback."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
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
                Spacer()
                Button(String(localized: "Continue")) {
                    saveAndContinue()
                }
                .buttonStyle(.borderedProminent)
                .tint(.orange)
                .disabled(busy)
            }
        }
        .padding(28)
        .frame(width: 540, height: 420)
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
    @State private var language = "tr-TR"
    @State private var addonsEnabled = true
    @State private var torrentEnabled = true
    @State private var ytdlpStatus = YtdlpStatusInfo(available: false, path: nil, managed: false)
    @State private var commands: [CommandInfo] = []
    @State private var editingCommandId: String?
    @State private var keyDraft = ""
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

            Section(String(localized: "Features")) {
                Toggle(String(localized: "Stremio Addons"), isOn: $addonsEnabled)
                    .onChange(of: addonsEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "addons", enabled: val) }
                    }
                Toggle(String(localized: "BitTorrent Engine"), isOn: $torrentEnabled)
                    .onChange(of: torrentEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "torrent", enabled: val) }
                    }
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
                        if editingCommandId == cmd.id {
                            TextField("cmd+k", text: $keyDraft)
                                .frame(width: 110)
                                .onSubmit { commitBinding(cmd.id) }
                            Button(String(localized: "Save")) { commitBinding(cmd.id) }
                        } else {
                            Text(cmd.key ?? "—")
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(.secondary)
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
                Button(String(localized: "Close"), action: onDismiss)
            }
        }
        .frame(width: 600, height: 520)
        .onAppear(perform: reload)
    }

    private func reload() {
        hasTmdbKey = (try? core.hasTmdbKey()) ?? false
        language = (try? core.metadataLanguage()) ?? "tr-TR"
        addonsEnabled = (try? core.featureEnabled(name: "addons")) ?? true
        torrentEnabled = (try? core.featureEnabled(name: "torrent")) ?? true
        ytdlpStatus = core.ytdlpStatus()
        commands = (try? core.commands()) ?? []
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
