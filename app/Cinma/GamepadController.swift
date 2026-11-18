import AppKit
import GameController

/// Event-driven Gamepad / Controller router (`GameController.framework`).
/// Consumes zero CPU when no hardware controller is connected and detaches
/// cleanly when disabled or unplugged.
@MainActor
final class GamepadController {
    static let shared = GamepadController()

    private var enabled = true
    private var observersInstalled = false
    private var connectedObservers: [NSObjectProtocol] = []
    private var lastStickFireAt: TimeInterval = 0
    private var lastDir: Direction?

    var onStatus: ((String) -> Void)?
    var activePlayer: (() -> PlayerViewController?)?
    var activeShell: (() -> MainShellViewController?)?

    private enum Direction: Equatable {
        case left, right, up, down
    }

    private init() {}

    func configure(core: CinmaCore) {
        let on = (try? core.playerPreferences())?.gamepadSupportEnabled ?? true
        syncEnabled(on)
    }

    func syncEnabled(_ isEnabled: Bool) {
        enabled = isEnabled
        if isEnabled {
            installObserversIfNeeded()
            for controller in GCController.controllers() {
                attach(controller)
            }
        } else {
            removeObservers()
            for controller in GCController.controllers() {
                controller.extendedGamepad?.valueChangedHandler = nil
                controller.microGamepad?.valueChangedHandler = nil
            }
        }
    }

    private func installObserversIfNeeded() {
        guard !observersInstalled else { return }
        observersInstalled = true
        let nc = NotificationCenter.default
        connectedObservers = [
            nc.addObserver(forName: .GCControllerDidConnect, object: nil, queue: .main) { [weak self] note in
                let vendor = (note.object as? GCController)?.vendorName
                Task { @MainActor [weak self] in
                    guard let self, self.enabled else { return }
                    for controller in GCController.controllers() {
                        self.attach(controller)
                    }
                    let name = vendor ?? String(localized: "Controller")
                    self.onStatus?(String(localized: "Gamepad connected: \(name)"))
                }
            },
            nc.addObserver(forName: .GCControllerDidDisconnect, object: nil, queue: .main) { [weak self] note in
                let vendor = (note.object as? GCController)?.vendorName
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    let name = vendor ?? String(localized: "Controller")
                    self.onStatus?(String(localized: "Gamepad disconnected: \(name)"))
                }
            },
        ]
    }

    private func removeObservers() {
        for obs in connectedObservers {
            NotificationCenter.default.removeObserver(obs)
        }
        connectedObservers.removeAll()
        observersInstalled = false
    }

    private func attach(_ controller: GCController) {
        controller.handlerQueue = .main

        if let pad = controller.extendedGamepad {
            pad.buttonA.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleConfirm() }
            }
            pad.buttonB.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleBack() }
            }
            pad.buttonX.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleSecondaryX() }
            }
            pad.buttonY.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleSecondaryY() }
            }
            pad.leftShoulder.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleShoulder(forward: false) }
            }
            pad.rightShoulder.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleShoulder(forward: true) }
            }
            pad.leftTrigger.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleTrigger(forward: false) }
            }
            pad.rightTrigger.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleTrigger(forward: true) }
            }
            pad.buttonMenu.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleMenu() }
            }
            pad.dpad.valueChangedHandler = { [weak self] _, x, y in
                Task { @MainActor [weak self] in self?.handleAxisInput(x: x, y: y, isStick: false) }
            }
            pad.leftThumbstick.valueChangedHandler = { [weak self] _, x, y in
                Task { @MainActor [weak self] in self?.handleAxisInput(x: x, y: y, isStick: true) }
            }
        } else if let micro = controller.microGamepad {
            micro.buttonA.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleConfirm() }
            }
            micro.buttonX.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleSecondaryX() }
            }
            micro.buttonMenu.pressedChangedHandler = { [weak self] _, _, pressed in
                guard pressed else { return }
                Task { @MainActor [weak self] in self?.handleBack() }
            }
            micro.dpad.valueChangedHandler = { [weak self] _, x, y in
                Task { @MainActor [weak self] in self?.handleAxisInput(x: x, y: y, isStick: true) }
            }
        }
    }

    // MARK: - Dispatch

    private func handleAxisInput(x: Float, y: Float, isStick: Bool) {
        guard enabled else { return }
        let threshold: Float = isStick ? 0.62 : 0.5
        let dir: Direction?
        if abs(x) >= abs(y), abs(x) >= threshold {
            dir = x > 0 ? .right : .left
        } else if abs(y) > abs(x), abs(y) >= threshold {
            dir = y > 0 ? .up : .down
        } else {
            dir = nil
        }

        guard let dir else {
            lastDir = nil
            return
        }
        let now = Date.timeIntervalSinceReferenceDate
        let repeatInterval: TimeInterval = isStick ? 0.20 : 0.16
        if dir == lastDir && (now - lastStickFireAt) < repeatInterval {
            return
        }
        lastDir = dir
        lastStickFireAt = now

        if let player = activePlayer?() {
            switch dir {
            case .left: player.gamepadSeekStep(forward: false)
            case .right: player.gamepadSeekStep(forward: true)
            case .up: player.gamepadAdjustVolume(delta: 5)
            case .down: player.gamepadAdjustVolume(delta: -5)
            }
        } else {
            let keyCode: UInt16 = switch dir {
            case .left: 123
            case .right: 124
            case .down: 125
            case .up: 126
            }
            sendSyntheticKey(keyCode: keyCode)
        }
    }

    private func handleConfirm() {
        guard enabled else { return }
        if let player = activePlayer?() {
            player.gamepadTogglePause()
        } else if let window = NSApp.keyWindow,
                  let btn = window.firstResponder as? NSButton
        {
            btn.performClick(nil)
        } else {
            sendSyntheticKey(keyCode: 36) // Return
        }
    }

    private func handleBack() {
        guard enabled else { return }
        if let player = activePlayer?() {
            player.gamepadBackOrClose()
        } else if let window = NSApp.keyWindow, window.attachedSheet != nil {
            sendSyntheticKey(keyCode: 53) // Esc closes active sheet
        } else if let shell = activeShell?(), shell.gamepadBack() {
            return
        } else {
            sendSyntheticKey(keyCode: 53)
        }
    }

    private func handleSecondaryX() {
        guard enabled else { return }
        if let player = activePlayer?() {
            player.gamepadSkipOrHud()
        } else {
            sendSyntheticKey(keyCode: 49) // Space / preview
        }
    }

    private func handleSecondaryY() {
        guard enabled else { return }
        if let player = activePlayer?() {
            player.gamepadCycleSubtitle()
        } else {
            activeShell?()?.openSettings()
        }
    }

    private func handleShoulder(forward: Bool) {
        guard enabled else { return }
        if let player = activePlayer?() {
            player.gamepadSeekStep(forward: forward)
        } else {
            activeShell?()?.gamepadStepTab(delta: forward ? 1 : -1)
        }
    }

    private func handleTrigger(forward: Bool) {
        guard enabled else { return }
        if let player = activePlayer?() {
            player.gamepadStepChapter(forward: forward)
        }
    }

    private func handleMenu() {
        guard enabled else { return }
        if let player = activePlayer?() {
            player.gamepadShowHud()
        } else {
            activeShell?()?.gamepadFocusSearch()
        }
    }

    private func sendSyntheticKey(keyCode: UInt16) {
        guard let window = NSApp.keyWindow else { return }
        let chars: String = switch keyCode {
        case 36: "\r"
        case 49: " "
        case 53: "\u{1B}"
        case 123: String(UnicodeScalar(NSLeftArrowFunctionKey)!)
        case 124: String(UnicodeScalar(NSRightArrowFunctionKey)!)
        case 125: String(UnicodeScalar(NSDownArrowFunctionKey)!)
        case 126: String(UnicodeScalar(NSUpArrowFunctionKey)!)
        default: ""
        }
        guard let down = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber,
            context: nil,
            characters: chars,
            charactersIgnoringModifiers: chars,
            isARepeat: false,
            keyCode: keyCode
        ) else { return }
        window.sendEvent(down)
    }
}
