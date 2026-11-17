import AppKit

/// Main menu built from the command registry (§5.11). Ids, contexts and key
/// bindings come from Rust; titles and AppKit actions are presentation.
@MainActor
enum MainMenu {
    private struct Item {
        let title: String
        let action: Selector
        /// `nil` → first responder chain.
        let targetsDelegate: Bool
    }

    private static func item(for id: String) -> Item? {
        switch id {
        case "app.about": Item(title: String(localized: "About Cinma"), action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), targetsDelegate: false)
        case "app.hide": Item(title: String(localized: "Hide Cinma"), action: #selector(NSApplication.hide(_:)), targetsDelegate: false)
        case "app.hide_others": Item(title: String(localized: "Hide Others"), action: #selector(NSApplication.hideOtherApplications(_:)), targetsDelegate: false)
        case "app.show_all": Item(title: String(localized: "Show All"), action: #selector(NSApplication.unhideAllApplications(_:)), targetsDelegate: false)
        case "app.quit": Item(title: String(localized: "Quit Cinma"), action: #selector(NSApplication.terminate(_:)), targetsDelegate: false)
        case "file.open": Item(title: String(localized: "Open File…"), action: #selector(AppDelegate.openFile(_:)), targetsDelegate: true)
        case "edit.undo": Item(title: String(localized: "Undo"), action: Selector(("undo:")), targetsDelegate: false)
        case "edit.redo": Item(title: String(localized: "Redo"), action: Selector(("redo:")), targetsDelegate: false)
        case "edit.cut": Item(title: String(localized: "Cut"), action: #selector(NSText.cut(_:)), targetsDelegate: false)
        case "edit.copy": Item(title: String(localized: "Copy"), action: #selector(NSText.copy(_:)), targetsDelegate: false)
        case "edit.paste": Item(title: String(localized: "Paste"), action: #selector(NSText.paste(_:)), targetsDelegate: false)
        case "edit.select_all": Item(title: String(localized: "Select All"), action: #selector(NSText.selectAll(_:)), targetsDelegate: false)
        case "window.minimize": Item(title: String(localized: "Minimize"), action: #selector(NSWindow.performMiniaturize(_:)), targetsDelegate: false)
        case "window.close": Item(title: String(localized: "Close"), action: #selector(NSWindow.performClose(_:)), targetsDelegate: false)
        default: nil
        }
    }

    private static let layout: [(title: String?, ids: [String?])] = [
        (nil, ["app.about", nil, "app.hide", "app.hide_others", "app.show_all", nil, "app.quit"]),
        (String(localized: "File"), ["file.open"]),
        (String(localized: "Edit"), ["edit.undo", "edit.redo", nil, "edit.cut", "edit.copy", "edit.paste", "edit.select_all"]),
        (String(localized: "Window"), ["window.minimize", "window.close"]),
    ]

    static func make(commands: [CommandInfo], delegate: AppDelegate) -> NSMenu {
        let keys = Dictionary(commands.map { ($0.id, $0.key) }, uniquingKeysWith: { first, _ in first })
        let menu = NSMenu()
        for section in layout {
            let submenu = NSMenu(title: section.title ?? "Cinma")
            for id in section.ids {
                guard let id else {
                    submenu.addItem(.separator())
                    continue
                }
                guard let spec = item(for: id) else { continue }
                let entry = NSMenuItem(title: spec.title, action: spec.action, keyEquivalent: "")
                entry.target = spec.targetsDelegate ? delegate : nil
                entry.identifier = NSUserInterfaceItemIdentifier(id)
                if let combo = keys[id] ?? nil, let (key, modifiers) = KeyCombo.keyEquivalent(combo) {
                    entry.keyEquivalent = key
                    entry.keyEquivalentModifierMask = modifiers
                }
                submenu.addItem(entry)
            }
            let holder = NSMenuItem(title: submenu.title, action: nil, keyEquivalent: "")
            holder.submenu = submenu
            menu.addItem(holder)
            if section.title == String(localized: "Window") {
                NSApp.windowsMenu = submenu
            }
        }
        return menu
    }
}

/// Conversions between AppKit key events and registry combos ("cmd+shift+z").
enum KeyCombo {
    private static let named: [UInt16: String] = [
        49: "space", 123: "left", 124: "right", 125: "down", 126: "up",
        53: "escape", 36: "return", 76: "return", 48: "tab", 51: "delete",
        122: "f1", 120: "f2", 99: "f3", 118: "f4", 96: "f5", 97: "f6",
        98: "f7", 100: "f8", 101: "f9", 109: "f10", 103: "f11", 111: "f12",
    ]

    static func from(_ event: NSEvent) -> String? {
        let key: String
        if let name = named[event.keyCode] {
            key = name
        } else if let chars = event.charactersIgnoringModifiers?.lowercased(), chars.count == 1 {
            key = chars
        } else {
            return nil
        }
        let flags = event.modifierFlags
        var parts: [String] = []
        if flags.contains(.command) { parts.append("cmd") }
        if flags.contains(.control) { parts.append("ctrl") }
        if flags.contains(.option) { parts.append("alt") }
        if flags.contains(.shift) { parts.append("shift") }
        return (parts + [key]).joined(separator: "+")
    }

    static func keyEquivalent(_ combo: String) -> (String, NSEvent.ModifierFlags)? {
        let parts = combo.split(separator: "+").map(String.init)
        guard let key = parts.last else { return nil }
        var flags: NSEvent.ModifierFlags = []
        for modifier in parts.dropLast() {
            switch modifier {
            case "cmd": flags.insert(.command)
            case "ctrl": flags.insert(.control)
            case "alt": flags.insert(.option)
            case "shift": flags.insert(.shift)
            default: return nil
            }
        }
        let special: [String: Int] = [
            "left": NSLeftArrowFunctionKey, "right": NSRightArrowFunctionKey,
            "up": NSUpArrowFunctionKey, "down": NSDownArrowFunctionKey,
            "f1": NSF1FunctionKey, "f2": NSF2FunctionKey, "f3": NSF3FunctionKey, "f4": NSF4FunctionKey,
            "f5": NSF5FunctionKey, "f6": NSF6FunctionKey, "f7": NSF7FunctionKey, "f8": NSF8FunctionKey,
            "f9": NSF9FunctionKey, "f10": NSF10FunctionKey, "f11": NSF11FunctionKey, "f12": NSF12FunctionKey,
        ]
        switch key {
        case "space": return (" ", flags)
        case "escape": return ("\u{1b}", flags)
        case "return": return ("\r", flags)
        case "tab": return ("\t", flags)
        case "delete": return ("\u{8}", flags)
        default:
            if let code = special[key], let scalar = UnicodeScalar(code) {
                return (String(Character(scalar)), flags)
            }
            return key.count == 1 ? (key, flags) : nil
        }
    }
}
