import AppKit
import SwiftUI
import UniformTypeIdentifiers

// MARK: - Phase 6: Integrations & Stremio One-Time Import

struct IntegrationsSheetView: View {
    let core: CinmaCore
    let onDismiss: () -> Void

    @State private var castingEnabled = false
    @State private var trackingEnabled = false
    @State private var adapters: [AdapterStatusInfo] = []
    @State private var statusMessage = ""

    // Trakt Device Code OAuth
    @State private var traktClientId = ""
    @State private var traktClientSecret = ""
    @State private var traktDevice: TraktDeviceCodeInfo?

    // Simkl / AniList / MAL direct token inputs
    @State private var tokenAdapter = "anilist"
    @State private var tokenDraft = ""

    // Letterboxd RSS
    @State private var letterboxdUser = ""
    @State private var letterboxdEntries: [LetterboxdEntryInfo] = []

    // One-time Stremio Import
    @State private var stremioEmail = ""
    @State private var stremioPassword = ""
    @State private var busy = false

    var body: some View {
        Form {
            Section(String(localized: "Casting (Chromecast & DLNA)")) {
                Toggle(String(localized: "Enable LAN Device Discovery & Casting"), isOn: $castingEnabled)
                    .onChange(of: castingEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "casting", enabled: val) }
                    }
                Text(String(localized: "Zero background sockets run when idle. Casting opens an ephemeral local HTTP Range stream only for active sessions. Note: AirPlay requires AVPlayer and is not compatible with libmpv."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section(String(localized: "Watch Tracking & Outbox")) {
                Toggle(String(localized: "Enable Watch Tracking & Offline Sync Outbox"), isOn: $trackingEnabled)
                    .onChange(of: trackingEnabled) { _, val in
                        Task { try? await core.setFeatureEnabled(name: "tracking", enabled: val) }
                    }

                ForEach(adapters, id: \.adapter) { item in
                    HStack {
                        Toggle(item.adapter.capitalized, isOn: Binding(
                            get: { item.enabled },
                            set: { val in
                                try? core.setTrackingAdapterEnabled(adapter: item.adapter, enabled: val)
                                reload()
                            }
                        ))
                        Spacer()
                        Text(item.connected ? String(localized: "Connected") : String(localized: "Not linked"))
                            .font(.caption)
                            .foregroundStyle(item.connected ? .green : .secondary)
                        if item.outboxPending > 0 {
                            Text("\(item.outboxPending) queued")
                                .font(.caption2)
                                .foregroundStyle(.orange)
                        }
                    }
                }

                HStack {
                    Button(String(localized: "Sync Outbox Now")) { drainOutbox() }
                        .disabled(!trackingEnabled || busy)
                    Button(String(localized: "Backfill Watched History")) {
                        let n = (try? core.backfillTracking()) ?? 0
                        statusMessage = String(localized: "Queued \(n) items for sync")
                        reload()
                    }
                    .disabled(!trackingEnabled)
                    Button(String(localized: "Seed Anime ID Map")) {
                        let n = (try? core.seedAnimeIdMap()) ?? 0
                        statusMessage = String(localized: "Loaded \(n) starter anime ID mappings")
                    }
                }
            }

            if trackingEnabled {
                Section(String(localized: "Trakt Device OAuth")) {
                    TextField(String(localized: "Trakt Client ID"), text: $traktClientId)
                    SecureField(String(localized: "Trakt Client Secret"), text: $traktClientSecret)
                    HStack {
                        Button(String(localized: "Start Trakt Sign-In")) { startTraktAuth() }
                            .disabled(traktClientId.isEmpty || busy)
                        if let dev = traktDevice {
                            Text("\(dev.verificationUrl) · Code: \(dev.userCode)")
                                .font(.caption.monospaced())
                            Button(String(localized: "Verify Code")) { pollTraktAuth(dev) }
                        }
                    }
                }

                Section(String(localized: "Token Sign-In (AniList / MAL / Simkl)")) {
                    Picker(String(localized: "Service"), selection: $tokenAdapter) {
                        Text("AniList").tag("anilist")
                        Text("MyAnimeList").tag("mal")
                        Text("Simkl").tag("simkl")
                    }
                    HStack {
                        SecureField(String(localized: "OAuth Access Token (saved to Keychain)"), text: $tokenDraft)
                        Button(String(localized: "Save Token")) {
                            try? core.setTrackingToken(
                                adapter: tokenAdapter,
                                token: tokenDraft.trimmingCharacters(in: .whitespacesAndNewlines)
                            )
                            tokenDraft = ""
                            statusMessage = String(localized: "Token saved to Keychain")
                            reload()
                        }
                        .disabled(tokenDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                }

                Section(String(localized: "Letterboxd (RSS & Official CSV Export)")) {
                    HStack {
                        TextField(String(localized: "Letterboxd username"), text: $letterboxdUser)
                        Button(String(localized: "Load Recent")) { fetchLetterboxd() }
                            .disabled(letterboxdUser.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy)
                        Button(String(localized: "Export CSV…")) { exportLetterboxdCsv() }
                    }
                    ForEach(Array(letterboxdEntries.prefix(5).enumerated()), id: \.offset) { _, entry in
                        Text("\(entry.title) \(entry.watchedDate ?? "")")
                            .font(.caption)
                    }
                }
            }

            Section(String(localized: "One-Time Stremio Account Import")) {
                TextField(String(localized: "Stremio Email"), text: $stremioEmail)
                SecureField(String(localized: "Stremio Password (never stored)"), text: $stremioPassword)
                Button(String(localized: "Import Addons & Library")) { runStremioImport() }
                    .disabled(busy || stremioEmail.isEmpty || stremioPassword.isEmpty)
            }

            if !statusMessage.isEmpty {
                Text(statusMessage)
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
        .frame(width: 620, height: 580)
        .onAppear(perform: reload)
    }

    private func reload() {
        castingEnabled = (try? core.featureEnabled(name: "casting")) ?? false
        trackingEnabled = (try? core.featureEnabled(name: "tracking")) ?? false
        adapters = (try? core.trackingAdapters()) ?? []
    }

    private func drainOutbox() {
        busy = true
        Task {
            defer { busy = false; reload() }
            do {
                let n = try await core.drainTrackingOutbox()
                statusMessage = String(localized: "Synced \(n) outbox entries")
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }

    private func startTraktAuth() {
        busy = true
        Task {
            defer { busy = false }
            do {
                let dev = try await core.traktStartDeviceAuth(clientId: traktClientId.trimmingCharacters(in: .whitespacesAndNewlines))
                traktDevice = dev
                if let u = URL(string: dev.verificationUrl) { NSWorkspace.shared.open(u) }
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }

    private func pollTraktAuth(_ dev: TraktDeviceCodeInfo) {
        Task {
            do {
                let ok = try await core.traktPollDeviceAuth(
                    clientId: traktClientId.trimmingCharacters(in: .whitespacesAndNewlines),
                    clientSecret: traktClientSecret.trimmingCharacters(in: .whitespacesAndNewlines),
                    deviceCode: dev.deviceCode
                )
                statusMessage = ok ? String(localized: "Trakt connected") : String(localized: "Still waiting for authorization on trakt.tv/activate")
                if ok { traktDevice = nil; reload() }
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }

    private func fetchLetterboxd() {
        busy = true
        Task {
            defer { busy = false }
            do {
                letterboxdEntries = try await core.letterboxdRss(username: letterboxdUser)
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }

    private func exportLetterboxdCsv() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.commaSeparatedText]
        panel.nameFieldStringValue = "cinma-letterboxd-export.csv"
        guard panel.runModal() == .OK, let dest = panel.url else { return }
        do {
            let count = try core.exportLetterboxdCsv(destPath: dest.path)
            statusMessage = String(localized: "Exported \(count) watched films to CSV")
        } catch {
            statusMessage = error.localizedDescription
        }
    }

    private func runStremioImport() {
        let email = stremioEmail
        let pw = stremioPassword
        stremioPassword = ""
        busy = true
        statusMessage = String(localized: "Importing from Stremio…")
        Task {
            defer { busy = false }
            do {
                let res = try await core.importStremioAccount(email: email, password: pw)
                statusMessage = String(localized: "Imported \(res.addonsImported) addons and \(res.libraryImported) library items")
            } catch {
                statusMessage = error.localizedDescription
            }
        }
    }
}

// MARK: - Player LAN Cast Device Picker Sheet

struct CastDeviceSheetView: View {
    let core: CinmaCore
    let request: PlayRequest
    let title: String
    let onDismiss: () -> Void

    @State private var devices: [CastDeviceInfo] = []
    @State private var activeDevice: CastDeviceInfo?
    @State private var scanning = false
    @State private var status = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(String(localized: "Cast to Device"))
                    .font(.title2.bold())
                Spacer()
                Button(String(localized: "Close"), action: onDismiss)
            }

            if let active = activeDevice {
                HStack {
                    Image(systemName: "tv.and.mediabox")
                    Text("\(String(localized: "Casting to")) \(active.name)")
                        .font(.headline)
                    Spacer()
                    Button(String(localized: "Play")) { Task { try? await core.castPlay() } }
                    Button(String(localized: "Pause")) { Task { try? await core.castPause() } }
                    Button(String(localized: "Stop"), role: .destructive) {
                        Task {
                            try? await core.castStop()
                            activeDevice = nil
                        }
                    }
                }
                .padding(10)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 10))
            }

            HStack {
                Button(scanning ? String(localized: "Scanning…") : String(localized: "Discover LAN Devices")) {
                    scan()
                }
                .disabled(scanning)
                Spacer()
                Text(String(localized: "Supports Chromecast & DLNA"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            List(devices, id: \.id) { dev in
                HStack {
                    Image(systemName: dev.kind == "chromecast" ? "tv" : "hifispeaker.2")
                    VStack(alignment: .leading) {
                        Text(dev.name).font(.headline)
                        Text("\(dev.kind.uppercased()) · \(dev.host):\(dev.port)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(String(localized: "Cast")) { cast(to: dev) }
                        .buttonStyle(.borderedProminent)
                        .tint(.orange)
                }
            }
            .frame(minHeight: 160)

            if !status.isEmpty {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(24)
        .frame(width: 480, height: 380)
        .onAppear {
            activeDevice = core.activeCastDevice()
            scan()
        }
    }

    private func scan() {
        scanning = true
        status = ""
        Task {
            defer { scanning = false }
            do {
                if !((try? core.featureEnabled(name: "casting")) ?? false) {
                    try await core.setFeatureEnabled(name: "casting", enabled: true)
                }
                devices = try await core.discoverCastDevices(timeoutMs: 1800)
                if devices.isEmpty {
                    status = String(localized: "No Chromecast or DLNA devices responded on your LAN.")
                }
            } catch {
                status = error.localizedDescription
            }
        }
    }

    private func cast(to dev: CastDeviceInfo) {
        status = String(localized: "Starting cast session…")
        Task {
            do {
                switch request {
                case .download(let id, _):
                    try await core.castDownload(device: dev, downloadId: id, title: title)
                case .file(let url):
                    try await core.castLocalFile(device: dev, path: url.path, title: title)
                case .stream(let stream, _):
                    try await core.castStream(device: dev, stream: stream, title: title)
                case .trailer:
                    status = String(localized: "Trailers cannot be cast directly.")
                    return
                }
                activeDevice = core.activeCastDevice()
                status = String(localized: "Casting started")
            } catch {
                status = FriendlyError.explain(error)
            }
        }
    }
}
