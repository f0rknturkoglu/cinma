import AppKit

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
    let rankedStreams: [RankedStreamInfo]
}

enum PlayRequest: Sendable {
    case stream(StreamInfo, metadata: PlaybackMetadata? = nil)
    case file(URL)
    case trailer(url: String, title: String)
}

@MainActor
enum CinemaTheme {
    /// Warm-neutral near-black base (§5.15).
    static let background = NSColor(srgbRed: 0x0B / 255, green: 0x0B / 255, blue: 0x0D / 255, alpha: 1)
    static let elevatedBackground = NSColor(srgbRed: 0x16 / 255, green: 0x16 / 255, blue: 0x1A / 255, alpha: 1)
    static let textPrimary = NSColor(srgbRed: 0xF2 / 255, green: 0xF0 / 255, blue: 0xEC / 255, alpha: 1)
    static let textSecondary = NSColor(srgbRed: 0xC9 / 255, green: 0xC5 / 255, blue: 0xBE / 255, alpha: 1)
    static let textMuted = NSColor(srgbRed: 0xA8 / 255, green: 0xA4 / 255, blue: 0x9C / 255, alpha: 1)
    static let accent = NSColor(srgbRed: 0xE8 / 255, green: 0xA3 / 255, blue: 0x3D / 255, alpha: 1)
    static let accentInk = NSColor(srgbRed: 0x1A / 255, green: 0x12 / 255, blue: 0x06 / 255, alpha: 1)
    static let green = NSColor(srgbRed: 0x5C / 255, green: 0xC9 / 255, blue: 0x8A / 255, alpha: 1)
    static let cardBackground = NSColor(white: 1, alpha: 0.04)
    static let cardBorder = NSColor(white: 1, alpha: 0.09)
    static let highlightBackground = NSColor(srgbRed: 0xE8 / 255, green: 0xA3 / 255, blue: 0x3D / 255, alpha: 0.10)
    static let highlightBorder = NSColor(srgbRed: 0xE8 / 255, green: 0xA3 / 255, blue: 0x3D / 255, alpha: 0.45)

    static var reduceMotion: Bool {
        NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    }

    static var reduceTransparency: Bool {
        NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
    }

    enum GlassKind {
        case regular
        case clear
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
            let glass = NSGlassEffectView()
            glass.style = kind == .clear ? .clear : .regular
            glass.cornerRadius = cornerRadius
            glass.tintColor = NSColor(white: 0.10, alpha: 0.32)
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
        let button = NSButton(title: title, target: target, action: action)
        button.isBordered = false
        button.wantsLayer = true
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
