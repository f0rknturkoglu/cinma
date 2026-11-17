import AppKit
import SwiftUI

/// Unified Download Queue view (`NSTableView` + `NSTableViewDiffableDataSource`).
/// Supports live "Play Now" streaming via `Blob::source()`, pause/resume/remove,
/// disk quota usage indicator, and manual HTTP / `yt-dlp` queue entry.
@MainActor
final class DownloadsViewController: NSViewController, NSTableViewDelegate {
    private let core: CinmaCore
    private let onPlay: (PlayRequest) -> Void

    private let usageLabel = NSTextField(labelWithString: "")
    private let emptyLabel = NSTextField(labelWithString: "")
    private let tableView = NSTableView()
    private let scrollView = NSScrollView()

    private var dataSource: NSTableViewDiffableDataSource<Int, Int64>?
    private var rowsById: [Int64: DownloadInfo] = [:]

    private static let titleColId = NSUserInterfaceItemIdentifier("title")
    private static let stateColId = NSUserInterfaceItemIdentifier("state")
    private static let progressColId = NSUserInterfaceItemIdentifier("progress")
    private static let actionsColId = NSUserInterfaceItemIdentifier("actions")

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
        root.layer?.backgroundColor = CinemaTheme.background.cgColor

        let header = NSTextField(labelWithString: String(localized: "Downloads"))
        header.font = .systemFont(ofSize: 28, weight: .bold)
        header.textColor = CinemaTheme.textPrimary
        header.setAccessibilityLabel(String(localized: "Downloads"))

        usageLabel.font = .monospacedDigitSystemFont(ofSize: 12.5, weight: .medium)
        usageLabel.textColor = CinemaTheme.textSecondary

        let addBtn = CinemaTheme.makePillButton(
            title: String(localized: "Add URL / yt-dlp…"),
            symbol: "plus",
            primary: true,
            target: self,
            action: #selector(openAddSheet)
        )
        addBtn.setAccessibilityLabel(String(localized: "Add download by URL"))

        let trimBtn = CinemaTheme.makePillButton(
            title: String(localized: "Trim Cache"),
            symbol: "internaldrive",
            primary: false,
            target: self,
            action: #selector(trimCacheClicked)
        )
        trimBtn.setAccessibilityLabel(String(localized: "Trim disk cache to quota"))

        let topBar = NSStackView(views: [header, usageLabel, NSView(), trimBtn, addBtn])
        topBar.orientation = .horizontal
        topBar.alignment = .centerY
        topBar.spacing = 12
        topBar.translatesAutoresizingMaskIntoConstraints = false

        setupTable()
        scrollView.translatesAutoresizingMaskIntoConstraints = false
        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = false

        emptyLabel.stringValue = String(localized: "No downloads yet. Download streams from a title page or add an HTTP / yt-dlp URL.")
        emptyLabel.font = .systemFont(ofSize: 14)
        emptyLabel.textColor = CinemaTheme.textMuted
        emptyLabel.alignment = .center
        emptyLabel.translatesAutoresizingMaskIntoConstraints = false

        root.addSubview(topBar)
        root.addSubview(scrollView)
        root.addSubview(emptyLabel)

        NSLayoutConstraint.activate([
            topBar.topAnchor.constraint(equalTo: root.topAnchor, constant: 78),
            topBar.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 32),
            topBar.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -32),

            scrollView.topAnchor.constraint(equalTo: topBar.bottomAnchor, constant: 18),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 32),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -32),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -24),

            emptyLabel.centerXAnchor.constraint(equalTo: scrollView.centerXAnchor),
            emptyLabel.centerYAnchor.constraint(equalTo: scrollView.centerYAnchor),
            emptyLabel.widthAnchor.constraint(lessThanOrEqualToConstant: 480),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        configureDiffableDataSource()
        reload()
    }

    private func setupTable() {
        tableView.backgroundColor = .clear
        tableView.usesAlternatingRowBackgroundColors = false
        tableView.rowHeight = 58
        tableView.intercellSpacing = NSSize(width: 10, height: 6)
        tableView.headerView = NSTableHeaderView()
        tableView.delegate = self
        tableView.doubleAction = #selector(rowDoubleClicked)
        tableView.target = self
        tableView.setAccessibilityLabel(String(localized: "Download queue"))

        let cols: [(NSUserInterfaceItemIdentifier, String, CGFloat)] = [
            (Self.titleColId, String(localized: "Title"), 320),
            (Self.stateColId, String(localized: "Status"), 150),
            (Self.progressColId, String(localized: "Progress"), 210),
            (Self.actionsColId, String(localized: "Actions"), 250),
        ]
        for (id, title, width) in cols {
            let col = NSTableColumn(identifier: id)
            col.title = title
            col.width = width
            col.minWidth = 90
            tableView.addTableColumn(col)
        }
    }

    private func configureDiffableDataSource() {
        let ds = NSTableViewDiffableDataSource<Int, Int64>(tableView: tableView) { [weak self] table, column, _, itemId in
            guard let self, let info = self.rowsById[itemId] else { return NSView() }
            return self.cellView(for: info, column: column.identifier, in: table)
        }
        dataSource = ds
    }

    func reload() {
        let items = (try? core.downloads()) ?? []
        var byId: [Int64: DownloadInfo] = [:]
        for item in items { byId[item.id] = item }
        rowsById = byId

        var snapshot = NSDiffableDataSourceSnapshot<Int, Int64>()
        snapshot.appendSections([0])
        snapshot.appendItems(items.map(\.id), toSection: 0)
        dataSource?.apply(snapshot, animatingDifferences: !CinemaTheme.reduceMotion)

        emptyLabel.isHidden = !items.isEmpty
        refreshUsageSummary()
    }

    private func refreshUsageSummary() {
        if let usage = try? core.diskUsageSummary(),
           let settings = try? core.diskPolicySettings()
        {
            let used = ByteCountFormatter.string(fromByteCount: Int64(usage.totalBytes), countStyle: .file)
            let kept = ByteCountFormatter.string(fromByteCount: Int64(usage.keptBytes), countStyle: .file)
            let quotaText = settings.cacheQuotaMb == 0
                ? String(localized: "Unlimited")
                : ByteCountFormatter.string(fromByteCount: Int64(settings.cacheQuotaMb) * 1024 * 1024, countStyle: .file)
            usageLabel.stringValue = "\(used) / \(quotaText) · \(String(localized: "Kept")): \(kept)"
        }
    }

    private func cellView(for info: DownloadInfo, column: NSUserInterfaceItemIdentifier, in table: NSTableView) -> NSView {
        switch column {
        case Self.titleColId:
            let stack = NSStackView()
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 2
            let title = NSTextField(labelWithString: info.title)
            title.font = .systemFont(ofSize: 13.5, weight: .semibold)
            title.textColor = CinemaTheme.textPrimary
            title.lineBreakMode = .byTruncatingMiddle
            let sub = NSTextField(labelWithString: info.kind.uppercased())
            sub.font = .monospacedSystemFont(ofSize: 10.5, weight: .bold)
            sub.textColor = CinemaTheme.accent
            stack.addArrangedSubview(title)
            stack.addArrangedSubview(sub)
            return stack

        case Self.stateColId:
            let label = NSTextField(labelWithString: stateDisplayText(info))
            label.font = .systemFont(ofSize: 12.5, weight: .medium)
            label.textColor = info.state == "failed" ? .systemRed : CinemaTheme.textSecondary
            label.toolTip = info.error
            return label

        case Self.progressColId:
            let stack = NSStackView()
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 4
            let bar = NSProgressIndicator()
            bar.isIndeterminate = info.bytesTotal == nil && info.state == "active"
            bar.minValue = 0
            bar.maxValue = 1
            if let total = info.bytesTotal, total > 0 {
                bar.doubleValue = min(1.0, Double(info.bytesDone) / Double(total))
            } else {
                bar.doubleValue = info.state == "completed" ? 1.0 : 0.0
            }
            let doneStr = ByteCountFormatter.string(fromByteCount: Int64(info.bytesDone), countStyle: .file)
            let totStr = info.bytesTotal.map { ByteCountFormatter.string(fromByteCount: Int64($0), countStyle: .file) } ?? "—"
            let bytesLabel = NSTextField(labelWithString: "\(doneStr) / \(totStr)")
            bytesLabel.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
            bytesLabel.textColor = CinemaTheme.textMuted
            stack.addArrangedSubview(bar)
            stack.addArrangedSubview(bytesLabel)
            bar.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -8).isActive = true
            return stack

        case Self.actionsColId:
            var buttons: [NSView] = []
            if info.canPlayNow {
                let playBtn = CinemaTheme.makePillButton(
                    title: info.state == "completed" ? String(localized: "Play") : String(localized: "Play Now"),
                    symbol: "play.fill",
                    primary: true,
                    target: self,
                    action: #selector(playClicked(_:))
                )
                playBtn.tag = Int(info.id)
                playBtn.setAccessibilityLabel("\(String(localized: "Play")) \(info.title)")
                buttons.append(playBtn)
            }
            if info.state == "active" || info.state == "queued" {
                let pauseBtn = makeSmallButton("pause.fill", tooltip: String(localized: "Pause"), tag: info.id, action: #selector(pauseClicked(_:)))
                buttons.append(pauseBtn)
            } else if info.state == "paused" || info.state == "failed" {
                let resumeBtn = makeSmallButton("play.circle", tooltip: String(localized: "Resume"), tag: info.id, action: #selector(resumeClicked(_:)))
                buttons.append(resumeBtn)
            }
            let delBtn = makeSmallButton("trash", tooltip: String(localized: "Remove"), tag: info.id, action: #selector(removeClicked(_:)))
            buttons.append(delBtn)

            let rowStack = NSStackView(views: buttons)
            rowStack.orientation = .horizontal
            rowStack.spacing = 6
            return rowStack

        default:
            return NSView()
        }
    }

    private func makeSmallButton(_ symbol: String, tooltip: String, tag: Int64, action: Selector) -> NSButton {
        let btn = NSButton()
        btn.isBordered = false
        btn.image = NSImage(systemSymbolName: symbol, accessibilityDescription: tooltip)
        btn.contentTintColor = CinemaTheme.textPrimary
        btn.toolTip = tooltip
        btn.tag = Int(tag)
        btn.target = self
        btn.action = action
        btn.setAccessibilityLabel(tooltip)
        return btn
    }

    private func stateDisplayText(_ info: DownloadInfo) -> String {
        switch info.state {
        case "queued": String(localized: "Queued")
        case "active": String(localized: "Downloading")
        case "paused": String(localized: "Paused")
        case "completed": String(localized: "Ready Offline")
        case "failed": info.error ?? String(localized: "Failed")
        default: info.state
        }
    }

    @objc private func rowDoubleClicked() {
        let row = tableView.clickedRow
        guard row >= 0, let id = dataSource?.itemIdentifier(forRow: row), let info = rowsById[id], info.canPlayNow else {
            return
        }
        onPlay(.download(id: info.id, title: info.title))
    }

    @objc private func playClicked(_ sender: NSButton) {
        let id = Int64(sender.tag)
        guard let info = rowsById[id] else { return }
        onPlay(.download(id: info.id, title: info.title))
    }

    @objc private func pauseClicked(_ sender: NSButton) {
        try? core.pauseDownload(id: Int64(sender.tag))
        reload()
    }

    @objc private func resumeClicked(_ sender: NSButton) {
        try? core.resumeDownload(id: Int64(sender.tag))
        reload()
    }

    @objc private func removeClicked(_ sender: NSButton) {
        try? core.removeDownload(id: Int64(sender.tag), deleteData: true)
        reload()
    }

    @objc private func trimCacheClicked() {
        _ = try? core.enforceDiskQuota()
        refreshUsageSummary()
    }

    @objc private func openAddSheet() {
        var holder: NSHostingController<AddDownloadSheetView>?
        let vc = NSHostingController(rootView: AddDownloadSheetView(core: core) { [weak self] in
            if let vc = holder { self?.dismiss(vc) }
            self?.reload()
        })
        holder = vc
        presentAsSheet(vc)
    }
}

private struct AddDownloadSheetView: View {
    let core: CinmaCore
    let onDismiss: () -> Void

    @State private var title = ""
    @State private var url = ""
    @State private var mode = "http" // "http" | "ytdlp"
    @State private var maxHeight = 1080
    @State private var audioOnly = false
    @State private var errorText = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(String(localized: "Add Download"))
                    .font(.title2.bold())
                Spacer()
                Button(String(localized: "Close"), action: onDismiss)
            }

            Picker(String(localized: "Engine"), selection: $mode) {
                Text(String(localized: "Direct HTTP/S (Segmented)")).tag("http")
                Text(String(localized: "yt-dlp (Web / Video)")).tag("ytdlp")
            }
            .pickerStyle(.segmented)

            TextField(String(localized: "Title"), text: $title)
                .textFieldStyle(.roundedBorder)
            TextField("https://…", text: $url)
                .textFieldStyle(.roundedBorder)

            if mode == "ytdlp" {
                Toggle(String(localized: "Audio Only (M4A)"), isOn: $audioOnly)
                if !audioOnly {
                    Picker(String(localized: "Max Resolution"), selection: $maxHeight) {
                        Text("4K (2160p)").tag(2160)
                        Text("1080p").tag(1080)
                        Text("720p").tag(720)
                    }
                }
            }

            if !errorText.isEmpty {
                Text(errorText)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            HStack {
                Spacer()
                Button(String(localized: "Queue Download")) { enqueue() }
                    .buttonStyle(.borderedProminent)
                    .tint(.orange)
                    .disabled(url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 460)
    }

    private func enqueue() {
        let trimmedUrl = url.trimmingCharacters(in: .whitespacesAndNewlines)
        let label = title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? trimmedUrl : title.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            if mode == "ytdlp" {
                _ = try core.addYtdlpDownload(
                    title: label,
                    url: trimmedUrl,
                    maxHeight: audioOnly ? nil : UInt32(maxHeight),
                    audioOnly: audioOnly,
                    priority: 5
                )
            } else {
                _ = try core.addHttpDownload(
                    title: label,
                    url: trimmedUrl,
                    metaId: nil,
                    mediaType: nil,
                    priority: 5
                )
            }
            onDismiss()
        } catch {
            errorText = FriendlyError.explain(error)
        }
    }
}
