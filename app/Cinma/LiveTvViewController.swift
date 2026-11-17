import AppKit
import SwiftUI

/// Live TV / IPTV channel browser + XMLTV Now/Next EPG (`ROADMAP.md` §8 priority 9).
/// Constructed lazily only inside `MainShellViewController` when `Feature::Iptv` is active.
@MainActor
final class LiveTvViewController: NSViewController, NSSearchFieldDelegate {
    private let core: CinmaCore
    private let onPlayChannel: (PlayRequest) -> Void

    private let headerTitle = NSTextField(labelWithString: String(localized: "Live TV"))
    private let groupStack = NSStackView()
    private let searchField = NSSearchField()
    private let statusLabel = NSTextField(labelWithString: "")
    private let channelsStack = NSStackView()
    private let scrollView = NSScrollView()

    private var allChannels: [IptvChannelInfo] = []
    private var selectedGroup: String?
    private var queryText: String = ""

    init(core: CinmaCore, onPlayChannel: @escaping (PlayRequest) -> Void) {
        self.core = core
        self.onPlayChannel = onPlayChannel
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = CinemaTheme.background.cgColor

        headerTitle.font = .systemFont(ofSize: 28, weight: .bold)
        headerTitle.textColor = CinemaTheme.textPrimary

        let addBtn = CinemaTheme.makePillButton(
            title: String(localized: "Add Playlist…"),
            symbol: "plus",
            primary: true,
            target: self,
            action: #selector(openSourceSheet)
        )
        let epgBtn = CinemaTheme.makePillButton(
            title: String(localized: "Refresh EPG"),
            symbol: "arrow.clockwise",
            primary: false,
            target: self,
            action: #selector(refreshAllEpg)
        )

        searchField.placeholderString = String(localized: "Filter channels…")
        searchField.delegate = self
        searchField.translatesAutoresizingMaskIntoConstraints = false
        searchField.widthAnchor.constraint(equalToConstant: 220).isActive = true

        let topRow = NSStackView(views: [headerTitle, NSView(), searchField, epgBtn, addBtn])
        topRow.orientation = .horizontal
        topRow.alignment = .centerY
        topRow.spacing = 10
        topRow.translatesAutoresizingMaskIntoConstraints = false

        groupStack.orientation = .horizontal
        groupStack.spacing = 8
        groupStack.translatesAutoresizingMaskIntoConstraints = false

        let groupScroll = NSScrollView()
        groupScroll.drawsBackground = false
        groupScroll.hasHorizontalScroller = false
        groupScroll.documentView = groupStack
        groupScroll.translatesAutoresizingMaskIntoConstraints = false

        statusLabel.font = .systemFont(ofSize: 13)
        statusLabel.textColor = CinemaTheme.textMuted
        statusLabel.translatesAutoresizingMaskIntoConstraints = false

        channelsStack.orientation = .vertical
        channelsStack.alignment = .leading
        channelsStack.spacing = 10
        channelsStack.translatesAutoresizingMaskIntoConstraints = false

        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        let doc = NSView()
        doc.translatesAutoresizingMaskIntoConstraints = false
        doc.addSubview(channelsStack)
        scrollView.documentView = doc
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        root.addSubview(topRow)
        root.addSubview(groupScroll)
        root.addSubview(statusLabel)
        root.addSubview(scrollView)

        NSLayoutConstraint.activate([
            topRow.topAnchor.constraint(equalTo: root.topAnchor, constant: 86),
            topRow.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 48),
            topRow.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -48),

            groupScroll.topAnchor.constraint(equalTo: topRow.bottomAnchor, constant: 14),
            groupScroll.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 48),
            groupScroll.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -48),
            groupScroll.heightAnchor.constraint(equalToConstant: 34),

            statusLabel.topAnchor.constraint(equalTo: groupScroll.bottomAnchor, constant: 10),
            statusLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 48),
            statusLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -48),

            scrollView.topAnchor.constraint(equalTo: statusLabel.bottomAnchor, constant: 12),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 48),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -48),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -24),

            channelsStack.topAnchor.constraint(equalTo: doc.topAnchor),
            channelsStack.leadingAnchor.constraint(equalTo: doc.leadingAnchor),
            channelsStack.trailingAnchor.constraint(equalTo: doc.trailingAnchor),
            channelsStack.bottomAnchor.constraint(equalTo: doc.bottomAnchor),
            doc.widthAnchor.constraint(equalTo: scrollView.widthAnchor),
        ])
        view = root
    }

    func reload() {
        allChannels = (try? core.listIptvChannels(group: nil, query: nil)) ?? []
        rebuildGroups()
        renderChannels()
    }

    private func rebuildGroups() {
        for v in groupStack.arrangedSubviews {
            groupStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        let groups = Array(Set(allChannels.compactMap(\.groupTitle))).sorted()
        let allBtn = CinemaTheme.makePillButton(
            title: String(localized: "All Channels"),
            primary: selectedGroup == nil,
            target: self,
            action: #selector(selectGroup(_:))
        )
        allBtn.identifier = nil
        groupStack.addArrangedSubview(allBtn)
        for g in groups.prefix(20) {
            let b = CinemaTheme.makePillButton(
                title: g,
                primary: selectedGroup == g,
                target: self,
                action: #selector(selectGroup(_:))
            )
            b.identifier = NSUserInterfaceItemIdentifier(g)
            groupStack.addArrangedSubview(b)
        }
    }

    @objc private func selectGroup(_ sender: NSButton) {
        selectedGroup = sender.identifier?.rawValue
        rebuildGroups()
        renderChannels()
    }

    func controlTextDidChange(_ obj: Notification) {
        queryText = searchField.stringValue
        renderChannels()
    }

    private func renderChannels() {
        for v in channelsStack.arrangedSubviews {
            channelsStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        let filtered = (try? core.listIptvChannels(
            group: selectedGroup,
            query: queryText.isEmpty ? nil : queryText
        )) ?? []

        if filtered.isEmpty {
            statusLabel.stringValue = allChannels.isEmpty
                ? String(localized: "No IPTV playlists added yet. Click Add Playlist… to import an M3U URL or Xtream account.")
                : String(localized: "No channels match your filter.")
            return
        }
        statusLabel.stringValue = String(localized: "\(filtered.count) channels")

        for ch in filtered.prefix(200) {
            let row = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 14)
            row.translatesAutoresizingMaskIntoConstraints = false

            let title = NSTextField(labelWithString: ch.name)
            title.font = .systemFont(ofSize: 15, weight: .semibold)
            title.textColor = CinemaTheme.textPrimary

            var subParts: [String] = []
            if let g = ch.groupTitle { subParts.append(g) }
            if let epgId = ch.epgChannelId,
               let epg = (try? core.iptvEpgNowNext(epgChannelId: epgId)),
               let now = epg.first
            {
                subParts.append("\(String(localized: "Now")): \(now.title)")
                if epg.count > 1 {
                    subParts.append("\(String(localized: "Next")): \(epg[1].title)")
                }
            }
            let sub = NSTextField(labelWithString: subParts.joined(separator: " · "))
            sub.font = .systemFont(ofSize: 12.5)
            sub.textColor = CinemaTheme.textSecondary
            sub.lineBreakMode = .byTruncatingTail

            let info = NSStackView(views: [title, sub])
            info.orientation = .vertical
            info.alignment = .leading
            info.spacing = 3

            let favBtn = CinemaTheme.makePillButton(
                title: "",
                symbol: ch.isFavorite ? "star.fill" : "star",
                primary: ch.isFavorite,
                target: self,
                action: #selector(toggleFavorite(_:))
            )
            favBtn.tag = Int(ch.id)
            favBtn.identifier = NSUserInterfaceItemIdentifier(ch.isFavorite ? "1" : "0")

            let playBtn = CinemaTheme.makePillButton(
                title: String(localized: "Watch"),
                symbol: "play.fill",
                primary: true,
                target: self,
                action: #selector(playChannel(_:))
            )
            playBtn.identifier = NSUserInterfaceItemIdentifier(ch.streamUrl)
            playBtn.toolTip = ch.name

            let h = NSStackView(views: [info, NSView(), favBtn, playBtn])
            h.orientation = .horizontal
            h.alignment = .centerY
            h.spacing = 10
            h.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(h)

            NSLayoutConstraint.activate([
                row.heightAnchor.constraint(equalToConstant: 64),
                h.topAnchor.constraint(equalTo: row.topAnchor, constant: 10),
                h.bottomAnchor.constraint(equalTo: row.bottomAnchor, constant: -10),
                h.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: 14),
                h.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -14),
            ])
            channelsStack.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: channelsStack.widthAnchor).isActive = true
        }
    }

    @objc private func toggleFavorite(_ sender: NSButton) {
        let current = sender.identifier?.rawValue == "1"
        try? core.setIptvFavorite(channelId: Int64(sender.tag), favorite: !current)
        reload()
    }

    @objc private func playChannel(_ sender: NSButton) {
        guard let urlStr = sender.identifier?.rawValue, let url = URL(string: urlStr) else { return }
        onPlayChannel(.file(url))
    }

    @objc private func refreshAllEpg() {
        let sources = (try? core.listIptvSources()) ?? []
        let epgUrls = sources.compactMap(\.epgUrl).filter { !$0.isEmpty }
        guard !epgUrls.isEmpty else {
            statusLabel.stringValue = String(localized: "No XMLTV EPG URL configured on current sources.")
            return
        }
        statusLabel.stringValue = String(localized: "Refreshing EPG…")
        Task {
            var total: UInt32 = 0
            for url in epgUrls {
                total += (try? await core.refreshXmltv(url: url)) ?? 0
            }
            statusLabel.stringValue = String(localized: "Updated \(total) EPG programmes")
            renderChannels()
        }
    }

    @objc private func openSourceSheet() {
        let vc = NSHostingController(rootView: IptvSourcesSheetView(core: core) { [weak self] in
            if let sheet = self?.presentedViewControllers?.first {
                self?.dismiss(sheet)
            }
            self?.reload()
        })
        presentAsSheet(vc)
    }
}

private struct IptvSourcesSheetView: View {
    let core: CinmaCore
    let onDismiss: () -> Void

    @State private var mode = "m3u"
    @State private var name = "Live TV"
    @State private var url = ""
    @State private var epgUrl = ""
    @State private var username = ""
    @State private var password = ""
    @State private var sources: [IptvSourceInfo] = []
    @State private var status = ""
    @State private var busy = false

    var body: some View {
        Form {
            Section(String(localized: "Add IPTV Source")) {
                Picker(String(localized: "Type"), selection: $mode) {
                    Text("M3U / M3U8 URL").tag("m3u")
                    Text("Xtream Codes").tag("xtream")
                }
                TextField(String(localized: "Name"), text: $name)
                TextField(mode == "m3u" ? "https://example.com/playlist.m3u8" : "http://host:port", text: $url)
                if mode == "m3u" {
                    TextField(String(localized: "XMLTV EPG URL (optional)"), text: $epgUrl)
                } else {
                    TextField(String(localized: "Username"), text: $username)
                    SecureField(String(localized: "Password"), text: $password)
                }
                Button(busy ? String(localized: "Importing…") : String(localized: "Import Channels")) {
                    importSource()
                }
                .disabled(busy || url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }

            Section(String(localized: "Configured Sources")) {
                if sources.isEmpty {
                    Text(String(localized: "No IPTV sources configured."))
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(sources, id: \.id) { src in
                        HStack {
                            VStack(alignment: .leading) {
                                Text("\(src.name) (\(src.kind.uppercased()))").font(.headline)
                                Text(src.url).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                            }
                            Spacer()
                            Button(role: .destructive) {
                                try? core.removeIptvSource(sourceId: src.id)
                                reloadSources()
                            } label: {
                                Image(systemName: "trash")
                            }
                        }
                    }
                }
            }

            if !status.isEmpty {
                Text(status).font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button(String(localized: "Close"), action: onDismiss)
            }
        }
        .frame(width: 540, height: 460)
        .onAppear(perform: reloadSources)
    }

    private func reloadSources() {
        sources = (try? core.listIptvSources()) ?? []
    }

    private func importSource() {
        busy = true
        status = ""
        Task {
            defer { busy = false; reloadSources() }
            do {
                let n: UInt32
                if mode == "m3u" {
                    n = try await core.addM3uSource(
                        name: name,
                        url: url,
                        epgUrl: epgUrl.isEmpty ? nil : epgUrl
                    )
                } else {
                    n = try await core.addXtreamSource(
                        name: name,
                        baseUrl: url,
                        username: username,
                        password: password
                    )
                }
                status = String(localized: "Imported \(n) channels")
            } catch {
                status = FriendlyError.explain(error)
            }
        }
    }
}
