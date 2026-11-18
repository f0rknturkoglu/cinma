import AppKit
import CoreAudio
import SwiftUI
import UniformTypeIdentifiers
import VisionKit

@MainActor
extension PlayerViewController {
    // MARK: - Actions & Keyboard Shortcuts

    @objc func toggleStats() {
        statsVisible.toggle()
        statsCard.isHidden = !statsVisible
        statsTask?.cancel()
        guard statsVisible else { return }
        statsTask = Task { [weak self] in
            while let self, self.statsVisible, !Task.isCancelled {
                if let s = self.session?.stats() {
                    let ab = (s.abLoopA, s.abLoopB)
                    let abStr = (ab.0 != nil || ab.1 != nil)
                        ? "A: \(ab.0.map(Self.format) ?? "—")  B: \(ab.1.map(Self.format) ?? "—")"
                        : String(localized: "Off")
                    self.statsText.stringValue = """
                    Video: \(s.videoCodec) (\(s.width)×\(s.height)) · hwdec: \(s.hwdec)
                    FPS: \(String(format: "%.2f", s.fps)) (est \(String(format: "%.2f", s.estimatedFps))) · Gamma: \(s.gamma)
                    Audio: \(s.audioCodec) (\(s.audioChannels)ch)
                    Cache: \(String(format: "%.1fs", s.cacheSeconds)) · Drops: \(s.droppedFrames)
                    Speed: \(String(format: "%.2gx", s.speed)) · A/B: \(abStr)
                    Delays: Sub \(String(format: "%+.2fs", s.subDelay)) · Audio \(String(format: "%+.2fs", s.audioDelay))
                    """
                }
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    @objc func takeScreenshot() {
        guard let session else { return }
        let desktop = FileManager.default.urls(for: .desktopDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        let formatter = ISO8601DateFormatter()
        let stamp = formatter.string(from: Date()).replacingOccurrences(of: ":", with: "-")
        let out = desktop.appendingPathComponent("Cinma-\(stamp).png")
        let includeSubs = currentPrefs()?.screenshotIncludeSubtitles ?? true
        try? session.screenshotToFileMode(path: out.path, includeSubtitles: includeSubs)
        showStatus(String(localized: "Saved screenshot: \(out.lastPathComponent)"))
    }

    @objc func cycleAbLoop() {
        if abLoopA == nil {
            abLoopA = position
            try? session?.setAbLoopA(seconds: position)
            performHaptic(.alignment)
            showStatus(String(localized: "A/B loop: A = \(Self.format(position))"))
        } else if abLoopB == nil {
            abLoopB = position
            try? session?.setAbLoopB(seconds: position)
            performHaptic(.alignment)
            showStatus(String(localized: "A/B loop: A = \(Self.format(abLoopA ?? 0)), B = \(Self.format(position))"))
        } else {
            abLoopA = nil
            abLoopB = nil
            try? session?.setAbLoopA(seconds: nil)
            try? session?.setAbLoopB(seconds: nil)
            showStatus(String(localized: "A/B Loop cleared"))
        }
        scrubBar.abLoopA = abLoopA
        scrubBar.abLoopB = abLoopB
    }

    @objc func toggleFullscreen() {
        view.window?.toggleFullScreen(nil)
    }

    func setSpeed(_ val: Double) {
        speed = min(max(val, 0.25), 4.0)
        speedButton.title = String(format: "%.2g×", speed)
        try? session?.setSpeed(speed: speed)
        pushNowPlaying()
    }

    @objc func toggleMute() {
        guard !isScreenLocked else { return }
        muted.toggle()
        try? session?.setMuted(muted: muted)
        volumeButton.image = NSImage(
            systemSymbolName: muted ? "speaker.slash.fill" : "speaker.wave.2.fill",
            accessibilityDescription: nil
        )
        showVolumeHudIfEnabled()
    }

    func showVolumeHudIfEnabled() {
        guard currentPrefs()?.volumeHud ?? true else { return }
        let msg = muted
            ? String(localized: "Muted")
            : String(localized: "Volume: \(Int(volume.rounded()))%")
        showStatus(msg, autoHideAfter: 1.8)
    }

    func toggleSubtitlesVisibility() {
        subtitlesEnabled.toggle()
        sessionSubEnabled = subtitlesEnabled
        if subtitlesEnabled {
            let target = lastSelectedSid ?? tracks.first(where: { $0.trackType == "sub" })?.id
            if let target, let chosen = tracks.first(where: { $0.trackType == "sub" && $0.id == target }) {
                sessionSubLang = chosen.lang
                sessionSubTitle = chosen.title
            }
            try? session?.setSubtitleTrack(id: target)
            if let secondarySid {
                try? session?.setSecondarySubtitleTrack(id: secondarySid)
            }
        } else {
            lastSelectedSid = tracks.first(where: { $0.trackType == "sub" && $0.selected })?.id ?? lastSelectedSid
            try? session?.setSubtitleTrack(id: nil)
            try? session?.setSecondarySubtitleTrack(id: nil)
        }
        showStatus(subtitlesEnabled ? String(localized: "Subtitles on") : String(localized: "Subtitles off"))
    }

    func cycleTrack(type: String, isSub: Bool) {
        let group = tracks.filter { $0.trackType == type }
        guard !group.isEmpty else { return }
        let idx = group.firstIndex(where: { $0.selected }) ?? -1
        let next = group[(idx + 1) % group.count]
        if isSub {
            lastSelectedSid = next.id
            subtitlesEnabled = true
            sessionSubEnabled = true
            sessionSubLang = next.lang
            sessionSubTitle = next.title
            if !next.external {
                activeSubtitleFilePath = nil
            }
            try? session?.setSubtitleTrack(id: next.id)
        } else {
            sessionAudioLang = next.lang
            sessionAudioTitle = next.title
            try? session?.setAudioTrack(id: next.id)
        }
        tracks = (try? session?.tracks()) ?? []
        showStatus("\(type.uppercased()): \(next.lang ?? next.title ?? "Track \(next.id)")")
    }

    func handleKey(_ event: NSEvent) -> Bool {
        showHudAndResetTimer()
        guard let combo = KeyCombo.from(event) else { return false }
        let resolvedCommand = try? core.resolveKey(context: "player", combo: combo)
        if isScreenLocked {
            if resolvedCommand == "player.screen_lock" {
                toggleScreenLock()
            }
            return true
        }
        autoPlayedEpisodesCount = 0
        guard let command = resolvedCommand else {
            if combo == "p" {
                enterPiPClicked()
                return true
            }
            return false
        }
        if event.isARepeat && command == "player.toggle_pause" {
            return true
        }
        switch command {
        case "player.toggle_pause":
            if currentPrefs()?.holdToBoostEnabled ?? true {
                holdBoostKeyWaiting = combo
                beginHoldBoostCandidate(triggerKey: combo)
            } else {
                togglePause()
            }
        case "player.seek_back": seekBack10()
        case "player.seek_forward": seekForward10()
        case "player.close":
            if popoverBackdrop != nil {
                dismissFloatingPopover()
            } else if view.window?.styleMask.contains(.fullScreen) == true {
                toggleFullscreen()
            } else {
                requestClose()
            }
        case "player.fullscreen": toggleFullscreen()
        case "player.toggle_subtitles": toggleSubtitlesVisibility()
        case "player.cycle_subtitle": cycleTrack(type: "sub", isSub: true)
        case "player.cycle_audio": cycleTrack(type: "audio", isSub: false)
        case "player.screenshot": takeScreenshot()
        case "player.stats": toggleStats()
        case "player.ab_loop": cycleAbLoop()
        case "player.speed_up": setSpeed(speed + 0.25)
        case "player.speed_down": setSpeed(speed - 0.25)
        case "player.frame_step":
            removeLiveTextOverlay()
            try? session?.frameStep(forward: true)
        case "player.frame_back_step":
            removeLiveTextOverlay()
            try? session?.frameStep(forward: false)
        case "player.sub_seek_prev": try? session?.subSeek(offset: -1)
        case "player.sub_seek_next": try? session?.subSeek(offset: 1)
        case "player.search_dialogue": showDialogueSearchPopover()
        case "player.live_text": toggleLiveTextOverlay()
        case "player.screen_lock":
            if currentPrefs()?.screenLockEnabled ?? true {
                toggleScreenLock()
            }
        case "player.volume_up": adjustVolume(by: 10)
        case "player.volume_down": adjustVolume(by: -10)
        case "player.mute": toggleMute()
        case "player.next_episode": playNextEpisodeManually()
        default: return false
        }
        return true
    }

    func handleKeyUp(_ event: NSEvent) -> Bool {
        guard let expected = holdBoostKeyWaiting,
              let combo = KeyCombo.from(event),
              combo == expected
        else { return false }
        let boosted = endHoldBoost(triggerKey: combo)
        if !boosted && !isScreenLocked {
            togglePause()
        }
        return true
    }

    func adjustVolume(by delta: Double) {
        let maxVol = Double(currentPrefs()?.volumeBoostMax ?? 150)
        let prev = volume
        volume = min(maxVol, max(0, prev + delta))
        if (prev < 100 && volume >= 100) || (prev > 100 && volume <= 100) {
            performHaptic(.levelChange)
        }
        try? session?.setVolumeMax(maxPct: UInt32(maxVol))
        try? session?.setVolume(volume: volume)
        showVolumeHudIfEnabled()
    }

    func isPointOnHud(_ point: NSPoint) -> Bool {
        if popoverBackdrop != nil { return true }
        let interactiveViews: [NSView] = [titlePill, bar, skipButton, nextCard, unlockPill, clockPill, statsCard]
        return interactiveViews.contains { !$0.isHidden && $0.frame.contains(point) }
    }

    func beginHoldBoostCandidate(triggerKey: String?) {
        holdBoostTask?.cancel()
        guard !isScreenLocked, !closing, !holdBoostActive else { return }
        let prefs = currentPrefs()
        guard prefs?.holdToBoostEnabled ?? true else { return }
        let boostPct = prefs?.holdBoostSpeedPct ?? 200
        holdBoostTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(260))
            guard let self, !Task.isCancelled, !self.closing, !self.isScreenLocked else { return }
            self.speedBeforeHoldBoost = self.speed
            self.holdBoostActive = true
            let boostSpeed = Double(max(125, min(300, boostPct))) / 100.0
            self.setSpeed(boostSpeed)
            self.performHaptic(.alignment)
            self.showStatus("⏩ \(String(format: "%.2g×", boostSpeed))", autoHideAfter: 0)
        }
    }

    func endHoldBoost(triggerKey: String?) -> Bool {
        holdBoostTask?.cancel()
        holdBoostTask = nil
        if triggerKey == nil || holdBoostKeyWaiting == triggerKey {
            holdBoostKeyWaiting = nil
        }
        guard holdBoostActive else { return false }
        holdBoostActive = false
        setSpeed(speedBeforeHoldBoost)
        showStatus("", autoHideAfter: 0)
        return true
    }

    func handleScrollGesture(_ event: NSEvent) {
        if event.phase == .ended || event.phase == .cancelled {
            scrollAccumX = 0
            scrollAccumY = 0
            return
        }
        guard event.momentumPhase == [] else { return }
        guard !isScreenLocked, !closing, popoverBackdrop == nil else { return }
        let prefs = currentPrefs()
        guard prefs?.trackpadGesturesEnabled ?? true else { return }

        let dx = event.scrollingDeltaX
        let dy = event.scrollingDeltaY
        if event.hasPreciseScrollingDeltas, abs(dx) > abs(dy) * 1.35 {
            let signedDx = event.isDirectionInvertedFromDevice ? dx : -dx
            scrollAccumX += signedDx
            let threshold: CGFloat = 24
            if abs(scrollAccumX) >= threshold {
                let steps = Int(scrollAccumX / threshold)
                scrollAccumX -= CGFloat(steps) * threshold
                let exact = prefs?.exactSeek ?? true
                let deltaS = Double(steps) * 2.0
                removeLiveTextOverlay()
                try? session?.seekRelativeMode(seconds: deltaS, exact: exact)
                saveProgressIfNeeded(force: false)
                showStatus(deltaS >= 0 ? "+\(Int(deltaS))s" : "\(Int(deltaS))s", autoHideAfter: 1.2)
            }
        } else if abs(dy) > 0.1 {
            if event.hasPreciseScrollingDeltas {
                let signedDy = event.isDirectionInvertedFromDevice ? -dy : dy
                scrollAccumY += signedDy
                let threshold: CGFloat = 14
                if abs(scrollAccumY) >= threshold {
                    let steps = Int(scrollAccumY / threshold)
                    scrollAccumY -= CGFloat(steps) * threshold
                    adjustVolume(by: Double(steps) * 3.0)
                }
            } else {
                adjustVolume(by: (dy > 0 ? 1 : -1) * 5.0)
            }
        }
    }

    func handleDroppedFile(_ url: URL) {
        guard !closing, !isScreenLocked else { return }
        let ext = url.pathExtension.lowercased()
        if ["srt", "ass", "ssa", "vtt"].contains(ext) {
            loadLocalSubtitle(url: url)
            return
        }
        request = .file(url)
        thumbCache?.clear()
        thumbCache = ThumbnailPreviewCache(localURL: url)
        duration = 0
        position = 0
        appliedAutoTracks = false
        abLoopA = nil
        abLoopB = nil
        scrubBar.abLoopA = nil
        scrubBar.abLoopB = nil
        chapters = []
        scrubBar.chapters = []
        activeSubtitleFilePath = nil
        nextPlan = nil
        setNextCardVisible(false)
        nextDismissed = false
        sleepEndOfEpisode = false
        refreshTitles()
        showStatus(String(localized: "Loading \(url.lastPathComponent)…"), autoHideAfter: 0)
        let session = session
        Task { [weak self] in
            do {
                try session?.playUrl(url: url.path)
            } catch {
                await MainActor.run {
                    self?.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                }
            }
        }
    }

    func performHaptic(_ pattern: NSHapticFeedbackManager.FeedbackPattern = .generic) {
        guard currentPrefs()?.hapticFeedbackEnabled ?? true else { return }
        NSHapticFeedbackManager.defaultPerformer.perform(pattern, performanceTime: .default)
    }

    @objc func showDialogueSearchPopover() {
        guard let anchor = moreButton else { return }
        let root = DialogueSearchPopoverView(
            core: core,
            subtitlePath: activeSubtitleFilePath,
            subDelay: subDelay,
            onSeek: { [weak self] secs in
                self?.seekAbsolute(secs)
            },
            onSubSeek: { [weak self] offset in
                try? self?.session?.subSeek(offset: offset)
            },
            onDismiss: { [weak self] in
                self?.dismissFloatingPopover()
            }
        )
        presentFloatingPopover(root, relativeTo: anchor)
    }

    @objc func toggleBlackoutOtherScreens() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.blackoutOtherScreens.toggle()
        cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        syncMonitorBlackout()
        showStatus(prefs.blackoutOtherScreens
            ? String(localized: "Other monitors dimmed in fullscreen")
            : String(localized: "Monitor blackout off"))
    }

    func syncMonitorBlackout() {
        clearBlackoutWindows()
        guard !closing,
              let window = view.window,
              window.styleMask.contains(.fullScreen),
              currentPrefs()?.blackoutOtherScreens == true
        else { return }
        for screen in NSScreen.screens where screen != window.screen {
            let win = NSWindow(
                contentRect: screen.frame,
                styleMask: .borderless,
                backing: .buffered,
                defer: false
            )
            win.backgroundColor = .black
            win.isOpaque = true
            win.ignoresMouseEvents = true
            win.level = .floating
            win.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            win.orderFront(nil)
            blackoutWindows.append(win)
        }
    }

    func clearBlackoutWindows() {
        for win in blackoutWindows {
            win.orderOut(nil)
        }
        blackoutWindows.removeAll()
    }

    func handleWindowOcclusionChanged() {
        guard !closing, !paused, !isInPiP,
              let window = view.window,
              !window.isMiniaturized,
              !window.occlusionState.contains(.visible),
              currentPrefs()?.autoPipOnOcclusion == true
        else { return }
        clearBlackoutWindows()
        enterPiPClicked()
    }

    func installAudioRouteObserver() {
        guard audioRouteListener == nil else { return }
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
            Task { @MainActor in
                guard let self, !self.closing, !self.paused,
                      self.currentPrefs()?.pauseOnAudioOutputChange ?? true
                else { return }
                try? self.session?.setPaused(paused: true)
                self.showStatus(String(localized: "Paused — audio output changed"))
            }
        }
        audioRouteListener = block
        AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &audioRouteAddress,
            DispatchQueue.main,
            block
        )
    }

    func removeAudioRouteObserver() {
        guard let block = audioRouteListener else { return }
        AudioObjectRemovePropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &audioRouteAddress,
            DispatchQueue.main,
            block
        )
        audioRouteListener = nil
    }

    @objc func toggleLiveTextOverlay() {
        if liveTextOverlay != nil {
            removeLiveTextOverlay()
            return
        }
        guard !closing, !isScreenLocked, ImageAnalyzer.isSupported, let session else {
            showStatus(String(localized: "Live Text unavailable on this frame"))
            return
        }
        if !paused {
            try? session.setPaused(paused: true)
        }
        liveTextTask?.cancel()
        showStatus(String(localized: "Scanning text in frame…"), autoHideAfter: 0)
        liveTextTask = Task { [weak self] in
            let cgImage: CGImage? = await Task.detached(priority: .userInitiated) {
                guard let frame = try? session.captureRawFrame() else { return nil }
                return Self.makeCGImage(from: frame)
            }.value
            guard let self, !Task.isCancelled, !self.closing else { return }
            guard let cgImage else {
                self.showStatus(String(localized: "Could not capture paused frame for Live Text"))
                return
            }
            let analyzer = ImageAnalyzer()
            let analysis = try? await analyzer.analyze(
                cgImage,
                orientation: .up,
                configuration: ImageAnalyzer.Configuration([.text])
            )
            guard !Task.isCancelled, !self.closing else { return }
            guard let analysis, analysis.hasResults(for: .text) else {
                self.showStatus(String(localized: "No text detected in paused frame"))
                return
            }
            let targetRect = Self.aspectFitRect(
                content: CGSize(width: cgImage.width, height: cgImage.height),
                in: self.videoView.bounds
            )
            let overlay = ImageAnalysisOverlayView(frame: targetRect)
            overlay.autoresizingMask = [.width, .height, .minXMargin, .maxXMargin, .minYMargin, .maxYMargin]
            overlay.analysis = analysis
            overlay.preferredInteractionTypes = .textSelection
            self.videoView.addSubview(overlay)
            self.liveTextOverlay = overlay
            self.showStatus(String(localized: "Live Text ready — drag to select text (Press T to clear)"))
        }
    }

    static func aspectFitRect(content: CGSize, in bounds: CGRect) -> CGRect {
        guard content.width > 0, content.height > 0, bounds.width > 0, bounds.height > 0 else {
            return bounds
        }
        let scale = min(bounds.width / content.width, bounds.height / content.height)
        let w = content.width * scale
        let h = content.height * scale
        return CGRect(
            x: bounds.midX - w * 0.5,
            y: bounds.midY - h * 0.5,
            width: w,
            height: h
        )
    }

    func removeLiveTextOverlay() {
        liveTextTask?.cancel()
        liveTextTask = nil
        liveTextOverlay?.removeFromSuperview()
        liveTextOverlay = nil
    }

    nonisolated private static func makeCGImage(from frame: RawFrameFfi) -> CGImage? {
        let w = Int(frame.width)
        let h = Int(frame.height)
        let stride = Int(frame.stride)
        guard w > 0, h > 0, stride >= w * 4, frame.bgr0.count >= h * stride else { return nil }
        let cfData = Data(frame.bgr0) as CFData
        guard let provider = CGDataProvider(data: cfData) else { return nil }
        let bitmapInfo = CGBitmapInfo(
            rawValue: CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.noneSkipFirst.rawValue
        )
        return CGImage(
            width: w,
            height: h,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: stride,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: bitmapInfo,
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        )
    }

    @objc func toggleScreenLock() {
        isScreenLocked.toggle()
        lockButton.image = NSImage(
            systemSymbolName: isScreenLocked ? "lock.fill" : "lock.open",
            accessibilityDescription: isScreenLocked ? String(localized: "Unlock Screen (⇧L)") : String(localized: "Lock Screen (⇧L)")
        )
        if isScreenLocked {
            dismissFloatingPopover()
            thumbCard.isHidden = true
            skipButton.isHidden = true
            setNextCardVisible(false)
            titlePill.isHidden = true
            bar.isHidden = true
            clockPill.isHidden = true
            unlockPill.isHidden = false
            unlockPill.alphaValue = 1.0
            showHudAndResetTimer()
        } else {
            unlockPill.isHidden = true
            showHudAndResetTimer()
            updateSkipSegmentVisibility()
            updateClockPill()
        }
    }

    @objc func requestClose() {
        guard !closing else { return }
        let shouldConfirm = currentPrefs()?.confirmOnLeave ?? false
        guard shouldConfirm, !paused, duration > 0 else {
            close()
            return
        }
        let alert = NSAlert()
        alert.messageText = String(localized: "Leave Playback?")
        alert.informativeText = String(localized: "Are you sure you want to close active playback?")
        alert.addButton(withTitle: String(localized: "Leave"))
        alert.addButton(withTitle: String(localized: "Continue Watching"))
        if let window = view.window {
            Task { @MainActor in
                let resp = await alert.beginSheetModal(for: window)
                if resp == .alertFirstButtonReturn { self.close() }
            }
        } else if alert.runModal() == .alertFirstButtonReturn {
            close()
        }
    }

    @objc func togglePause() {
        guard !isScreenLocked else { return }
        pausedBySystemEvent = false
        try? session?.togglePause()
    }

    @objc func seekBack10() {
        guard !isScreenLocked else { return }
        autoPlayedEpisodesCount = 0
        removeLiveTextOverlay()
        let p = currentPrefs()
        let exact = p?.exactSeek ?? true
        let step = Double(max(3, min(60, p?.seekBackStepS ?? 10)))
        try? session?.seekRelativeMode(seconds: -step, exact: exact)
        saveProgressIfNeeded(force: true)
    }

    @objc func seekForward10() {
        guard !isScreenLocked else { return }
        autoPlayedEpisodesCount = 0
        removeLiveTextOverlay()
        let p = currentPrefs()
        let exact = p?.exactSeek ?? true
        let step = Double(max(3, min(60, p?.seekForwardStepS ?? 10)))
        try? session?.seekRelativeMode(seconds: step, exact: exact)
        saveProgressIfNeeded(force: true)
    }

    func seekAbsolute(_ seconds: Double) {
        removeLiveTextOverlay()
        position = seconds
        scrubBar.position = seconds
        try? session?.seekAbsolute(seconds: seconds)
        saveProgressIfNeeded(force: true)
        pushNowPlaying()
    }

}
