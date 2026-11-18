import AppKit

@MainActor
extension PlayerViewController {
    // MARK: - Track / Subtitle / Offset Automation

    func applySavedStyleAndOffsets() {
        guard let session else { return }
        if let settings = try? core.subtitleSettings() {
            applySubtitleStyle(settings, session: session)
        }
        if let prefs = currentPrefs(reload: true) {
            let paths = (try? core.resolveShaderPaths(preset: prefs.shaderPreset)) ?? []
            try? session.setGlslShaders(paths: paths)
            try? session.setHdrTargetPeak(nits: prefs.hdrTargetPeak)
            try? session.setVolumeMax(maxPct: prefs.volumeBoostMax)
            volume = min(volume, Double(prefs.volumeBoostMax))
            try? session.setCropMode(mode: prefs.cropMode)
            try? session.setAudioProcessing(normalize: prefs.audioNormalize, downmixStereo: prefs.downmixStereo)
            try? session.setToneMapping(mode: prefs.hdrToneMapping)
            try? session.setVideoEq(
                brightness: prefs.eqBrightness,
                contrast: prefs.eqContrast,
                gamma: prefs.eqGamma,
                saturation: prefs.eqSaturation
            )
            scrubBar.showChapterMarkers = prefs.seekBarChapters
            lockButton.isHidden = !prefs.screenLockEnabled
            view.window?.level = prefs.playerAlwaysOnTop ? .floating : .normal
        }
        refreshP2pSpeedChip()
        updateClockPill()
        try? session.setSpeed(speed: speed)
        if abs(volume - 100.0) > 0.5 {
            try? session.setVolume(volume: volume)
        }
        if muted {
            try? session.setMuted(muted: true)
        }
        if let videoId = metadata?.videoId, let offsets = try? core.videoOffsets(videoId: videoId) {
            subDelay = abs(offsets.subDelay) > 0.001 ? offsets.subDelay : (sessionSubDelay ?? 0)
            audioDelay = abs(offsets.audioDelay) > 0.001 ? offsets.audioDelay : (sessionAudioDelay ?? 0)
        } else {
            subDelay = sessionSubDelay ?? 0
            audioDelay = sessionAudioDelay ?? 0
        }
        try? session.setSubtitleDelay(seconds: subDelay)
        try? session.setAudioDelay(seconds: audioDelay)
    }

    func effectiveSubtitlePos(for settings: SubtitleSettingsFfi? = nil) -> UInt32 {
        if let settings {
            baseSubtitlePos = settings.srtPos
            liftSubtitlesAboveControls = settings.liftAboveControls
        }
        return (liftSubtitlesAboveControls && hudVisible) ? min(baseSubtitlePos, 78) : baseSubtitlePos
    }

    func syncSubtitlePosForHud() {
        try? session?.setSubtitlePos(pos: effectiveSubtitlePos())
    }

    func applySubtitleStyle(_ s: SubtitleSettingsFfi, session: PlaybackSession?) {
        let pos = effectiveSubtitlePos(for: s)
        try? session?.setSubtitleStyle(
            font: s.srtFont,
            fontSize: s.srtFontSize,
            colorHex: s.srtColor,
            borderSize: s.srtBorderSize,
            borderColorHex: s.srtBorderColor,
            backColorHex: s.srtBackColor,
            bold: s.srtBold,
            pos: pos,
            assOverride: s.assOverride,
            opacityPct: s.srtOpacity,
            boxOpacityPct: s.srtBoxOpacity,
            alignX: s.srtAlignX,
            secondaryPos: s.secondaryPos,
            scalePct: s.subScalePct,
            assScaleWithWindow: s.assScaleWithWindow,
            hdrPeakNits: s.subHdrPeakNits,
            useMargins: s.subUseMargins
        )
    }

    func refreshTracksAndChapters() {
        guard let session else { return }
        tracks = (try? session.tracks()) ?? []
        chapters = (try? session.chapters()) ?? []
        scrubBar.chapters = chapters

        guard !appliedAutoTracks, !tracks.isEmpty else { return }
        appliedAutoTracks = true
        autoSelectPreferredTracks()
    }

    func autoSelectPreferredTracks() {
        guard let session, let prefs = try? core.subtitleSettings() else { return }
        let audioTracks = tracks.filter { $0.trackType == "audio" }
        var matchedAudio: MediaTrackInfo?
        if let wantLang = sessionAudioLang {
            if let wantTitle = sessionAudioTitle, !wantTitle.isEmpty {
                matchedAudio = audioTracks.first { trackMatchesLang($0, wantLang) && $0.title == wantTitle }
            }
            matchedAudio = matchedAudio ?? audioTracks.first { trackMatchesLang($0, wantLang) }
        } else if let wantTitle = sessionAudioTitle, !wantTitle.isEmpty {
            matchedAudio = audioTracks.first { $0.title == wantTitle }
        }
        if matchedAudio == nil {
            for lang in prefs.audioLangs {
                if let match = audioTracks.first(where: { trackMatchesLang($0, lang) }) {
                    matchedAudio = match
                    break
                }
            }
        }
        if let match = matchedAudio, !match.selected {
            try? session.setAudioTrack(id: match.id)
        }

        let subTracks = tracks.filter { $0.trackType == "sub" }
        let activeAudio = matchedAudio ?? audioTracks.first(where: { $0.selected })
        let audioIsNative = activeAudio.map { tr in
            prefs.audioLangs.contains(where: { trackMatchesLang(tr, $0) })
        } ?? false

        if sessionSubEnabled == false || (sessionSubLang == nil && prefs.offByDefault) {
            subtitlesEnabled = false
            try? session.setSubtitleTrack(id: nil)
            try? session.setSecondarySubtitleTrack(id: nil)
            return
        }

        var primaryId: Int64?
        if let wantSubLang = sessionSubLang {
            if let match = pickBestSubTrack(
                in: subTracks,
                lang: wantSubLang,
                title: sessionSubTitle,
                forcedOnly: false,
                preferEmbedded: prefs.preferEmbedded,
                hideSdh: prefs.hideSdh
            ) {
                primaryId = match.id
                lastSelectedSid = match.id
                subtitlesEnabled = true
                try? session.setSubtitleTrack(id: match.id)
            }
        }
        if primaryId == nil {
            let needForcedOnly = prefs.forcedWhenNativeAudio && audioIsNative
            let candidateLangs = needForcedOnly
                ? (prefs.preferredLangs + [activeAudio?.lang].compactMap { $0 })
                : prefs.preferredLangs
            for lang in candidateLangs {
                if let match = pickBestSubTrack(
                    in: subTracks,
                    lang: lang,
                    title: nil,
                    forcedOnly: needForcedOnly,
                    preferEmbedded: prefs.preferEmbedded,
                    hideSdh: prefs.hideSdh
                ) {
                    primaryId = match.id
                    lastSelectedSid = match.id
                    subtitlesEnabled = true
                    try? session.setSubtitleTrack(id: match.id)
                    break
                }
            }
            if primaryId == nil && needForcedOnly {
                subtitlesEnabled = false
                try? session.setSubtitleTrack(id: nil)
            }
        }

        if sessionSecondarySubEnabled == false {
            secondarySid = nil
            try? session.setSecondarySubtitleTrack(id: nil)
        } else if let secLang = sessionSecondarySubLang ?? prefs.dualSecondaryLang, !secLang.isEmpty,
                  let dualMatch = pickBestSubTrack(
                      in: subTracks.filter { $0.id != primaryId },
                      lang: secLang,
                      title: nil,
                      forcedOnly: false,
                      preferEmbedded: prefs.preferEmbedded,
                      hideSdh: prefs.hideSdh
                  )
        {
            secondarySid = dualMatch.id
            try? session.setSecondarySubtitleTrack(id: dualMatch.id)
        }
    }

    func pickBestSubTrack(
        in subTracks: [MediaTrackInfo],
        lang: String,
        title: String?,
        forcedOnly: Bool,
        preferEmbedded: Bool,
        hideSdh: Bool
    ) -> MediaTrackInfo? {
        let langMatches = subTracks.filter { trackMatchesLang($0, lang) }
        guard !langMatches.isEmpty else { return nil }
        let sdhPool = (hideSdh && langMatches.contains(where: { !$0.hearingImpaired }))
            ? langMatches.filter { !$0.hearingImpaired }
            : langMatches
        if forcedOnly {
            let forced = sdhPool.filter { $0.forced }
            if preferEmbedded, let emb = forced.first(where: { !$0.external }) { return emb }
            return forced.first
        }
        if let title, !title.isEmpty {
            if let exact = sdhPool.first(where: { $0.title == title && (!preferEmbedded || !$0.external) })
                ?? sdhPool.first(where: { $0.title == title })
            {
                return exact
            }
        }
        let nonForced = sdhPool.filter { !$0.forced }
        let orderedPool = nonForced.isEmpty ? sdhPool : nonForced
        if preferEmbedded, let emb = orderedPool.first(where: { !$0.external }) {
            return emb
        }
        return orderedPool.first
    }

    func trackMatchesLang(_ track: MediaTrackInfo, _ pref: String) -> Bool {
        let p = pref.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard let l = track.lang?.lowercased() else { return false }
        if l == p || l.hasPrefix(p) || p.hasPrefix(l) { return true }
        let aliases: [String: Set<String>] = [
            "tur": ["tr", "tur", "trk", "tr-tr"],
            "eng": ["en", "eng", "en-us", "en-gb"],
            "jpn": ["ja", "jpn"],
        ]
        for group in aliases.values where group.contains(p) && group.contains(l) {
            return true
        }
        return false
    }

    func scheduleOnlineSubtitleLookup() {
        let meta = metadata
        let core = core
        Task { [weak self] in
            // Allow torrent head+tail warmup a moment to compute the OpenSubtitles hash.
            var identity = self?.session?.videoIdentity()
            if identity?.hash == nil {
                try? await Task.sleep(for: .seconds(1.5))
                identity = self?.session?.videoIdentity()
            }
            guard let self, !self.closing else { return }
            let input = SubtitleSearchInput(
                mediaType: meta?.mediaType ?? "movie",
                videoId: meta?.videoId ?? (identity?.filename ?? "local"),
                imdbId: meta?.metaId.hasPrefix("tt") == true ? meta?.metaId : nil,
                season: meta?.season.map { UInt32($0) },
                episode: meta?.episode.map { UInt32($0) },
                videoHash: identity?.hash,
                videoSize: identity?.size,
                filename: identity?.filename
            )
            self.searchingSubtitles = true
            self.refreshSubtitlesPopoverIfVisible()
            let results = (try? await core.fetchSubtitles(input: input)) ?? []
            guard !self.closing else { return }
            self.searchingSubtitles = false
            self.onlineSubtitles = results
            self.subtitleSearchStatus = identity?.hash != nil
                ? String(localized: "Matched with OpenSubtitles hash")
                : ""
            self.refreshSubtitlesPopoverIfVisible()
        }
    }

    func loadSkipSegmentsIfEnabled() {
        guard let meta = metadata else { return }
        manualSkipMarkers = (try? core.manualSkipMarkers(metaId: meta.metaId))
            ?? ManualSkipMarkersFfi(introEndS: nil, outroLastS: nil)
        let core = core
        Task { [weak self] in
            let segs = (try? await core.skipSegments(
                mediaType: meta.mediaType,
                metaId: meta.metaId,
                season: meta.season.map { UInt32($0) },
                episode: meta.episode.map { UInt32($0) }
            )) ?? []
            await MainActor.run {
                self?.apiSkipSegments = segs
                self?.updateSkipSegmentVisibility()
            }
        }
    }

    // MARK: - Next Episode Prefetch & Autoplay (§5.9)

    func checkNextEpisodeTrigger() {
        guard !loadingNextEpisode, !closing,
              let meta = metadata, meta.mediaType == "series",
              let season = meta.season, let ep = meta.episode,
              duration > 30
        else { return }

        let prefs = currentPrefs()
        let leadS = Double(max(10, min(90, prefs?.nextEpisodeLeadS ?? 35)))
        let remaining = max(0, duration - position)
        let progress = position / duration
        let inOutro = chapters.last(where: { $0.startS <= position }).map { isOutroChapter($0.title) } ?? false
        if !prefetchedTriggered && (remaining <= leadS + 20 || progress >= 0.88 || inOutro) {
            prefetchedTriggered = true
            session?.startNextEpisodePrefetch(
                metaId: meta.metaId,
                currentSeason: UInt32(max(1, season)),
                currentEpisode: UInt32(max(0, ep)),
                bingeGroup: meta.bingeGroup
            )
        }

        if prefetchedTriggered && nextPlan == nil, let plan = session?.prefetchedNextEpisode() {
            nextPlan = plan
        }

        if !sleepEndOfEpisode, !nextDismissed, (remaining <= leadS || progress >= 0.92 || inOutro), let plan = nextPlan {
            showNextEpisodeCard(plan)
        }
    }

    func setNextCardVisible(_ visible: Bool) {
        nextCard.isHidden = !visible
        skipButtonTrailingConstraint?.constant = visible ? -360 : -28
    }

    func showNextEpisodeCard(_ plan: NextEpisodePrefetchInfo) {
        guard !loadingNextEpisode, !isScreenLocked, nextCard.isHidden else { return }
        nextTitleLabel.stringValue = String(localized: "S\(plan.season) · E\(plan.episode) — \(plan.title)")
        nextSubtitleLabel.stringValue = plan.bestStream != nil
            ? String(localized: "Prefetched & ready")
            : String(localized: "Ready to load")
        setNextCardVisible(true)
        showHudAndCancelTimer()

        let prefs = currentPrefs()
        guard prefs?.autoplayNext != false else {
            nextCountdownLabel.stringValue = ""
            return
        }
        let total = Int(prefs?.autoplayCountdownS ?? 10)
        nextCountdownTask?.cancel()
        nextCountdownTask = Task { [weak self] in
            for remaining in stride(from: total, through: 1, by: -1) {
                guard let self, !Task.isCancelled, !self.loadingNextEpisode, !self.nextDismissed, !self.closing else { return }
                self.nextCountdownLabel.stringValue = String(localized: "Auto-playing in \(remaining)s…")
                try? await Task.sleep(for: .seconds(1))
            }
            guard let self, !Task.isCancelled, !self.loadingNextEpisode, !self.nextDismissed, !self.closing else { return }
            self.autoAdvanceToNextEpisode()
        }
    }

    func autoAdvanceToNextEpisode() {
        let prefs = currentPrefs()
        let limit = prefs?.stillWatchingAfterEpisodes ?? 0
        if limit > 0, autoPlayedEpisodesCount >= limit {
            autoPlayedEpisodesCount = 0
            nextCountdownTask?.cancel()
            nextCountdownTask = nil
            if !paused { try? session?.setPaused(paused: true) }
            setNextCardVisible(true)
            showHudAndCancelTimer()
            nextCountdownLabel.stringValue = String(localized: "Still watching? Click Play Next to continue.")
            return
        }
        autoPlayedEpisodesCount += 1
        playNextEpisodeNow()
    }

    @objc func playNextEpisodeManually() {
        autoPlayedEpisodesCount = 0
        playNextEpisodeNow()
    }

    @objc func playNextEpisodeNow() {
        guard !loadingNextEpisode,
              let plan = nextPlan ?? session?.prefetchedNextEpisode(),
              let stream = plan.bestStream
        else { return }
        loadingNextEpisode = true
        nextCountdownTask?.cancel()
        nextCountdownTask = nil
        setNextCardVisible(false)
        nextCountdownLabel.stringValue = ""
        skipButton.isHidden = true
        nextPlan = nil
        prefetchedTriggered = false
        nextDismissed = false
        pendingSwitchPosition = nil
        appliedResume = true
        failedRetries = 0
        currentStreamIndex = 0
        activeSubtitleFilePath = nil
        position = 0
        duration = 0
        scrubBar.position = 0
        scrubBar.duration = 0
        updateTime()
        chapters = []
        scrubBar.chapters = []
        apiSkipSegments = []
        lastAutoSkippedSegmentStart = nil
        onlineSubtitles = []

        if let prev = metadata {
            let nextBingeGroup: String? = {
                guard let data = stream.raw.data(using: .utf8),
                      let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                      let hints = obj["behaviorHints"] as? [String: Any]
                else { return nil }
                return hints["bingeGroup"] as? String
            }()
            let fallbackSummary = stream.name ?? stream.title ?? stream.addonName
            let newMeta = PlaybackMetadata(
                videoId: plan.videoId,
                metaId: prev.metaId,
                mediaType: prev.mediaType,
                title: prev.title,
                subtitle: String(localized: "S\(plan.season) · E\(plan.episode) — \(plan.title)"),
                artwork: plan.still ?? prev.artwork,
                season: Int64(plan.season),
                episode: Int64(plan.episode),
                resumePosition: nil,
                streamSummary: fallbackSummary,
                isHdr: false,
                bingeGroup: nextBingeGroup,
                rankedStreams: []
            )
            request = .stream(stream, metadata: newMeta)
            refreshTitles()
        }
        showStatus(String(localized: "Loading next episode…"), autoHideAfter: 0)
        armStallWatchdog()
        let session = session
        let core = core
        let targetVideoId = plan.videoId
        let targetMediaType = metadata?.mediaType ?? "series"
        Task { [weak self] in
            async let rankedTask: RankResultInfo? = {
                guard let raw = try? await core.streams(mediaType: targetMediaType, id: targetVideoId),
                      !raw.isEmpty
                else { return nil }
                return try? await core.rankStreamsWithDebrid(streams: raw, preferences: nil)
            }()
            do {
                try await session?.play(stream: stream)
                if let ranked = await rankedTask, !ranked.ranked.isEmpty {
                    await MainActor.run {
                        guard let self,
                              case .stream(let activeStream, var meta) = self.request,
                              meta?.videoId == targetVideoId
                        else { return }
                        meta?.rankedStreams = ranked.ranked
                        if let idx = ranked.ranked.firstIndex(where: { $0.stream.raw == activeStream.raw }) {
                            self.currentStreamIndex = idx
                        }
                        self.request = .stream(activeStream, metadata: meta)
                    }
                }
            } catch {
                await MainActor.run {
                    self?.cancelStallWatchdog()
                    self?.loadingNextEpisode = false
                    self?.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                    self?.attemptStreamRecovery()
                }
            }
        }
    }

    @objc func cancelNextEpisode() {
        nextDismissed = true
        nextCountdownTask?.cancel()
        setNextCardVisible(false)
        session?.cancelPlaybackPrefetchIfNeeded()
    }

    // MARK: - Skip Intro / Outro Chapters

    func isRecapChapter(_ title: String) -> Bool {
        let lower = title.lowercased()
        return lower.contains("recap") || lower.contains("önceki")
    }

    func isIntroChapter(_ title: String) -> Bool {
        let lower = title.lowercased()
        return lower.contains("intro") || lower.contains("opening") || lower == "op"
    }

    func isOutroChapter(_ title: String) -> Bool {
        let lower = title.lowercased()
        return lower.contains("outro") || lower.contains("credits") || lower.contains("ending") || lower == "ed"
    }

    func currentSkipSegment() -> (kind: String, startS: Double, endS: Double)? {
        if let outroLast = manualSkipMarkers.outroLastS,
           outroLast > 5,
           duration > outroLast + 10
        {
            let outroStart = max(0, duration - outroLast)
            if position >= outroStart && position < duration - 1.5 {
                return ("outro", outroStart, duration)
            }
        }
        if let seg = activeApiSkipSegment() {
            return (seg.kind, seg.startS, seg.endS)
        }
        guard let idx = chapters.lastIndex(where: { $0.startS <= position }) else { return nil }
        let ch = chapters[idx]
        let nextStart = idx + 1 < chapters.count ? chapters[idx + 1].startS : max(position, duration - 1.0)
        guard nextStart > position + 1.0 else { return nil }
        if isRecapChapter(ch.title) {
            return ("recap", ch.startS, nextStart)
        }
        if isIntroChapter(ch.title), idx + 1 < chapters.count {
            return ("intro", ch.startS, nextStart)
        }
        if isOutroChapter(ch.title) {
            return ("outro", ch.startS, nextStart)
        }
        return nil
    }

    func updateSkipSegmentVisibility() {
        guard !isScreenLocked, let seg = currentSkipSegment() else {
            skipButton.isHidden = true
            return
        }
        let prefs = currentPrefs()
        let autoSkip: Bool = switch seg.kind {
        case "outro": prefs?.autoSkipOutro ?? false
        case "recap": prefs?.autoSkipRecap ?? false
        default: prefs?.autoSkipIntro ?? false
        }
        if autoSkip && (lastAutoSkippedSegmentStart.map { abs($0 - seg.startS) > 2.0 } ?? true) {
            lastAutoSkippedSegmentStart = seg.startS
            seekAbsolute(seg.endS)
            skipButton.isHidden = true
            let toast: String = switch seg.kind {
            case "outro": String(localized: "Auto-skipped Outro")
            case "recap": String(localized: "Auto-skipped Recap")
            default: String(localized: "Auto-skipped Intro")
            }
            showStatus(toast)
            return
        }
        guard prefs?.showSkipButton ?? true else {
            skipButton.isHidden = true
            return
        }
        switch seg.kind {
        case "outro": skipButton.title = String(localized: "Skip Outro")
        case "recap": skipButton.title = String(localized: "Skip Recap")
        default: skipButton.title = String(localized: "Skip Intro")
        }
        skipButton.isHidden = false
    }

    func activeApiSkipSegment() -> SkipSegmentInfo? {
        apiSkipSegments.first { position >= $0.startS && position < ($0.endS - 1.0) }
    }

    @objc func skipCurrentSegment() {
        guard let seg = currentSkipSegment() else { return }
        lastAutoSkippedSegmentStart = seg.startS
        seekAbsolute(seg.endS)
        skipButton.isHidden = true
    }

    // MARK: - Stream Switching & Auto-Recovery (§5.2)

    func armStallWatchdog() {
        cancelStallWatchdog()
        let prefs = currentPrefs()
        guard prefs?.autoNextStreamOnStall ?? true else { return }
        let timeoutS = Int(max(5, min(30, prefs?.stallTimeoutS ?? 12)))
        stallRecoveryTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(timeoutS))
            guard let self, !Task.isCancelled, !self.closing else { return }
            self.attemptStreamRecovery()
        }
    }

    func cancelStallWatchdog() {
        stallRecoveryTask?.cancel()
        stallRecoveryTask = nil
    }

    func attemptStreamRecovery() {
        guard !closing, let session else { return }
        if failedRetries == 0, case .stream(let current, _) = request {
            failedRetries = 1
            pendingSwitchPosition = position > 5 ? position : nil
            showStatus(String(localized: "Stream stalled, retrying…"), autoHideAfter: 0)
            armStallWatchdog()
            Task { [weak self] in
                do {
                    try await session.play(stream: current)
                } catch {
                    await MainActor.run {
                        self?.cancelStallWatchdog()
                        self?.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                        self?.attemptStreamRecovery()
                    }
                }
            }
            return
        }
        guard let meta = metadata, !meta.rankedStreams.isEmpty else { return }
        let nextIndex = currentStreamIndex + 1
        guard nextIndex < meta.rankedStreams.count else {
            showStatus(String(localized: "No more fallback streams available."), autoHideAfter: 4.5)
            return
        }
        currentStreamIndex = nextIndex
        failedRetries = 0
        let fallback = meta.rankedStreams[nextIndex].stream
        showStatus(String(localized: "Switching to fallback stream…"), autoHideAfter: 0)
        switchToStream(fallback)
    }

    func switchToStream(_ stream: StreamInfo) {
        pendingSwitchPosition = position > 5 ? position : nil
        if case .stream(_, let meta) = request {
            request = .stream(stream, metadata: meta)
            refreshTitles()
        }
        guard let session else { return }
        armStallWatchdog()
        Task { [weak self] in
            do {
                try await session.play(stream: stream)
            } catch {
                await MainActor.run {
                    self?.cancelStallWatchdog()
                    self?.showStatus(FriendlyError.explain(error), autoHideAfter: 4.5)
                    self?.attemptStreamRecovery()
                }
            }
        }
    }

}
