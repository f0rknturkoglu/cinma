import AppKit

struct PosterStyleContext {
    var scalePct: UInt32 = 100
    var cornerRadius: CGFloat = 12
    var hideTitles: Bool = false
    var showImdbBadge: Bool = true
    var showTypeBadge: Bool = true
    var showWatchedBadge: Bool = true
    var showLibraryBadge: Bool = false
    var watchedIds: Set<String> = []
    var libraryIds: Set<String> = []

    var scaleFactor: CGFloat { CGFloat(max(80, min(125, scalePct))) / 100.0 }
    var artworkWidth: CGFloat { (168 * scaleFactor).rounded() }
    var artworkHeight: CGFloat { (252 * scaleFactor).rounded() }
    var itemSize: NSSize { NSSize(width: artworkWidth, height: artworkHeight + (hideTitles ? 4 : 50)) }

    static func load(from core: CinmaCore) -> PosterStyleContext {
        guard let prefs = try? core.playerPreferences() else { return PosterStyleContext() }
        let watched = prefs.showWatchedBadge ? Set((try? core.watchedMetaIds()) ?? []) : []
        let lib = prefs.showLibraryBadge ? Set((try? core.libraryMetaIds()) ?? []) : []
        return PosterStyleContext(
            scalePct: prefs.posterScalePct,
            cornerRadius: CGFloat(max(4, min(24, prefs.posterRadiusPt))),
            hideTitles: prefs.hidePosterTitles,
            showImdbBadge: prefs.showImdbBadge,
            showTypeBadge: prefs.showTypeBadge,
            showWatchedBadge: prefs.showWatchedBadge,
            showLibraryBadge: prefs.showLibraryBadge,
            watchedIds: watched,
            libraryIds: lib
        )
    }
}

/// Portrait poster card (`168×252` artwork + metadata line, matching mockup M0).
@MainActor
final class PosterCardItem: NSCollectionViewItem {
    static let identifier = NSUserInterfaceItemIdentifier("PosterCardItem")
    static let itemSize = NSSize(width: 168, height: 302)

    private let cardBox = NSView()
    private let gradientLayer = CAGradientLayer()
    private let artworkView = AspectFillImageView()
    private let fallbackTitle = NSTextField(wrappingLabelWithString: "")
    private let badgeBox = NSView()
    private let badgeLabel = NSTextField(labelWithString: "")
    private let statusBadgeBox = NSView()
    private let statusBadgeLabel = NSTextField(labelWithString: "")
    private let titleLabel = NSTextField(labelWithString: "")
    private let metaLabel = NSTextField(labelWithString: "")
    private var cardHeightConstraint: NSLayoutConstraint?
    private var loadTask: Task<Void, Never>?

    override func loadView() {
        let root = NSView(frame: NSRect(origin: .zero, size: Self.itemSize))

        cardBox.wantsLayer = true
        cardBox.layer?.cornerRadius = 12
        cardBox.layer?.masksToBounds = true
        cardBox.layer?.borderWidth = 1
        cardBox.layer?.borderColor = NSColor(white: 1, alpha: 0.08).cgColor
        gradientLayer.startPoint = CGPoint(x: 0, y: 1)
        gradientLayer.endPoint = CGPoint(x: 1, y: 0)
        cardBox.layer?.insertSublayer(gradientLayer, at: 0)
        cardBox.translatesAutoresizingMaskIntoConstraints = false

        artworkView.translatesAutoresizingMaskIntoConstraints = false

        let scrim = GradientLayerView(
            colors: [.clear, NSColor.black.withAlphaComponent(0.75)],
            start: CGPoint(x: 0.5, y: 0.6),
            end: CGPoint(x: 0.5, y: 0)
        )
        scrim.translatesAutoresizingMaskIntoConstraints = false

        fallbackTitle.font = .systemFont(ofSize: 14, weight: .heavy)
        fallbackTitle.textColor = CinemaTheme.textPrimary
        fallbackTitle.maximumNumberOfLines = 3
        fallbackTitle.translatesAutoresizingMaskIntoConstraints = false

        badgeBox.wantsLayer = true
        badgeBox.layer?.cornerRadius = 6
        badgeBox.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.55).cgColor
        badgeBox.layer?.borderWidth = 1
        badgeBox.layer?.borderColor = NSColor(white: 1, alpha: 0.20).cgColor
        badgeBox.translatesAutoresizingMaskIntoConstraints = false

        badgeLabel.font = .monospacedDigitSystemFont(ofSize: 10, weight: .bold)
        badgeLabel.textColor = CinemaTheme.textPrimary
        badgeLabel.translatesAutoresizingMaskIntoConstraints = false
        badgeBox.addSubview(badgeLabel)

        statusBadgeBox.wantsLayer = true
        statusBadgeBox.layer?.cornerRadius = 6
        statusBadgeBox.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.62).cgColor
        statusBadgeBox.layer?.borderWidth = 1
        statusBadgeBox.layer?.borderColor = NSColor(white: 1, alpha: 0.22).cgColor
        statusBadgeBox.translatesAutoresizingMaskIntoConstraints = false
        statusBadgeBox.isHidden = true

        statusBadgeLabel.font = .systemFont(ofSize: 10.5, weight: .bold)
        statusBadgeLabel.textColor = CinemaTheme.green
        statusBadgeLabel.translatesAutoresizingMaskIntoConstraints = false
        statusBadgeBox.addSubview(statusBadgeLabel)

        cardBox.addSubview(artworkView)
        cardBox.addSubview(scrim)
        cardBox.addSubview(fallbackTitle)
        cardBox.addSubview(badgeBox)
        cardBox.addSubview(statusBadgeBox)

        titleLabel.font = .systemFont(ofSize: 13.5, weight: .semibold)
        titleLabel.textColor = CinemaTheme.textPrimary
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.translatesAutoresizingMaskIntoConstraints = false

        metaLabel.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        metaLabel.textColor = CinemaTheme.textMuted
        metaLabel.lineBreakMode = .byTruncatingTail
        metaLabel.translatesAutoresizingMaskIntoConstraints = false

        root.addSubview(cardBox)
        root.addSubview(titleLabel)
        root.addSubview(metaLabel)

        let hConstraint = cardBox.heightAnchor.constraint(equalToConstant: 252)
        cardHeightConstraint = hConstraint

        NSLayoutConstraint.activate([
            cardBox.topAnchor.constraint(equalTo: root.topAnchor),
            cardBox.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            cardBox.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            hConstraint,

            artworkView.topAnchor.constraint(equalTo: cardBox.topAnchor),
            artworkView.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor),
            artworkView.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor),
            artworkView.bottomAnchor.constraint(equalTo: cardBox.bottomAnchor),

            scrim.topAnchor.constraint(equalTo: cardBox.topAnchor),
            scrim.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor),
            scrim.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor),
            scrim.bottomAnchor.constraint(equalTo: cardBox.bottomAnchor),

            fallbackTitle.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor, constant: 12),
            fallbackTitle.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor, constant: -12),
            fallbackTitle.bottomAnchor.constraint(equalTo: cardBox.bottomAnchor, constant: -14),

            badgeBox.topAnchor.constraint(equalTo: cardBox.topAnchor, constant: 10),
            badgeBox.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor, constant: -10),
            badgeLabel.leadingAnchor.constraint(equalTo: badgeBox.leadingAnchor, constant: 7),
            badgeLabel.trailingAnchor.constraint(equalTo: badgeBox.trailingAnchor, constant: -7),
            badgeLabel.topAnchor.constraint(equalTo: badgeBox.topAnchor, constant: 2),
            badgeLabel.bottomAnchor.constraint(equalTo: badgeBox.bottomAnchor, constant: -2),

            statusBadgeBox.topAnchor.constraint(equalTo: cardBox.topAnchor, constant: 10),
            statusBadgeBox.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor, constant: 10),
            statusBadgeLabel.leadingAnchor.constraint(equalTo: statusBadgeBox.leadingAnchor, constant: 7),
            statusBadgeLabel.trailingAnchor.constraint(equalTo: statusBadgeBox.trailingAnchor, constant: -7),
            statusBadgeLabel.topAnchor.constraint(equalTo: statusBadgeBox.topAnchor, constant: 2),
            statusBadgeLabel.bottomAnchor.constraint(equalTo: statusBadgeBox.bottomAnchor, constant: -2),

            titleLabel.topAnchor.constraint(equalTo: cardBox.bottomAnchor, constant: 8),
            titleLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            titleLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor),

            metaLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 2),
            metaLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            metaLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor),
        ])
        view = root
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        gradientLayer.frame = cardBox.bounds
        CATransaction.commit()
    }

    override var isSelected: Bool {
        didSet { updateSelectionRing() }
    }

    private func updateSelectionRing() {
        cardBox.layer?.borderWidth = isSelected ? 2.5 : 1
        cardBox.layer?.borderColor = isSelected
            ? CinemaTheme.accent.cgColor
            : NSColor(white: 1, alpha: 0.10).cgColor
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        loadTask?.cancel()
        loadTask = nil
        artworkView.image = nil
        updateSelectionRing()
    }

    func configure(with item: MetaPreviewInfo, style: PosterStyleContext = PosterStyleContext()) {
        cardBox.layer?.cornerRadius = style.cornerRadius
        cardHeightConstraint?.constant = style.artworkHeight
        gradientLayer.colors = ArtworkPlaceholder.gradient(for: item.id)
        titleLabel.stringValue = item.name
        titleLabel.isHidden = style.hideTitles
        fallbackTitle.stringValue = item.poster == nil ? item.name.uppercased() : ""
        fallbackTitle.isHidden = item.poster != nil

        let year = item.releaseInfo ?? ""
        let kind = style.showTypeBadge ? (item.mediaType == "series" ? String(localized: "Series") : String(localized: "Movie")) : ""
        metaLabel.stringValue = [year, kind].filter { !$0.isEmpty }.joined(separator: " · ")
        metaLabel.isHidden = style.hideTitles

        if style.showImdbBadge, let rating = item.imdbRating, !rating.isEmpty {
            badgeLabel.stringValue = "★ \(rating)"
            badgeBox.isHidden = false
        } else {
            badgeBox.isHidden = true
        }

        let isWatched = style.showWatchedBadge && style.watchedIds.contains(item.id)
        let inLib = style.showLibraryBadge && style.libraryIds.contains(item.id)
        var statusTokens: [String] = []
        if isWatched { statusTokens.append("✓") }
        if inLib { statusTokens.append("♥") }
        if statusTokens.isEmpty {
            statusBadgeBox.isHidden = true
        } else {
            statusBadgeLabel.stringValue = statusTokens.joined(separator: " ")
            statusBadgeLabel.textColor = isWatched ? CinemaTheme.green : CinemaTheme.accent
            statusBadgeBox.isHidden = false
        }

        artworkView.setRemoteImage(item.poster, pointSize: NSSize(width: style.artworkWidth, height: style.artworkHeight), taskStorage: &loadTask)
        view.setAccessibilityElement(true)
        view.setAccessibilityRole(.button)
        view.setAccessibilityTitle(item.name)
        view.setAccessibilityLabel("\(item.name), \(metaLabel.stringValue)")
        view.setAccessibilityHelp(String(localized: "Press Return to view sources"))
        updateSelectionRing()
    }
}

/// Landscape Continue Watching card (`300×168` + amber progress bar, matching mockup M0).
@MainActor
final class ContinueWatchingCardItem: NSCollectionViewItem {
    static let identifier = NSUserInterfaceItemIdentifier("ContinueWatchingCardItem")
    static let itemSize = NSSize(width: 300, height: 196)

    private let cardBox = NSView()
    private let gradientLayer = CAGradientLayer()
    private let artworkView = AspectFillImageView()
    private let titleLabel = NSTextField(labelWithString: "")
    private let progressTrack = NSView()
    private let progressFill = NSView()
    private var progressWidth: NSLayoutConstraint?
    private let metaLabel = NSTextField(labelWithString: "")
    private var loadTask: Task<Void, Never>?

    override func loadView() {
        let root = NSView(frame: NSRect(origin: .zero, size: Self.itemSize))

        cardBox.wantsLayer = true
        cardBox.layer?.cornerRadius = 14
        cardBox.layer?.masksToBounds = true
        cardBox.layer?.borderWidth = 1
        cardBox.layer?.borderColor = NSColor(white: 1, alpha: 0.08).cgColor
        gradientLayer.startPoint = CGPoint(x: 0, y: 1)
        gradientLayer.endPoint = CGPoint(x: 1, y: 0)
        cardBox.layer?.insertSublayer(gradientLayer, at: 0)
        cardBox.translatesAutoresizingMaskIntoConstraints = false

        artworkView.translatesAutoresizingMaskIntoConstraints = false

        let scrim = GradientLayerView(
            colors: [.clear, NSColor.black.withAlphaComponent(0.78)],
            start: CGPoint(x: 0.5, y: 0.55),
            end: CGPoint(x: 0.5, y: 0)
        )
        scrim.translatesAutoresizingMaskIntoConstraints = false

        titleLabel.font = .systemFont(ofSize: 16.5, weight: .heavy)
        titleLabel.textColor = CinemaTheme.textPrimary
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.translatesAutoresizingMaskIntoConstraints = false

        progressTrack.wantsLayer = true
        progressTrack.layer?.backgroundColor = NSColor(white: 1, alpha: 0.18).cgColor
        progressTrack.translatesAutoresizingMaskIntoConstraints = false

        progressFill.wantsLayer = true
        progressFill.layer?.backgroundColor = CinemaTheme.accent.cgColor
        progressFill.translatesAutoresizingMaskIntoConstraints = false
        progressTrack.addSubview(progressFill)

        cardBox.addSubview(artworkView)
        cardBox.addSubview(scrim)
        cardBox.addSubview(titleLabel)
        cardBox.addSubview(progressTrack)

        metaLabel.font = .monospacedDigitSystemFont(ofSize: 12.5, weight: .regular)
        metaLabel.textColor = CinemaTheme.textMuted
        metaLabel.lineBreakMode = .byTruncatingTail
        metaLabel.translatesAutoresizingMaskIntoConstraints = false

        root.addSubview(cardBox)
        root.addSubview(metaLabel)

        let widthConstraint = progressFill.widthAnchor.constraint(equalTo: progressTrack.widthAnchor, multiplier: 0.01)
        progressWidth = widthConstraint

        NSLayoutConstraint.activate([
            cardBox.topAnchor.constraint(equalTo: root.topAnchor),
            cardBox.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            cardBox.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            cardBox.heightAnchor.constraint(equalToConstant: 168),

            artworkView.topAnchor.constraint(equalTo: cardBox.topAnchor),
            artworkView.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor),
            artworkView.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor),
            artworkView.bottomAnchor.constraint(equalTo: cardBox.bottomAnchor),

            scrim.topAnchor.constraint(equalTo: cardBox.topAnchor),
            scrim.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor),
            scrim.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor),
            scrim.bottomAnchor.constraint(equalTo: cardBox.bottomAnchor),

            titleLabel.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor, constant: 14),
            titleLabel.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor, constant: -14),
            titleLabel.bottomAnchor.constraint(equalTo: progressTrack.topAnchor, constant: -12),

            progressTrack.leadingAnchor.constraint(equalTo: cardBox.leadingAnchor),
            progressTrack.trailingAnchor.constraint(equalTo: cardBox.trailingAnchor),
            progressTrack.bottomAnchor.constraint(equalTo: cardBox.bottomAnchor),
            progressTrack.heightAnchor.constraint(equalToConstant: 4),

            progressFill.leadingAnchor.constraint(equalTo: progressTrack.leadingAnchor),
            progressFill.topAnchor.constraint(equalTo: progressTrack.topAnchor),
            progressFill.bottomAnchor.constraint(equalTo: progressTrack.bottomAnchor),
            widthConstraint,

            metaLabel.topAnchor.constraint(equalTo: cardBox.bottomAnchor, constant: 8),
            metaLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            metaLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor),
        ])
        view = root
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        gradientLayer.frame = cardBox.bounds
        CATransaction.commit()
    }

    override var isSelected: Bool {
        didSet { updateSelectionRing() }
    }

    private func updateSelectionRing() {
        cardBox.layer?.borderWidth = isSelected ? 2.5 : 1
        cardBox.layer?.borderColor = isSelected
            ? CinemaTheme.accent.cgColor
            : NSColor(white: 1, alpha: 0.10).cgColor
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        loadTask?.cancel()
        loadTask = nil
        artworkView.image = nil
        updateSelectionRing()
    }

    func configure(with entry: WatchProgressInfo) {
        gradientLayer.colors = ArtworkPlaceholder.gradient(for: entry.metaId)
        titleLabel.stringValue = entry.title

        let ratio = entry.durationS > 0 ? min(max(entry.positionS / entry.durationS, 0.02), 1.0) : 0.02
        progressWidth?.isActive = false
        let updated = progressFill.widthAnchor.constraint(equalTo: progressTrack.widthAnchor, multiplier: ratio)
        updated.isActive = true
        progressWidth = updated

        let remainingMin = max(1, Int(((entry.durationS - entry.positionS) / 60).rounded()))
        let prefix: String
        if let s = entry.season, let e = entry.episode {
            prefix = "S\(s) · B\(e)"
        } else {
            prefix = String(localized: "Movie")
        }
        metaLabel.stringValue = String(localized: "\(prefix) · \(remainingMin) min left")

        artworkView.setRemoteImage(entry.artwork, pointSize: NSSize(width: 300, height: 168), taskStorage: &loadTask)
        view.setAccessibilityElement(true)
        view.setAccessibilityRole(.button)
        view.setAccessibilityTitle(entry.title)
        view.setAccessibilityLabel("\(entry.title), \(metaLabel.stringValue)")
        view.setAccessibilityHelp(String(localized: "Press Return to resume playback"))
        updateSelectionRing()
    }
}
