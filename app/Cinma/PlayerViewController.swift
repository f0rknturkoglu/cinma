import AppKit

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

/// Root view that takes keyboard focus for player shortcuts.
private final class PlayerRootView: NSView {
    var onKey: ((NSEvent) -> Bool)?
    override var acceptsFirstResponder: Bool { true }
    override func keyDown(with event: NSEvent) {
        if onKey?(event) != true { super.keyDown(with: event) }
    }
}

/// Cinema player view matching mockup M2: floating Liquid Glass top title &
/// stream pills + bottom glass scrub bar + throttled watch-progress saving.
@MainActor
final class PlayerViewController: NSViewController {
    private let core: CinmaCore
    private let request: PlayRequest
    private let onClose: () -> Void

    private let videoView = NSView()
    private let videoLayer = VideoLayer()
    private let statusLabel = NSTextField(labelWithString: "")
    private let playButton = NSButton()
    private let slider = NSSlider(value: 0, minValue: 0, maxValue: 1, target: nil, action: nil)
    private let timeLabel = NSTextField(labelWithString: "--:-- / --:--")

    private var session: PlaybackSession?
    private var duration: Double = 0
    private var position: Double = 0
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

    init(core: CinmaCore, request: PlayRequest, onClose: @escaping () -> Void) {
        self.core = core
        self.request = request
        self.onClose = onClose
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func loadView() {
        let root = PlayerRootView()
        root.wantsLayer = true
        root.layer?.backgroundColor = NSColor.black.cgColor
        root.onKey = { [weak self] event in self?.handleKey(event) ?? false }

        videoView.layer = videoLayer   // layer-hosting view
        videoView.wantsLayer = true
        videoView.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(videoView)

        // Top-left floating glass title pill (mockup M2)
        let titlePill = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 22)
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

        let (titleText, subText, summaryText) = displayTitles()
        let titleField = NSTextField(labelWithString: titleText)
        titleField.font = .systemFont(ofSize: 13.5, weight: .bold)
        titleField.textColor = CinemaTheme.textPrimary
        let subField = NSTextField(labelWithString: subText ?? "")
        subField.font = .systemFont(ofSize: 11.5, weight: .regular)
        subField.textColor = CinemaTheme.textSecondary
        subField.isHidden = subText == nil

        let titleCol = NSStackView(views: [titleField, subField])
        titleCol.orientation = .vertical
        titleCol.alignment = .leading
        titleCol.spacing = 1

        let topLeftStack = NSStackView(views: [backBtn, titleCol])
        topLeftStack.orientation = .horizontal
        topLeftStack.alignment = .centerY
        topLeftStack.spacing = 10
        topLeftStack.translatesAutoresizingMaskIntoConstraints = false
        titlePill.addSubview(topLeftStack)

        // Top-right stream info pill
        let infoPill = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 17)
        infoPill.translatesAutoresizingMaskIntoConstraints = false
        infoPill.isHidden = summaryText == nil
        let dot = NSView()
        dot.wantsLayer = true
        dot.layer?.cornerRadius = 4
        dot.layer?.backgroundColor = CinemaTheme.green.cgColor
        dot.translatesAutoresizingMaskIntoConstraints = false
        dot.widthAnchor.constraint(equalToConstant: 8).isActive = true
        dot.heightAnchor.constraint(equalToConstant: 8).isActive = true

        let infoField = NSTextField(labelWithString: summaryText ?? "")
        infoField.font = .monospacedDigitSystemFont(ofSize: 12, weight: .medium)
        infoField.textColor = CinemaTheme.textPrimary
        let infoStack = NSStackView(views: [dot, infoField])
        infoStack.orientation = .horizontal
        infoStack.alignment = .centerY
        infoStack.spacing = 8
        infoStack.translatesAutoresizingMaskIntoConstraints = false
        infoPill.addSubview(infoStack)

        // Bottom floating glass control bar
        let bar = CinemaTheme.makeGlassView(kind: .regular, cornerRadius: 22)
        bar.translatesAutoresizingMaskIntoConstraints = false

        let back10 = NSButton(
            image: NSImage(systemSymbolName: "gobackward.10", accessibilityDescription: String(localized: "Seek back 10s")) ?? NSImage(),
            target: self,
            action: #selector(seekBack10)
        )
        let fwd10 = NSButton(
            image: NSImage(systemSymbolName: "goforward.10", accessibilityDescription: String(localized: "Seek forward 10s")) ?? NSImage(),
            target: self,
            action: #selector(seekForward10)
        )
        for btn in [back10, playButton, fwd10] {
            btn.isBordered = false
            btn.contentTintColor = CinemaTheme.textPrimary
        }

        playButton.image = NSImage(systemSymbolName: "pause.fill", accessibilityDescription: String(localized: "Pause"))
        playButton.target = self
        playButton.action = #selector(togglePause)

        slider.isContinuous = false
        slider.trackFillColor = CinemaTheme.accent
        slider.target = self
        slider.action = #selector(seek)

        timeLabel.font = .monospacedDigitSystemFont(ofSize: 12.5, weight: .semibold)
        timeLabel.textColor = CinemaTheme.textPrimary

        let transportRow = NSStackView(views: [back10, playButton, fwd10, timeLabel, NSView()])
        transportRow.orientation = .horizontal
        transportRow.alignment = .centerY
        transportRow.spacing = 12

        let barStack = NSStackView(views: [slider, transportRow])
        barStack.orientation = .vertical
        barStack.alignment = .leading
        barStack.spacing = 10
        barStack.translatesAutoresizingMaskIntoConstraints = false
        bar.addSubview(barStack)

        statusLabel.textColor = CinemaTheme.textPrimary
        statusLabel.font = .systemFont(ofSize: 13, weight: .medium)
        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        statusLabel.lineBreakMode = .byTruncatingTail

        root.addSubview(titlePill)
        root.addSubview(infoPill)
        root.addSubview(bar)
        root.addSubview(statusLabel)

        NSLayoutConstraint.activate([
            videoView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            videoView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            videoView.topAnchor.constraint(equalTo: root.topAnchor),
            videoView.bottomAnchor.constraint(equalTo: root.bottomAnchor),

            topLeftStack.topAnchor.constraint(equalTo: titlePill.topAnchor, constant: 6),
            topLeftStack.bottomAnchor.constraint(equalTo: titlePill.bottomAnchor, constant: -6),
            topLeftStack.leadingAnchor.constraint(equalTo: titlePill.leadingAnchor, constant: 6),
            topLeftStack.trailingAnchor.constraint(equalTo: titlePill.trailingAnchor, constant: -16),
            titlePill.topAnchor.constraint(equalTo: root.topAnchor, constant: 22),
            titlePill.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 88),

            infoStack.topAnchor.constraint(equalTo: infoPill.topAnchor, constant: 8),
            infoStack.bottomAnchor.constraint(equalTo: infoPill.bottomAnchor, constant: -8),
            infoStack.leadingAnchor.constraint(equalTo: infoPill.leadingAnchor, constant: 14),
            infoStack.trailingAnchor.constraint(equalTo: infoPill.trailingAnchor, constant: -14),
            infoPill.centerYAnchor.constraint(equalTo: titlePill.centerYAnchor),
            infoPill.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -28),

            barStack.topAnchor.constraint(equalTo: bar.topAnchor, constant: 14),
            barStack.bottomAnchor.constraint(equalTo: bar.bottomAnchor, constant: -14),
            barStack.leadingAnchor.constraint(equalTo: bar.leadingAnchor, constant: 20),
            barStack.trailingAnchor.constraint(equalTo: bar.trailingAnchor, constant: -20),
            slider.widthAnchor.constraint(equalTo: barStack.widthAnchor),
            transportRow.widthAnchor.constraint(equalTo: barStack.widthAnchor),

            bar.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 28),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -28),
            bar.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -24),

            statusLabel.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            statusLabel.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -14),
        ])
        view = root
    }

    private func displayTitles() -> (String, String?, String?) {
        switch request {
        case .stream(let s, let meta):
            return (
                meta?.title ?? s.addonName,
                meta?.subtitle,
                meta?.streamSummary ?? s.name
            )
        case .file(let url):
            return (url.lastPathComponent, nil, String(localized: "Local File"))
        case .trailer(_, let title):
            return (title, nil, String(localized: "Trailer"))
        }
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        guard session == nil, !closing else { return }
        videoLayer.contentsScale = view.window?.backingScaleFactor ?? 2
        view.window?.makeFirstResponder(view)
        start()
        if let delay = DevAutomation.autoCloseAfter {
            Task {
                try? await Task.sleep(for: .seconds(delay))
                DevAutomation.log("auto-close")
                close()
            }
        }
    }

    private func start() {
        statusLabel.stringValue = String(localized: "Starting player…")
        let sink = PlaybackEventSink { [weak self] event in
            Task { @MainActor in self?.handle(event) }
        }
        let core = core
        let request = request
        Task.detached(priority: .userInitiated) {
            do {
                let session = try core.createPlayback(observer: sink)
                // mpv's video output needs the render context before loadfile.
                await withCheckedContinuation { (ready: CheckedContinuation<Void, Never>) in
                    Task { @MainActor in
                        self.session = session
                        self.videoLayer.onRenderReady = { ready.resume() }
                        self.videoLayer.attach(session)
                    }
                }
                let proceed = await MainActor.run { () -> Bool in
                    if case .stream(let s, _) = request, s.infoHash != nil {
                        self.statusLabel.stringValue = String(localized: "Connecting to peers…")
                    }
                    return !self.closing
                }
                guard proceed else { return }
                switch request {
                case .stream(let stream, _): try await session.play(stream: stream)
                case .file(let url): try session.playUrl(url: url.path)
                case .trailer(let url, _): try session.playUrl(url: url)
                }
            } catch {
                await MainActor.run {
                    self.statusLabel.stringValue = String(localized: "Playback failed: \(error.localizedDescription)")
                }
            }
        }
    }

    private func handle(_ event: PlaybackEvent) {
        switch event {
        case .fileLoaded:
            statusLabel.stringValue = ""
            if !appliedResume, let resume = metadata?.resumePosition, resume > 10 {
                appliedResume = true
                try? session?.seekAbsolute(seconds: resume)
            }
        case .playbackRestart:
            if statusLabel.stringValue == String(localized: "Buffering…") { statusLabel.stringValue = "" }
        case .position(let seconds):
            position = seconds
            if NSEvent.pressedMouseButtons == 0 { slider.doubleValue = seconds }
            updateTime()
            saveProgressIfNeeded(force: false)
        case .duration(let seconds):
            duration = seconds
            slider.maxValue = max(seconds, 1)
            updateTime()
        case .paused(let paused):
            let name = paused ? "play.fill" : "pause.fill"
            let label = paused ? String(localized: "Play") : String(localized: "Pause")
            playButton.image = NSImage(systemSymbolName: name, accessibilityDescription: label)
            if paused { saveProgressIfNeeded(force: true) }
        case .buffering(let buffering):
            statusLabel.stringValue = buffering ? String(localized: "Buffering…") : ""
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
            if case .error = reason {
                let msg = error ?? "unknown"
                statusLabel.stringValue = String(localized: "Playback error: \(msg)")
            }
        case .shutdown:
            if let start = closeRequestedAt {
                DevAutomation.log("shutdown \(start.duration(to: .now)) after close")
            }
            shutdownWaiter?.resume()
            shutdownWaiter = nil
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

    /// Keys resolve through the command registry, so they can be remapped.
    private func handleKey(_ event: NSEvent) -> Bool {
        guard let combo = KeyCombo.from(event),
              let command = try? core.resolveKey(context: "player", combo: combo)
        else { return false }
        switch command {
        case "player.toggle_pause": togglePause()
        case "player.seek_back": seekBack10()
        case "player.seek_forward": seekForward10()
        case "player.close": close()
        default: return false   // global commands go through the menu
        }
        return true
    }

    @objc private func togglePause() {
        try? session?.togglePause()
    }

    @objc private func seekBack10() {
        try? session?.seekRelative(seconds: -10)
        saveProgressIfNeeded(force: true)
    }

    @objc private func seekForward10() {
        try? session?.seekRelative(seconds: 10)
        saveProgressIfNeeded(force: true)
    }

    @objc private func seek() {
        position = slider.doubleValue
        try? session?.seekAbsolute(seconds: slider.doubleValue)
        saveProgressIfNeeded(force: true)
    }

    @objc private func close() {
        guard !closing else { return }
        closing = true
        closeRequestedAt = .now
        saveProgressIfNeeded(force: true)
        statusLabel.stringValue = ""
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
                    // Fallback if Shutdown never arrives.
                    try? await Task.sleep(for: .seconds(3))
                    if shutdownWaiter != nil { DevAutomation.log("shutdown timed out (fallback)") }
                    shutdownWaiter?.resume()
                    shutdownWaiter = nil
                }
            }
            self.session = nil
            onClose()
        }
    }
}
