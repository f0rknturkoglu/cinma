import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// Forwards Rust playback events (background thread) to a handler.
final class PlaybackEventSink: PlaybackObserver {
    private let handler: @Sendable (PlaybackEvent) -> Void

    init(_ handler: @escaping @Sendable (PlaybackEvent) -> Void) {
        self.handler = handler
    }

    func onEvent(event: PlaybackEvent) {
        handler(event)
    }
}

private final class PlayerRootView: NSView {
    var onKey: ((NSEvent) -> Bool)?
    var onMouseActivity: (() -> Void)?
    private var tracking: NSTrackingArea?

    override var acceptsFirstResponder: Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseMoved, .mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        tracking = area
    }

    override func mouseMoved(with event: NSEvent) {
        onMouseActivity?()
        super.mouseMoved(with: event)
    }

    override func mouseDown(with event: NSEvent) {
        onMouseActivity?()
        super.mouseDown(with: event)
    }

    override func keyDown(with event: NSEvent) {
        if onKey?(event) != true { super.keyDown(with: event) }
    }
}

private final class ClickCatcherView: NSView {
    var onClick: (() -> Void)?

    override func mouseDown(with event: NSEvent) {
        onClick?()
    }
}

/// Complete cinema player (`CLAUDE.md` §5.2, §5.8, §5.9): dedicated CA GL thread,
/// SDR 8-bit optimization, hover thumbnails, dual/encoded subtitles, chapters,
/// next-episode prefetch/countdown, stream switcher & auto-recovery, stats/AB/sleep/shaders,
/// media keys & power assertion.
@MainActor
final class PlayerViewController: NSViewController {
    private let core: CinmaCore
    private var request: PlayRequest
    private let onClose: () -> Void

    private let videoView = NSView()
    private let videoLayer = VideoLayer()
    private let titlePill = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 22)
    private let infoLabel = NSTextField(labelWithString: "")
    private let titleField = NSTextField(labelWithString: "")
    private let subField = NSTextField(labelWithString: "")
    private let bar = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 22)
    private let statusPill = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 14)
    private let statusLabel = NSTextField(labelWithString: "")
    private let scrubBar = ScrubBarView()
    private let playButton = NSButton()
    private let speedButton = NSButton()
    private let volumeButton = NSButton()
    private let timeLabel = NSTextField(labelWithString: "--:-- / --:--")

    // Hover thumbnail preview
    private let thumbCard = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 12)
    private let thumbImageView = NSImageView()
    private let thumbTimeLabel = NSTextField(labelWithString: "00:00")
    private var thumbCenterXConstraint: NSLayoutConstraint?

    // Stats overlay & Skip intro / Next episode cards
    private let statsCard = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 14)
    private let statsText = NSTextField(wrappingLabelWithString: "")
    private lazy var skipButton = CinemaTheme.makePillButton(title: "", symbol: "forward.end.fill", primary: true, target: self, action: #selector(skipCurrentSegment))
    private let nextCard = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 16)
    private let nextTitleLabel = NSTextField(labelWithString: "")
    private let nextSubtitleLabel = NSTextField(labelWithString: "")
    private let nextCountdownLabel = NSTextField(labelWithString: "")

    private var session: PlaybackSession?
    private var thumbCache: ThumbnailPreviewCache?
    private let nowPlaying = NowPlayingBridge()

    private var duration: Double = 0
    private var position: Double = 0
    private var paused = false
    private var muted = false
    private var volume: Double = 100
    private var speed: Double = 1.0
    private var subDelay: Double = 0
    private var audioDelay: Double = 0
    private var subtitlesEnabled = true
    private var lastSelectedSid: Int64?
    private var secondarySid: Int64?
    private var abLoopA: Double?
    private var abLoopB: Double?
    private var tracks: [MediaTrackInfo] = []
    private var chapters: [ChapterInfo] = []
    private var apiSkipSegments: [SkipSegmentInfo] = []
    private var appliedAutoTracks = false
    private var onlineSubtitles: [SubtitleCandidateInfo] = []
    private var searchingSubtitles = false
    private var subtitleSearchStatus = ""
    private var prefetchedTriggered = false
    private var nextPlan: NextEpisodePrefetchInfo?
    private var nextCountdownTask: Task<Void, Never>?
    private var nextDismissed = false
    private var loadingNextEpisode = false
    private var sessionAudioLang: String?
    private var sessionAudioTitle: String?
    private var sessionSubEnabled: Bool?
    private var sessionSubLang: String?
    private var sessionSubTitle: String?
    private var sessionSecondarySubEnabled: Bool?
    private var sessionSecondarySubLang: String?
    private var sessionSubDelay: Double?
    private var sessionAudioDelay: Double?
    private var statsVisible = false
    private var statsTask: Task<Void, Never>?
    private var hudHideTask: Task<Void, Never>?
    private var statusClearTask: Task<Void, Never>?
    private var popoverBackdrop: ClickCatcherView?
    private var popoverCard: NSView?
    private var didTrimInitialMalloc = false
    private var sleepTimerTask: Task<Void, Never>?
    private var stallRecoveryTask: Task<Void, Never>?
    private var failedRetries = 0
    private var currentStreamIndex = 0
    private var pendingSwitchPosition: Double?
    private var isHoveringScrub = false

    var onEnterPiP: (() -> Void)?
    var onPauseStateChanged: ((Bool) -> Void)?
    var isPausedForPiP: Bool { paused }

    private var closing = false
    private var appliedResume = false
    private var lastSavedAt: ContinuousClock.Instant?
    private var shutdownWaiter: CheckedContinuation<Void, Never>?
    private var pendingSeeks = DevAutomation.autoSeeks
    private var closeRequestedAt: ContinuousClock.Instant?

    private var metadata: PlaybackMetadata? {
        if case .stream(_, let meta) = request { return meta }
        return nil
    }

    private var activeStreamRaw: String? {
        if case .stream(let s, _) = request { return s.raw }
        return nil
    }

    init(core: CinmaCore, request: PlayRequest, onClose: @escaping () -> Void) {
        self.core = core
        self.request = request
        self.onClose = onClose
        if case .file(let url) = request {
            self.thumbCache = ThumbnailPreviewCache(localURL: url)
        } else {
            self.thumbCache = ThumbnailPreviewCache(localURL: nil)
        }
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = PlayerRootView()
        root.wantsLayer = true
        root.layer?.backgroundColor = NSColor.black.cgColor
        root.onKey = { [weak self] event in self?.handleKey(event) ?? false }
        root.onMouseActivity = { [weak self] in self?.showHudAndResetTimer() }

        videoView.wantsLayer = true
        videoView.layer?.backgroundColor = NSColor.black.cgColor
        videoView.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(videoView)

        buildTopBars()
        buildBottomBar()
        buildThumbPreview()
        buildStatsCard()
        buildSkipAndNextCards()

        statusPill.translatesAutoresizingMaskIntoConstraints = false
        statusPill.isHidden = true
        statusLabel.textColor = CinemaTheme.textPrimary
        statusLabel.font = .systemFont(ofSize: 13, weight: .medium)
        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        statusLabel.lineBreakMode = .byTruncatingTail
        statusPill.addSubview(statusLabel)

        for sub in [titlePill, bar, thumbCard, statsCard, skipButton, nextCard, statusPill] {
            root.addSubview(sub)
        }

        let thumbAnchor = thumbCard.centerXAnchor.constraint(equalTo: bar.leadingAnchor, constant: 100)
        thumbAnchor.priority = .defaultHigh
        thumbCenterXConstraint = thumbAnchor

        NSLayoutConstraint.activate([
            videoView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            videoView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            videoView.topAnchor.constraint(equalTo: root.topAnchor),
            videoView.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            titlePill.topAnchor.constraint(equalTo: root.topAnchor, constant: 22),
            titlePill.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 88),
            titlePill.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -24),

            bar.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 24),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -24),
            bar.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -20),

            thumbAnchor,
            thumbCard.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 24),
            thumbCard.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -24),
            thumbCard.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -10),

            statsCard.topAnchor.constraint(equalTo: titlePill.bottomAnchor, constant: 12),
            statsCard.leadingAnchor.constraint(equalTo: titlePill.leadingAnchor),

            skipButton.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -28),
            skipButton.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -16),
            skipButton.heightAnchor.constraint(equalToConstant: 36),

            nextCard.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -28),
            nextCard.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -16),
            nextCard.widthAnchor.constraint(equalToConstant: 320),

            statusLabel.topAnchor.constraint(equalTo: statusPill.topAnchor, constant: 7),
            statusLabel.bottomAnchor.constraint(equalTo: statusPill.bottomAnchor, constant: -7),
            statusLabel.leadingAnchor.constraint(equalTo: statusPill.leadingAnchor, constant: 14),
            statusLabel.trailingAnchor.constraint(equalTo: statusPill.trailingAnchor, constant: -14),

            statusPill.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            statusPill.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -14),
            statusPill.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 24),
            statusPill.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -24),
        ])
        view = root
    }

    private func buildTopBars() {
        titlePill.translatesAutoresizingMaskIntoConstraints = false
        let backBtn = NSButton(
            image: NSImage(systemSymbolName: "chevron.left", accessibilityDescription: String(localized: "Close")) ?? NSImage(),
            target: self,
            action: #selector(close)
        )
        backBtn.isBordered = false
        backBtn.contentTintColor = CinemaTheme.textPrimary
        backBtn.wantsLayer = true
        backBtn.layer?.cornerRadius = 16
        backBtn.layer?.backgroundColor = NSColor(white: 1, alpha: 0.10).cgColor
        backBtn.translatesAutoresizingMaskIntoConstraints = false
        backBtn.widthAnchor.constraint(equalToConstant: 32).isActive = true
        backBtn.heightAnchor.constraint(equalToConstant: 32).isActive = true

        refreshTitles()
        titleField.font = .systemFont(ofSize: 13.5, weight: .bold)
        titleField.textColor = CinemaTheme.textPrimary
        titleField.lineBreakMode = .byTruncatingTail
        subField.font = .systemFont(ofSize: 11.5, weight: .regular)
        subField.textColor = CinemaTheme.textSecondary
        subField.lineBreakMode = .byTruncatingTail

        let titleCol = NSStackView(views: [titleField, subField])
        titleCol.orientation = .vertical
        titleCol.alignment = .leading
        titleCol.spacing = 1

        let leftStack = NSStackView(views: [backBtn, titleCol])
        leftStack.orientation = .horizontal
        leftStack.alignment = .centerY
        leftStack.spacing = 10
        leftStack.translatesAutoresizingMaskIntoConstraints = false
        titlePill.addSubview(leftStack)
        NSLayoutConstraint.activate([
            leftStack.topAnchor.constraint(equalTo: titlePill.topAnchor, constant: 6),
            leftStack.bottomAnchor.constraint(equalTo: titlePill.bottomAnchor, constant: -6),
            leftStack.leadingAnchor.constraint(equalTo: titlePill.leadingAnchor, constant: 6),
            leftStack.trailingAnchor.constraint(equalTo: titlePill.trailingAnchor, constant: -16),
        ])
    }

    private func buildBottomBar() {
        bar.translatesAutoresizingMaskIntoConstraints = false

        scrubBar.translatesAutoresizingMaskIntoConstraints = false
        scrubBar.heightAnchor.constraint(equalToConstant: 20).isActive = true
        scrubBar.onSeek = { [weak self] secs in self?.seekAbsolute(secs) }
        scrubBar.onHover = { [weak self] secs, localX in
            self?.handleScrubHover(seconds: secs, localX: localX)
        }

        let back10 = makeIconButton("gobackward.10", tooltip: String(localized: "Seek back 10s"), action: #selector(seekBack10))
        let fwd10 = makeIconButton("goforward.10", tooltip: String(localized: "Seek forward 10s"), action: #selector(seekForward10))
        playButton.isBordered = false
        playButton.contentTintColor = CinemaTheme.textPrimary
        playButton.image = NSImage(systemSymbolName: "pause.fill", accessibilityDescription: String(localized: "Pause"))
        playButton.target = self
        playButton.action = #selector(togglePause)
        playButton.translatesAutoresizingMaskIntoConstraints = false
        playButton.widthAnchor.constraint(equalToConstant: 30).isActive = true
        playButton.heightAnchor.constraint(equalToConstant: 28).isActive = true

        timeLabel.font = .monospacedDigitSystemFont(ofSize: 12.5, weight: .semibold)
        timeLabel.textColor = CinemaTheme.textPrimary
        timeLabel.setContentCompressionResistancePriority(.required, for: .horizontal)

        infoLabel.font = .monospacedDigitSystemFont(ofSize: 11, weight: .medium)
        infoLabel.textColor = CinemaTheme.textSecondary
        infoLabel.lineBreakMode = .byTruncatingTail
        infoLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        infoLabel.setContentHuggingPriority(.defaultLow, for: .horizontal)

        speedButton.title = "1.0×"
        speedButton.isBordered = false
        speedButton.font = .monospacedDigitSystemFont(ofSize: 12, weight: .semibold)
        speedButton.contentTintColor = CinemaTheme.textPrimary
        speedButton.target = self
        speedButton.action = #selector(showSpeedMenu(_:))

        volumeButton.isBordered = false
        volumeButton.image = NSImage(systemSymbolName: "speaker.wave.2.fill", accessibilityDescription: String(localized: "Mute"))
        volumeButton.contentTintColor = CinemaTheme.textPrimary
        volumeButton.target = self
        volumeButton.action = #selector(toggleMute)
        volumeButton.translatesAutoresizingMaskIntoConstraints = false
        volumeButton.widthAnchor.constraint(equalToConstant: 28).isActive = true
        volumeButton.heightAnchor.constraint(equalToConstant: 28).isActive = true

        let subBtn = makeIconButton("captions.bubble", tooltip: String(localized: "Subtitles"), action: #selector(showSubtitlesPopover(_:)))
        let audioBtn = makeIconButton("waveform", tooltip: String(localized: "Audio Track & Delay"), action: #selector(showAudioMenu(_:)))
        let chapBtn = makeIconButton("list.bullet.rectangle", tooltip: String(localized: "Chapters"), action: #selector(showChaptersMenu(_:)))
        let globeBtn = makeIconButton("globe", tooltip: String(localized: "Streaming Details"), action: #selector(showStreamDetailsPopover(_:)))
        let streamsBtn = makeIconButton("rectangle.stack", tooltip: String(localized: "Switch Stream"), action: #selector(showStreamSwitcher(_:)))
        let castBtn = makeIconButton("tv.and.mediabox", tooltip: String(localized: "Cast to LAN Device"), action: #selector(openCastSheet(_:)))
        let pipBtn = makeIconButton("pip.enter", tooltip: String(localized: "Mini Player (P)"), action: #selector(enterPiPClicked))
        let statsBtn = makeIconButton("info.circle", tooltip: String(localized: "Playback Stats (I)"), action: #selector(toggleStats))
        let shotBtn = makeIconButton("camera", tooltip: String(localized: "Screenshot (S)"), action: #selector(takeScreenshot))
        let moreBtn = makeIconButton("ellipsis.circle", tooltip: String(localized: "Video, A/B Loop & Sleep Timer"), action: #selector(showMoreMenu(_:)))
        let fsBtn = makeIconButton("arrow.up.left.and.arrow.down.right", tooltip: String(localized: "Fullscreen (F)"), action: #selector(toggleFullscreen))

        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        spacer.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        // All player controls consolidated in the bottom glass bar (Harbor / Stremio layout).
        let transportRow = NSStackView(views: [
            back10, playButton, fwd10, timeLabel, infoLabel,
            spacer,
            volumeButton, speedButton, subBtn, audioBtn, chapBtn,
            globeBtn, streamsBtn, castBtn, pipBtn, statsBtn, shotBtn, moreBtn, fsBtn,
        ])
        transportRow.orientation = .horizontal
        transportRow.alignment = .centerY
        transportRow.spacing = 8

        let barStack = NSStackView(views: [scrubBar, transportRow])
        barStack.orientation = .vertical
        barStack.alignment = .leading
        barStack.spacing = 8
        barStack.translatesAutoresizingMaskIntoConstraints = false
        bar.addSubview(barStack)
        NSLayoutConstraint.activate([
            barStack.topAnchor.constraint(equalTo: bar.topAnchor, constant: 12),
            barStack.bottomAnchor.constraint(equalTo: bar.bottomAnchor, constant: -12),
            barStack.leadingAnchor.constraint(equalTo: bar.leadingAnchor, constant: 18),
            barStack.trailingAnchor.constraint(equalTo: bar.trailingAnchor, constant: -18),
            scrubBar.widthAnchor.constraint(equalTo: barStack.widthAnchor),
            transportRow.widthAnchor.constraint(equalTo: barStack.widthAnchor),
        ])
    }

    private func buildThumbPreview() {
        thumbCard.translatesAutoresizingMaskIntoConstraints = false
        thumbCard.isHidden = true

        thumbImageView.imageScaling = .scaleProportionallyUpOrDown
        thumbImageView.wantsLayer = true
        thumbImageView.layer?.cornerRadius = 8
        thumbImageView.layer?.masksToBounds = true
        thumbImageView.layer?.backgroundColor = NSColor(white: 1, alpha: 0.06).cgColor
        thumbImageView.translatesAutoresizingMaskIntoConstraints = false
        thumbImageView.widthAnchor.constraint(equalToConstant: 160).isActive = true
        thumbImageView.heightAnchor.constraint(equalToConstant: 90).isActive = true

        thumbTimeLabel.font = .monospacedDigitSystemFont(ofSize: 11.5, weight: .semibold)
        thumbTimeLabel.textColor = CinemaTheme.textPrimary

        let col = NSStackView(views: [thumbImageView, thumbTimeLabel])
        col.orientation = .vertical
        col.alignment = .centerX
        col.spacing = 4
        col.translatesAutoresizingMaskIntoConstraints = false
        thumbCard.addSubview(col)
        NSLayoutConstraint.activate([
            col.topAnchor.constraint(equalTo: thumbCard.topAnchor, constant: 6),
            col.bottomAnchor.constraint(equalTo: thumbCard.bottomAnchor, constant: -6),
            col.leadingAnchor.constraint(equalTo: thumbCard.leadingAnchor, constant: 6),
            col.trailingAnchor.constraint(equalTo: thumbCard.trailingAnchor, constant: -6),
        ])
    }

    private func buildStatsCard() {
        statsCard.translatesAutoresizingMaskIntoConstraints = false
        statsCard.isHidden = true
        statsText.font = .monospacedSystemFont(ofSize: 11.5, weight: .regular)
        statsText.textColor = CinemaTheme.textPrimary
        statsText.preferredMaxLayoutWidth = 360
        statsText.setContentHuggingPriority(.required, for: .horizontal)
        statsText.setContentCompressionResistancePriority(.required, for: .horizontal)
        statsText.translatesAutoresizingMaskIntoConstraints = false
        statsCard.addSubview(statsText)
        NSLayoutConstraint.activate([
            statsCard.widthAnchor.constraint(greaterThanOrEqualToConstant: 260),
            statsText.topAnchor.constraint(equalTo: statsCard.topAnchor, constant: 10),
            statsText.bottomAnchor.constraint(equalTo: statsCard.bottomAnchor, constant: -10),
            statsText.leadingAnchor.constraint(equalTo: statsCard.leadingAnchor, constant: 12),
            statsText.trailingAnchor.constraint(equalTo: statsCard.trailingAnchor, constant: -12),
            statsText.widthAnchor.constraint(lessThanOrEqualToConstant: 380),
        ])
    }

    private func buildSkipAndNextCards() {
        skipButton.translatesAutoresizingMaskIntoConstraints = false
        skipButton.target = self
        skipButton.action = #selector(skipCurrentSegment)
        skipButton.isHidden = true

        nextCard.translatesAutoresizingMaskIntoConstraints = false
        nextCard.isHidden = true

        let header = NSTextField(labelWithString: String(localized: "UP NEXT"))
        header.font = .systemFont(ofSize: 10.5, weight: .bold)
        header.textColor = CinemaTheme.accent

        nextTitleLabel.font = .systemFont(ofSize: 14, weight: .bold)
        nextTitleLabel.textColor = CinemaTheme.textPrimary
        nextTitleLabel.lineBreakMode = .byTruncatingTail

        nextSubtitleLabel.font = .systemFont(ofSize: 12, weight: .regular)
        nextSubtitleLabel.textColor = CinemaTheme.textSecondary
        nextSubtitleLabel.lineBreakMode = .byTruncatingTail

        nextCountdownLabel.font = .monospacedDigitSystemFont(ofSize: 11.5, weight: .medium)
        nextCountdownLabel.textColor = CinemaTheme.textMuted

        let playNextBtn = CinemaTheme.makePillButton(
            title: String(localized: "Play Next"),
            symbol: "play.fill",
            primary: true,
            target: self,
            action: #selector(playNextEpisodeNow)
        )
        let cancelNextBtn = CinemaTheme.makePillButton(
            title: String(localized: "Cancel"),
            primary: false,
            target: self,
            action: #selector(cancelNextEpisode)
        )
        let btns = NSStackView(views: [playNextBtn, cancelNextBtn])
        btns.orientation = .horizontal
        btns.spacing = 8

        let col = NSStackView(views: [header, nextTitleLabel, nextSubtitleLabel, nextCountdownLabel, btns])
        col.orientation = .vertical
        col.alignment = .leading
        col.spacing = 5
        col.translatesAutoresizingMaskIntoConstraints = false
        nextCard.addSubview(col)
        NSLayoutConstraint.activate([
            col.topAnchor.constraint(equalTo: nextCard.topAnchor, constant: 12),
            col.bottomAnchor.constraint(equalTo: nextCard.bottomAnchor, constant: -12),
            col.leadingAnchor.constraint(equalTo: nextCard.leadingAnchor, constant: 14),
            col.trailingAnchor.constraint(equalTo: nextCard.trailingAnchor, constant: -14),
            btns.heightAnchor.constraint(equalToConstant: 30),
        ])
    }

    private func makeIconButton(_ symbol: String, tooltip: String, action: Selector) -> NSButton {
        let btn = NSButton(
            image: NSImage(systemSymbolName: symbol, accessibilityDescription: tooltip) ?? NSImage(),
            target: self,
            action: action
        )
        btn.isBordered = false
        btn.toolTip = tooltip
        btn.contentTintColor = CinemaTheme.textPrimary
        btn.translatesAutoresizingMaskIntoConstraints = false
        btn.widthAnchor.constraint(equalToConstant: 28).isActive = true
        btn.heightAnchor.constraint(equalToConstant: 28).isActive = true
        return btn
    }

    private func refreshTitles() {
        switch request {
        case .stream(let s, let meta):
            titleField.stringValue = meta?.title ?? s.addonName
            subField.stringValue = meta?.subtitle ?? ""
            subField.isHidden = (meta?.subtitle ?? "").isEmpty
            infoLabel.stringValue = meta?.streamSummary ?? s.name ?? s.addonName
        case .file(let url):
            titleField.stringValue = url.lastPathComponent
            subField.stringValue = ""
            subField.isHidden = true
            infoLabel.stringValue = String(localized: "Local File")
        case .trailer(_, let title):
            titleField.stringValue = title
            subField.stringValue = ""
            subField.isHidden = true
            infoLabel.stringValue = String(localized: "Trailer")
        case .download(_, let title):
            titleField.stringValue = title
            subField.stringValue = ""
            subField.isHidden = true
            infoLabel.stringValue = String(localized: "Offline Buffer")
        }
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        guard session == nil, !closing else { return }
        videoLayer.contentsScale = view.window?.backingScaleFactor ?? 2
        view.window?.makeFirstResponder(view)
        installMediaKeys()
        start()
        showHudAndResetTimer()
        if let delay = DevAutomation.autoCloseAfter {
            Task {
                try? await Task.sleep(for: .seconds(delay))
                DevAutomation.log("auto-close")
                close()
            }
        }
    }

    private func installMediaKeys() {
        nowPlaying.onTogglePause = { [weak self] in self?.togglePause() }
        nowPlaying.onPlay = { [weak self] in try? self?.session?.togglePause() }
        nowPlaying.onPause = { [weak self] in try? self?.session?.togglePause() }
        nowPlaying.onNextEpisode = { [weak self] in self?.playNextEpisodeNow() }
        nowPlaying.onSeekAbsolute = { [weak self] secs in self?.seekAbsolute(secs) }
        nowPlaying.installCommands()
    }

    private func start() {
        showStatus(String(localized: "Starting player…"), autoHideAfter: 0)
        loadSkipSegmentsIfEnabled()
        let sink = PlaybackEventSink { [weak self] event in
            Task { @MainActor in self?.handle(event) }
        }
        let core = core
        let request = request
        Task.detached(priority: .userInitiated) {
            let prefs = (try? core.playerPreferences()) ?? PlayerPreferencesFfi(
                exactSeek: true,
                hdrTargetPeak: nil,
                shaderPreset: "none",
                autoplayNext: true,
                autoplayCountdownS: 10,
                forceHdrSurface: false
            )
            let isHdr = await HDRProbe.detectIsHdr(for: request, forceHdr: prefs.forceHdrSurface)
            do {
                await MainActor.run {
                    self.videoLayer.configureSurface(hdr: isHdr)
                    if self.videoView.layer !== self.videoLayer {
                        self.videoView.layer = self.videoLayer
                    }
                }
                let session = try core.createPlaybackForSurface(observer: sink, hdrSurface: isHdr)
                await withCheckedContinuation { (ready: CheckedContinuation<Void, Never>) in
                    Task { @MainActor in
                        self.session = session
                        self.videoLayer.attach(session) { ready.resume() }
                    }
                }
                let proceed = await MainActor.run { () -> Bool in
                    if case .stream(let s, _) = request, s.infoHash != nil {
                        self.showStatus(String(localized: "Connecting to peers…"), autoHideAfter: 0)
                    }
                    return !self.closing
                }
                guard proceed else { return }
                switch request {
                case .stream(let stream, let meta):
                    if let meta {
                        await MainActor.run {
                            if let idx = meta.rankedStreams.firstIndex(where: { $0.stream.raw == stream.raw }) {
                                self.currentStreamIndex = idx
                            }
                        }
                    }
                    try await session.play(stream: stream)
                case .file(let url):
                    try session.playUrl(url: url.path)
                case .trailer(let url, _):
                    try session.playUrl(url: url)
                case .download(let id, _):
                    try session.playDownload(downloadId: id)
                }
            } catch {
                await MainActor.run {
                    self.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                }
            }
        }
    }

    private func handle(_ event: PlaybackEvent) {
        switch event {
        case .fileLoaded:
            loadingNextEpisode = false
            nextCountdownTask?.cancel()
            nextCountdownTask = nil
            nextCard.isHidden = true
            nextCountdownLabel.stringValue = ""
            showStatus("", autoHideAfter: 0)
            failedRetries = 0
            cancelStallWatchdog()
            appliedAutoTracks = false
            applySavedStyleAndOffsets()
            refreshTracksAndChapters()
            scheduleOnlineSubtitleLookup()
            if metadata != nil {
                loadSkipSegmentsIfEnabled()
            }
            if let switchPos = pendingSwitchPosition {
                pendingSwitchPosition = nil
                try? session?.seekAbsoluteExact(seconds: switchPos)
            } else if !appliedResume, let resume = metadata?.resumePosition, resume > 10 {
                appliedResume = true
                try? session?.seekAbsoluteExact(seconds: resume)
            }
            pushNowPlaying()

        case .playbackRestart:
            cancelStallWatchdog()
            if statusLabel.stringValue == String(localized: "Buffering…") { showStatus("", autoHideAfter: 0) }
            if !didTrimInitialMalloc {
                didTrimInitialMalloc = true
                ImagePipeline.releaseMallocPressure()
            }

        case .tracksChanged:
            refreshTracksAndChapters()

        case .chaptersChanged:
            refreshTracksAndChapters()

        case .position(let seconds):
            guard !loadingNextEpisode else { return }
            position = seconds
            scrubBar.position = seconds
            updateTime()
            updateSkipSegmentVisibility()
            checkNextEpisodeTrigger()
            saveProgressIfNeeded(force: false)

        case .duration(let seconds):
            guard !loadingNextEpisode else { return }
            duration = seconds
            scrubBar.duration = seconds
            updateTime()
            pushNowPlaying()

        case .paused(let isPaused):
            paused = isPaused
            onPauseStateChanged?(isPaused)
            let name = isPaused ? "play.fill" : "pause.fill"
            let label = isPaused ? String(localized: "Play") : String(localized: "Pause")
            playButton.image = NSImage(systemSymbolName: name, accessibilityDescription: label)
            if isPaused {
                saveProgressIfNeeded(force: true)
                showHudAndCancelTimer()
            } else {
                showHudAndResetTimer()
            }
            pushNowPlaying()

        case .buffering(let isBuffering):
            showStatus(isBuffering ? String(localized: "Buffering…") : "", autoHideAfter: 0)
            if isBuffering {
                armStallWatchdog()
            } else {
                cancelStallWatchdog()
            }

        case .videoGamma(let gamma):
            let hdr = gamma == "pq" || gamma == "hlg"
            if LaunchMetrics.isEnabled {
                Task {
                    for delay in [2, 4, 8] {
                        try? await Task.sleep(for: .seconds(delay == 2 ? 2 : delay / 2))
                        let screen = view.window?.screen
                        DevAutomation.log("t+\(delay)s gamma=\(gamma) hdr=\(hdr) edr_current=\(screen?.maximumExtendedDynamicRangeColorComponentValue ?? 0) edr_potential=\(screen?.maximumPotentialExtendedDynamicRangeColorComponentValue ?? 0)")
                    }
                }
            }

        case .metric(let name, let millis):
            NSLog("cinma metric %@ = %.0f ms", name, millis)
            continueAutomation()

        case .endFile(let reason, let error):
            switch reason {
            case .error:
                let msg = error ?? "unknown"
                showStatus(String(localized: "Playback error: \(msg)"), autoHideAfter: 4.5)
                attemptStreamRecovery()
            case .eof:
                if !loadingNextEpisode, nextPlan != nil, !nextDismissed {
                    playNextEpisodeNow()
                }
            case .stopped:
                break
            }

        case .shutdown:
            if let start = closeRequestedAt {
                DevAutomation.log("shutdown \(start.duration(to: .now)) after close")
            }
            shutdownWaiter?.resume()
            shutdownWaiter = nil
        }
    }

    // MARK: - Track / Subtitle / Offset Automation

    private func applySavedStyleAndOffsets() {
        guard let session else { return }
        if let settings = try? core.subtitleSettings() {
            applySubtitleStyle(settings, session: session)
        }
        if let prefs = try? core.playerPreferences() {
            let paths = (try? core.resolveShaderPaths(preset: prefs.shaderPreset)) ?? []
            try? session.setGlslShaders(paths: paths)
            try? session.setHdrTargetPeak(nits: prefs.hdrTargetPeak)
        }
        if abs(speed - 1.0) > 0.01 {
            try? session.setSpeed(speed: speed)
        }
        if abs(volume - 100.0) > 0.5 {
            try? session.setVolume(volume: volume)
        }
        if muted {
            try? session.setMuted(muted: true)
        }
        if let videoId = metadata?.videoId, let offsets = try? core.videoOffsets(videoId: videoId) {
            subDelay = abs(offsets.subDelay) > 0.001 ? offsets.subDelay : (sessionSubDelay ?? 0)
            audioDelay = abs(offsets.audioDelay) > 0.001 ? offsets.audioDelay : (sessionAudioDelay ?? 0)
        } else {
            subDelay = sessionSubDelay ?? 0
            audioDelay = sessionAudioDelay ?? 0
        }
        if abs(subDelay) > 0.001 { try? session.setSubtitleDelay(seconds: subDelay) }
        if abs(audioDelay) > 0.001 { try? session.setAudioDelay(seconds: audioDelay) }
    }

    private func applySubtitleStyle(_ s: SubtitleSettingsFfi, session: PlaybackSession?) {
        try? session?.setSubtitleStyle(
            font: s.srtFont,
            fontSize: s.srtFontSize,
            colorHex: s.srtColor,
            borderSize: s.srtBorderSize,
            borderColorHex: s.srtBorderColor,
            backColorHex: s.srtBackColor,
            bold: s.srtBold,
            pos: s.srtPos,
            assOverride: s.assOverride
        )
    }

    private func refreshTracksAndChapters() {
        guard let session else { return }
        tracks = (try? session.tracks()) ?? []
        chapters = (try? session.chapters()) ?? []
        scrubBar.chapters = chapters

        guard !appliedAutoTracks, !tracks.isEmpty else { return }
        appliedAutoTracks = true
        autoSelectPreferredTracks()
    }

    private func autoSelectPreferredTracks() {
        guard let session, let prefs = try? core.subtitleSettings() else { return }
        let audioTracks = tracks.filter { $0.trackType == "audio" }
        var matchedAudio: MediaTrackInfo?
        if let wantLang = sessionAudioLang {
            if let wantTitle = sessionAudioTitle, !wantTitle.isEmpty {
                matchedAudio = audioTracks.first { trackMatchesLang($0, wantLang) && $0.title == wantTitle }
            }
            matchedAudio = matchedAudio ?? audioTracks.first { trackMatchesLang($0, wantLang) }
        } else if let wantTitle = sessionAudioTitle, !wantTitle.isEmpty {
            matchedAudio = audioTracks.first { $0.title == wantTitle }
        }
        if matchedAudio == nil {
            for lang in prefs.audioLangs {
                if let match = audioTracks.first(where: { trackMatchesLang($0, lang) }) {
                    matchedAudio = match
                    break
                }
            }
        }
        if let match = matchedAudio, !match.selected {
            try? session.setAudioTrack(id: match.id)
        }

        let subTracks = tracks.filter { $0.trackType == "sub" }
        if sessionSubEnabled == false {
            subtitlesEnabled = false
            try? session.setSubtitleTrack(id: nil)
            try? session.setSecondarySubtitleTrack(id: nil)
            return
        }

        var primaryId: Int64?
        if let wantSubLang = sessionSubLang {
            var match: MediaTrackInfo?
            if let wantSubTitle = sessionSubTitle, !wantSubTitle.isEmpty {
                match = subTracks.first { trackMatchesLang($0, wantSubLang) && $0.title == wantSubTitle }
            }
            match = match
                ?? subTracks.first { trackMatchesLang($0, wantSubLang) && !$0.forced }
                ?? subTracks.first { trackMatchesLang($0, wantSubLang) }
            if let match {
                primaryId = match.id
                lastSelectedSid = match.id
                subtitlesEnabled = true
                try? session.setSubtitleTrack(id: match.id)
            }
        }
        if primaryId == nil {
            for lang in prefs.preferredLangs {
                if let match = subTracks.first(where: { trackMatchesLang($0, lang) && !$0.forced })
                    ?? subTracks.first(where: { trackMatchesLang($0, lang) })
                {
                    primaryId = match.id
                    lastSelectedSid = match.id
                    subtitlesEnabled = true
                    try? session.setSubtitleTrack(id: match.id)
                    break
                }
            }
        }

        if sessionSecondarySubEnabled == false {
            secondarySid = nil
            try? session.setSecondarySubtitleTrack(id: nil)
        } else if let secLang = sessionSecondarySubLang ?? prefs.dualSecondaryLang, !secLang.isEmpty,
                  let dualMatch = subTracks.first(where: { trackMatchesLang($0, secLang) && $0.id != primaryId })
        {
            secondarySid = dualMatch.id
            try? session.setSecondarySubtitleTrack(id: dualMatch.id)
        }
    }

    private func trackMatchesLang(_ track: MediaTrackInfo, _ pref: String) -> Bool {
        let p = pref.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard let l = track.lang?.lowercased() else { return false }
        if l == p || l.hasPrefix(p) || p.hasPrefix(l) { return true }
        let aliases: [String: Set<String>] = [
            "tur": ["tr", "tur", "trk", "tr-tr"],
            "eng": ["en", "eng", "en-us", "en-gb"],
            "jpn": ["ja", "jpn"],
        ]
        for group in aliases.values where group.contains(p) && group.contains(l) {
            return true
        }
        return false
    }

    private func scheduleOnlineSubtitleLookup() {
        let meta = metadata
        let core = core
        Task { [weak self] in
            // Allow torrent head+tail warmup a moment to compute the OpenSubtitles hash.
            var identity = self?.session?.videoIdentity()
            if identity?.hash == nil {
                try? await Task.sleep(for: .seconds(1.5))
                identity = self?.session?.videoIdentity()
            }
            guard let self, !self.closing else { return }
            let input = SubtitleSearchInput(
                mediaType: meta?.mediaType ?? "movie",
                videoId: meta?.videoId ?? (identity?.filename ?? "local"),
                imdbId: meta?.metaId.hasPrefix("tt") == true ? meta?.metaId : nil,
                season: meta?.season.map { UInt32($0) },
                episode: meta?.episode.map { UInt32($0) },
                videoHash: identity?.hash,
                videoSize: identity?.size,
                filename: identity?.filename
            )
            self.searchingSubtitles = true
            let results = (try? await core.fetchSubtitles(input: input)) ?? []
            guard !self.closing else { return }
            self.searchingSubtitles = false
            self.onlineSubtitles = results
            self.subtitleSearchStatus = identity?.hash != nil
                ? String(localized: "Matched with OpenSubtitles hash")
                : ""
        }
    }

    private func loadSkipSegmentsIfEnabled() {
        guard let meta = metadata else { return }
        let core = core
        Task { [weak self] in
            let segs = (try? await core.skipSegments(
                mediaType: meta.mediaType,
                metaId: meta.metaId,
                season: meta.season.map { UInt32($0) },
                episode: meta.episode.map { UInt32($0) }
            )) ?? []
            await MainActor.run {
                self?.apiSkipSegments = segs
                self?.updateSkipSegmentVisibility()
            }
        }
    }

    // MARK: - Next Episode Prefetch & Autoplay (§5.9)

    private func checkNextEpisodeTrigger() {
        guard !loadingNextEpisode, !closing,
              let meta = metadata, meta.mediaType == "series",
              let season = meta.season, let ep = meta.episode,
              duration > 30
        else { return }

        let progress = position / duration
        let inOutro = chapters.last(where: { $0.startS <= position }).map { isOutroChapter($0.title) } ?? false
        if !prefetchedTriggered && (progress >= 0.88 || inOutro) {
            prefetchedTriggered = true
            session?.startNextEpisodePrefetch(
                metaId: meta.metaId,
                currentSeason: UInt32(max(1, season)),
                currentEpisode: UInt32(max(0, ep)),
                bingeGroup: meta.bingeGroup
            )
        }

        if prefetchedTriggered && nextPlan == nil, let plan = session?.prefetchedNextEpisode() {
            nextPlan = plan
        }

        if !nextDismissed && (progress >= 0.92 || inOutro), let plan = nextPlan {
            showNextEpisodeCard(plan)
        }
    }

    private func showNextEpisodeCard(_ plan: NextEpisodePrefetchInfo) {
        guard !loadingNextEpisode, nextCard.isHidden else { return }
        nextTitleLabel.stringValue = String(localized: "S\(plan.season) · E\(plan.episode) — \(plan.title)")
        nextSubtitleLabel.stringValue = plan.bestStream != nil
            ? String(localized: "Prefetched & ready")
            : String(localized: "Ready to load")
        nextCard.isHidden = false
        showHudAndCancelTimer()

        let prefs = try? core.playerPreferences()
        guard prefs?.autoplayNext != false else {
            nextCountdownLabel.stringValue = ""
            return
        }
        let total = Int(prefs?.autoplayCountdownS ?? 10)
        nextCountdownTask?.cancel()
        nextCountdownTask = Task { [weak self] in
            for remaining in stride(from: total, through: 1, by: -1) {
                guard let self, !Task.isCancelled, !self.loadingNextEpisode, !self.nextDismissed, !self.closing else { return }
                self.nextCountdownLabel.stringValue = String(localized: "Auto-playing in \(remaining)s…")
                try? await Task.sleep(for: .seconds(1))
            }
            guard let self, !Task.isCancelled, !self.loadingNextEpisode, !self.nextDismissed, !self.closing else { return }
            self.playNextEpisodeNow()
        }
    }

    @objc private func playNextEpisodeNow() {
        guard !loadingNextEpisode,
              let plan = nextPlan ?? session?.prefetchedNextEpisode(),
              let stream = plan.bestStream
        else { return }
        loadingNextEpisode = true
        nextCountdownTask?.cancel()
        nextCountdownTask = nil
        nextCard.isHidden = true
        nextCountdownLabel.stringValue = ""
        skipButton.isHidden = true
        nextPlan = nil
        prefetchedTriggered = false
        nextDismissed = false
        pendingSwitchPosition = nil
        appliedResume = true
        position = 0
        duration = 0
        scrubBar.position = 0
        scrubBar.duration = 0
        updateTime()
        chapters = []
        scrubBar.chapters = []
        apiSkipSegments = []
        onlineSubtitles = []

        if let prev = metadata {
            let newMeta = PlaybackMetadata(
                videoId: plan.videoId,
                metaId: prev.metaId,
                mediaType: prev.mediaType,
                title: prev.title,
                subtitle: String(localized: "S\(plan.season) · E\(plan.episode) — \(plan.title)"),
                artwork: plan.still ?? prev.artwork,
                season: Int64(plan.season),
                episode: Int64(plan.episode),
                resumePosition: nil,
                streamSummary: prev.streamSummary,
                isHdr: prev.isHdr,
                bingeGroup: prev.bingeGroup,
                rankedStreams: []
            )
            request = .stream(stream, metadata: newMeta)
            refreshTitles()
        }
        showStatus(String(localized: "Loading next episode…"), autoHideAfter: 0)
        let session = session
        Task { [weak self] in
            do {
                try await session?.play(stream: stream)
            } catch {
                await MainActor.run {
                    self?.loadingNextEpisode = false
                    self?.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                }
            }
        }
    }

    @objc private func cancelNextEpisode() {
        nextDismissed = true
        nextCountdownTask?.cancel()
        nextCard.isHidden = true
        session?.cancelPlaybackPrefetchIfNeeded()
    }

    // MARK: - Skip Intro / Outro Chapters

    private func isIntroChapter(_ title: String) -> Bool {
        let lower = title.lowercased()
        return lower.contains("intro") || lower.contains("opening") || lower == "op" || lower.contains("recap") || lower.contains("önceki")
    }

    private func isOutroChapter(_ title: String) -> Bool {
        let lower = title.lowercased()
        return lower.contains("outro") || lower.contains("credits") || lower.contains("ending") || lower == "ed"
    }

    private func updateSkipSegmentVisibility() {
        if let seg = activeApiSkipSegment() {
            switch seg.kind {
            case "outro": skipButton.title = String(localized: "Skip Outro")
            case "recap": skipButton.title = String(localized: "Skip Recap")
            default: skipButton.title = String(localized: "Skip Intro")
            }
            skipButton.isHidden = false
            return
        }
        guard let idx = chapters.lastIndex(where: { $0.startS <= position }),
              isIntroChapter(chapters[idx].title),
              idx + 1 < chapters.count
        else {
            skipButton.isHidden = true
            return
        }
        skipButton.title = String(localized: "Skip Intro")
        skipButton.isHidden = false
    }

    private func activeApiSkipSegment() -> SkipSegmentInfo? {
        apiSkipSegments.first { position >= $0.startS && position < ($0.endS - 1.0) }
    }

    @objc private func skipCurrentSegment() {
        if let seg = activeApiSkipSegment() {
            seekAbsolute(seg.endS)
            skipButton.isHidden = true
            return
        }
        guard let idx = chapters.lastIndex(where: { $0.startS <= position }),
              idx + 1 < chapters.count
        else { return }
        seekAbsolute(chapters[idx + 1].startS)
        skipButton.isHidden = true
    }

    // MARK: - Stream Switching & Auto-Recovery (§5.2)

    private func armStallWatchdog() {
        cancelStallWatchdog()
        stallRecoveryTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(12))
            guard let self, !Task.isCancelled, !self.closing else { return }
            self.attemptStreamRecovery()
        }
    }

    private func cancelStallWatchdog() {
        stallRecoveryTask?.cancel()
        stallRecoveryTask = nil
    }

    private func attemptStreamRecovery() {
        guard !closing else { return }
        if failedRetries == 0, case .stream(let current, _) = request {
            failedRetries = 1
            pendingSwitchPosition = position > 5 ? position : nil
            showStatus(String(localized: "Stream stalled, retrying…"), autoHideAfter: 0)
            let session = session
            Task { try? await session?.play(stream: current) }
            return
        }
        guard let meta = metadata, !meta.rankedStreams.isEmpty else { return }
        let nextIndex = currentStreamIndex + 1
        guard nextIndex < meta.rankedStreams.count else { return }
        currentStreamIndex = nextIndex
        failedRetries = 0
        let fallback = meta.rankedStreams[nextIndex].stream
        showStatus(String(localized: "Switching to fallback stream…"), autoHideAfter: 0)
        switchToStream(fallback)
    }

    private func switchToStream(_ stream: StreamInfo) {
        pendingSwitchPosition = position > 5 ? position : nil
        if case .stream(_, let meta) = request {
            request = .stream(stream, metadata: meta)
            refreshTitles()
        }
        let session = session
        Task {
            try? await session?.play(stream: stream)
        }
    }

    // MARK: - Hover Thumbnail Preview

    private func handleScrubHover(seconds: Double?, localX: CGFloat) {
        guard let seconds, duration > 0 else {
            isHoveringScrub = false
            thumbCard.isHidden = true
            showHudAndResetTimer()
            return
        }
        isHoveringScrub = true
        showHudAndCancelTimer()
        thumbCard.isHidden = false
        let clampedX = min(max(localX, 88), max(88, bar.bounds.width - 88))
        thumbCenterXConstraint?.constant = clampedX
        let chName = chapters.last(where: { $0.startS <= seconds })?.title
        if let chName, !chName.isEmpty {
            thumbTimeLabel.stringValue = "\(Self.format(seconds)) · \(chName)"
        } else {
            thumbTimeLabel.stringValue = Self.format(seconds)
        }
        if let cached = thumbCache?.cachedImage(at: seconds) {
            thumbImageView.image = cached
            return
        }
        thumbCache?.requestThumbnail(at: seconds, session: session) { [weak self] img in
            self?.thumbImageView.image = img
        }
    }

    // MARK: - Popovers & Menus

    @objc private func showStreamDetailsPopover(_ sender: NSButton) {
        showHudAndCancelTimer()
        let addon: String? = if case .stream(let s, _) = request { s.addonName } else { nil }
        let view = StreamDetailsPopoverView(
            session: session,
            addonName: addon,
            streamSummary: metadata?.streamSummary
        )
        presentFloatingPopover(view, relativeTo: sender)
    }

    @objc private func showStreamSwitcher(_ sender: NSButton) {
        showHudAndCancelTimer()
        let streams = metadata?.rankedStreams ?? []
        let view = StreamSwitcherPopoverView(
            streams: streams,
            activeRaw: activeStreamRaw
        ) { [weak self] selected in
            self?.dismissFloatingPopover()
            self?.switchToStream(selected)
        }
        presentFloatingPopover(view, relativeTo: sender)
    }

    @objc private func showSubtitlesPopover(_ sender: NSButton) {
        showHudAndCancelTimer()
        let settings = (try? core.subtitleSettings()) ?? SubtitleSettingsFfi(
            preferredLangs: ["tur", "eng"],
            audioLangs: ["eng"],
            dualSecondaryLang: nil,
            srtFont: nil,
            srtFontSize: 46,
            srtColor: "#FFFFFF",
            srtBorderSize: 3,
            srtBorderColor: "#000000",
            srtBackColor: nil,
            srtBold: false,
            srtPos: 95,
            assOverride: false
        )
        let root = SubtitleManagerPopoverView(
            candidates: onlineSubtitles,
            loading: searchingSubtitles,
            statusMessage: subtitleSearchStatus,
            tracks: tracks,
            secondaryId: secondarySid,
            subDelay: subDelay,
            settings: settings,
            onSearchOnline: { [weak self] in
                self?.dismissFloatingPopover()
                self?.scheduleOnlineSubtitleLookup()
            },
            onSelectOnline: { [weak self] cand in
                self?.dismissFloatingPopover()
                self?.loadOnlineSubtitle(cand)
            },
            onLoadLocal: { [weak self] in
                self?.dismissFloatingPopover()
                self?.openLocalSubtitleFile()
            },
            onSelectPrimary: { [weak self] id in
                guard let self else { return }
                self.lastSelectedSid = id
                self.subtitlesEnabled = id != nil
                self.sessionSubEnabled = id != nil
                if let id, let chosen = self.tracks.first(where: { $0.trackType == "sub" && $0.id == id }) {
                    self.sessionSubLang = chosen.lang
                    self.sessionSubTitle = chosen.title
                }
                try? self.session?.setSubtitleTrack(id: id)
                self.tracks = (try? self.session?.tracks()) ?? []
                self.dismissFloatingPopover()
            },
            onSelectSecondary: { [weak self] id in
                guard let self else { return }
                self.secondarySid = id
                self.sessionSecondarySubEnabled = id != nil
                if let id, let chosen = self.tracks.first(where: { $0.trackType == "sub" && $0.id == id }) {
                    self.sessionSecondarySubLang = chosen.lang
                } else if id == nil {
                    self.sessionSecondarySubLang = nil
                }
                try? self.session?.setSecondarySubtitleTrack(id: id)
                self.dismissFloatingPopover()
            },
            onAdjustDelay: { [weak self] delta in
                self?.adjustSubDelay(delta)
            },
            onResetDelay: { [weak self] in
                self?.setSubDelay(0)
            },
            onUpdateStyle: { [weak self] updated in
                guard let self else { return }
                try? self.core.setSubtitleSettings(settings: updated)
                self.applySubtitleStyle(updated, session: self.session)
            }
        )
        presentFloatingPopover(root, relativeTo: sender)
    }

    /// Presents a Liquid Glass card inside `view` anchored above `anchor` and
    /// clamped within `[leading + 20, trailing - 20]` so it never overflows off-screen.
    private func presentFloatingPopover<V: View>(_ rootView: V, relativeTo anchor: NSView) {
        dismissFloatingPopover()

        let backdrop = ClickCatcherView()
        backdrop.translatesAutoresizingMaskIntoConstraints = false
        backdrop.onClick = { [weak self] in self?.dismissFloatingPopover() }

        let card = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 16)
        card.translatesAutoresizingMaskIntoConstraints = false

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(host)

        view.addSubview(backdrop)
        view.addSubview(card)

        let centerAlign = card.centerXAnchor.constraint(equalTo: anchor.centerXAnchor)
        centerAlign.priority = .defaultHigh

        NSLayoutConstraint.activate([
            backdrop.topAnchor.constraint(equalTo: view.topAnchor),
            backdrop.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            backdrop.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            backdrop.trailingAnchor.constraint(equalTo: view.trailingAnchor),

            host.topAnchor.constraint(equalTo: card.topAnchor),
            host.bottomAnchor.constraint(equalTo: card.bottomAnchor),
            host.leadingAnchor.constraint(equalTo: card.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: card.trailingAnchor),

            centerAlign,
            card.leadingAnchor.constraint(greaterThanOrEqualTo: view.leadingAnchor, constant: 20),
            card.trailingAnchor.constraint(lessThanOrEqualTo: view.trailingAnchor, constant: -20),
            card.topAnchor.constraint(greaterThanOrEqualTo: titlePill.bottomAnchor, constant: 12),
            card.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -10),
        ])
        popoverBackdrop = backdrop
        popoverCard = card
    }

    private func dismissFloatingPopover() {
        popoverCard?.removeFromSuperview()
        popoverBackdrop?.removeFromSuperview()
        popoverCard = nil
        popoverBackdrop = nil
        if !closing {
            showHudAndResetTimer()
        }
    }

    private func loadOnlineSubtitle(_ candidate: SubtitleCandidateInfo) {
        showStatus(String(localized: "Loading subtitle…"), autoHideAfter: 0)
        let core = core
        let session = session
        Task { [weak self] in
            do {
                let prepared = try await core.downloadAndPrepareSubtitle(candidate: candidate)
                try session?.addSubtitle(
                    path: prepared.localPath,
                    title: candidate.title ?? candidate.provider,
                    lang: candidate.lang,
                    select: true
                )
                await MainActor.run {
                    self?.subtitlesEnabled = true
                    self?.sessionSubEnabled = true
                    self?.sessionSubLang = candidate.lang
                    self?.sessionSubTitle = candidate.title ?? candidate.provider
                    self?.showStatus(String(localized: "Loaded \(candidate.lang.uppercased()) subtitle (\(prepared.detectedEncoding))"))
                    self?.tracks = (try? session?.tracks()) ?? []
                }
            } catch {
                await MainActor.run {
                    self?.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                }
            }
        }
    }

    private func openLocalSubtitleFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [
            UTType(filenameExtension: "srt"),
            UTType(filenameExtension: "ass"),
            UTType(filenameExtension: "ssa"),
            UTType(filenameExtension: "vtt"),
        ].compactMap { $0 }
        panel.allowsOtherFileTypes = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        if let prepared = try? core.prepareLocalSubtitle(path: url.path) {
            try? session?.addSubtitle(path: prepared.localPath, title: url.lastPathComponent, lang: nil, select: true)
            tracks = (try? session?.tracks()) ?? []
            showStatus(String(localized: "Loaded \(url.lastPathComponent) (\(prepared.detectedEncoding))"))
        }
    }

    private func adjustSubDelay(_ delta: Double) {
        setSubDelay(subDelay + delta)
    }

    private func setSubDelay(_ val: Double) {
        subDelay = val
        sessionSubDelay = val
        try? session?.setSubtitleDelay(seconds: val)
        persistOffsets()
        showStatus(String(format: "%@ %+.1fs", String(localized: "Subtitle delay:"), val))
    }

    private func adjustAudioDelay(_ delta: Double) {
        audioDelay += delta
        sessionAudioDelay = audioDelay
        try? session?.setAudioDelay(seconds: audioDelay)
        persistOffsets()
        showStatus(String(format: "%@ %+.1fs", String(localized: "Audio delay:"), audioDelay))
    }

    private func persistOffsets() {
        guard let videoId = metadata?.videoId else { return }
        try? core.setVideoOffsets(
            videoId: videoId,
            offsets: VideoOffsetsFfi(subDelay: subDelay, audioDelay: audioDelay)
        )
    }

    @objc private func showAudioMenu(_ sender: NSButton) {
        showHudAndCancelTimer()
        let menu = NSMenu()
        let audioTracks = tracks.filter { $0.trackType == "audio" }
        if audioTracks.isEmpty {
            menu.addItem(withTitle: String(localized: "No audio tracks"), action: nil, keyEquivalent: "")
        } else {
            for t in audioTracks {
                let label = [t.lang?.uppercased(), t.title, t.codec].compactMap { $0 }.joined(separator: " · ")
                let item = NSMenuItem(
                    title: label.isEmpty ? "Track \(t.id)" : label,
                    action: #selector(selectAudioTrackItem(_:)),
                    keyEquivalent: ""
                )
                item.target = self
                item.tag = Int(t.id)
                item.state = t.selected ? .on : .off
                menu.addItem(item)
            }
        }
        menu.addItem(.separator())
        for (title, delta) in [(String(localized: "Audio Delay -0.1s"), -0.1), (String(localized: "Audio Delay +0.1s"), 0.1)] {
            let item = NSMenuItem(title: title, action: #selector(adjustAudioDelayItem(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = delta
            menu.addItem(item)
        }
        let reset = NSMenuItem(title: String(localized: "Reset Audio Delay"), action: #selector(resetAudioDelayItem), keyEquivalent: "")
        reset.target = self
        menu.addItem(reset)
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc private func selectAudioTrackItem(_ sender: NSMenuItem) {
        let tid = Int64(sender.tag)
        if let chosen = tracks.first(where: { $0.trackType == "audio" && $0.id == tid }) {
            sessionAudioLang = chosen.lang
            sessionAudioTitle = chosen.title
        }
        try? session?.setAudioTrack(id: tid)
        tracks = (try? session?.tracks()) ?? []
    }

    @objc private func adjustAudioDelayItem(_ sender: NSMenuItem) {
        if let d = sender.representedObject as? Double { adjustAudioDelay(d) }
    }

    @objc private func resetAudioDelayItem() {
        audioDelay = 0
        sessionAudioDelay = 0
        try? session?.setAudioDelay(seconds: 0)
        persistOffsets()
    }

    @objc private func showChaptersMenu(_ sender: NSButton) {
        showHudAndCancelTimer()
        let menu = NSMenu()
        if chapters.isEmpty {
            menu.addItem(withTitle: String(localized: "No chapters"), action: nil, keyEquivalent: "")
        } else {
            for ch in chapters {
                let item = NSMenuItem(
                    title: "\(Self.format(ch.startS))  \(ch.title)",
                    action: #selector(jumpToChapter(_:)),
                    keyEquivalent: ""
                )
                item.target = self
                item.representedObject = ch.startS
                menu.addItem(item)
            }
        }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc private func jumpToChapter(_ sender: NSMenuItem) {
        if let t = sender.representedObject as? Double { seekAbsolute(t) }
    }

    @objc private func showSpeedMenu(_ sender: NSButton) {
        let menu = NSMenu()
        for val in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
            let item = NSMenuItem(
                title: String(format: "%.2g×", val),
                action: #selector(selectSpeedItem(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.representedObject = val
            item.state = abs(speed - val) < 0.01 ? .on : .off
            menu.addItem(item)
        }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc private func selectSpeedItem(_ sender: NSMenuItem) {
        if let val = sender.representedObject as? Double { setSpeed(val) }
    }

    @objc private func openCastSheet(_ sender: NSButton) {
        showHudAndCancelTimer()
        let titleText = titleField.stringValue.isEmpty ? "Cinma" : titleField.stringValue
        presentSheetView { dismiss in
            CastDeviceSheetView(core: core, request: request, title: titleText, onDismiss: dismiss)
        }
    }

    @objc private func showMoreMenu(_ sender: NSButton) {
        showHudAndCancelTimer()
        let menu = NSMenu()

        // A/B Loop
        let abTitle: String
        if abLoopA == nil {
            abTitle = String(localized: "A/B Loop: Set Point A")
        } else if abLoopB == nil {
            abTitle = String(localized: "A/B Loop: Set Point B")
        } else {
            abTitle = String(localized: "A/B Loop: Clear")
        }
        let abItem = NSMenuItem(title: abTitle, action: #selector(cycleAbLoop), keyEquivalent: "")
        abItem.target = self
        menu.addItem(abItem)

        // Shaders submenu
        let shaderMenu = NSMenu()
        for (label, preset) in [(String(localized: "Off"), "none"), (String(localized: "Adaptive Sharpen (CAS)"), "sharpen")] {
            let item = NSMenuItem(title: label, action: #selector(selectShaderPreset(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = preset
            shaderMenu.addItem(item)
        }
        let shaderHolder = NSMenuItem(title: String(localized: "Upscaling / Shader"), action: nil, keyEquivalent: "")
        shaderHolder.submenu = shaderMenu
        menu.addItem(shaderHolder)

        // HDR Target Peak submenu
        let peakMenu = NSMenu()
        for nits: UInt32? in [nil, 400, 600, 1000, 1600] {
            let title = nits.map { "\($0) nits" } ?? String(localized: "Auto")
            let item = NSMenuItem(title: title, action: #selector(selectHdrPeak(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = nits.map { NSNumber(value: $0) }
            peakMenu.addItem(item)
        }
        let peakHolder = NSMenuItem(title: String(localized: "HDR Target Peak"), action: nil, keyEquivalent: "")
        peakHolder.submenu = peakMenu
        menu.addItem(peakHolder)

        // Sleep timer submenu
        let sleepMenu = NSMenu()
        for (label, mins) in [(String(localized: "Off"), 0), ("15m", 15), ("30m", 30), ("60m", 60)] {
            let item = NSMenuItem(title: label, action: #selector(setSleepTimerMinutes(_:)), keyEquivalent: "")
            item.target = self
            item.tag = mins
            sleepMenu.addItem(item)
        }
        let sleepHolder = NSMenuItem(title: String(localized: "Sleep Timer"), action: nil, keyEquivalent: "")
        sleepHolder.submenu = sleepMenu
        menu.addItem(sleepHolder)

        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc private func selectShaderPreset(_ sender: NSMenuItem) {
        guard let preset = sender.representedObject as? String else { return }
        if var prefs = try? core.playerPreferences() {
            prefs.shaderPreset = preset
            try? core.setPlayerPreferences(prefs: prefs)
        }
        let paths = (try? core.resolveShaderPaths(preset: preset)) ?? []
        try? session?.setGlslShaders(paths: paths)
        showStatus(String(localized: "Shader: \(preset)"))
    }

    @objc private func selectHdrPeak(_ sender: NSMenuItem) {
        let nits = (sender.representedObject as? NSNumber)?.uint32Value
        if var prefs = try? core.playerPreferences() {
            prefs.hdrTargetPeak = nits
            try? core.setPlayerPreferences(prefs: prefs)
        }
        try? session?.setHdrTargetPeak(nits: nits)
        showStatus(nits.map { "HDR peak: \($0) nits" } ?? "HDR peak: Auto")
    }

    @objc private func setSleepTimerMinutes(_ sender: NSMenuItem) {
        sleepTimerTask?.cancel()
        sleepTimerTask = nil
        let mins = sender.tag
        guard mins > 0 else {
            showStatus(String(localized: "Sleep timer off"))
            return
        }
        showStatus(String(localized: "Sleep timer set for \(mins) min"))
        sleepTimerTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(mins * 60))
            guard let self, !Task.isCancelled, !self.closing else { return }
            self.close()
        }
    }

    // MARK: - Actions & Keyboard Shortcuts

    @objc private func toggleStats() {
        statsVisible.toggle()
        statsCard.isHidden = !statsVisible
        statsTask?.cancel()
        guard statsVisible else { return }
        statsTask = Task { [weak self] in
            while let self, self.statsVisible, !Task.isCancelled {
                if let s = self.session?.stats() {
                    let ab = (s.abLoopA, s.abLoopB)
                    let abStr = (ab.0 != nil || ab.1 != nil)
                        ? "A: \(ab.0.map(Self.format) ?? "—")  B: \(ab.1.map(Self.format) ?? "—")"
                        : String(localized: "Off")
                    self.statsText.stringValue = """
                    Video: \(s.videoCodec) (\(s.width)×\(s.height)) · hwdec: \(s.hwdec)
                    FPS: \(String(format: "%.2f", s.fps)) (est \(String(format: "%.2f", s.estimatedFps))) · Gamma: \(s.gamma)
                    Audio: \(s.audioCodec) (\(s.audioChannels)ch)
                    Cache: \(String(format: "%.1fs", s.cacheSeconds)) · Drops: \(s.droppedFrames)
                    Speed: \(String(format: "%.2gx", s.speed)) · A/B: \(abStr)
                    Delays: Sub \(String(format: "%+.2fs", s.subDelay)) · Audio \(String(format: "%+.2fs", s.audioDelay))
                    """
                }
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    @objc private func takeScreenshot() {
        guard let session else { return }
        let desktop = FileManager.default.urls(for: .desktopDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        let formatter = ISO8601DateFormatter()
        let stamp = formatter.string(from: Date()).replacingOccurrences(of: ":", with: "-")
        let out = desktop.appendingPathComponent("Cinma-\(stamp).png")
        try? session.screenshotToFile(path: out.path)
        showStatus(String(localized: "Saved screenshot: \(out.lastPathComponent)"))
    }

    @objc private func cycleAbLoop() {
        if abLoopA == nil {
            abLoopA = position
            try? session?.setAbLoopA(seconds: position)
            showStatus(String(localized: "A/B loop: A = \(Self.format(position))"))
        } else if abLoopB == nil {
            abLoopB = position
            try? session?.setAbLoopB(seconds: position)
            showStatus(String(localized: "A/B loop: A = \(Self.format(abLoopA ?? 0)), B = \(Self.format(position))"))
        } else {
            abLoopA = nil
            abLoopB = nil
            try? session?.setAbLoopA(seconds: nil)
            try? session?.setAbLoopB(seconds: nil)
            showStatus(String(localized: "A/B Loop cleared"))
        }
        scrubBar.abLoopA = abLoopA
        scrubBar.abLoopB = abLoopB
    }

    @objc private func toggleFullscreen() {
        view.window?.toggleFullScreen(nil)
    }

    private func setSpeed(_ val: Double) {
        speed = min(max(val, 0.25), 4.0)
        speedButton.title = String(format: "%.2g×", speed)
        try? session?.setSpeed(speed: speed)
        pushNowPlaying()
    }

    @objc private func toggleMute() {
        muted.toggle()
        try? session?.setMuted(muted: muted)
        volumeButton.image = NSImage(
            systemSymbolName: muted ? "speaker.slash.fill" : "speaker.wave.2.fill",
            accessibilityDescription: nil
        )
    }

    private func toggleSubtitlesVisibility() {
        subtitlesEnabled.toggle()
        sessionSubEnabled = subtitlesEnabled
        if subtitlesEnabled {
            let target = lastSelectedSid ?? tracks.first(where: { $0.trackType == "sub" })?.id
            if let target, let chosen = tracks.first(where: { $0.trackType == "sub" && $0.id == target }) {
                sessionSubLang = chosen.lang
                sessionSubTitle = chosen.title
            }
            try? session?.setSubtitleTrack(id: target)
        } else {
            lastSelectedSid = tracks.first(where: { $0.trackType == "sub" && $0.selected })?.id ?? lastSelectedSid
            try? session?.setSubtitleTrack(id: nil)
        }
        showStatus(subtitlesEnabled ? String(localized: "Subtitles on") : String(localized: "Subtitles off"))
    }

    private func cycleTrack(type: String, isSub: Bool) {
        let group = tracks.filter { $0.trackType == type }
        guard !group.isEmpty else { return }
        let idx = group.firstIndex(where: { $0.selected }) ?? -1
        let next = group[(idx + 1) % group.count]
        if isSub {
            lastSelectedSid = next.id
            subtitlesEnabled = true
            sessionSubEnabled = true
            sessionSubLang = next.lang
            sessionSubTitle = next.title
            try? session?.setSubtitleTrack(id: next.id)
        } else {
            sessionAudioLang = next.lang
            sessionAudioTitle = next.title
            try? session?.setAudioTrack(id: next.id)
        }
        tracks = (try? session?.tracks()) ?? []
        showStatus("\(type.uppercased()): \(next.lang ?? next.title ?? "Track \(next.id)")")
    }

    private func handleKey(_ event: NSEvent) -> Bool {
        showHudAndResetTimer()
        guard let combo = KeyCombo.from(event) else { return false }
        guard let command = try? core.resolveKey(context: "player", combo: combo) else {
            if combo == "p" {
                enterPiPClicked()
                return true
            }
            return false
        }
        switch command {
        case "player.toggle_pause": togglePause()
        case "player.seek_back": seekBack10()
        case "player.seek_forward": seekForward10()
        case "player.close":
            if popoverBackdrop != nil {
                dismissFloatingPopover()
            } else if view.window?.styleMask.contains(.fullScreen) == true {
                toggleFullscreen()
            } else {
                close()
            }
        case "player.fullscreen": toggleFullscreen()
        case "player.toggle_subtitles": toggleSubtitlesVisibility()
        case "player.cycle_subtitle": cycleTrack(type: "sub", isSub: true)
        case "player.cycle_audio": cycleTrack(type: "audio", isSub: false)
        case "player.screenshot": takeScreenshot()
        case "player.stats": toggleStats()
        case "player.ab_loop": cycleAbLoop()
        case "player.speed_up": setSpeed(speed + 0.25)
        case "player.speed_down": setSpeed(speed - 0.25)
        case "player.volume_up":
            volume = min(150, volume + 10)
            try? session?.setVolume(volume: volume)
        case "player.volume_down":
            volume = max(0, volume - 10)
            try? session?.setVolume(volume: volume)
        case "player.mute": toggleMute()
        case "player.next_episode": playNextEpisodeNow()
        default: return false
        }
        return true
    }

    @objc private func togglePause() { try? session?.togglePause() }

    @objc private func seekBack10() {
        let exact = (try? core.playerPreferences())?.exactSeek ?? true
        try? session?.seekRelativeMode(seconds: -10, exact: exact)
        saveProgressIfNeeded(force: true)
    }

    @objc private func seekForward10() {
        let exact = (try? core.playerPreferences())?.exactSeek ?? true
        try? session?.seekRelativeMode(seconds: 10, exact: exact)
        saveProgressIfNeeded(force: true)
    }

    private func seekAbsolute(_ seconds: Double) {
        position = seconds
        scrubBar.position = seconds
        try? session?.seekAbsolute(seconds: seconds)
        saveProgressIfNeeded(force: true)
        pushNowPlaying()
    }

    // MARK: - HUD Auto-Hide & Persistence

    func playerTitleForPiP() -> String {
        titleField.stringValue.isEmpty ? "Cinma" : titleField.stringValue
    }

    func detachVideoViewForPiP() -> NSView {
        hudHideTask?.cancel()
        return videoView
    }

    func reattachVideoViewFromPiP() {
        videoView.removeFromSuperview()
        videoView.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(videoView, positioned: .below, relativeTo: titlePill)
        NSLayoutConstraint.activate([
            videoView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            videoView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            videoView.topAnchor.constraint(equalTo: view.topAnchor),
            videoView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        showHudAndResetTimer()
    }

    func togglePauseFromPiP() { togglePause() }
    func closeFromPiP() { close() }

    @objc private func enterPiPClicked() {
        showHudAndCancelTimer()
        onEnterPiP?()
    }

    private func showHudAndResetTimer() {
        setHudAlpha(1.0)
        hudHideTask?.cancel()
        guard !paused, !isHoveringScrub, popoverBackdrop == nil else { return }
        hudHideTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(2.8))
            guard let self, !Task.isCancelled, !self.paused, !self.isHoveringScrub, self.popoverBackdrop == nil, !self.closing else { return }
            self.setHudAlpha(0.0)
        }
    }

    private func showHudAndCancelTimer() {
        hudHideTask?.cancel()
        setHudAlpha(1.0)
    }

    private func setHudAlpha(_ alpha: CGFloat) {
        let pills = [titlePill, bar]
        if alpha > 0 {
            for p in pills { p.isHidden = false }
        }
        let duration: TimeInterval = CinemaTheme.reduceMotion ? 0 : 0.18
        if duration == 0 {
            for p in pills {
                p.alphaValue = alpha
                p.isHidden = alpha == 0
            }
            return
        }
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = duration
            for p in pills { p.animator().alphaValue = alpha }
        }, completionHandler: {
            Task { @MainActor in
                guard !self.closing else { return }
                for p in pills where p.alphaValue == 0 {
                    p.isHidden = true
                }
            }
        })
    }

    private func pushNowPlaying() {
        nowPlaying.update(
            title: titleField.stringValue,
            subtitle: subField.isHidden ? nil : subField.stringValue,
            duration: duration,
            position: position,
            paused: paused,
            speed: speed
        )
        let core = core
        let title = titleField.stringValue.isEmpty ? "Cinma" : titleField.stringValue
        let subtitle = subField.isHidden || subField.stringValue.isEmpty ? nil : subField.stringValue
        let dur: UInt64? = duration > 0 ? UInt64(duration) : nil
        let pos = UInt64(max(0, position))
        let isPaused = paused
        Task {
            try? await core.updateDiscordPresence(
                title: title,
                subtitle: subtitle,
                durationS: dur,
                positionS: pos,
                paused: isPaused
            )
        }
    }

    private func saveProgressIfNeeded(force: Bool) {
        guard let meta = metadata, duration > 0, position >= 0 else { return }
        if !force, let last = lastSavedAt, last.duration(to: .now) < .seconds(10) {
            return
        }
        lastSavedAt = .now
        try? core.saveWatchProgress(input: WatchProgressInputFfi(
            videoId: meta.videoId,
            metaId: meta.metaId,
            mediaType: meta.mediaType,
            title: meta.title,
            artwork: meta.artwork,
            season: meta.season,
            episode: meta.episode,
            positionS: position,
            durationS: duration
        ))
    }

    /// Drives CINMA_AUTOSEEK / CINMA_AUTOQUIT (see DevAutomation).
    private func continueAutomation() {
        guard !pendingSeeks.isEmpty || DevAutomation.autoQuit else { return }
        Task {
            try? await Task.sleep(for: .seconds(pendingSeeks.isEmpty ? 5 : 2))
            if pendingSeeks.isEmpty {
                guard DevAutomation.autoQuit, !closing else { return }
                if DevAutomation.closeOnly {
                    DevAutomation.log("closing player")
                    close()
                    return
                }
                closing = true
                nowPlaying.teardown()
                thumbCache?.clear()
                videoLayer.detach()
                try? await session?.stop()
                try? await Task.sleep(for: .milliseconds(500))
                NSApp.terminate(nil)
            } else if duration > 0 {
                let fraction = pendingSeeks.removeFirst()
                DevAutomation.log("seek to \(fraction)")
                try? session?.seekAbsolute(seconds: duration * fraction)
            }
        }
    }

    private func updateTime() {
        timeLabel.stringValue = "\(Self.format(position)) / \(Self.format(duration))"
    }

    private static func format(_ seconds: Double) -> String {
        guard seconds.isFinite, seconds >= 0 else { return "--:--" }
        let total = Int(seconds)
        let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%02d:%02d", m, s)
    }

    private func showStatus(_ text: String, autoHideAfter seconds: TimeInterval = 3.0) {
        statusClearTask?.cancel()
        statusClearTask = nil
        statusLabel.stringValue = text
        statusPill.isHidden = text.isEmpty
        guard !text.isEmpty, seconds > 0 else { return }
        statusClearTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(seconds))
            guard let self, !Task.isCancelled, !self.closing else { return }
            self.statusLabel.stringValue = ""
            self.statusPill.isHidden = true
        }
    }

    @objc private func close() {
        guard !closing else { return }
        closing = true
        closeRequestedAt = .now
        hudHideTask?.cancel()
        statusClearTask?.cancel()
        statsTask?.cancel()
        sleepTimerTask?.cancel()
        nextCountdownTask?.cancel()
        cancelStallWatchdog()
        dismissFloatingPopover()
        nowPlaying.teardown()
        let discordCore = core
        Task { try? await discordCore.clearDiscordPresence() }
        thumbCache?.clear()
        thumbCache = nil
        saveProgressIfNeeded(force: true)
        statusLabel.stringValue = ""
        statusPill.isHidden = true
        guard let session else {
            onClose()
            return
        }
        videoLayer.detach()
        Task {
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                shutdownWaiter = continuation
                Task {
                    try? await session.stop()
                    try? await Task.sleep(for: .seconds(3))
                    if shutdownWaiter != nil { DevAutomation.log("shutdown timed out (fallback)") }
                    shutdownWaiter?.resume()
                    shutdownWaiter = nil
                }
            }
            self.session = nil
            ImagePipeline.releaseMallocPressure()
            onClose()
        }
    }
}

private extension PlaybackSession {
    func cancelPlaybackPrefetchIfNeeded() {
        cancelPrefetch()
    }
}
