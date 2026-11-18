import AppKit
import SwiftUI
import UniformTypeIdentifiers

@MainActor
extension PlayerViewController {
    // MARK: - Hover Thumbnail Preview

    func handleScrubHover(seconds: Double?, localX: CGFloat) {
        guard let seconds, duration > 0 else {
            isHoveringScrub = false
            thumbCard.isHidden = true
            showHudAndResetTimer()
            return
        }
        isHoveringScrub = true
        showHudAndCancelTimer()
        thumbCard.isHidden = false
        let clampedX = min(max(localX, 88), max(88, bar.bounds.width - 88))
        thumbCenterXConstraint?.constant = clampedX
        let chName = chapters.last(where: { $0.startS <= seconds })?.title
        if let chName, !chName.isEmpty {
            thumbTimeLabel.stringValue = "\(Self.format(seconds)) · \(chName)"
        } else {
            thumbTimeLabel.stringValue = Self.format(seconds)
        }
        if let cached = thumbCache?.cachedImage(at: seconds) {
            thumbImageView.image = cached
            return
        }
        thumbCache?.requestThumbnail(at: seconds, session: session) { [weak self] img in
            self?.thumbImageView.image = img
        }
    }

    // MARK: - Popovers & Menus

    @objc func showStreamDetailsPopover(_ sender: NSButton) {
        showHudAndCancelTimer()
        let addon: String? = if case .stream(let s, _) = request { s.addonName } else { nil }
        let view = StreamDetailsPopoverView(
            session: session,
            addonName: addon,
            streamSummary: metadata?.streamSummary
        )
        presentFloatingPopover(view, relativeTo: sender)
    }

    @objc func showStreamSwitcher(_ sender: NSButton) {
        showHudAndCancelTimer()
        let streams = metadata?.rankedStreams ?? []
        let view = StreamSwitcherPopoverView(
            streams: streams,
            activeRaw: activeStreamRaw
        ) { [weak self] selected in
            self?.dismissFloatingPopover()
            self?.switchToStream(selected)
        }
        presentFloatingPopover(view, relativeTo: sender)
    }

    @objc func showSubtitlesPopover(_ sender: NSButton) {
        showHudAndCancelTimer()
        let host = presentFloatingPopover(makeSubtitlesPopoverView(), relativeTo: sender)
        activeSubtitlesHost = host
    }

    func refreshSubtitlesPopoverIfVisible() {
        guard let host = activeSubtitlesHost else { return }
        host.rootView = makeSubtitlesPopoverView()
    }

    func makeSubtitlesPopoverView() -> SubtitleManagerPopoverView {
        let settings = (try? core.subtitleSettings()) ?? SubtitleSettingsFfi(
            preferredLangs: ["tur", "eng"],
            audioLangs: ["eng"],
            dualSecondaryLang: nil,
            srtFont: nil,
            srtFontSize: 46,
            srtColor: "#FFFFFF",
            srtBorderSize: 3,
            srtBorderColor: "#000000",
            srtBackColor: nil,
            srtBold: false,
            srtPos: 95,
            assOverride: false,
            srtOpacity: 100,
            srtBoxOpacity: 0,
            srtAlignX: "center",
            secondaryPos: 12,
            liftAboveControls: true,
            offByDefault: false,
            preferEmbedded: false,
            forcedWhenNativeAudio: false,
            hideSdh: false,
            subScalePct: 100,
            assScaleWithWindow: true,
            enableOpensubtitles: true,
            enableAddonSubtitles: true,
            subHdrPeakNits: 0,
            subUseMargins: true
        )
        return SubtitleManagerPopoverView(
            candidates: onlineSubtitles,
            loading: searchingSubtitles,
            statusMessage: subtitleSearchStatus,
            tracks: tracks,
            secondaryId: secondarySid,
            subDelay: subDelay,
            settings: settings,
            onSearchOnline: { [weak self] in
                guard let self else { return }
                self.searchingSubtitles = true
                self.subtitleSearchStatus = String(localized: "Searching online subtitles…")
                self.refreshSubtitlesPopoverIfVisible()
                self.scheduleOnlineSubtitleLookup()
            },
            onSelectOnline: { [weak self] cand in
                self?.dismissFloatingPopover()
                self?.loadOnlineSubtitle(cand)
            },
            onLoadLocal: { [weak self] in
                self?.dismissFloatingPopover()
                self?.openLocalSubtitleFile()
            },
            onSelectPrimary: { [weak self] id in
                guard let self else { return }
                self.lastSelectedSid = id
                self.subtitlesEnabled = id != nil
                self.sessionSubEnabled = id != nil
                if let id, let chosen = self.tracks.first(where: { $0.trackType == "sub" && $0.id == id }) {
                    self.sessionSubLang = chosen.lang
                    self.sessionSubTitle = chosen.title
                    if !chosen.external {
                        self.activeSubtitleFilePath = nil
                    }
                } else {
                    self.activeSubtitleFilePath = nil
                }
                try? self.session?.setSubtitleTrack(id: id)
                self.tracks = (try? self.session?.tracks()) ?? []
                self.refreshSubtitlesPopoverIfVisible()
            },
            onSelectSecondary: { [weak self] id in
                guard let self else { return }
                self.secondarySid = id
                self.sessionSecondarySubEnabled = id != nil
                if let id, let chosen = self.tracks.first(where: { $0.trackType == "sub" && $0.id == id }) {
                    self.sessionSecondarySubLang = chosen.lang
                } else if id == nil {
                    self.sessionSecondarySubLang = nil
                }
                try? self.session?.setSecondarySubtitleTrack(id: id)
                self.refreshSubtitlesPopoverIfVisible()
            },
            onAdjustDelay: { [weak self] delta in
                self?.adjustSubDelay(delta)
                self?.refreshSubtitlesPopoverIfVisible()
            },
            onResetDelay: { [weak self] in
                self?.setSubDelay(0)
                self?.refreshSubtitlesPopoverIfVisible()
            },
            onUpdateStyle: { [weak self] updated in
                guard let self else { return }
                self.applySubtitleStyle(updated, session: self.session)
                self.pendingStyleSettings = updated
                self.stylePersistTask?.cancel()
                self.stylePersistTask = Task { @MainActor [weak self] in
                    try? await Task.sleep(for: .milliseconds(250))
                    guard let self, !Task.isCancelled else { return }
                    if let pending = self.pendingStyleSettings {
                        self.pendingStyleSettings = nil
                        try? self.core.setSubtitleSettings(settings: pending)
                    }
                }
            }
        )
    }

    /// Presents a Liquid Glass card inside `view` anchored above `anchor` and
    /// clamped within `[leading + 20, trailing - 20]` so it never overflows off-screen.
    @discardableResult
    func presentFloatingPopover<V: View>(_ rootView: V, relativeTo anchor: NSView) -> NSHostingView<V>? {
        guard !isScreenLocked else { return nil }
        dismissFloatingPopover()

        let backdrop = ClickCatcherView()
        backdrop.translatesAutoresizingMaskIntoConstraints = false
        backdrop.onClick = { [weak self] in self?.dismissFloatingPopover() }

        let card = CinemaTheme.makeGlassView(kind: .hud, cornerRadius: 16)
        card.translatesAutoresizingMaskIntoConstraints = false

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(host)

        view.addSubview(backdrop)
        view.addSubview(card)

        let centerAlign = card.centerXAnchor.constraint(equalTo: anchor.centerXAnchor)
        centerAlign.priority = .defaultHigh

        NSLayoutConstraint.activate([
            backdrop.topAnchor.constraint(equalTo: view.topAnchor),
            backdrop.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            backdrop.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            backdrop.trailingAnchor.constraint(equalTo: view.trailingAnchor),

            host.topAnchor.constraint(equalTo: card.topAnchor),
            host.bottomAnchor.constraint(equalTo: card.bottomAnchor),
            host.leadingAnchor.constraint(equalTo: card.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: card.trailingAnchor),

            centerAlign,
            card.leadingAnchor.constraint(greaterThanOrEqualTo: view.leadingAnchor, constant: 20),
            card.trailingAnchor.constraint(lessThanOrEqualTo: view.trailingAnchor, constant: -20),
            card.topAnchor.constraint(greaterThanOrEqualTo: titlePill.bottomAnchor, constant: 12),
            card.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -10),
        ])
        popoverBackdrop = backdrop
        popoverCard = card
        return host
    }

    func dismissFloatingPopover() {
        flushPendingPersistedSettings()
        activeSubtitlesHost = nil
        popoverCard?.removeFromSuperview()
        popoverBackdrop?.removeFromSuperview()
        popoverCard = nil
        popoverBackdrop = nil
        if !closing {
            showHudAndResetTimer()
        }
    }

    func loadOnlineSubtitle(_ candidate: SubtitleCandidateInfo) {
        showStatus(String(localized: "Loading subtitle…"), autoHideAfter: 0)
        let core = core
        let session = session
        Task { [weak self] in
            do {
                let prepared = try await core.downloadAndPrepareSubtitle(candidate: candidate)
                try session?.addSubtitle(
                    path: prepared.localPath,
                    title: candidate.title ?? candidate.provider,
                    lang: candidate.lang,
                    select: true
                )
                await MainActor.run {
                    self?.subtitlesEnabled = true
                    self?.sessionSubEnabled = true
                    self?.activeSubtitleFilePath = prepared.localPath
                    self?.sessionSubLang = candidate.lang
                    self?.sessionSubTitle = candidate.title ?? candidate.provider
                    self?.showStatus(String(localized: "Loaded \(candidate.lang.uppercased()) subtitle (\(prepared.detectedEncoding))"))
                    self?.tracks = (try? session?.tracks()) ?? []
                }
            } catch {
                await MainActor.run {
                    self?.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                }
            }
        }
    }

    func openLocalSubtitleFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [
            UTType(filenameExtension: "srt"),
            UTType(filenameExtension: "ass"),
            UTType(filenameExtension: "ssa"),
            UTType(filenameExtension: "vtt"),
        ].compactMap { $0 }
        panel.allowsOtherFileTypes = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        loadLocalSubtitle(url: url)
    }

    func loadLocalSubtitle(url: URL) {
        if let prepared = try? core.prepareLocalSubtitle(path: url.path) {
            try? session?.addSubtitle(path: prepared.localPath, title: url.lastPathComponent, lang: nil, select: true)
            activeSubtitleFilePath = prepared.localPath
            subtitlesEnabled = true
            sessionSubEnabled = true
            tracks = (try? session?.tracks()) ?? []
            showStatus(String(localized: "Loaded \(url.lastPathComponent) (\(prepared.detectedEncoding))"))
        }
    }

    func adjustSubDelay(_ delta: Double) {
        setSubDelay(subDelay + delta)
    }

    func setSubDelay(_ val: Double) {
        subDelay = val
        sessionSubDelay = val
        try? session?.setSubtitleDelay(seconds: val)
        persistOffsets()
        showStatus(String(format: "%@ %+.1fs", String(localized: "Subtitle delay:"), val))
    }

    func adjustAudioDelay(_ delta: Double) {
        audioDelay += delta
        sessionAudioDelay = audioDelay
        try? session?.setAudioDelay(seconds: audioDelay)
        persistOffsets()
        showStatus(String(format: "%@ %+.1fs", String(localized: "Audio delay:"), audioDelay))
    }

    func persistOffsets() {
        guard let videoId = metadata?.videoId else { return }
        try? core.setVideoOffsets(
            videoId: videoId,
            offsets: VideoOffsetsFfi(subDelay: subDelay, audioDelay: audioDelay)
        )
    }

    @objc func showAudioMenu(_ sender: NSButton) {
        showHudAndCancelTimer()
        let menu = NSMenu()
        let audioTracks = tracks.filter { $0.trackType == "audio" }
        if audioTracks.isEmpty {
            menu.addItem(withTitle: String(localized: "No audio tracks"), action: nil, keyEquivalent: "")
        } else {
            for t in audioTracks {
                let label = [t.lang?.uppercased(), t.title, t.codec].compactMap { $0 }.joined(separator: " · ")
                let item = NSMenuItem(
                    title: label.isEmpty ? "Track \(t.id)" : label,
                    action: #selector(selectAudioTrackItem(_:)),
                    keyEquivalent: ""
                )
                item.target = self
                item.tag = Int(t.id)
                item.state = t.selected ? .on : .off
                menu.addItem(item)
            }
        }
        menu.addItem(.separator())
        for (title, delta) in [(String(localized: "Audio Delay -0.1s"), -0.1), (String(localized: "Audio Delay +0.1s"), 0.1)] {
            let item = NSMenuItem(title: title, action: #selector(adjustAudioDelayItem(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = delta
            menu.addItem(item)
        }
        let reset = NSMenuItem(title: String(localized: "Reset Audio Delay"), action: #selector(resetAudioDelayItem), keyEquivalent: "")
        reset.target = self
        menu.addItem(reset)

        menu.addItem(.separator())
        let prefs = currentPrefs()
        let normItem = NSMenuItem(title: String(localized: "Audio Normalization (Dynaudnorm)"), action: #selector(toggleAudioNormalize), keyEquivalent: "")
        normItem.target = self
        normItem.state = (prefs?.audioNormalize ?? false) ? .on : .off
        menu.addItem(normItem)

        let stereoItem = NSMenuItem(title: String(localized: "Downmix to Stereo (2.0)"), action: #selector(toggleDownmixStereo), keyEquivalent: "")
        stereoItem.target = self
        stereoItem.state = (prefs?.downmixStereo ?? false) ? .on : .off
        menu.addItem(stereoItem)

        if let devices = try? session?.audioDevices(), !devices.isEmpty {
            let currentDev = (try? session?.currentAudioDevice()) ?? "auto"
            let devMenu = NSMenu()
            for dev in devices {
                let item = NSMenuItem(title: dev.description, action: #selector(selectAudioDeviceItem(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = dev.name
                item.state = currentDev == dev.name ? .on : .off
                devMenu.addItem(item)
            }
            let devHolder = NSMenuItem(title: String(localized: "Audio Output Device"), action: nil, keyEquivalent: "")
            devHolder.submenu = devMenu
            menu.addItem(.separator())
            menu.addItem(devHolder)
        }

        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc func selectAudioDeviceItem(_ sender: NSMenuItem) {
        guard let devName = sender.representedObject as? String else { return }
        try? session?.setAudioDevice(name: devName)
        showStatus("\(String(localized: "Audio Output")): \(sender.title)")
    }

    @objc func toggleAudioNormalize() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.audioNormalize.toggle()
        cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        try? session?.setAudioProcessing(normalize: prefs.audioNormalize, downmixStereo: prefs.downmixStereo)
        showStatus(prefs.audioNormalize ? String(localized: "Audio normalization on") : String(localized: "Audio normalization off"))
    }

    @objc func toggleDownmixStereo() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.downmixStereo.toggle()
        cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        try? session?.setAudioProcessing(normalize: prefs.audioNormalize, downmixStereo: prefs.downmixStereo)
        showStatus(prefs.downmixStereo ? String(localized: "Stereo downmix on") : String(localized: "Stereo downmix off"))
    }

    @objc func selectAudioTrackItem(_ sender: NSMenuItem) {
        let tid = Int64(sender.tag)
        if let chosen = tracks.first(where: { $0.trackType == "audio" && $0.id == tid }) {
            sessionAudioLang = chosen.lang
            sessionAudioTitle = chosen.title
        }
        try? session?.setAudioTrack(id: tid)
        tracks = (try? session?.tracks()) ?? []
    }

    @objc func adjustAudioDelayItem(_ sender: NSMenuItem) {
        if let d = sender.representedObject as? Double { adjustAudioDelay(d) }
    }

    @objc func resetAudioDelayItem() {
        audioDelay = 0
        sessionAudioDelay = 0
        try? session?.setAudioDelay(seconds: 0)
        persistOffsets()
    }

    @objc func showChaptersMenu(_ sender: NSButton) {
        showHudAndCancelTimer()
        let menu = NSMenu()
        if chapters.isEmpty {
            menu.addItem(withTitle: String(localized: "No chapters"), action: nil, keyEquivalent: "")
        } else {
            for ch in chapters {
                let item = NSMenuItem(
                    title: "\(Self.format(ch.startS))  \(ch.title)",
                    action: #selector(jumpToChapter(_:)),
                    keyEquivalent: ""
                )
                item.target = self
                item.representedObject = ch.startS
                menu.addItem(item)
            }
        }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc func jumpToChapter(_ sender: NSMenuItem) {
        if let t = sender.representedObject as? Double { seekAbsolute(t) }
    }

    @objc func showSpeedMenu(_ sender: NSButton) {
        let menu = NSMenu()
        for val in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
            let item = NSMenuItem(
                title: String(format: "%.2g×", val),
                action: #selector(selectSpeedItem(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.representedObject = val
            item.state = abs(speed - val) < 0.01 ? .on : .off
            menu.addItem(item)
        }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc func selectSpeedItem(_ sender: NSMenuItem) {
        if let val = sender.representedObject as? Double { setSpeed(val) }
    }

    @objc func openCastSheet(_ sender: NSButton) {
        showHudAndCancelTimer()
        let titleText = titleField.stringValue.isEmpty ? "Cinma" : titleField.stringValue
        presentSheetView { dismiss in
            CastDeviceSheetView(core: core, request: request, title: titleText, onDismiss: dismiss)
        }
    }

    @objc func showMoreMenu(_ sender: NSButton) {
        showHudAndCancelTimer()
        let menu = NSMenu()

        // A/B Loop
        let abTitle: String
        if abLoopA == nil {
            abTitle = String(localized: "A/B Loop: Set Point A")
        } else if abLoopB == nil {
            abTitle = String(localized: "A/B Loop: Set Point B")
        } else {
            abTitle = String(localized: "A/B Loop: Clear")
        }
        let abItem = NSMenuItem(title: abTitle, action: #selector(cycleAbLoop), keyEquivalent: "")
        abItem.target = self
        menu.addItem(abItem)

        let eqItem = NSMenuItem(title: String(localized: "Video EQ & Color…"), action: #selector(showVideoEqPopoverFromMenu), keyEquivalent: "")
        eqItem.target = self
        menu.addItem(eqItem)

        let searchItem = NSMenuItem(title: String(localized: "Search Dialogue… (⌘F)"), action: #selector(showDialogueSearchPopover), keyEquivalent: "")
        searchItem.target = self
        menu.addItem(searchItem)

        let liveTextItem = NSMenuItem(title: String(localized: "Scan Frame Text (Live Text · T)"), action: #selector(toggleLiveTextOverlay), keyEquivalent: "")
        liveTextItem.target = self
        liveTextItem.state = liveTextOverlay != nil ? .on : .off
        menu.addItem(liveTextItem)

        if let meta = metadata {
            let skipMenu = NSMenu()
            let curPos = Self.format(position)
            skipMenu.addItem(ClosureMenuProxy.item(title: String(localized: "Mark Intro End Here (\(curPos))"), systemSymbol: "forward.frame") { [weak self] in
                guard let self else { return }
                self.manualSkipMarkers.introEndS = max(5, self.position)
                try? self.core.setManualSkipMarkers(metaId: meta.metaId, markers: self.manualSkipMarkers)
                self.loadSkipSegmentsIfEnabled()
                self.showStatus(String(localized: "Saved manual intro end: \(Self.format(self.position))"))
            })
            skipMenu.addItem(ClosureMenuProxy.item(title: String(localized: "Mark Outro Start Here (\(curPos))"), systemSymbol: "forward.end") { [weak self] in
                guard let self, self.duration > self.position + 5 else { return }
                self.manualSkipMarkers.outroLastS = max(10, self.duration - self.position)
                try? self.core.setManualSkipMarkers(metaId: meta.metaId, markers: self.manualSkipMarkers)
                self.updateSkipSegmentVisibility()
                self.showStatus(String(localized: "Saved manual outro offset"))
            })
            if manualSkipMarkers.introEndS != nil || manualSkipMarkers.outroLastS != nil {
                skipMenu.addItem(.separator())
                skipMenu.addItem(ClosureMenuProxy.item(title: String(localized: "Clear Manual Skip Markers"), systemSymbol: "trash") { [weak self] in
                    guard let self else { return }
                    self.manualSkipMarkers = ManualSkipMarkersFfi(introEndS: nil, outroLastS: nil)
                    try? self.core.setManualSkipMarkers(metaId: meta.metaId, markers: self.manualSkipMarkers)
                    self.loadSkipSegmentsIfEnabled()
                    self.showStatus(String(localized: "Manual skip markers cleared"))
                })
            }
            let skipHolder = NSMenuItem(title: String(localized: "Skip Markers (Series)"), action: nil, keyEquivalent: "")
            skipHolder.submenu = skipMenu
            menu.addItem(skipHolder)
        }

        let prefs = currentPrefs()
        let onTopItem = NSMenuItem(title: String(localized: "Always on Top"), action: #selector(toggleAlwaysOnTop), keyEquivalent: "")
        onTopItem.target = self
        onTopItem.state = (prefs?.playerAlwaysOnTop ?? false) ? .on : .off
        menu.addItem(onTopItem)

        let blackoutItem = NSMenuItem(title: String(localized: "Black Out Other Monitors (Fullscreen)"), action: #selector(toggleBlackoutOtherScreens), keyEquivalent: "")
        blackoutItem.target = self
        blackoutItem.state = (prefs?.blackoutOtherScreens ?? false) ? .on : .off
        menu.addItem(blackoutItem)

        // Aspect / Crop submenu
        let currentCrop = prefs?.cropMode ?? "fit"
        let cropMenu = NSMenu()
        for (label, mode) in [
            (String(localized: "Fit (Original)"), "fit"),
            (String(localized: "Fill Screen (Crop)"), "fill"),
            ("16:9", "16:9"),
            ("21:9 (Ultrawide)", "21:9"),
            ("4:3", "4:3"),
        ] {
            let item = NSMenuItem(title: label, action: #selector(selectCropMode(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = mode
            item.state = currentCrop == mode ? .on : .off
            cropMenu.addItem(item)
        }
        let cropHolder = NSMenuItem(title: String(localized: "Aspect / Crop"), action: nil, keyEquivalent: "")
        cropHolder.submenu = cropMenu
        menu.addItem(cropHolder)

        // Tone Mapping submenu
        let currentTm = prefs?.hdrToneMapping ?? "auto"
        let tmMenu = NSMenu()
        for (label, mode) in [
            (String(localized: "Auto"), "auto"),
            ("BT.2390", "bt.2390"),
            ("Reinhard", "reinhard"),
            ("Hable", "hable"),
            ("Clip", "clip"),
        ] {
            let item = NSMenuItem(title: label, action: #selector(selectToneMapping(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = mode
            item.state = currentTm == mode ? .on : .off
            tmMenu.addItem(item)
        }
        let tmHolder = NSMenuItem(title: String(localized: "HDR Tone Mapping"), action: nil, keyEquivalent: "")
        tmHolder.submenu = tmMenu
        menu.addItem(tmHolder)

        // Shaders submenu
        let currentShader = prefs?.shaderPreset ?? "none"
        let shaderMenu = NSMenu()
        for (label, preset) in [(String(localized: "Off"), "none"), (String(localized: "Adaptive Sharpen (CAS)"), "sharpen")] {
            let item = NSMenuItem(title: label, action: #selector(selectShaderPreset(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = preset
            item.state = currentShader == preset ? .on : .off
            shaderMenu.addItem(item)
        }
        let shaderHolder = NSMenuItem(title: String(localized: "Upscaling / Shader"), action: nil, keyEquivalent: "")
        shaderHolder.submenu = shaderMenu
        menu.addItem(shaderHolder)

        // HDR Target Peak submenu
        let currentPeak = prefs?.hdrTargetPeak
        let peakMenu = NSMenu()
        for nits: UInt32? in [nil, 400, 600, 1000, 1600] {
            let title = nits.map { "\($0) nits" } ?? String(localized: "Auto")
            let item = NSMenuItem(title: title, action: #selector(selectHdrPeak(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = nits.map { NSNumber(value: $0) }
            item.state = currentPeak == nits ? .on : .off
            peakMenu.addItem(item)
        }
        let peakHolder = NSMenuItem(title: String(localized: "HDR Target Peak"), action: nil, keyEquivalent: "")
        peakHolder.submenu = peakMenu
        menu.addItem(peakHolder)

        // Sleep timer submenu
        let sleepMenu = NSMenu()
        var sleepOptions: [(String, Int)] = [
            (String(localized: "Off"), 0),
            (String(localized: "End of Episode"), -1),
            ("15m", 15),
            ("30m", 30),
            ("60m", 60),
        ]
        let customMins = Int(prefs?.customSleepMinutes ?? 45)
        if customMins > 0 && ![15, 30, 60].contains(customMins) {
            sleepOptions.append((String(localized: "Custom (\(customMins)m)"), customMins))
        }
        for (label, mins) in sleepOptions {
            let item = NSMenuItem(title: label, action: #selector(setSleepTimerMinutes(_:)), keyEquivalent: "")
            item.target = self
            item.tag = mins
            if mins == -1 && sleepEndOfEpisode {
                item.state = .on
            } else if mins > 0 && !sleepEndOfEpisode && activeSleepMinutes == mins {
                item.state = .on
            } else if mins == 0 && !sleepEndOfEpisode && activeSleepMinutes == 0 {
                item.state = .on
            }
            sleepMenu.addItem(item)
        }
        let sleepHolder = NSMenuItem(title: String(localized: "Sleep Timer"), action: nil, keyEquivalent: "")
        sleepHolder.submenu = sleepMenu
        menu.addItem(sleepHolder)

        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 6), in: sender)
    }

    @objc func toggleAlwaysOnTop() {
        guard var prefs = try? core.playerPreferences() else { return }
        prefs.playerAlwaysOnTop.toggle()
        cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        view.window?.level = prefs.playerAlwaysOnTop ? .floating : .normal
        showStatus(prefs.playerAlwaysOnTop ? String(localized: "Always on top enabled") : String(localized: "Always on top disabled"))
    }

    @objc func selectToneMapping(_ sender: NSMenuItem) {
        guard let mode = sender.representedObject as? String else { return }
        if var prefs = try? core.playerPreferences() {
            prefs.hdrToneMapping = mode
            cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        }
        try? session?.setToneMapping(mode: mode)
        showStatus("\(String(localized: "Tone Mapping")): \(sender.title)")
    }

    @objc func selectCropMode(_ sender: NSMenuItem) {
        guard let mode = sender.representedObject as? String else { return }
        if var prefs = try? core.playerPreferences() {
            prefs.cropMode = mode
            cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        }
        try? session?.setCropMode(mode: mode)
        showStatus("\(String(localized: "Aspect")): \(sender.title)")
    }

    @objc func selectShaderPreset(_ sender: NSMenuItem) {
        guard let preset = sender.representedObject as? String else { return }
        if var prefs = try? core.playerPreferences() {
            prefs.shaderPreset = preset
            cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        }
        let paths = (try? core.resolveShaderPaths(preset: preset)) ?? []
        try? session?.setGlslShaders(paths: paths)
        showStatus(String(localized: "Shader: \(preset)"))
    }

    @objc func selectHdrPeak(_ sender: NSMenuItem) {
        let nits = (sender.representedObject as? NSNumber)?.uint32Value
        if var prefs = try? core.playerPreferences() {
            prefs.hdrTargetPeak = nits
            cachedPrefs = prefs
        try? core.setPlayerPreferences(prefs: prefs)
        }
        try? session?.setHdrTargetPeak(nits: nits)
        showStatus(nits.map { "HDR peak: \($0) nits" } ?? "HDR peak: Auto")
    }

    @objc func setSleepTimerMinutes(_ sender: NSMenuItem) {
        sleepTimerTask?.cancel()
        sleepTimerTask = nil
        let mins = sender.tag
        if mins == -1 {
            activeSleepMinutes = 0
            sleepEndOfEpisode = true
            nextCountdownTask?.cancel()
            setNextCardVisible(false)
            showStatus(String(localized: "Sleep timer: stop after this episode"))
            return
        }
        sleepEndOfEpisode = false
        guard mins > 0 else {
            activeSleepMinutes = 0
            showStatus(String(localized: "Sleep timer off"))
            return
        }
        activeSleepMinutes = mins
        showStatus(String(localized: "Sleep timer set for \(mins) min"))
        sleepTimerTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(mins * 60))
            guard let self, !Task.isCancelled, !self.closing else { return }
            self.activeSleepMinutes = 0
            self.close()
        }
    }

    @objc func showVideoEqPopoverFromMenu() {
        guard let anchor = moreButton else { return }
        let prefs = currentPrefs()
        let root = VideoEqPopoverView(
            brightness: prefs?.eqBrightness ?? 0,
            contrast: prefs?.eqContrast ?? 0,
            gamma: prefs?.eqGamma ?? 0,
            saturation: prefs?.eqSaturation ?? 0
        ) { [weak self] b, c, g, s in
            guard let self else { return }
            try? self.session?.setVideoEq(brightness: b, contrast: c, gamma: g, saturation: s)
            self.pendingEqPrefs = (b, c, g, s)
            self.eqPersistTask?.cancel()
            self.eqPersistTask = Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(250))
                guard let self, !Task.isCancelled else { return }
                if let eq = self.pendingEqPrefs {
                    self.pendingEqPrefs = nil
                    self.updatePrefs { prefs in
                        prefs.eqBrightness = eq.0
                        prefs.eqContrast = eq.1
                        prefs.eqGamma = eq.2
                        prefs.eqSaturation = eq.3
                    }
                }
            }
        }
        presentFloatingPopover(root, relativeTo: anchor)
    }

}
