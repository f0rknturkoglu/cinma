import AppKit
import SwiftUI

/// Zero-copy always-on-top Picture-in-Picture mini-player `NSPanel`
/// (`ADR 0011` §4, Owner Decision #4). Reuses the live `VideoLayer` (`CAOpenGLLayer`)
/// with zero frame copies so HDR10 and `libass` subtitles stay intact.
@MainActor
final class MiniPlayerPanel: NSPanel, NSWindowDelegate {
    private let containerView = MiniPlayerContainerView()
    private let controlsPill = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 16)
    private let playPauseButton = NSButton()
    private let expandButton = NSButton()
    private let closeButton = NSButton()

    private let onTogglePause: () -> Void
    private let onExpand: () -> Void
    private let onClosePanel: () -> Void

    init(
        videoView: NSView,
        title: String,
        isPaused: Bool,
        onTogglePause: @escaping () -> Void,
        onExpand: @escaping () -> Void,
        onClosePanel: @escaping () -> Void
    ) {
        self.onTogglePause = onTogglePause
        self.onExpand = onExpand
        self.onClosePanel = onClosePanel

        let screen = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1280, height: 800)
        let width: CGFloat = 480
        let height: CGFloat = 270
        let rect = NSRect(
            x: screen.maxX - width - 28,
            y: screen.minY + 28,
            width: width,
            height: height
        )

        super.init(
            contentRect: rect,
            styleMask: [.titled, .closable, .resizable, .fullSizeContentView, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )

        self.title = title
        titleVisibility = .hidden
        titlebarAppearsTransparent = true
        isFloatingPanel = true
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        isMovableByWindowBackground = true
        backgroundColor = .black
        aspectRatio = NSSize(width: 16, height: 9)
        minSize = NSSize(width: 288, height: 162)
        isReleasedWhenClosed = false
        delegate = self

        standardWindowButton(.closeButton)?.isHidden = true
        standardWindowButton(.miniaturizeButton)?.isHidden = true
        standardWindowButton(.zoomButton)?.isHidden = true

        setupViews(videoView: videoView, isPaused: isPaused)
        contentView = containerView
    }

    private func setupViews(videoView: NSView, isPaused: Bool) {
        containerView.wantsLayer = true
        containerView.layer?.backgroundColor = NSColor.black.cgColor

        videoView.removeFromSuperview()
        videoView.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(videoView)

        controlsPill.translatesAutoresizingMaskIntoConstraints = false
        controlsPill.alphaValue = 0

        configureButton(
            playPauseButton,
            symbol: isPaused ? "play.fill" : "pause.fill",
            label: isPaused ? String(localized: "Play") : String(localized: "Pause"),
            action: #selector(playClicked)
        )
        configureButton(
            expandButton,
            symbol: "pip.exit",
            label: String(localized: "Return to Window"),
            action: #selector(expandClicked)
        )
        configureButton(
            closeButton,
            symbol: "xmark",
            label: String(localized: "Close"),
            action: #selector(closeClicked)
        )

        let stack = NSStackView(views: [playPauseButton, expandButton, closeButton])
        stack.orientation = .horizontal
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        controlsPill.addSubview(stack)
        containerView.addSubview(controlsPill)

        NSLayoutConstraint.activate([
            videoView.topAnchor.constraint(equalTo: containerView.topAnchor),
            videoView.leadingAnchor.constraint(equalTo: containerView.leadingAnchor),
            videoView.trailingAnchor.constraint(equalTo: containerView.trailingAnchor),
            videoView.bottomAnchor.constraint(equalTo: containerView.bottomAnchor),

            stack.topAnchor.constraint(equalTo: controlsPill.topAnchor, constant: 6),
            stack.bottomAnchor.constraint(equalTo: controlsPill.bottomAnchor, constant: -6),
            stack.leadingAnchor.constraint(equalTo: controlsPill.leadingAnchor, constant: 12),
            stack.trailingAnchor.constraint(equalTo: controlsPill.trailingAnchor, constant: -12),

            controlsPill.centerXAnchor.constraint(equalTo: containerView.centerXAnchor),
            controlsPill.bottomAnchor.constraint(equalTo: containerView.bottomAnchor, constant: -12),
        ])

        containerView.onHoverChanged = { [weak self] hovering in
            guard let self else { return }
            let dur: TimeInterval = CinemaTheme.reduceMotion ? 0 : 0.16
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = dur
                self.controlsPill.animator().alphaValue = hovering ? 1 : 0
            }
        }
    }

    private func configureButton(_ btn: NSButton, symbol: String, label: String, action: Selector) {
        btn.isBordered = false
        btn.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        btn.contentTintColor = CinemaTheme.textPrimary
        btn.toolTip = label
        btn.setAccessibilityLabel(label)
        btn.target = self
        btn.action = action
    }

    func updatePaused(_ paused: Bool) {
        let symbol = paused ? "play.fill" : "pause.fill"
        let label = paused ? String(localized: "Play") : String(localized: "Pause")
        playPauseButton.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        playPauseButton.setAccessibilityLabel(label)
    }

    @objc private func playClicked() { onTogglePause() }
    @objc private func expandClicked() { onExpand() }
    @objc private func closeClicked() { onClosePanel() }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        onClosePanel()
        return false
    }
}

@MainActor
private final class MiniPlayerContainerView: NSView {
    var onHoverChanged: ((Bool) -> Void)?
    private var tracking: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) { onHoverChanged?(true) }
    override func mouseExited(with event: NSEvent) { onHoverChanged?(false) }
}

// MARK: - About & Open-Source Credits Window

@MainActor
enum AboutWindow {
    private static var controller: NSWindowController?

    static func show() {
        if let existing = controller?.window {
            existing.makeKeyAndOrderFront(nil)
            NSApp.activate()
            return
        }
        let vc = NSHostingController(rootView: AboutView())
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 460, height: 420),
            styleMask: [.titled, .closable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = String(localized: "About Cinma")
        window.titlebarAppearsTransparent = true
        window.contentViewController = vc
        window.center()
        let wc = NSWindowController(window: window)
        controller = wc
        wc.showWindow(nil)
        NSApp.activate()
    }
}

private struct AboutView: View {
    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.1.0"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 14) {
                if let icon = NSApp.applicationIconImage {
                    Image(nsImage: icon)
                        .resizable()
                        .frame(width: 56, height: 56)
                        .cornerRadius(12)
                }
                VStack(alignment: .leading, spacing: 3) {
                    Text("Cinma")
                        .font(.title.bold())
                    Text("v\(appVersion) · Core \(coreVersion())")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                    Text(String(localized: "Native Apple Silicon media player, Stremio client & download manager"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Divider()

            Text(String(localized: "Open-Source Credits & Licenses"))
                .font(.headline)

            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    creditRow("libmpv", "LGPL v2.1+ / GPL — Hardware-accelerated media playback")
                    creditRow("FFmpeg (libavformat / libavcodec / libswscale)", "LGPL v2.1+ / GPL — Demuxing, thumbnails & container remuxing")
                    creditRow("librqbit", "Apache-2.0 — Rust BitTorrent engine")
                    creditRow("SQLite & rusqlite", "Public Domain / MIT — Local WAL persistence")
                    creditRow("tokio, reqwest, rustls, ring, uniffi-rs", "MIT / Apache-2.0 / ISC — Async runtime, TLS, cryptography & Swift FFI")
                    creditRow("yt-dlp (optional user install)", "Unlicense — Web media metadata extraction")
                }
            }
            .frame(maxHeight: 200)
        }
        .padding(24)
        .frame(width: 460, height: 420)
    }

    private func creditRow(_ name: String, _ desc: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(name)
                .font(.subheadline.weight(.semibold))
            Text(desc)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}
