import AppKit

@MainActor
enum Theme {
    /// Warm-neutral near-black base (§5.15).
    static let background = NSColor(srgbRed: 0x0B / 255, green: 0x0B / 255, blue: 0x0D / 255, alpha: 1)
}

@MainActor
enum MainWindow {
    static func make(content: NSViewController) -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "Cinma"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.appearance = NSAppearance(named: .darkAqua)
        window.backgroundColor = Theme.background
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 720, height: 480)
        window.contentViewController = content
        window.setContentSize(NSSize(width: 1280, height: 800))
        window.center()
        window.setFrameAutosaveName("MainWindow")
        return window
    }
}

/// Phase 0 placeholder for the Home screen.
@MainActor
final class HomeViewController: NSViewController {
    private let statusLabel = NSTextField(labelWithString: String(localized: "Loading…"))

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = Theme.background.cgColor

        let title = NSTextField(labelWithString: "Cinma")
        title.font = .systemFont(ofSize: 56, weight: .bold)
        title.textColor = .white

        statusLabel.font = .monospacedDigitSystemFont(ofSize: 13, weight: .regular)
        statusLabel.textColor = .secondaryLabelColor

        let stack = NSStackView(views: [title, statusLabel])
        stack.orientation = .vertical
        stack.alignment = .centerX
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: root.centerYAnchor),
        ])
        view = root
    }

    func showStatus(_ text: String) {
        statusLabel.stringValue = text
    }
}
