import AppKit
import SwiftUI

/// Helper to parse `infoHash` and `sources` (`tracker:...`) from raw Stremio stream JSON.
enum Dalga3StreamHelpers {
    static func extractTorrentMeta(from stream: StreamInfo) -> (infoHash: String, trackers: [String])? {
        guard let hash = stream.infoHash, !hash.isEmpty else { return nil }
        var trackers: [String] = []
        if let data = stream.raw.data(using: .utf8),
           let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let sources = obj["sources"] as? [String]
        {
            for s in sources where s.hasPrefix("tracker:") {
                trackers.append(String(s.dropFirst("tracker:".count)))
            }
        }
        return (hash, trackers)
    }

    static func isBatchTorrent(_ stream: StreamInfo) -> Bool {
        guard let hash = stream.infoHash, !hash.isEmpty else { return false }
        if stream.fileIdx != nil {
            return false
        }
        if let data = stream.raw.data(using: .utf8),
           let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let rawIdx = obj["fileIdx"],
           !(rawIdx is NSNull)
        {
            return false
        }
        return true
    }
}

/// Multi-file / Season-Pack Torrent Inspector & Batch Queue Sheet (#N21).
struct BatchTorrentSheetView: View {
    let core: CinmaCore
    let infoHash: String
    let trackers: [String]
    let title: String
    let metaId: String?
    let mediaType: String?
    let onDismiss: () -> Void
    let onQueued: (Int) -> Void

    @State private var files: [TorrentFileItemInfo] = []
    @State private var selectedIndices: Set<UInt64> = []
    @State private var loading = true
    @State private var queueing = false
    @State private var errorText: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(String(localized: "Batch / Season Pack Inspector"))
                        .font(.title3.weight(.bold))
                    Text(title)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer()
                Button(String(localized: "Close")) { onDismiss() }
                    .keyboardShortcut(.cancelAction)
            }

            if loading {
                HStack(spacing: 10) {
                    ProgressView()
                        .controlSize(.small)
                    Text(String(localized: "Fetching torrent file list from peers…"))
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            } else if let errorText {
                VStack(alignment: .leading, spacing: 8) {
                    Text(errorText)
                        .foregroundStyle(.red)
                    Button(String(localized: "Retry")) { loadFiles() }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            } else if files.isEmpty {
                Text(String(localized: "No video files discovered in this torrent."))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            } else {
                HStack(spacing: 8) {
                    Button(String(localized: "Select All")) {
                        selectedIndices = Set(files.map(\.fileIdx))
                    }
                    Button(String(localized: "Episodes Only")) {
                        selectedIndices = Set(files.filter { $0.episode != nil }.map(\.fileIdx))
                    }
                    Button(String(localized: "Clear")) {
                        selectedIndices.removeAll()
                    }
                    Spacer()
                    Text("\(selectedIndices.count) / \(files.count)")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }

                List(files, id: \.fileIdx) { file in
                    Toggle(isOn: Binding(
                        get: { selectedIndices.contains(file.fileIdx) },
                        set: { on in
                            if on { selectedIndices.insert(file.fileIdx) }
                            else { selectedIndices.remove(file.fileIdx) }
                        }
                    )) {
                        HStack(spacing: 8) {
                            if let s = file.season, let e = file.episode {
                                Text(String(format: "S%02dE%02d", s, e))
                                    .font(.caption.monospacedDigit().weight(.semibold))
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Color.accentColor.opacity(0.18), in: Capsule())
                            }
                            Text(file.name)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer()
                            Text(ByteCountFormatter.string(fromByteCount: Int64(file.sizeBytes), countStyle: .file))
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .listStyle(.inset)
            }

            HStack {
                Spacer()
                Button(String(localized: "Queue Selected (\(selectedIndices.count))")) {
                    queueSelected()
                }
                .buttonStyle(.borderedProminent)
                .disabled(selectedIndices.isEmpty || loading || queueing)
            }
        }
        .padding(20)
        .frame(width: 640, height: 480)
        .onAppear { loadFiles() }
    }

    private func loadFiles() {
        loading = true
        errorText = nil
        let core = core
        let hash = infoHash
        let tr = trackers
        Task {
            do {
                let list = try await core.listTorrentFiles(infoHash: hash, trackers: tr)
                await MainActor.run {
                    self.files = list
                    let eps = list.filter { $0.episode != nil }
                    self.selectedIndices = Set((eps.isEmpty ? list : eps).map(\.fileIdx))
                    self.loading = false
                }
            } catch {
                await MainActor.run {
                    self.errorText = FriendlyError.explain(error)
                    self.loading = false
                }
            }
        }
    }

    private func queueSelected() {
        queueing = true
        let chosen = files
            .filter { selectedIndices.contains($0.fileIdx) }
            .map { item -> TorrentBatchItemInput in
                let display = item.name.components(separatedBy: "/").last ?? item.name
                return TorrentBatchItemInput(fileIdx: item.fileIdx, title: display)
            }
        do {
            let ids = try core.queueTorrentBatch(
                infoHash: infoHash,
                trackers: trackers,
                metaId: metaId,
                mediaType: mediaType,
                items: chosen,
                priority: 10
            )
            onQueued(ids.count)
            onDismiss()
        } catch {
            queueing = false
            errorText = FriendlyError.explain(error)
        }
    }
}

/// Direct WebDAV Network Browser Sheet (Zurg / TorBox WebDAV / NAS / Nextcloud, #N24).
struct WebDavBrowserSheetView: View {
    let core: CinmaCore
    let onPlayUrl: (String, String) -> Void
    let onDismiss: () -> Void

    @State private var sources: [WebDavSourceFfi] = []
    @State private var selectedSourceId: String?
    @State private var subPath = ""
    @State private var entries: [WebDavEntryInfo] = []
    @State private var loading = false
    @State private var errorText: String?

    @State private var showAddForm = false
    @State private var newName = ""
    @State private var newUrl = ""
    @State private var newUser = ""
    @State private var newPassword = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Label(String(localized: "WebDAV Network Sources"), systemImage: "externaldrive.connected.to.line.below")
                    .font(.title3.weight(.bold))
                Spacer()
                Button(showAddForm ? String(localized: "Cancel") : String(localized: "Add Source")) {
                    showAddForm.toggle()
                }
                Button(String(localized: "Close")) { onDismiss() }
                    .keyboardShortcut(.cancelAction)
            }

            if showAddForm {
                VStack(alignment: .leading, spacing: 8) {
                    TextField(String(localized: "Name (e.g. Zurg / TorBox / NAS)"), text: $newName)
                    TextField(String(localized: "WebDAV URL (https://...)"), text: $newUrl)
                    HStack {
                        TextField(String(localized: "Username (optional)"), text: $newUser)
                        SecureField(String(localized: "Password (Keychain)"), text: $newPassword)
                    }
                    HStack {
                        Spacer()
                        Button(String(localized: "Save Source")) { addSource() }
                            .buttonStyle(.borderedProminent)
                            .disabled(newUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                }
                .padding(12)
                .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
            }

            if sources.isEmpty {
                Text(String(localized: "No WebDAV servers configured. Add Zurg, TorBox WebDAV, Nextcloud, or NAS URLs above."))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            } else {
                HStack(spacing: 8) {
                    Picker(String(localized: "Source"), selection: $selectedSourceId) {
                        ForEach(sources, id: \.id) { src in
                            Text(src.name).tag(Optional(src.id))
                        }
                    }
                    .frame(maxWidth: 260)
                    .onChange(of: selectedSourceId) { _, _ in
                        subPath = ""
                        browseCurrent()
                    }

                    if !subPath.isEmpty {
                        Button {
                            goUp()
                        } label: {
                            Label(String(localized: "Up"), systemImage: "chevron.up")
                        }
                    }

                    Text(subPath.isEmpty ? "/" : "/\(subPath)")
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.head)

                    Spacer()

                    if let sid = selectedSourceId {
                        Button(role: .destructive) {
                            try? core.removeWebdavSource(id: sid)
                            reloadSources()
                        } label: {
                            Image(systemName: "trash")
                        }
                        .help(String(localized: "Remove WebDAV Source"))
                    }
                }

                if loading {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
                } else if let errorText {
                    VStack(spacing: 8) {
                        Text(errorText).foregroundStyle(.red)
                        Button(String(localized: "Retry")) { browseCurrent() }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
                } else if entries.isEmpty {
                    Text(String(localized: "Folder contains no media files."))
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
                } else {
                    List(entries, id: \.subPath) { entry in
                        let ext = (entry.name as NSString).pathExtension.lowercased()
                        let isSubtitle = !entry.isDir && ["srt", "ass", "ssa", "vtt"].contains(ext)
                        Button {
                            if entry.isDir {
                                subPath = entry.subPath
                                browseCurrent()
                            } else if !isSubtitle {
                                onDismiss()
                                onPlayUrl(entry.streamUrl, entry.name)
                            }
                        } label: {
                            HStack(spacing: 10) {
                                Image(systemName: entry.isDir ? "folder.fill" : (isSubtitle ? "captions.bubble" : "play.rectangle.fill"))
                                    .foregroundStyle(entry.isDir ? Color.accentColor : Color.secondary)
                                Text(entry.name)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                                Spacer()
                                if isSubtitle {
                                    Text(String(localized: "Subtitle"))
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                } else if !entry.isDir && entry.sizeBytes > 0 {
                                    Text(ByteCountFormatter.string(fromByteCount: Int64(entry.sizeBytes), countStyle: .file))
                                        .font(.caption.monospacedDigit())
                                        .foregroundStyle(.secondary)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(isSubtitle)
                    }
                    .listStyle(.inset)
                }
            }
        }
        .padding(20)
        .frame(width: 680, height: 500)
        .onAppear { reloadSources() }
    }

    private func reloadSources() {
        sources = (try? core.webdavSources()) ?? []
        if selectedSourceId == nil || !sources.contains(where: { $0.id == selectedSourceId }) {
            selectedSourceId = sources.first?.id
            subPath = ""
        }
        browseCurrent()
    }

    private func addSource() {
        errorText = nil
        do {
            let src = try core.addWebdavSource(
                name: newName,
                baseUrl: newUrl,
                username: newUser.isEmpty ? nil : newUser,
                password: newPassword.isEmpty ? nil : newPassword
            )
            newName = ""
            newUrl = ""
            newUser = ""
            newPassword = ""
            showAddForm = false
            selectedSourceId = src.id
            subPath = ""
            reloadSources()
        } catch {
            errorText = FriendlyError.explain(error)
        }
    }

    private func goUp() {
        var comps = subPath.split(separator: "/").map(String.init)
        if !comps.isEmpty { comps.removeLast() }
        subPath = comps.joined(separator: "/")
        browseCurrent()
    }

    private func browseCurrent() {
        guard let sid = selectedSourceId else {
            entries = []
            return
        }
        loading = true
        errorText = nil
        let core = core
        let path = subPath
        Task {
            do {
                let list = try await core.browseWebdav(sourceId: sid, subPath: path)
                await MainActor.run {
                    self.entries = list
                    self.loading = false
                }
            } catch {
                await MainActor.run {
                    self.errorText = FriendlyError.explain(error)
                    self.loading = false
                }
            }
        }
    }
}

/// Shared filter/refresh state between AppKit `LibraryViewController` and SwiftUI `LocalFilesLibraryView`.
@MainActor
final class LocalFilesFilterModel: ObservableObject {
    @Published var refreshToken = 0
    @Published var filterQuery = ""
    @Published var mediaFilter = 0
}

/// Embedded Local Media Library view shown inside `LibraryViewController` when "Local Files" is active (#N23).
struct LocalFilesLibraryView: View {
    let core: CinmaCore
    @ObservedObject var model: LocalFilesFilterModel
    let onPlayFile: (URL) -> Void
    let onOpenSettings: (() -> Void)?

    @State private var entries: [LocalMediaEntryInfo] = []
    @State private var folders: [String] = []
    @State private var scanning = false

    private var filteredEntries: [LocalMediaEntryInfo] {
        let q = model.filterQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return entries.filter { item in
            if model.mediaFilter == 1 && item.mediaType != "movie" { return false }
            if model.mediaFilter == 2 && item.mediaType != "series" { return false }
            if !q.isEmpty
                && !item.title.lowercased().contains(q)
                && !item.filename.lowercased().contains(q)
            {
                return false
            }
            return true
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Button {
                    addFolder()
                } label: {
                    Label(String(localized: "Add Folder…"), systemImage: "folder.badge.plus")
                }
                .buttonStyle(.bordered)

                Button {
                    scanNow()
                } label: {
                    Label(
                        scanning ? String(localized: "Scanning…") : String(localized: "Scan Folders"),
                        systemImage: "arrow.triangle.2.circlepath"
                    )
                }
                .buttonStyle(.borderedProminent)
                .disabled(scanning || folders.isEmpty)

                Text(String(localized: "\(folders.count) folders · \(filteredEntries.count) videos"))
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Spacer()

                if let onOpenSettings {
                    Button(String(localized: "Manage Folders…")) {
                        onOpenSettings()
                    }
                    .buttonStyle(.plain)
                    .font(.caption)
                    .foregroundStyle(Color.accentColor)
                }
            }
            .padding(.horizontal, 64)

            if folders.isEmpty {
                VStack(spacing: 10) {
                    Text(String(localized: "No local media folders configured yet."))
                        .font(.headline)
                        .foregroundStyle(.secondary)
                    Text(String(localized: "Add your Movies, Shows, or mounted NAS folders to browse and play offline files directly."))
                        .font(.subheadline)
                        .foregroundStyle(.tertiary)
                    Button(String(localized: "Add Folder…")) { addFolder() }
                        .buttonStyle(.borderedProminent)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if filteredEntries.isEmpty {
                VStack(spacing: 10) {
                    Text(String(localized: "No video files matched in configured folders."))
                        .foregroundStyle(.secondary)
                    Button(String(localized: "Scan Folders Now")) { scanNow() }
                        .buttonStyle(.bordered)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(filteredEntries, id: \.path) { item in
                            Button {
                                onPlayFile(URL(fileURLWithPath: item.path))
                            } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: item.mediaType == "series" ? "tv" : "film")
                                        .foregroundStyle(Color.accentColor)
                                        .frame(width: 24)
                                    VStack(alignment: .leading, spacing: 2) {
                                        HStack(spacing: 6) {
                                            Text(item.title)
                                                .font(.body.weight(.semibold))
                                                .foregroundStyle(.primary)
                                            if let s = item.season, let e = item.episode {
                                                Text(String(format: "S%02dE%02d", s, e))
                                                    .font(.caption.monospacedDigit().weight(.bold))
                                                    .padding(.horizontal, 6)
                                                    .padding(.vertical, 2)
                                                    .background(Color.accentColor.opacity(0.18), in: Capsule())
                                            }
                                            if let y = item.year {
                                                Text(y)
                                                    .font(.caption)
                                                    .foregroundStyle(.secondary)
                                            }
                                        }
                                        Text(item.filename)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                            .truncationMode(.middle)
                                    }
                                    Spacer()
                                    Text(ByteCountFormatter.string(fromByteCount: Int64(item.sizeBytes), countStyle: .file))
                                        .font(.caption.monospacedDigit())
                                        .foregroundStyle(.secondary)
                                    Image(systemName: "play.circle.fill")
                                        .font(.title3)
                                        .foregroundStyle(Color.accentColor)
                                }
                                .padding(.horizontal, 14)
                                .padding(.vertical, 10)
                                .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 12))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 64)
                    .padding(.bottom, 32)
                }
            }
        }
        .onAppear { reload() }
        .onChange(of: model.refreshToken) { _, _ in reload() }
    }

    private func reload() {
        folders = (try? core.playerPreferences().localMediaFolders) ?? []
        entries = (try? core.localMediaEntries()) ?? []
    }

    private func addFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.prompt = String(localized: "Add Media Folder")
        guard panel.runModal() == .OK else { return }
        guard var prefs = try? core.playerPreferences() else { return }
        for url in panel.urls where !prefs.localMediaFolders.contains(url.path) {
            prefs.localMediaFolders.append(url.path)
        }
        try? core.setPlayerPreferences(prefs: prefs)
        scanNow()
    }

    private func scanNow() {
        scanning = true
        folders = (try? core.playerPreferences().localMediaFolders) ?? []
        let core = core
        Task.detached(priority: .userInitiated) {
            let scanned = (try? core.scanLocalFolders()) ?? []
            await MainActor.run {
                entries = scanned
                scanning = false
            }
        }
    }
}
