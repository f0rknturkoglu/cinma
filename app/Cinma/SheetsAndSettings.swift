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
