import AppKit
import CoreAudio
import SwiftUI
import UniformTypeIdentifiers
import VisionKit

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
    private static let videoExtensions: Set<String> = [
        "mkv", "mp4", "m4v", "mov", "avi", "webm", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg",
    ]
    private static let subtitleExtensions: Set<String> = ["srt", "ass", "ssa", "vtt"]

    var onKey: ((NSEvent) -> Bool)?
    var onKeyUp: ((NSEvent) -> Bool)?
    var onMouseActivity: (() -> Void)?
    var onHoldPressDown: ((NSPoint) -> Void)?
    var onHoldPressUp: (() -> Void)?
    var onScrollGesture: ((NSEvent) -> Void)?
    var onDropFile: ((URL) -> Void)?
    private var tracking: NSTrackingArea?

    override var acceptsFirstResponder: Bool { true }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        registerForDraggedTypes([.fileURL])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        guard firstSupportedURL(from: sender.draggingPasteboard) != nil else { return [] }
        wantsLayer = true
        layer?.borderWidth = 3
        layer?.borderColor = CinemaTheme.accent.cgColor
        return .copy
    }

    override func draggingExited(_ sender: (any NSDraggingInfo)?) {
        layer?.borderWidth = 0
        layer?.borderColor = nil
    }

    override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        layer?.borderWidth = 0
        layer?.borderColor = nil
        guard let url = firstSupportedURL(from: sender.draggingPasteboard) else { return false }
        onDropFile?(url)
        return true
    }

    private func firstSupportedURL(from pasteboard: NSPasteboard) -> URL? {
        let urls = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        return urls.first {
            let ext = $0.pathExtension.lowercased()
            return Self.videoExtensions.contains(ext) || Self.subtitleExtensions.contains(ext)
        }
    }

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
        let pt = convert(event.locationInWindow, from: nil)
        onHoldPressDown?(pt)
        super.mouseDown(with: event)
    }

    override func mouseUp(with event: NSEvent) {
        onHoldPressUp?()
        super.mouseUp(with: event)
    }

    override func scrollWheel(with event: NSEvent) {
        onMouseActivity?()
        onScrollGesture?(event)
        super.scrollWheel(with: event)
    }

    override func keyDown(with event: NSEvent) {
        if onKey?(event) != true { super.keyDown(with: event) }
    }

    override func keyUp(with event: NSEvent) {
        if onKeyUp?(event) != true { super.keyUp(with: event) }
    }
}

final class ClickCatcherView: NSView {
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
    let core: CinmaCore
    var request: PlayRequest
    let onClose: () -> Void

    let videoView = NSView()
    let videoLayer = VideoLayer()
    let titlePill = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 22)
    let infoLabel = NSTextField(labelWithString: "")
    let titleField = NSTextField(labelWithString: "")
    let subField = NSTextField(labelWithString: "")
    let bar = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 22)
    let statusPill = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 14)
    let statusLabel = NSTextField(labelWithString: "")
    let scrubBar = ScrubBarView()
    let playButton = NSButton()
    let speedButton = NSButton()
    let volumeButton = NSButton()
    let lockButton = NSButton()
    let p2pSpeedLabel = NSTextField(labelWithString: "")
    let clockPill = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 16)
    let clockLabel = NSTextField(labelWithString: "")
    let unlockPill = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 18)
    let timeLabel = NSTextField(labelWithString: "--:-- / --:--")

    // Hover thumbnail preview
    let thumbCard = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 12)
    let thumbImageView = NSImageView()
    let thumbTimeLabel = NSTextField(labelWithString: "00:00")
    var thumbCenterXConstraint: NSLayoutConstraint?

    // Stats overlay & Skip intro / Next episode cards
    let statsCard = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 14)
    let statsText = NSTextField(wrappingLabelWithString: "")
    lazy var skipButton = CinemaTheme.makePillButton(title: "", symbol: "forward.end.fill", primary: true, target: self, action: #selector(skipCurrentSegment))
    var skipButtonTrailingConstraint: NSLayoutConstraint?
    let nextCard = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 16)
    let nextTitleLabel = NSTextField(labelWithString: "")
    let nextSubtitleLabel = NSTextField(labelWithString: "")
    let nextCountdownLabel = NSTextField(labelWithString: "")

    var session: PlaybackSession?
    var thumbCache: ThumbnailPreviewCache?
    let nowPlaying = NowPlayingBridge()

    var duration: Double = 0
    var position: Double = 0
    var paused = false
    var muted = false
    var volume: Double = 100
    var speed: Double = 1.0
    var subDelay: Double = 0
    var audioDelay: Double = 0
    var subtitlesEnabled = true
    var lastSelectedSid: Int64?
    var secondarySid: Int64?
    var abLoopA: Double?
    var abLoopB: Double?
    var tracks: [MediaTrackInfo] = []
    var chapters: [ChapterInfo] = []
    var apiSkipSegments: [SkipSegmentInfo] = []
    var appliedAutoTracks = false
    var onlineSubtitles: [SubtitleCandidateInfo] = []
    var searchingSubtitles = false
    var subtitleSearchStatus = ""
    var prefetchedTriggered = false
    var nextPlan: NextEpisodePrefetchInfo?
    var nextCountdownTask: Task<Void, Never>?
    var nextDismissed = false
    var loadingNextEpisode = false
    var sessionAudioLang: String?
    var sessionAudioTitle: String?
    var sessionSubEnabled: Bool?
    var sessionSubLang: String?
    var sessionSubTitle: String?
    var sessionSecondarySubEnabled: Bool?
    var sessionSecondarySubLang: String?
    var sessionSubDelay: Double?
    var sessionAudioDelay: Double?
    var statsVisible = false
    var statsTask: Task<Void, Never>?
    var hudHideTask: Task<Void, Never>?
    var statusClearTask: Task<Void, Never>?
    var popoverBackdrop: ClickCatcherView?
    var popoverCard: NSView?
    weak var activeSubtitlesHost: NSHostingView<SubtitleManagerPopoverView>?
    var didTrimInitialMalloc = false
    var sleepTimerTask: Task<Void, Never>?
    var activeSleepMinutes: Int = 0
    var stallRecoveryTask: Task<Void, Never>?
    var failedRetries = 0
    var currentStreamIndex = 0
    var pendingSwitchPosition: Double?
    var isHoveringScrub = false
    var hudVisible = true
    var baseSubtitlePos: UInt32 = 95
    var liftSubtitlesAboveControls = true
    var lastAutoSkippedSegmentStart: Double?
    var focusObservers: [NSObjectProtocol] = []
    var pausedBySystemEvent = false
    var isScreenLocked = false
    var autoPlayedEpisodesCount: UInt32 = 0
    var p2pChipTask: Task<Void, Never>?
    var moreButton: NSButton?
    var sleepEndOfEpisode = false
    var holdBoostTask: Task<Void, Never>?
    var holdBoostActive = false
    var holdBoostKeyWaiting: String?
    var speedBeforeHoldBoost: Double = 1.0
    var scrollAccumX: CGFloat = 0
    var scrollAccumY: CGFloat = 0
    var activeSubtitleFilePath: String?
    var manualSkipMarkers = ManualSkipMarkersFfi(introEndS: nil, outroLastS: nil)
    var audioRouteAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var audioRouteListener: AudioObjectPropertyListenerBlock?
    var liveTextOverlay: ImageAnalysisOverlayView?
    var liveTextTask: Task<Void, Never>?
    var blackoutWindows: [NSWindow] = []
    var isInPiP = false
    var cachedPrefs: PlayerPreferencesFfi?
    var pendingStyleSettings: SubtitleSettingsFfi?
    var stylePersistTask: Task<Void, Never>?
    var pendingEqPrefs: (Int32, Int32, Int32, Int32)?
    var eqPersistTask: Task<Void, Never>?

    static let clockFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm"
        return f
    }()

    var onEnterPiP: (() -> Void)?
    var onPauseStateChanged: ((Bool) -> Void)?
    var isPausedForPiP: Bool { paused }

    var closing = false
    var appliedResume = false
    var lastSavedAt: ContinuousClock.Instant?
    var shutdownWaiter: CheckedContinuation<Void, Never>?
    var pendingSeeks = DevAutomation.autoSeeks
    var closeRequestedAt: ContinuousClock.Instant?

    var metadata: PlaybackMetadata? {
        if case .stream(_, let meta) = request { return meta }
        return nil
    }

    func currentPrefs(reload: Bool = false) -> PlayerPreferencesFfi? {
        if reload || cachedPrefs == nil {
            cachedPrefs = try? core.playerPreferences()
        }
        return cachedPrefs
    }

    func updatePrefs(_ mutate: (inout PlayerPreferencesFfi) -> Void) {
        guard var prefs = currentPrefs(reload: true) else { return }
        mutate(&prefs)
        cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
    }

    func flushPendingPersistedSettings() {
        stylePersistTask?.cancel()
        stylePersistTask = nil
        if let pending = pendingStyleSettings {
            pendingStyleSettings = nil
            try? core.setSubtitleSettings(settings: pending)
        }
        eqPersistTask?.cancel()
        eqPersistTask = nil
        if let eq = pendingEqPrefs {
            pendingEqPrefs = nil
            updatePrefs { prefs in
                prefs.eqBrightness = eq.0
                prefs.eqContrast = eq.1
                prefs.eqGamma = eq.2
                prefs.eqSaturation = eq.3
            }
        }
    }

    var activeStreamRaw: String? {
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
        root.onKeyUp = { [weak self] event in self?.handleKeyUp(event) ?? false }
        root.onMouseActivity = { [weak self] in self?.showHudAndResetTimer() }
        root.onHoldPressDown = { [weak self] point in
            guard let self, !self.isScreenLocked, !self.isPointOnHud(point) else { return }
            self.beginHoldBoostCandidate(triggerKey: nil)
        }
        root.onHoldPressUp = { [weak self] in
            _ = self?.endHoldBoost(triggerKey: nil)
        }
        root.onScrollGesture = { [weak self] event in
            self?.handleScrollGesture(event)
        }
        root.onDropFile = { [weak self] url in
            self?.handleDroppedFile(url)
        }

        videoView.wantsLayer = true
        videoView.layer?.backgroundColor = NSColor.black.cgColor
        videoView.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(videoView)

        buildTopBars()
        buildClockAndLockPills()
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

        for sub in [titlePill, clockPill, unlockPill, bar, thumbCard, statsCard, skipButton, nextCard, statusPill] {
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
            titlePill.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -180),

            clockPill.topAnchor.constraint(equalTo: root.topAnchor, constant: 22),
            clockPill.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -24),

            unlockPill.topAnchor.constraint(equalTo: root.topAnchor, constant: 22),
            unlockPill.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -24),

            bar.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 24),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -24),
            bar.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -20),

            thumbAnchor,
            thumbCard.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 24),
            thumbCard.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -24),
            thumbCard.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -10),

            statsCard.topAnchor.constraint(equalTo: titlePill.bottomAnchor, constant: 12),
            statsCard.leadingAnchor.constraint(equalTo: titlePill.leadingAnchor),

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
        let skipTrailing = skipButton.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -28)
        skipTrailing.isActive = true
        skipButtonTrailingConstraint = skipTrailing
        view = root
    }

    func buildTopBars() {
        titlePill.translatesAutoresizingMaskIntoConstraints = false
        let backBtn = NSButton(
            image: NSImage(systemSymbolName: "chevron.left", accessibilityDescription: String(localized: "Close")) ?? NSImage(),
            target: self,
            action: #selector(requestClose)
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

    func buildClockAndLockPills() {
        clockPill.translatesAutoresizingMaskIntoConstraints = false
        clockPill.isHidden = true
        clockLabel.font = .monospacedDigitSystemFont(ofSize: 12.5, weight: .semibold)
        clockLabel.textColor = CinemaTheme.textPrimary
        clockLabel.translatesAutoresizingMaskIntoConstraints = false
        clockPill.addSubview(clockLabel)
        NSLayoutConstraint.activate([
            clockLabel.topAnchor.constraint(equalTo: clockPill.topAnchor, constant: 7),
            clockLabel.bottomAnchor.constraint(equalTo: clockPill.bottomAnchor, constant: -7),
            clockLabel.leadingAnchor.constraint(equalTo: clockPill.leadingAnchor, constant: 12),
            clockLabel.trailingAnchor.constraint(equalTo: clockPill.trailingAnchor, constant: -12),
        ])

        unlockPill.translatesAutoresizingMaskIntoConstraints = false
        unlockPill.isHidden = true
        let unlockBtn = CinemaTheme.makePillButton(
            title: String(localized: "Unlock Screen"),
            symbol: "lock.fill",
            primary: false,
            target: self,
            action: #selector(toggleScreenLock)
        )
        unlockBtn.translatesAutoresizingMaskIntoConstraints = false
        unlockPill.addSubview(unlockBtn)
        NSLayoutConstraint.activate([
            unlockBtn.topAnchor.constraint(equalTo: unlockPill.topAnchor, constant: 4),
            unlockBtn.bottomAnchor.constraint(equalTo: unlockPill.bottomAnchor, constant: -4),
            unlockBtn.leadingAnchor.constraint(equalTo: unlockPill.leadingAnchor, constant: 4),
            unlockBtn.trailingAnchor.constraint(equalTo: unlockPill.trailingAnchor, constant: -4),
            unlockBtn.heightAnchor.constraint(equalToConstant: 30),
        ])
    }

    func buildBottomBar() {
        bar.translatesAutoresizingMaskIntoConstraints = false

        scrubBar.translatesAutoresizingMaskIntoConstraints = false
        scrubBar.heightAnchor.constraint(equalToConstant: 20).isActive = true
        scrubBar.onSeek = { [weak self] secs in
            guard let self, !self.isScreenLocked else { return }
            self.autoPlayedEpisodesCount = 0
            self.seekAbsolute(secs)
        }
        scrubBar.onHover = { [weak self] secs, localX in
            guard let self, !self.isScreenLocked else { return }
            self.handleScrubHover(seconds: secs, localX: localX)
        }
        scrubBar.onChapterCrossed = { [weak self] in
            self?.performHaptic(.alignment)
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
        p2pSpeedLabel.font = .monospacedDigitSystemFont(ofSize: 10.5, weight: .semibold)
        p2pSpeedLabel.textColor = CinemaTheme.textSecondary
        p2pSpeedLabel.isHidden = true
        p2pSpeedLabel.setContentCompressionResistancePriority(.required, for: .horizontal)
        let streamsBtn = makeIconButton("rectangle.stack", tooltip: String(localized: "Switch Stream"), action: #selector(showStreamSwitcher(_:)))
        let castBtn = makeIconButton("tv.and.mediabox", tooltip: String(localized: "Cast to LAN Device"), action: #selector(openCastSheet(_:)))
        let pipBtn = makeIconButton("pip.enter", tooltip: String(localized: "Mini Player (P)"), action: #selector(enterPiPClicked))
        let statsBtn = makeIconButton("info.circle", tooltip: String(localized: "Playback Stats (I)"), action: #selector(toggleStats))
        let shotBtn = makeIconButton("camera", tooltip: String(localized: "Screenshot (S)"), action: #selector(takeScreenshot))
        let moreBtn = makeIconButton("ellipsis.circle", tooltip: String(localized: "Video, A/B Loop & Sleep Timer"), action: #selector(showMoreMenu(_:)))
        self.moreButton = moreBtn
        lockButton.isBordered = false
        lockButton.image = NSImage(systemSymbolName: "lock.open", accessibilityDescription: String(localized: "Lock Screen (⇧L)"))
        lockButton.toolTip = String(localized: "Lock Screen (⇧L)")
        lockButton.contentTintColor = CinemaTheme.textPrimary
        lockButton.target = self
        lockButton.action = #selector(toggleScreenLock)
        lockButton.translatesAutoresizingMaskIntoConstraints = false
        lockButton.widthAnchor.constraint(equalToConstant: 28).isActive = true
        lockButton.heightAnchor.constraint(equalToConstant: 28).isActive = true
        let fsBtn = makeIconButton("arrow.up.left.and.arrow.down.right", tooltip: String(localized: "Fullscreen (F)"), action: #selector(toggleFullscreen))

        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        spacer.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        // All player controls consolidated in the bottom glass bar (Harbor / Stremio layout).
        let transportRow = NSStackView(views: [
            back10, playButton, fwd10, timeLabel, infoLabel,
            spacer,
            volumeButton, speedButton, subBtn, audioBtn, chapBtn,
            globeBtn, p2pSpeedLabel, streamsBtn, castBtn, pipBtn, statsBtn, shotBtn, moreBtn, lockButton, fsBtn,
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

    func buildThumbPreview() {
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

    func buildStatsCard() {
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

    func buildSkipAndNextCards() {
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
            action: #selector(playNextEpisodeManually)
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

    func makeIconButton(_ symbol: String, tooltip: String, action: Selector) -> NSButton {
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

    func refreshTitles() {
        switch request {
        case .stream(let s, let meta):
            let seriesFirst = currentPrefs()?.playerTitleSeriesFirst ?? true
            let seriesName = meta?.title ?? s.addonName
            let episodeSub = meta?.subtitle ?? ""
            if !seriesFirst && !episodeSub.isEmpty {
                titleField.stringValue = episodeSub
                subField.stringValue = seriesName
                subField.isHidden = false
            } else {
                titleField.stringValue = seriesName
                subField.stringValue = episodeSub
                subField.isHidden = episodeSub.isEmpty
            }
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
        installWindowFocusObservers()
        installAudioRouteObserver()
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

    func installMediaKeys() {
        nowPlaying.onTogglePause = { [weak self] in self?.togglePause() }
        nowPlaying.onPlay = { [weak self] in try? self?.session?.setPaused(paused: false) }
        nowPlaying.onPause = { [weak self] in try? self?.session?.setPaused(paused: true) }
        nowPlaying.onNextEpisode = { [weak self] in self?.playNextEpisodeNow() }
        nowPlaying.onSeekAbsolute = { [weak self] secs in self?.seekAbsolute(secs) }
        nowPlaying.installCommands()
    }

    func installWindowFocusObservers() {
        guard focusObservers.isEmpty, let window = view.window else { return }
        let nc = NotificationCenter.default
        focusObservers = [
            nc.addObserver(forName: NSWindow.didMiniaturizeNotification, object: window, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.handlePauseOnSystemEvent(isMinimize: true) }
            },
            nc.addObserver(forName: NSWindow.didDeminiaturizeNotification, object: window, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.handleResumeAfterSystemEvent() }
            },
            nc.addObserver(forName: NSWindow.didResignKeyNotification, object: window, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.handlePauseOnSystemEvent(isMinimize: false) }
            },
            nc.addObserver(forName: NSWindow.didBecomeKeyNotification, object: window, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.handleResumeAfterSystemEvent() }
            },
            nc.addObserver(forName: NSWindow.didEnterFullScreenNotification, object: window, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.syncMonitorBlackout() }
            },
            nc.addObserver(forName: NSWindow.didExitFullScreenNotification, object: window, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.clearBlackoutWindows() }
            },
            nc.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.handleWindowOcclusionChanged() }
            },
        ]
    }

    func handlePauseOnSystemEvent(isMinimize: Bool) {
        _ = endHoldBoost(triggerKey: nil)
        guard !closing, let session, !paused else { return }
        guard let prefs = try? core.playerPreferences() else { return }
        let shouldPause = isMinimize ? prefs.pauseMinimized : prefs.pauseUnfocused
        guard shouldPause else { return }
        pausedBySystemEvent = true
        try? session.togglePause()
    }

    func handleResumeAfterSystemEvent() {
        guard !closing, let session, paused, pausedBySystemEvent else { return }
        pausedBySystemEvent = false
        try? session.togglePause()
    }

    func start() {
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
                forceHdrSurface: false,
                seekBackStepS: 10,
                seekForwardStepS: 10,
                defaultSpeedPct: 100,
                volumeBoostMax: 150,
                pauseMinimized: true,
                pauseUnfocused: false,
                autoSkipIntro: false,
                autoSkipRecap: false,
                autoSkipOutro: false,
                showSkipButton: true,
                autoNextStreamOnStall: true,
                stallTimeoutS: 12,
                cropMode: "fit",
                audioNormalize: false,
                downmixStereo: false,
                hideSpoilers: false,
                rememberLastStream: true,
                keepSourceNextEpisode: true,
                trackBlockWords: [],
                streamSort: "score",
                pickerShowFilename: true,
                fullscreenClock: true,
                fullscreenClockEndTime: true,
                nextEpisodeLeadS: 35,
                stillWatchingAfterEpisodes: 0,
                hideWatchedInCatalogs: false,
                hideUnreleased: false,
                episodeSortAsc: true,
                episodeCompactLayout: false,
                confirmOnLeave: false,
                resumeBehavior: "always_resume",
                volumeHud: true,
                screenLockEnabled: true,
                hdrToneMapping: "auto",
                macEdrEnabled: true,
                forceYuv420p: false,
                mpvScalePreset: "balanced",
                mpvHwdecMode: "videotoolbox",
                mpvBufferMb: 8,
                mpvExtraOptions: "",
                debridOnlyMode: false,
                addonTimeoutS: 15,
                cwAtTop: true,
                maxAddonRails: 6,
                posterScalePct: 100,
                posterRadiusPt: 12,
                hidePosterTitles: false,
                showImdbBadge: true,
                showTypeBadge: true,
                showWatchedBadge: true,
                showLibraryBadge: false,
                seekBarChapters: true,
                playerTitleSeriesFirst: true,
                uiLanguage: "system",
                playerAlwaysOnTop: false,
                discordHideTitle: false,
                discordShowWhenPaused: true,
                discordShowWhenBrowsing: false,
                weekStartsMonday: true,
                cwHideCaughtUp: false,
                customSleepMinutes: 45,
                showP2pSpeedChip: true,
                preferOriginalTitles: false,
                tmdbImageLang: "auto",
                themeAccent: "amber",
                reduceGlassEffect: false,
                backdropDimPct: 50,
                hiddenNavTabs: [],
                externalPlayerEnabled: false,
                externalPlayerPath: "",
                confirmStremioDeeplink: true,
                malCountRewatches: false,
                holdToBoostEnabled: true,
                holdBoostSpeedPct: 200,
                eqBrightness: 0,
                eqContrast: 0,
                eqGamma: 0,
                eqSaturation: 0,
                screenshotIncludeSubtitles: true,
                trackpadGesturesEnabled: true,
                hapticFeedbackEnabled: true,
                externalPlayerPreset: "auto",
                splitContinueWatching: false,
                pauseOnAudioOutputChange: true,
                clipboardCatcherEnabled: false,
                blackoutOtherScreens: false,
                autoPipOnOcclusion: false,
                notifyOnDownloadComplete: true,
                quickPlayEnabled: true,
                autoSkipAnimeFillers: false,
                localMediaFolders: [],
                downloadScheduleEnabled: false,
                downloadScheduleStartHour: 1,
                downloadScheduleEndHour: 7,
                localApiSocketEnabled: false,
                gamepadSupportEnabled: true
            )
            let isHdr = await HDRProbe.detectIsHdr(for: request, forceHdr: prefs.forceHdrSurface, macEdrEnabled: prefs.macEdrEnabled)
            do {
                await MainActor.run {
                    let initialSpeed = Double(prefs.defaultSpeedPct) / 100.0
                    if abs(self.speed - 1.0) < 0.01 && abs(initialSpeed - 1.0) > 0.01 {
                        self.setSpeed(initialSpeed)
                    }
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
                            self.armStallWatchdog()
                        }
                    } else {
                        await MainActor.run {
                            self.armStallWatchdog()
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
                    self.cancelStallWatchdog()
                    self.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                    if case .stream = self.request {
                        self.attemptStreamRecovery()
                    }
                }
            }
        }
    }

    func handle(_ event: PlaybackEvent) {
        switch event {
        case .fileLoaded:
            loadingNextEpisode = false
            nextCountdownTask?.cancel()
            nextCountdownTask = nil
            setNextCardVisible(false)
            nextCountdownLabel.stringValue = ""
            showStatus("", autoHideAfter: 0)
            failedRetries = 0
            lastAutoSkippedSegmentStart = nil
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
            if !isPaused { pausedBySystemEvent = false }
            onPauseStateChanged?(isPaused)
            let name = isPaused ? "play.fill" : "pause.fill"
            let label = isPaused ? String(localized: "Play") : String(localized: "Pause")
            playButton.image = NSImage(systemSymbolName: name, accessibilityDescription: label)
            if isPaused {
                saveProgressIfNeeded(force: true)
                showHudAndCancelTimer()
            } else {
                removeLiveTextOverlay()
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
                if sleepEndOfEpisode {
                    sleepEndOfEpisode = false
                    showStatus(String(localized: "Sleep timer finished"))
                    close()
                } else if !loadingNextEpisode, nextPlan != nil, !nextDismissed {
                    autoAdvanceToNextEpisode()
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


    // MARK: - HUD Auto-Hide & Persistence

    func playerTitleForPiP() -> String {
        titleField.stringValue.isEmpty ? "Cinma" : titleField.stringValue
    }

    func detachVideoViewForPiP() -> NSView {
        isInPiP = true
        hudHideTask?.cancel()
        return videoView
    }

    func reattachVideoViewFromPiP() {
        isInPiP = false
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

    // MARK: - Gamepad Control Helpers

    func gamepadShowStatus(_ message: String) {
        showStatus(message)
    }

    func gamepadTogglePause() {
        togglePause()
        showHudAndResetTimer()
    }

    func gamepadBackOrClose() {
        if popoverBackdrop != nil {
            dismissFloatingPopover()
        } else if view.window?.styleMask.contains(.fullScreen) == true {
            toggleFullscreen()
        } else {
            requestClose()
        }
    }

    func gamepadSkipOrHud() {
        if !skipButton.isHidden {
            skipCurrentSegment()
        } else {
            showHudAndResetTimer()
        }
    }

    func gamepadCycleSubtitle() {
        cycleTrack(type: "sub", isSub: true)
        showHudAndResetTimer()
    }

    func gamepadSeekStep(forward: Bool) {
        if forward { seekForward10() } else { seekBack10() }
        showHudAndResetTimer()
    }

    func gamepadSeek(seconds: Double) {
        guard !isScreenLocked else { return }
        let exact = currentPrefs()?.exactSeek ?? true
        try? session?.seekRelativeMode(seconds: seconds, exact: exact)
        saveProgressIfNeeded(force: true)
        showHudAndResetTimer()
    }

    func gamepadAdjustVolume(delta: Double) {
        adjustVolume(by: delta)
    }

    func gamepadStepChapter(forward: Bool) {
        guard !isScreenLocked, !chapters.isEmpty else { return }
        let target: ChapterInfo? = if forward {
            chapters.first(where: { $0.startS > position + 1.0 })
        } else {
            chapters.last(where: { $0.startS < max(0, position - 3.0) })
        }
        if let target {
            seekAbsolute(target.startS)
            showStatus(target.title)
        }
        showHudAndResetTimer()
    }

    func gamepadShowHud() {
        showHudAndResetTimer()
    }

    @objc func enterPiPClicked() {
        showHudAndCancelTimer()
        onEnterPiP?()
    }

    func showHudAndResetTimer() {
        setHudAlpha(1.0)
        hudHideTask?.cancel()
        guard !paused, !isHoveringScrub, popoverBackdrop == nil else { return }
        hudHideTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(2.8))
            guard let self, !Task.isCancelled, !self.paused, !self.isHoveringScrub, self.popoverBackdrop == nil, !self.closing else { return }
            self.setHudAlpha(0.0)
        }
    }

    func showHudAndCancelTimer() {
        hudHideTask?.cancel()
        setHudAlpha(1.0)
    }

    func setHudAlpha(_ alpha: CGFloat) {
        let nowVisible = alpha > 0
        if hudVisible != nowVisible {
            hudVisible = nowVisible
            syncSubtitlePosForHud()
        }
        if isScreenLocked {
            titlePill.isHidden = true
            bar.isHidden = true
            clockPill.isHidden = true
            animatePills([unlockPill], alpha: alpha)
            return
        }
        unlockPill.isHidden = true
        let showClock = currentPrefs()?.fullscreenClock ?? true
        if nowVisible { updateClockPill() }
        if !showClock { clockPill.isHidden = true }
        let pills = showClock ? [titlePill, bar, clockPill] : [titlePill, bar]
        animatePills(pills, alpha: alpha)
    }

    func animatePills(_ pills: [NSView], alpha: CGFloat) {
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

    func pushNowPlaying() {
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

    func saveProgressIfNeeded(force: Bool) {
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
    func continueAutomation() {
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

    func updateTime() {
        timeLabel.stringValue = "\(Self.format(position)) / \(Self.format(duration))"
        updateClockPill()
    }

    func updateClockPill() {
        guard !isScreenLocked else {
            clockPill.isHidden = true
            return
        }
        let prefs = currentPrefs()
        guard prefs?.fullscreenClock ?? true else {
            clockPill.isHidden = true
            return
        }
        let now = Date()
        let nowStr = Self.clockFormatter.string(from: now)
        if (prefs?.fullscreenClockEndTime ?? true), duration > position, duration.isFinite {
            let endDate = now.addingTimeInterval(max(0, duration - position) / max(0.25, speed))
            let endStr = Self.clockFormatter.string(from: endDate)
            clockLabel.stringValue = "\(nowStr) · \(String(localized: "Ends \(endStr)"))"
        } else {
            clockLabel.stringValue = nowStr
        }
        clockPill.isHidden = !hudVisible
    }

    func refreshP2pSpeedChip() {
        p2pChipTask?.cancel()
        p2pChipTask = nil
        let showChip = currentPrefs()?.showP2pSpeedChip ?? true
        let isTorrentCandidate: Bool = {
            switch request {
            case .stream(let s, _): return s.infoHash != nil
            case .download: return true
            case .file, .trailer: return false
            }
        }()
        guard showChip, isTorrentCandidate else {
            p2pSpeedLabel.isHidden = true
            return
        }
        p2pChipTask = Task { [weak self] in
            while let self, !Task.isCancelled, !self.closing {
                if let st = self.session?.streamStats(), st.isTorrent {
                    let speedStr: String
                    if st.downloadSpeedBps >= 1024 * 1024 {
                        speedStr = String(format: "%.1f MB/s", Double(st.downloadSpeedBps) / (1024 * 1024))
                    } else {
                        speedStr = "\(st.downloadSpeedBps / 1024) KB/s"
                    }
                    self.p2pSpeedLabel.stringValue = "\(speedStr) · \(st.peersConnected)p"
                    self.p2pSpeedLabel.isHidden = false
                } else {
                    self.p2pSpeedLabel.isHidden = true
                    if let st = self.session?.streamStats(), st.sourceUrl != nil {
                        break
                    }
                }
                try? await Task.sleep(for: .milliseconds(1500))
            }
        }
    }

    static func format(_ seconds: Double) -> String {
        guard seconds.isFinite, seconds >= 0 else { return "--:--" }
        let total = Int(seconds)
        let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%02d:%02d", m, s)
    }

    func showStatus(_ text: String, autoHideAfter seconds: TimeInterval = 3.0) {
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

    @objc func close() {
        guard !closing else { return }
        closing = true
        flushPendingPersistedSettings()
        closeRequestedAt = .now
        hudHideTask?.cancel()
        statusClearTask?.cancel()
        statsTask?.cancel()
        p2pChipTask?.cancel()
        p2pChipTask = nil
        sleepTimerTask?.cancel()
        holdBoostTask?.cancel()
        holdBoostTask = nil
        holdBoostActive = false
        holdBoostKeyWaiting = nil
        sleepEndOfEpisode = false
        nextCountdownTask?.cancel()
        cancelStallWatchdog()
        removeLiveTextOverlay()
        removeAudioRouteObserver()
        clearBlackoutWindows()
        view.window?.level = .normal
        for obs in focusObservers { NotificationCenter.default.removeObserver(obs) }
        focusObservers.removeAll()
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

extension PlaybackSession {
    func cancelPlaybackPrefetchIfNeeded() {
        cancelPrefetch()
    }
}
