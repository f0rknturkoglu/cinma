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

/// Phase 1 player: mpv renders into `videoLayer` (render API, ADR 0004);
/// a minimal control bar sits on top. Full player UI is Phase 4.
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
    private var shutdownWaiter: CheckedContinuation<Void, Never>?
    private var pendingSeeks = DevAutomation.autoSeeks
    private var closeRequestedAt: ContinuousClock.Instant?

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

        let bar = NSVisualEffectView()
        bar.material = .hudWindow
        bar.blendingMode = .withinWindow
        bar.state = .active
        bar.wantsLayer = true
        bar.layer?.cornerRadius = 14
        bar.translatesAutoresizingMaskIntoConstraints = false

        playButton.bezelStyle = .accessoryBarAction
        playButton.isBordered = false
        playButton.image = NSImage(systemSymbolName: "pause.fill", accessibilityDescription: String(localized: "Pause"))
        playButton.target = self
        playButton.action = #selector(togglePause)
        let closeButton = NSButton(
            image: NSImage(systemSymbolName: "xmark", accessibilityDescription: String(localized: "Close")) ?? NSImage(),
            target: self, action: #selector(close)
        )
        closeButton.isBordered = false
        slider.isContinuous = false
        slider.target = self
        slider.action = #selector(seek)
        timeLabel.font = .monospacedDigitSystemFont(ofSize: 12, weight: .medium)
        timeLabel.textColor = .white

        let controls = NSStackView(views: [playButton, slider, timeLabel, closeButton])
        controls.spacing = 12
        controls.edgeInsets = NSEdgeInsets(top: 8, left: 14, bottom: 8, right: 14)
        controls.translatesAutoresizingMaskIntoConstraints = false
        bar.addSubview(controls)

        statusLabel.textColor = .white
        statusLabel.font = .systemFont(ofSize: 13, weight: .medium)
        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        statusLabel.lineBreakMode = .byTruncatingTail

        root.addSubview(bar)
        root.addSubview(statusLabel)
        NSLayoutConstraint.activate([
            videoView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            videoView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            videoView.topAnchor.constraint(equalTo: root.topAnchor),
            videoView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            controls.leadingAnchor.constraint(equalTo: bar.leadingAnchor),
            controls.trailingAnchor.constraint(equalTo: bar.trailingAnchor),
            controls.topAnchor.constraint(equalTo: bar.topAnchor),
            controls.bottomAnchor.constraint(equalTo: bar.bottomAnchor),
            bar.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 24),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -24),
            bar.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -24),
            statusLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 24),
            statusLabel.topAnchor.constraint(equalTo: root.topAnchor, constant: 40),
            statusLabel.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -24),
        ])
        view = root
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
                    if case .stream(let s) = request, s.infoHash != nil {
                        self.statusLabel.stringValue = String(localized: "Connecting to peers…")
                    }
                    return !self.closing
                }
                guard proceed else { return }
                switch request {
                case .stream(let stream): try await session.play(stream: stream)
                case .file(let url): try session.playUrl(url: url.path)
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
        case .playbackRestart:
            if statusLabel.stringValue == String(localized: "Buffering…") { statusLabel.stringValue = "" }
        case .position(let seconds):
            position = seconds
            if NSEvent.pressedMouseButtons == 0 { slider.doubleValue = seconds }
            updateTime()
        case .duration(let seconds):
            duration = seconds
            slider.maxValue = max(seconds, 1)
            updateTime()
        case .paused(let paused):
            let name = paused ? "play.fill" : "pause.fill"
            let label = paused ? String(localized: "Play") : String(localized: "Pause")
            playButton.image = NSImage(systemSymbolName: name, accessibilityDescription: label)
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
                statusLabel.stringValue = String(localized: "Playback error: \(error ?? "unknown")")
            }
        case .shutdown:
            if let start = closeRequestedAt {
                DevAutomation.log("shutdown \(start.duration(to: .now)) after close")
            }
            shutdownWaiter?.resume()
            shutdownWaiter = nil
        }
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

    private func handleKey(_ event: NSEvent) -> Bool {
        switch event.keyCode {
        case 49: togglePause(); return true                    // space
        case 123: try? session?.seekRelative(seconds: -10); return true  // ←
        case 124: try? session?.seekRelative(seconds: 10); return true   // →
        case 53: close(); return true                          // esc
        default: return false
        }
    }

    @objc private func togglePause() {
        try? session?.togglePause()
    }

    @objc private func seek() {
        try? session?.seekAbsolute(seconds: slider.doubleValue)
    }

    @objc private func close() {
        guard !closing else { return }
        closing = true
        closeRequestedAt = .now
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
