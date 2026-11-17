import AppKit
import UniformTypeIdentifiers

/// What the player should open.
enum PlayRequest {
    case stream(StreamInfo)
    case file(URL)
}

/// Phase 1 vertical-slice screen: install an addon, look up streams for an
/// id, play one. Replaced by the real browse UI in Phase 3.
@MainActor
final class SliceViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private let core: CinmaCore
    private let onPlay: (PlayRequest) -> Void

    private let addonField = NSTextField()
    private let addonsLabel = NSTextField(wrappingLabelWithString: "")
    private let typePopup = NSPopUpButton()
    private let idField = NSTextField()
    private let statusLabel = NSTextField(labelWithString: "")
    private let table = NSTableView()
    private var streams: [StreamInfo] = []

    init(core: CinmaCore, onPlay: @escaping (PlayRequest) -> Void) {
        self.core = core
        self.onPlay = onPlay
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = Theme.background.cgColor

        let title = NSTextField(labelWithString: "Cinma")
        title.font = .systemFont(ofSize: 28, weight: .bold)
        title.textColor = .white

        addonField.placeholderString = String(localized: "Addon manifest URL or stremio:// link")
        let installButton = NSButton(title: String(localized: "Install"), target: self, action: #selector(installAddon))
        addonField.target = self
        addonField.action = #selector(installAddon)
        let addonRow = row([addonField, installButton])

        addonsLabel.textColor = .secondaryLabelColor
        addonsLabel.font = .systemFont(ofSize: 12)

        typePopup.addItems(withTitles: ["movie", "series"])
        idField.placeholderString = String(localized: "IMDb id, e.g. tt1254207 or tt0944947:1:1")
        idField.target = self
        idField.action = #selector(findStreams)
        let findButton = NSButton(title: String(localized: "Find Streams"), target: self, action: #selector(findStreams))
        let openButton = NSButton(title: String(localized: "Open File…"), target: self, action: #selector(openFile))
        let queryRow = row([typePopup, idField, findButton, openButton])

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("stream"))
        column.resizingMask = .autoresizingMask
        table.addTableColumn(column)
        table.headerView = nil
        table.usesAutomaticRowHeights = true
        table.style = .inset
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(playSelected)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false

        statusLabel.textColor = .secondaryLabelColor
        statusLabel.font = .systemFont(ofSize: 12)
        statusLabel.lineBreakMode = .byTruncatingTail

        let stack = NSStackView(views: [title, addonRow, addonsLabel, queryRow, scroll, statusLabel])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 32),
            stack.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -32),
            stack.topAnchor.constraint(equalTo: root.topAnchor, constant: 48),
            stack.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -20),
            addonRow.widthAnchor.constraint(equalTo: stack.widthAnchor),
            queryRow.widthAnchor.constraint(equalTo: stack.widthAnchor),
            scroll.widthAnchor.constraint(equalTo: stack.widthAnchor),
            addonsLabel.widthAnchor.constraint(equalTo: stack.widthAnchor),
            statusLabel.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])
        scroll.setContentHuggingPriority(.defaultLow, for: .vertical)
        view = root
        refreshAddons()
    }

    private func row(_ views: [NSView]) -> NSStackView {
        let row = NSStackView(views: views)
        row.orientation = .horizontal
        row.spacing = 8
        views.first(where: { $0 is NSTextField })?.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return row
    }

    private func refreshAddons() {
        let names = ((try? core.addons()) ?? []).map(\.name)
        addonsLabel.stringValue = names.isEmpty
            ? String(localized: "No addons installed. Cinma ships without content sources; add one by URL.")
            : String(localized: "Addons: \(names.joined(separator: ", "))")
    }

    @objc private func installAddon() {
        let url = addonField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty else { return }
        statusLabel.stringValue = String(localized: "Installing addon…")
        let core = core
        Task {
            do {
                let addon = try await core.installAddon(url: url)
                statusLabel.stringValue = String(localized: "Installed \(addon.name) \(addon.version)")
                addonField.stringValue = ""
                refreshAddons()
            } catch {
                statusLabel.stringValue = String(localized: "Install failed: \(error.localizedDescription)")
            }
        }
    }

    @objc private func findStreams() {
        let id = idField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !id.isEmpty, let mediaType = typePopup.titleOfSelectedItem else { return }
        statusLabel.stringValue = String(localized: "Fetching streams…")
        let core = core
        Task {
            do {
                let found = try await core.streams(mediaType: mediaType, id: id)
                streams = found
                table.reloadData()
                statusLabel.stringValue = String(localized: "\(found.count) streams · double-click to play")
            } catch {
                streams = []
                table.reloadData()
                statusLabel.stringValue = String(localized: "Fetching streams failed: \(error.localizedDescription)")
            }
        }
    }

    @objc private func playSelected() {
        let row = table.clickedRow >= 0 ? table.clickedRow : table.selectedRow
        guard streams.indices.contains(row) else { return }
        onPlay(.stream(streams[row]))
    }

    @objc private func openFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.movie, .video, .audiovisualContent]
        panel.allowsOtherFileTypes = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        onPlay(.file(url))
    }

    // MARK: - Table

    func numberOfRows(in tableView: NSTableView) -> Int { streams.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let stream = streams[row]
        let label = NSTextField(wrappingLabelWithString: Self.describe(stream))
        label.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        label.textColor = .labelColor
        let cell = NSTableCellView()
        cell.addSubview(label)
        label.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 4),
            label.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            label.topAnchor.constraint(equalTo: cell.topAnchor, constant: 4),
            label.bottomAnchor.constraint(equalTo: cell.bottomAnchor, constant: -4),
        ])
        return cell
    }

    private static func describe(_ s: StreamInfo) -> String {
        let flat: (String?) -> String? = { $0?.replacingOccurrences(of: "\n", with: " · ") }
        var head = [s.addonName, flat(s.name)].compactMap { $0 }.joined(separator: " — ")
        if let size = s.videoSize {
            head += "  [" + ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file) + "]"
        }
        let kind = s.infoHash != nil ? "torrent" : (s.url != nil ? "url" : "?")
        let detail = flat(s.title) ?? s.filename ?? ""
        return "\(head)  (\(kind))\n\(detail)"
    }
}
