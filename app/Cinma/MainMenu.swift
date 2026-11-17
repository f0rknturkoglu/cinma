import AppKit

/// Static main menu. Moves onto the command registry (§5.11) in Phase 2.
@MainActor
enum MainMenu {
    static func make() -> NSMenu {
        let menu = NSMenu()

        let appMenu = NSMenu()
        appMenu.addItem(withTitle: String(localized: "About Cinma"),
                        action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: String(localized: "Hide Cinma"),
                        action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let hideOthers = appMenu.addItem(withTitle: String(localized: "Hide Others"),
                                         action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
        hideOthers.keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(withTitle: String(localized: "Show All"),
                        action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: String(localized: "Quit Cinma"),
                        action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        menu.addItem(submenu: appMenu, title: "Cinma")

        let windowMenu = NSMenu(title: String(localized: "Window"))
        windowMenu.addItem(withTitle: String(localized: "Minimize"),
                           action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: String(localized: "Close"),
                           action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        menu.addItem(submenu: windowMenu, title: windowMenu.title)
        NSApp.windowsMenu = windowMenu

        return menu
    }
}

private extension NSMenu {
    func addItem(submenu: NSMenu, title: String) {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.submenu = submenu
        addItem(item)
    }
}
