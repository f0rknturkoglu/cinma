import AppKit
import SwiftUI

/// Top-down (`y = 0` at top) document container for `NSScrollView` so dynamic
/// content growth at the bottom never shifts the top scroll origin.
@MainActor
final class FlippedDocumentView: NSView {
    override var isFlipped: Bool { true }
}

extension NSViewController {
    /// Presents a SwiftUI view as a sheet without retaining the hosting controller
    /// inside its own dismissal closure.
    func presentSheetView<V: View>(_ builder: (_ dismiss: @escaping @MainActor () -> Void) -> V) {
        weak var weakVc: NSViewController?
        let vc = NSHostingController(rootView: builder { [weak self] in
            Task { @MainActor in
                if let sheet = weakVc { self?.dismiss(sheet) }
            }
        })
        weakVc = vc
        presentAsSheet(vc)
    }
}

struct PlaybackMetadata: Sendable {
    let videoId: String
    let metaId: String
    let mediaType: String
    let title: String
    let subtitle: String?
    let artwork: String?
    let season: Int64?
    let episode: Int64?
    let resumePosition: Double?
    let streamSummary: String?
    let isHdr: Bool
    let bingeGroup: String?
    var rankedStreams: [RankedStreamInfo]
}

enum PlayRequest: Sendable {
    case stream(StreamInfo, metadata: PlaybackMetadata? = nil)
    case file(URL)
    case trailer(url: String, title: String)
    case download(id: Int64, title: String)
}

enum FriendlyError {
    enum RecoveryAction {
        case openAddons
        case openSettings
        case retry
    }

    static func rawMessage(_ error: Error) -> String {
        if let ce = error as? CinmaError {
            switch ce {
            case .Database(let m), .Io(let m), .Network(let m), .Torrent(let m),
                 .Player(let m), .Invalid(let m), .Internal(let m):
                return m
            }
        }
        return error.localizedDescription
    }

    static func actionHint(_ error: Error) -> RecoveryAction? {
        let lower = rawMessage(error).lowercased()
        if lower.contains("no stream") || lower.contains("no addon") || lower.contains("no enabled addon") {
            return .openAddons
        }
        if lower.contains("yt-dlp is not installed") || lower.contains("turned off") || lower.contains("tmdb") {
            return .openSettings
        }
        if lower.contains("timeout") || lower.contains("unreachable") || lower.contains("connection") {
            return .retry
        }
        return nil
    }

    static func explain(_ error: Error) -> String {
        let msg = rawMessage(error)
        let lower = msg.lowercased()
        if lower.contains("no stream") || lower.contains("no enabled addon") {
            return String(localized: "No enabled addon returned streams. Add or enable a stream addon under Profile → Addons.")
        }
        if lower.contains("no peers") || lower.contains("timed out waiting for torrent metadata") {
            return String(localized: "No active peers responded for this torrent. Try another stream with more seeders.")
        }
        if lower.contains("404") || lower.contains("410") || lower.contains("connection refused") || lower.contains("dns") || lower.contains("timed out") {
            return String(localized: "The server did not respond in time. Try another source or check Addons.")
        }
        if lower.contains("yt-dlp is not installed") {
            return String(localized: "yt-dlp is not installed. Install it from Settings → Downloads.")
        }
        if lower.contains("feature `") && lower.contains("turned off") {
            return String(localized: "This feature is turned off. Enable it in Settings to use it.")
        }
        if lower.contains("tmdb") && (lower.contains("401") || lower.contains("unauthorized") || lower.contains("key")) {
            return String(localized: "TMDB API key was rejected. Check your key in Settings.")
        }
        return msg
    }
}

@MainActor
enum CinemaTheme {
    private static let themeAccentKey = "cinma.themeAccent"
    private static let reduceGlassKey = "cinma.reduceGlassEffect"

    static func syncThemePreferences(from prefs: PlayerPreferencesFfi) {
        UserDefaults.standard.set(prefs.themeAccent, forKey: themeAccentKey)
        UserDefaults.standard.set(prefs.reduceGlassEffect, forKey: reduceGlassKey)
    }

    /// Warm-neutral near-black base (§5.15).
    static let background = NSColor(srgbRed: 0x0B / 255, green: 0x0B / 255, blue: 0x0D / 255, alpha: 1)
    static let elevatedBackground = NSColor(srgbRed: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255, alpha: 1)
    static let textPrimary = NSColor(srgbRed: 0xF2 / 255, green: 0xF0 / 255, blue: 0xEC / 255, alpha: 1)
    static let textSecondary = NSColor(srgbRed: 0xC9 / 255, green: 0xC5 / 255, blue: 0xBE / 255, alpha: 1)
    static let textMuted = NSColor(srgbRed: 0xA8 / 255, green: 0xA4 / 255, blue: 0x9C / 255, alpha: 1)
    static var accent: NSColor {
        switch UserDefaults.standard.string(forKey: themeAccentKey) {
        case "crimson": return NSColor(srgbRed: 0xE5 / 255, green: 0x48 / 255, blue: 0x4D / 255, alpha: 1)
        case "cyan": return NSColor(srgbRed: 0x38 / 255, green: 0xBD / 255, blue: 0xF8 / 255, alpha: 1)
        case "emerald": return NSColor(srgbRed: 0x34 / 255, green: 0xD3 / 255, blue: 0x99 / 255, alpha: 1)
        case "violet": return NSColor(srgbRed: 0xA7 / 255, green: 0x8B / 255, blue: 0xFA / 255, alpha: 1)
        default: return NSColor(srgbRed: 0xE8 / 255, green: 0xA3 / 255, blue: 0x3D / 255, alpha: 1)
        }
    }
    static let accentInk = NSColor(srgbRed: 0x1A / 255, green: 0x12 / 255, blue: 0x06 / 255, alpha: 1)
    static let green = NSColor(srgbRed: 0x5C / 255, green: 0xC9 / 255, blue: 0x8A / 255, alpha: 1)
    static let cardBackground = NSColor(white: 1, alpha: 0.04)
    static let cardBorder = NSColor(white: 1, alpha: 0.09)
    static var highlightBackground: NSColor { accent.withAlphaComponent(0.10) }
    static var highlightBorder: NSColor { accent.withAlphaComponent(0.45) }

    static var reduceMotion: Bool {
        NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    }

    static var reduceTransparency: Bool {
        NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
            || UserDefaults.standard.bool(forKey: reduceGlassKey)
    }

    enum GlassKind {
        case regular
        case clear
        case hud
    }

    /// Creates an `NSGlassEffectView` on macOS 26+ or `NSVisualEffectView` fallback
    /// (solid dark surface when Reduce Transparency is enabled).
    static func makeGlassView(kind: GlassKind = .regular, cornerRadius: CGFloat = 18) -> NSView {
        if reduceTransparency {
            let solid = NSView()
            solid.wantsLayer = true
            solid.layer?.backgroundColor = elevatedBackground.withAlphaComponent(0.95).cgColor
            solid.layer?.cornerRadius = cornerRadius
            solid.layer?.borderWidth = 1
            solid.layer?.borderColor = NSColor(white: 1, alpha: 0.14).cgColor
            return solid
        }
        if #available(macOS 26.0, *) {
            if kind == .hud {
                let host = NSView()
                host.wantsLayer = true
                host.layer?.cornerRadius = cornerRadius
                host.layer?.masksToBounds = true
                host.layer?.backgroundColor = NSColor(white: 0.03, alpha: 0.64).cgColor
                host.layer?.borderWidth = 1
                host.layer?.borderColor = NSColor(white: 1, alpha: 0.16).cgColor
                let glass = NSGlassEffectView()
                glass.style = .regular
                glass.cornerRadius = cornerRadius
                glass.tintColor = NSColor(white: 0.05, alpha: 0.72)
                glass.autoresizingMask = [.width, .height]
                host.addSubview(glass)
                return host
            }
            let glass = NSGlassEffectView()
            glass.style = kind == .clear ? .clear : .regular
            glass.cornerRadius = cornerRadius
            glass.tintColor = NSColor(white: 0.10, alpha: 0.36)
            return glass
        }
        let fx = NSVisualEffectView()
        fx.material = kind == .clear ? .underWindowBackground : .hudWindow
        fx.blendingMode = .withinWindow
        fx.state = .active
        fx.wantsLayer = true
        fx.layer?.cornerRadius = cornerRadius
        fx.layer?.masksToBounds = true
        fx.layer?.borderWidth = 1
        fx.layer?.borderColor = NSColor(white: 1, alpha: 0.16).cgColor
        return fx
    }

    static func makeChip(_ text: String, highlighted: Bool = false) -> NSView {
        let box = NSView()
        box.wantsLayer = true
        box.layer?.cornerRadius = 6
        box.layer?.backgroundColor = (highlighted ? accent : NSColor(white: 1, alpha: 0.10)).cgColor
        if !highlighted {
            box.layer?.borderWidth = 1
            box.layer?.borderColor = NSColor(white: 1, alpha: 0.16).cgColor
        }
        let label = NSTextField(labelWithString: text)
        label.font = .systemFont(ofSize: 10.5, weight: .bold)
        label.textColor = highlighted ? accentInk : textPrimary
        label.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 7),
            label.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -7),
            label.topAnchor.constraint(equalTo: box.topAnchor, constant: 3),
            label.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -3),
        ])
        return box
    }

    static func makePillButton(
        title: String,
        symbol: String? = nil,
        primary: Bool,
        target: AnyObject?,
        action: Selector
    ) -> NSButton {
        let button = PillButton(title: title, target: target, action: action)
        button.isBordered = false
        button.wantsLayer = true
        button.focusRingType = .exterior
        button.font = .systemFont(ofSize: 13.5, weight: primary ? .bold : .semibold)
        button.contentTintColor = primary ? accentInk : textPrimary
        if let symbol, let img = NSImage(systemSymbolName: symbol, accessibilityDescription: title) {
            button.image = img
            button.imagePosition = .imageLeading
            button.imageHugsTitle = true
        }
        button.layer?.cornerRadius = 18
        button.layer?.backgroundColor = (primary ? accent : NSColor(white: 1, alpha: 0.10)).cgColor
        if !primary {
            button.layer?.borderWidth = 1
            button.layer?.borderColor = NSColor(white: 1, alpha: 0.18).cgColor
        }
        button.attributedTitle = NSAttributedString(
            string: title,
            attributes: [
                .foregroundColor: primary ? accentInk : textPrimary,
                .font: NSFont.systemFont(ofSize: 13.5, weight: primary ? .bold : .semibold),
            ]
        )
        return button
    }
}

/// A borderless button whose natural size includes the pill's padding, so it
/// is never clipped where no explicit size constraints are set.
@MainActor
final class PillButton: NSButton {
    override var intrinsicContentSize: NSSize {
        let base = super.intrinsicContentSize
        return NSSize(width: base.width + 28, height: max(base.height + 14, 32))
    }

    override func drawFocusRingMask() {
        NSBezierPath(roundedRect: bounds, xRadius: 18, yRadius: 18).fill()
    }
}

/// `NSCollectionView` subclass supporting Return/Space item activation and
/// clean keyboard arrow-key navigation with scrolling.
@MainActor
final class KeyboardCollectionView: NSCollectionView {
    var onActivateSelection: ((IndexPath) -> Void)?

    override var acceptsFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok && selectionIndexPaths.isEmpty && numberOfItems(inSection: 0) > 0 {
            let first = IndexPath(item: 0, section: 0)
            selectItems(at: [first], scrollPosition: .nearestHorizontalEdge)
        }
        return ok
    }

    override func keyDown(with event: NSEvent) {
        // Return (36), Numpad Enter (76), or Space (49) activates selected item.
        if event.keyCode == 36 || event.keyCode == 76 || event.keyCode == 49 {
            if let ip = selectionIndexPaths.first {
                onActivateSelection?(ip)
                return
            }
        }
        super.keyDown(with: event)
        if let ip = selectionIndexPaths.first {
            scrollToItems(at: [ip], scrollPosition: [.nearestHorizontalEdge, .nearestVerticalEdge])
        }
    }
}

/// Hosts a Core Animation `CAGradientLayer` that stays sized to bounds.
@MainActor
final class GradientLayerView: NSView {
    private let gradient = CAGradientLayer()

    init(colors: [NSColor], locations: [NSNumber]? = nil, start: CGPoint, end: CGPoint) {
        super.init(frame: .zero)
        wantsLayer = true
        gradient.colors = colors.map(\.cgColor)
        gradient.locations = locations
        gradient.startPoint = start
        gradient.endPoint = end
        layer?.addSublayer(gradient)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        gradient.frame = bounds
        CATransaction.commit()
    }

    func updateColors(_ colors: [NSColor]) {
        gradient.colors = colors.map(\.cgColor)
    }
}

/// Deterministic cinema backdrop gradient fallback when an item has no artwork yet.
@MainActor
enum ArtworkPlaceholder {
    private static let palettes: [(NSColor, NSColor)] = [
        (NSColor(srgbRed: 0.55, green: 0.23, blue: 0.13, alpha: 1), NSColor(srgbRed: 0.16, green: 0.06, blue: 0.03, alpha: 1)),
        (NSColor(srgbRed: 0.18, green: 0.31, blue: 0.44, alpha: 1), NSColor(srgbRed: 0.05, green: 0.09, blue: 0.13, alpha: 1)),
        (NSColor(srgbRed: 0.24, green: 0.18, blue: 0.39, alpha: 1), NSColor(srgbRed: 0.07, green: 0.05, blue: 0.12, alpha: 1)),
        (NSColor(srgbRed: 0.12, green: 0.37, blue: 0.40, alpha: 1), NSColor(srgbRed: 0.03, green: 0.10, blue: 0.11, alpha: 1)),
        (NSColor(srgbRed: 0.42, green: 0.18, blue: 0.23, alpha: 1), NSColor(srgbRed: 0.11, green: 0.04, blue: 0.06, alpha: 1)),
    ]

    static func gradient(for key: String) -> [CGColor] {
        let hash = key.utf8.reduce(0) { ($0 &* 31) &+ Int($1) }
        let pair = palettes[abs(hash) % palettes.count]
        return [pair.0.cgColor, pair.1.cgColor]
    }
}
