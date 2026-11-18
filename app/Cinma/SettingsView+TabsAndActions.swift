import AppKit
import Darwin
import SwiftUI
import UniformTypeIdentifiers

extension SettingsView {
    var playbackTab: some View {
        Form {
            Section(String(localized: "Playback Behavior")) {
                Picker(String(localized: "Default Playback Speed"), selection: $playerPrefs.defaultSpeedPct) {
                    Text("0.5×").tag(UInt32(50))
                    Text("0.75×").tag(UInt32(75))
                    Text("1×").tag(UInt32(100))
                    Text("1.25×").tag(UInt32(125))
                    Text("1.5×").tag(UInt32(150))
                    Text("2×").tag(UInt32(200))
                }
                .onChange(of: playerPrefs.defaultSpeedPct) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Exact Relative Seeks (vs Keyframe)"), isOn: $playerPrefs.exactSeek)
                    .onChange(of: playerPrefs.exactSeek) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Seek Back Step")): \(playerPrefs.seekBackStepS)s", value: $playerPrefs.seekBackStepS, in: 3...60, step: 1)
                    .onChange(of: playerPrefs.seekBackStepS) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Seek Forward Step")): \(playerPrefs.seekForwardStepS)s", value: $playerPrefs.seekForwardStepS, in: 3...60, step: 1)
                    .onChange(of: playerPrefs.seekForwardStepS) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Pause When Minimized"), isOn: $playerPrefs.pauseMinimized)
                    .onChange(of: playerPrefs.pauseMinimized) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Pause When Window Loses Focus"), isOn: $playerPrefs.pauseUnfocused)
                    .onChange(of: playerPrefs.pauseUnfocused) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "Resume Playback"), selection: $playerPrefs.resumeBehavior) {
                    Text(String(localized: "Always Resume")).tag("always_resume")
                    Text(String(localized: "Ask Each Time")).tag("ask")
                    Text(String(localized: "Always Start Over")).tag("always_start_over")
                }
                .onChange(of: playerPrefs.resumeBehavior) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Confirm Before Leaving Playback"), isOn: $playerPrefs.confirmOnLeave)
                    .onChange(of: playerPrefs.confirmOnLeave) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Volume HUD"), isOn: $playerPrefs.volumeHud)
                    .onChange(of: playerPrefs.volumeHud) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Enable Player Screen Lock"), isOn: $playerPrefs.screenLockEnabled)
                    .onChange(of: playerPrefs.screenLockEnabled) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Clock in Player HUD"), isOn: $playerPrefs.fullscreenClock)
                    .onChange(of: playerPrefs.fullscreenClock) { _, _ in savePlayerPrefs() }
                if playerPrefs.fullscreenClock {
                    Toggle(String(localized: "Show Estimated Finish Time"), isOn: $playerPrefs.fullscreenClockEndTime)
                        .onChange(of: playerPrefs.fullscreenClockEndTime) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Keep Player Window Always on Top"), isOn: $playerPrefs.playerAlwaysOnTop)
                    .onChange(of: playerPrefs.playerAlwaysOnTop) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Chapter Markers on Seek Bar"), isOn: $playerPrefs.seekBarChapters)
                    .onChange(of: playerPrefs.seekBarChapters) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Series Title Before Episode Title"), isOn: $playerPrefs.playerTitleSeriesFirst)
                    .onChange(of: playerPrefs.playerTitleSeriesFirst) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show Live P2P Speed Next to Globe"), isOn: $playerPrefs.showP2pSpeedChip)
                    .onChange(of: playerPrefs.showP2pSpeedChip) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Custom Sleep Timer")): \(playerPrefs.customSleepMinutes) min", value: $playerPrefs.customSleepMinutes, in: 5...240, step: 5)
                    .onChange(of: playerPrefs.customSleepMinutes) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Show \"Open in External Player\" Button on Direct Streams"), isOn: $playerPrefs.externalPlayerEnabled)
                    .onChange(of: playerPrefs.externalPlayerEnabled) { _, _ in savePlayerPrefs() }
                if playerPrefs.externalPlayerEnabled {
                    Picker(String(localized: "External Player Mode"), selection: $playerPrefs.externalPlayerPreset) {
                        Text(String(localized: "Auto Detect")).tag("auto")
                        Text("IINA (iina://)").tag("iina")
                        Text("Infuse (infuse://)").tag("infuse")
                        Text("VLC (vlc://)").tag("vlc")
                        Text(String(localized: "Custom App")).tag("custom")
                    }
                    .onChange(of: playerPrefs.externalPlayerPreset) { _, _ in savePlayerPrefs() }
                    if playerPrefs.externalPlayerPreset == "auto" || playerPrefs.externalPlayerPreset == "custom" {
                        HStack {
                            Text(String(localized: "External Player App"))
                            Spacer()
                            TextField("/Applications/IINA.app", text: $externalPlayerPathText)
                                .frame(width: 220)
                                .onSubmit { savePlayerPrefs() }
                        }
                    }
                }
                Toggle(String(localized: "Hold Space / Mouse for Speed Boost"), isOn: $playerPrefs.holdToBoostEnabled)
                    .onChange(of: playerPrefs.holdToBoostEnabled) { _, _ in savePlayerPrefs() }
                if playerPrefs.holdToBoostEnabled {
                    Picker(String(localized: "Hold Boost Speed"), selection: $playerPrefs.holdBoostSpeedPct) {
                        Text("1.5×").tag(UInt32(150))
                        Text("2.0×").tag(UInt32(200))
                        Text("2.5×").tag(UInt32(250))
                        Text("3.0×").tag(UInt32(300))
                    }
                    .onChange(of: playerPrefs.holdBoostSpeedPct) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Trackpad Two-Finger Seek & Volume Gestures"), isOn: $playerPrefs.trackpadGesturesEnabled)
                    .onChange(of: playerPrefs.trackpadGesturesEnabled) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Force Touch Haptic Feedback"), isOn: $playerPrefs.hapticFeedbackEnabled)
                    .onChange(of: playerPrefs.hapticFeedbackEnabled) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Include Subtitles in Screenshots"), isOn: $playerPrefs.screenshotIncludeSubtitles)
                    .onChange(of: playerPrefs.screenshotIncludeSubtitles) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Pause When Audio Output Changes (Headphone Disconnect)"), isOn: $playerPrefs.pauseOnAudioOutputChange)
                    .onChange(of: playerPrefs.pauseOnAudioOutputChange) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Black Out Other Monitors in Fullscreen"), isOn: $playerPrefs.blackoutOtherScreens)
                    .onChange(of: playerPrefs.blackoutOtherScreens) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto Mini Player (PiP) When Space/Window Hidden"), isOn: $playerPrefs.autoPipOnOcclusion)
                    .onChange(of: playerPrefs.autoPipOnOcclusion) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Enable Gamepad / Controller Navigation"), isOn: $playerPrefs.gamepadSupportEnabled)
                    .onChange(of: playerPrefs.gamepadSupportEnabled) { _, _ in savePlayerPrefs() }
            }

            Section(String(localized: "Video & Audio")) {
                Picker(String(localized: "Aspect / Crop Mode"), selection: $playerPrefs.cropMode) {
                    Text(String(localized: "Fit (Original)")).tag("fit")
                    Text(String(localized: "Fill Screen (Crop)")).tag("fill")
                    Text("16:9").tag("16:9")
                    Text("21:9").tag("21:9")
                    Text("4:3").tag("4:3")
                }
                .onChange(of: playerPrefs.cropMode) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "Upscaling / Shader"), selection: $playerPrefs.shaderPreset) {
                    Text(String(localized: "Off")).tag("none")
                    Text(String(localized: "Adaptive Sharpen (CAS)")).tag("sharpen")
                }
                .onChange(of: playerPrefs.shaderPreset) { _, _ in savePlayerPrefs() }
                Picker(
                    String(localized: "HDR Target Peak"),
                    selection: Binding(
                        get: { playerPrefs.hdrTargetPeak ?? 0 },
                        set: { val in
                            playerPrefs.hdrTargetPeak = val == 0 ? nil : val
                            savePlayerPrefs()
                        }
                    )
                ) {
                    Text(String(localized: "Auto")).tag(UInt32(0))
                    Text("400 nits").tag(UInt32(400))
                    Text("600 nits").tag(UInt32(600))
                    Text("1000 nits").tag(UInt32(1000))
                    Text("1600 nits").tag(UInt32(1600))
                }
                Toggle(String(localized: "Force 16-bit HDR Surface on SDR"), isOn: $playerPrefs.forceHdrSurface)
                    .onChange(of: playerPrefs.forceHdrSurface) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Enable macOS EDR HDR Output"), isOn: $playerPrefs.macEdrEnabled)
                    .onChange(of: playerPrefs.macEdrEnabled) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "HDR Tone Mapping"), selection: $playerPrefs.hdrToneMapping) {
                    Text(String(localized: "Auto")).tag("auto")
                    Text("BT.2390").tag("bt.2390")
                    Text("Reinhard").tag("reinhard")
                    Text("Hable").tag("hable")
                    Text("Clip").tag("clip")
                }
                .onChange(of: playerPrefs.hdrToneMapping) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Force YUV420P Pixel Format"), isOn: $playerPrefs.forceYuv420p)
                    .onChange(of: playerPrefs.forceYuv420p) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "mpv Scaler Quality"), selection: $playerPrefs.mpvScalePreset) {
                    Text(String(localized: "Fast (Bilinear)")).tag("fast")
                    Text(String(localized: "Balanced (Spline36)")).tag("balanced")
                    Text(String(localized: "High (EWA Lanczos)")).tag("high")
                }
                .onChange(of: playerPrefs.mpvScalePreset) { _, _ in savePlayerPrefs() }
                Picker(String(localized: "Hardware Decoding"), selection: $playerPrefs.mpvHwdecMode) {
                    Text("VideoToolbox").tag("videotoolbox")
                    Text("Auto-Safe").tag("auto-safe")
                    Text(String(localized: "Software (Off)")).tag("no")
                }
                .onChange(of: playerPrefs.mpvHwdecMode) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "mpv Buffer Size")): \(playerPrefs.mpvBufferMb) MiB", value: $playerPrefs.mpvBufferMb, in: 4...128, step: 8)
                    .onChange(of: playerPrefs.mpvBufferMb) { _, _ in savePlayerPrefs() }
                HStack {
                    Text(String(localized: "Extra mpv Options"))
                    Spacer()
                    TextField("deband=yes, video-sync=display-resample", text: $extraMpvText)
                        .frame(width: 240)
                        .onSubmit { savePlayerPrefs() }
                }
                Picker(String(localized: "Max Volume Boost"), selection: $playerPrefs.volumeBoostMax) {
                    Text("100%").tag(UInt32(100))
                    Text("150%").tag(UInt32(150))
                    Text("200%").tag(UInt32(200))
                }
                .onChange(of: playerPrefs.volumeBoostMax) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Audio Normalization (Dynaudnorm)"), isOn: $playerPrefs.audioNormalize)
                    .onChange(of: playerPrefs.audioNormalize) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Downmix Audio to Stereo"), isOn: $playerPrefs.downmixStereo)
                    .onChange(of: playerPrefs.downmixStereo) { _, _ in savePlayerPrefs() }
            }

            Section(String(localized: "Automation & Streams")) {
                Toggle(String(localized: "Show Skip Intro / Recap / Outro Button"), isOn: $playerPrefs.showSkipButton)
                    .onChange(of: playerPrefs.showSkipButton) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Intro"), isOn: $playerPrefs.autoSkipIntro)
                    .onChange(of: playerPrefs.autoSkipIntro) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Recap"), isOn: $playerPrefs.autoSkipRecap)
                    .onChange(of: playerPrefs.autoSkipRecap) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Outro / Credits"), isOn: $playerPrefs.autoSkipOutro)
                    .onChange(of: playerPrefs.autoSkipOutro) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Skip Anime Filler Episodes (MyAnimeList)"), isOn: $playerPrefs.autoSkipAnimeFillers)
                    .onChange(of: playerPrefs.autoSkipAnimeFillers) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Auto-Play Next Episode"), isOn: $playerPrefs.autoplayNext)
                    .onChange(of: playerPrefs.autoplayNext) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Keep Same Source for Next Episode"), isOn: $playerPrefs.keepSourceNextEpisode)
                    .onChange(of: playerPrefs.keepSourceNextEpisode) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Up Next Lead Time")): \(playerPrefs.nextEpisodeLeadS)s", value: $playerPrefs.nextEpisodeLeadS, in: 10...90, step: 5)
                    .onChange(of: playerPrefs.nextEpisodeLeadS) { _, _ in savePlayerPrefs() }
                Stepper(
                    "\(String(localized: "Pause \"Still Watching\" After")): \(playerPrefs.stillWatchingAfterEpisodes == 0 ? String(localized: "Off") : "\(playerPrefs.stillWatchingAfterEpisodes) ep")",
                    value: $playerPrefs.stillWatchingAfterEpisodes,
                    in: 0...10,
                    step: 1
                )
                .onChange(of: playerPrefs.stillWatchingAfterEpisodes) { _, _ in savePlayerPrefs() }
                if playerPrefs.autoplayNext {
                    Stepper("\(String(localized: "Auto-Play Countdown")): \(playerPrefs.autoplayCountdownS)s", value: $playerPrefs.autoplayCountdownS, in: 3...30)
                        .onChange(of: playerPrefs.autoplayCountdownS) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Auto-Switch Stream on Stall"), isOn: $playerPrefs.autoNextStreamOnStall)
                    .onChange(of: playerPrefs.autoNextStreamOnStall) { _, _ in savePlayerPrefs() }
                if playerPrefs.autoNextStreamOnStall {
                    Stepper("\(String(localized: "Stall Timeout")): \(playerPrefs.stallTimeoutS)s", value: $playerPrefs.stallTimeoutS, in: 5...30, step: 1)
                        .onChange(of: playerPrefs.stallTimeoutS) { _, _ in savePlayerPrefs() }
                }
                Toggle(String(localized: "Debrid / Direct Only (Hide P2P Torrents)"), isOn: $playerPrefs.debridOnlyMode)
                    .onChange(of: playerPrefs.debridOnlyMode) { _, _ in savePlayerPrefs() }
                Stepper("\(String(localized: "Addon HTTP Timeout")): \(playerPrefs.addonTimeoutS)s", value: $playerPrefs.addonTimeoutS, in: 3...60, step: 1)
                    .onChange(of: playerPrefs.addonTimeoutS) { _, _ in savePlayerPrefs() }
            }
        }
        .formStyle(.grouped)
    }

    var downloadsTab: some View {
        Form {
            Section(String(localized: "Folders & Menu Bar")) {
                Toggle(String(localized: "Keep Running in Menu Bar when Window Closes"), isOn: $menubarEnabled)
                    .onChange(of: menubarEnabled) { _, val in
                        try? core.setMenubarEnabled(enabled: val)
                    }
                Toggle(String(localized: "Notify When Download Completes"), isOn: $playerPrefs.notifyOnDownloadComplete)
                    .onChange(of: playerPrefs.notifyOnDownloadComplete) { _, _ in savePlayerPrefs() }
                Toggle(String(localized: "Detect Magnet Links in Clipboard on App Focus"), isOn: $playerPrefs.clipboardCatcherEnabled)
                    .onChange(of: playerPrefs.clipboardCatcherEnabled) { _, _ in savePlayerPrefs() }
                folderRow(String(localized: "Video Downloads"), path: diskPolicy.videoFolder) { picked in
                    diskPolicy.videoFolder = picked
                    saveDiskPolicy()
                }
                folderRow(String(localized: "Audio Downloads"), path: diskPolicy.audioFolder) { picked in
                    diskPolicy.audioFolder = picked
                    saveDiskPolicy()
                }
            }

            Section(String(localized: "Torrent Seeding Policy")) {
                Picker(String(localized: "Torrent Seeding Policy"), selection: $diskPolicy.seedingMode) {
                    Text(String(localized: "Off (stop after finish)")).tag("off")
                    Text(String(localized: "Target Ratio")).tag("ratio")
                    Text(String(localized: "Time Limit (minutes)")).tag("minutes")
                }
                .onChange(of: diskPolicy.seedingMode) { _, _ in saveDiskPolicy() }

                if diskPolicy.seedingMode == "ratio" {
                    Stepper("\(String(localized: "Seed Ratio")): \(String(format: "%.1f", diskPolicy.seedingRatio))", value: $diskPolicy.seedingRatio, in: 0.2...5.0, step: 0.2)
                        .onChange(of: diskPolicy.seedingRatio) { _, _ in saveDiskPolicy() }
                } else if diskPolicy.seedingMode == "minutes" {
                    Stepper("\(String(localized: "Seed Duration")): \(diskPolicy.seedingMinutes) min", value: $diskPolicy.seedingMinutes, in: 5...720, step: 15)
                        .onChange(of: diskPolicy.seedingMinutes) { _, _ in saveDiskPolicy() }
                }

                Stepper("\(String(localized: "Upload Limit")): \(diskPolicy.uploadLimitKbps == 0 ? String(localized: "Unlimited") : "\(diskPolicy.uploadLimitKbps) KB/s")", value: $diskPolicy.uploadLimitKbps, in: 0...50_000, step: 512)
                    .onChange(of: diskPolicy.uploadLimitKbps) { _, _ in saveDiskPolicy() }
                Stepper("\(String(localized: "Download Limit")): \(diskPolicy.downloadLimitKbps == 0 ? String(localized: "Unlimited") : "\(diskPolicy.downloadLimitKbps) KB/s")", value: $diskPolicy.downloadLimitKbps, in: 0...100_000, step: 1024)
                    .onChange(of: diskPolicy.downloadLimitKbps) { _, _ in saveDiskPolicy() }
            }

            Section(String(localized: "Download Schedule (Hour Window)")) {
                Toggle(String(localized: "Only Download During Allowed Hours"), isOn: $playerPrefs.downloadScheduleEnabled)
                    .onChange(of: playerPrefs.downloadScheduleEnabled) { _, _ in
                        savePlayerPrefs()
                        let hour = UInt8(Calendar.current.component(.hour, from: Date()))
                        try? core.enforceDownloadSchedule(currentHour: hour)
                    }
                if playerPrefs.downloadScheduleEnabled {
                    Picker(String(localized: "Start Hour"), selection: $playerPrefs.downloadScheduleStartHour) {
                        ForEach(0..<24, id: \.self) { h in
                            Text(String(format: "%02d:00", h)).tag(UInt8(h))
                        }
                    }
                    .onChange(of: playerPrefs.downloadScheduleStartHour) { _, _ in
                        savePlayerPrefs()
                        let hour = UInt8(Calendar.current.component(.hour, from: Date()))
                        try? core.enforceDownloadSchedule(currentHour: hour)
                    }
                    Picker(String(localized: "End Hour"), selection: $playerPrefs.downloadScheduleEndHour) {
                        ForEach(0..<24, id: \.self) { h in
                            Text(String(format: "%02d:00", h)).tag(UInt8(h))
                        }
                    }
                    .onChange(of: playerPrefs.downloadScheduleEndHour) { _, _ in
                        savePlayerPrefs()
                        let hour = UInt8(Calendar.current.component(.hour, from: Date()))
                        try? core.enforceDownloadSchedule(currentHour: hour)
                    }
                }
            }

            Section(String(localized: "Local Media Folders")) {
                if playerPrefs.localMediaFolders.isEmpty {
                    Text(String(localized: "No folders added. Add folders to browse local MKV/MP4 files in Library."))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(playerPrefs.localMediaFolders, id: \.self) { folder in
                        HStack {
                            Text((folder as NSString).abbreviatingWithTildeInPath)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer()
                            Button(role: .destructive) {
                                playerPrefs.localMediaFolders.removeAll { $0 == folder }
                                savePlayerPrefs()
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                }
                HStack {
                    Button(String(localized: "Add Folder…")) {
                        let panel = NSOpenPanel()
                        panel.canChooseDirectories = true
                        panel.canChooseFiles = false
                        panel.allowsMultipleSelection = true
                        if panel.runModal() == .OK {
                            for url in panel.urls where !playerPrefs.localMediaFolders.contains(url.path) {
                                playerPrefs.localMediaFolders.append(url.path)
                            }
                            savePlayerPrefs()
                        }
                    }
                    if !playerPrefs.localMediaFolders.isEmpty {
                        Button(String(localized: "Scan Now")) {
                            let found = (try? core.scanLocalFolders()) ?? []
                            status = String(localized: "Scanned \(found.count) local videos.")
                        }
                    }
                }
            }

            Section(String(localized: "Local Control Socket (Opt-In)")) {
                Toggle(String(localized: "Enable Local Unix Socket (`cinma.sock`)"), isOn: $playerPrefs.localApiSocketEnabled)
                    .onChange(of: playerPrefs.localApiSocketEnabled) { _, _ in savePlayerPrefs() }
                if playerPrefs.localApiSocketEnabled {
                    HStack {
                        Text(core.localApiSocketPath())
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer()
                        Button(String(localized: "Copy Path")) {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(core.localApiSocketPath(), forType: .string)
                            status = String(localized: "Socket path copied")
                        }
                    }
                    Text(String(localized: "Owner-only permissions (0600). Send newline-delimited JSON-RPC such as {\"id\":1,\"method\":\"status\"}."))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            Section(String(localized: "VPN Interface Binding & Kill Switch")) {
                Picker(
                    String(localized: "Bind Torrent Traffic to Interface"),
                    selection: Binding(
                        get: { vpnBinding.bindDeviceName ?? "" },
                        set: { val in
                            vpnBinding.bindDeviceName = val.isEmpty ? nil : val
                            saveVpnBinding()
                        }
                    )
                ) {
                    Text(String(localized: "Any (Default)")).tag("")
                    ForEach(availableIfaces, id: \.self) { iface in
                        Text(iface).tag(iface)
                    }
                }
                Toggle(String(localized: "VPN Kill Switch (block torrents if interface is unset/down)"), isOn: $vpnBinding.vpnKillSwitch)
                    .onChange(of: vpnBinding.vpnKillSwitch) { _, _ in saveVpnBinding() }
            }

            Section(String(localized: "Trailers & Web Downloads (yt-dlp)")) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(ytdlpStatus.available ? String(localized: "yt-dlp installed") : String(localized: "Not installed"))
                            .font(.subheadline.weight(.medium))
                        if let path = ytdlpStatus.path {
                            Text((path as NSString).abbreviatingWithTildeInPath)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    Spacer()
                    Button(ytdlpStatus.available ? String(localized: "Update yt-dlp") : String(localized: "Install yt-dlp")) {
                        status = String(localized: "Downloading yt-dlp…")
                        Task {
                            do {
                                let path = try await core.installOrUpdateYtdlp()
                                ytdlpStatus = core.ytdlpStatus()
                                status = "yt-dlp: \(path)"
                            } catch {
                                status = FriendlyError.explain(error)
                            }
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    var shortcutsTab: some View {
        Form {
            Section(String(localized: "Key Bindings")) {
                ForEach(commands, id: \.id) { cmd in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Self.commandTitle(for: cmd.id))
                                .font(.body)
                            Text(cmd.id)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        if recordingCommandId == cmd.id {
                            Text(String(localized: "Press shortcut… (Esc cancels)"))
                                .font(.caption)
                                .foregroundStyle(.orange)
                            Button(String(localized: "Cancel")) { stopRecording() }
                        } else if editingCommandId == cmd.id {
                            TextField("cmd+k", text: $keyDraft)
                                .frame(width: 110)
                                .onSubmit { commitBinding(cmd.id) }
                            Button(String(localized: "Save")) { commitBinding(cmd.id) }
                        } else {
                            Text(cmd.key ?? "—")
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(.secondary)
                            Button(String(localized: "Record")) { startRecording(cmd.id) }
                            Button(String(localized: "Edit")) {
                                editingCommandId = cmd.id
                                keyDraft = cmd.key ?? ""
                            }
                            if cmd.key != nil {
                                Button(String(localized: "Clear")) {
                                    do {
                                        try core.setKeyBinding(id: cmd.id, combo: nil)
                                        reload()
                                    } catch {
                                        status = FriendlyError.explain(error)
                                    }
                                }
                            }
                            Button(String(localized: "Reset")) {
                                do {
                                    try core.resetKeyBinding(id: cmd.id)
                                    reload()
                                } catch {
                                    status = FriendlyError.explain(error)
                                }
                            }
                        }
                    }
                }
            }

            Section(String(localized: "Game Controller Mapping")) {
                LabeledContent(String(localized: "A (Confirm / Play · Pause)"), value: String(localized: "Activate focused item or toggle pause"))
                LabeledContent(String(localized: "B (Back / Cancel)"), value: String(localized: "Close sheet, exit fullscreen, or go back"))
                LabeledContent(String(localized: "X (Skip / Play)"), value: String(localized: "Skip intro/outro, show HUD, or preview"))
                LabeledContent(String(localized: "Y (Subtitles / Settings)"), value: String(localized: "Cycle subtitle track or open Settings"))
                LabeledContent(String(localized: "LB / RB (Bumpers)"), value: String(localized: "Jump backward/forward by step or switch tabs"))
                LabeledContent(String(localized: "LT / RT (Triggers)"), value: String(localized: "Previous or next chapter"))
                LabeledContent(String(localized: "Menu"), value: String(localized: "Focus Search or toggle player HUD"))
            }
        }
        .formStyle(.grouped)
    }

    private static func commandTitle(for id: String) -> String {
        switch id {
        case "app.about": return String(localized: "About Cinma")
        case "app.hide": return String(localized: "Hide Cinma")
        case "app.hide_others": return String(localized: "Hide Others")
        case "app.show_all": return String(localized: "Show All")
        case "app.quit": return String(localized: "Quit Cinma")
        case "file.open": return String(localized: "Open File…")
        case "file.open_url": return String(localized: "Open URL or Magnet…")
        case "edit.undo": return String(localized: "Undo")
        case "edit.redo": return String(localized: "Redo")
        case "edit.cut": return String(localized: "Cut")
        case "edit.copy": return String(localized: "Copy")
        case "edit.paste": return String(localized: "Paste")
        case "edit.select_all": return String(localized: "Select All")
        case "window.minimize": return String(localized: "Minimize Window")
        case "window.close": return String(localized: "Close Window")
        case "player.toggle_pause": return String(localized: "Play / Pause")
        case "player.seek_back": return String(localized: "Seek Backward")
        case "player.seek_forward": return String(localized: "Seek Forward")
        case "player.frame_step": return String(localized: "Step Forward One Frame")
        case "player.frame_back_step": return String(localized: "Step Backward One Frame")
        case "player.close": return String(localized: "Exit Player / Fullscreen")
        case "player.fullscreen": return String(localized: "Toggle Fullscreen")
        case "player.toggle_subtitles": return String(localized: "Show / Hide Subtitles")
        case "player.cycle_subtitle": return String(localized: "Cycle Subtitle Track")
        case "player.cycle_audio": return String(localized: "Cycle Audio Track")
        case "player.screenshot": return String(localized: "Save Screenshot")
        case "player.stats": return String(localized: "Toggle Playback Stats")
        case "player.ab_loop": return String(localized: "A–B Loop")
        case "player.speed_up": return String(localized: "Increase Playback Speed")
        case "player.speed_down": return String(localized: "Decrease Playback Speed")
        case "player.volume_up": return String(localized: "Volume Up")
        case "player.volume_down": return String(localized: "Volume Down")
        case "player.mute": return String(localized: "Mute / Unmute")
        case "player.next_episode": return String(localized: "Next Episode")
        case "player.sub_seek_prev": return String(localized: "Seek to Previous Subtitle")
        case "player.sub_seek_next": return String(localized: "Seek to Next Subtitle")
        case "player.search_dialogue": return String(localized: "Search Dialogue…")
        case "player.live_text": return String(localized: "Scan Frame Text (Live Text)")
        case "player.screen_lock": return String(localized: "Lock / Unlock Player Screen")
        case "addon.install": return String(localized: "Install Addon Deep Link")
        default: return id
        }
    }

    var backupTab: some View {
        Form {
            Section(String(localized: "Backup & Restore (SQLite)")) {
                HStack {
                    Button(String(localized: "Export Backup…")) { exportBackup() }
                    Button(String(localized: "Restore Backup…")) { restoreBackup() }
                }
                Text(String(localized: "Exports a consistent snapshot of profiles, library, progress, addons, and settings. Secrets stay in macOS Keychain."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    func reload() {
        hasTmdbKey = (try? core.hasTmdbKey()) ?? false
        hasOpenSubKey = (try? core.hasOpensubtitlesKey()) ?? false
        language = (try? core.metadataLanguage()) ?? "tr-TR"
        addonsEnabled = (try? core.featureEnabled(name: "addons")) ?? true
        torrentEnabled = (try? core.featureEnabled(name: "torrent")) ?? true
        castingEnabled = (try? core.featureEnabled(name: "casting")) ?? false
        trackingEnabled = (try? core.featureEnabled(name: "tracking")) ?? false
        skipSegmentsEnabled = (try? core.featureEnabled(name: "skip_segments")) ?? false
        debridEnabled = (try? core.featureEnabled(name: "debrid")) ?? false
        iptvEnabled = (try? core.featureEnabled(name: "iptv")) ?? false
        discordEnabled = (try? core.featureEnabled(name: "discord")) ?? false
        calendarAlertsEnabled = CalendarNotifications.isEnabled
        spotlightEnabled = SpotlightLibraryIndexer.isEnabled
        if let v = try? core.vpnTorrentBinding() { vpnBinding = v }
        availableIfaces = Self.activeNetworkInterfaces(including: vpnBinding.bindDeviceName)
        if hasOpenSubKey {
            Task { osQuota = try? await core.opensubtitlesQuota() }
        }
        menubarEnabled = (try? core.menubarEnabled()) ?? true
        if let dp = try? core.diskPolicySettings() { diskPolicy = dp }
        ytdlpStatus = core.ytdlpStatus()
        if let s = try? core.subtitleSettings() {
            subSettings = s
            subLangsText = s.preferredLangs.joined(separator: ", ")
            audioLangsText = s.audioLangs.joined(separator: ", ")
            dualLangText = s.dualSecondaryLang ?? ""
            srtFontText = s.srtFont ?? ""
        }
        if let p = try? core.playerPreferences() {
            playerPrefs = p
            blockWordsText = p.trackBlockWords.joined(separator: ", ")
            extraMpvText = p.mpvExtraOptions
            externalPlayerPathText = p.externalPlayerPath
        }
        commands = (try? core.commands()) ?? []
    }

    func navTabVisibleBinding(_ key: String) -> Binding<Bool> {
        Binding(
            get: { !playerPrefs.hiddenNavTabs.contains(key) },
            set: { visible in
                var set = Set(playerPrefs.hiddenNavTabs)
                if visible {
                    set.remove(key)
                } else {
                    set.insert(key)
                }
                playerPrefs.hiddenNavTabs = set.sorted()
                savePlayerPrefs()
            }
        )
    }

    func saveSubSettings() {
        let parseList: (String) -> [String] = { text in
            text.split(separator: ",").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        }
        subSettings.preferredLangs = parseList(subLangsText)
        subSettings.audioLangs = parseList(audioLangsText)
        let dual = dualLangText.trimmingCharacters(in: .whitespacesAndNewlines)
        subSettings.dualSecondaryLang = dual.isEmpty ? nil : dual
        let font = srtFontText.trimmingCharacters(in: .whitespacesAndNewlines)
        subSettings.srtFont = font.isEmpty ? nil : font
        try? core.setSubtitleSettings(settings: subSettings)
    }

    func savePlayerPrefs() {
        playerPrefs.trackBlockWords = blockWordsText
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        playerPrefs.mpvExtraOptions = extraMpvText.trimmingCharacters(in: .whitespacesAndNewlines)
        playerPrefs.externalPlayerPath = externalPlayerPathText.trimmingCharacters(in: .whitespacesAndNewlines)
        try? core.setPlayerPreferences(prefs: playerPrefs)
        CinemaTheme.syncThemePreferences(from: playerPrefs)
        GamepadController.shared.syncEnabled(playerPrefs.gamepadSupportEnabled)
        NotificationCenter.default.post(name: .cinmaNavTabsChanged, object: nil)
    }

    func applyAppLanguagePreference(_ code: String) {
        if code == "system" {
            UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        } else {
            UserDefaults.standard.set([code], forKey: "AppleLanguages")
        }
        status = String(localized: "App language updated. Relaunch Cinma to apply across all windows.")
    }

    func saveVpnBinding() {
        let binding = vpnBinding
        Task {
            try? await core.setVpnTorrentBinding(
                bindDeviceName: binding.bindDeviceName,
                vpnKillSwitch: binding.vpnKillSwitch
            )
            status = binding.bindDeviceName.map { String(localized: "Torrents bound to \($0)") }
                ?? String(localized: "VPN interface binding cleared")
        }
    }

    func signInOpenSubtitles() {
        let u = osUsername
        let p = osPassword
        osPassword = ""
        Task {
            do {
                osQuota = try await core.opensubtitlesLogin(username: u, password: p)
                status = String(localized: "Signed in to OpenSubtitles")
            } catch {
                status = FriendlyError.explain(error)
            }
        }
    }

    private static func activeNetworkInterfaces(including current: String?) -> [String] {
        var names = Set<String>()
        if let current, !current.isEmpty { names.insert(current) }
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        if getifaddrs(&ifaddr) == 0, let first = ifaddr {
            var ptr: UnsafeMutablePointer<ifaddrs>? = first
            while let cur = ptr {
                let name = String(cString: cur.pointee.ifa_name)
                if name.hasPrefix("utun") || name.hasPrefix("en") || name.hasPrefix("ipsec") || name.hasPrefix("ppp") {
                    names.insert(name)
                }
                ptr = cur.pointee.ifa_next
            }
            freeifaddrs(first)
        }
        return names.sorted()
    }

    private func folderRow(_ label: String, path: String, onPick: @escaping (String) -> Void) -> some View {
        HStack {
            Text(label)
            Spacer()
            Text((path as NSString).abbreviatingWithTildeInPath)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(path)
            Button(String(localized: "Choose…")) {
                let panel = NSOpenPanel()
                panel.canChooseDirectories = true
                panel.canChooseFiles = false
                panel.canCreateDirectories = true
                panel.allowsMultipleSelection = false
                panel.directoryURL = URL(fileURLWithPath: path)
                if panel.runModal() == .OK, let url = panel.url {
                    onPick(url.path)
                }
            }
        }
    }

    func saveDiskPolicy() {
        try? core.setDiskPolicySettings(s: diskPolicy)
    }

    func exportBackup() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "cinma-backup.sqlite3"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try core.exportBackup(destPath: url.path)
            status = "\(String(localized: "Backup saved to")) \(url.lastPathComponent)"
        } catch {
            status = FriendlyError.explain(error)
        }
    }

    func restoreBackup() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let schema = try core.importBackup(srcPath: url.path)
            reload()
            status = "\(String(localized: "Restored backup (schema")) v\(schema))"
        } catch {
            status = FriendlyError.explain(error)
        }
    }

    func saveTmdbKey() {
        let trimmed = tmdbKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        status = String(localized: "Verifying TMDB key…")
        Task {
            do {
                try await core.verifyTmdbKey(key: trimmed)
                try core.setTmdbKey(key: trimmed)
                hasTmdbKey = true
                tmdbKey = ""
                status = String(localized: "TMDB key verified and saved to Keychain")
            } catch {
                status = FriendlyError.explain(error)
            }
        }
    }

    func saveOpenSubKey() {
        let trimmed = openSubKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            try core.setOpensubtitlesCredentials(apiKey: trimmed, token: nil)
            hasOpenSubKey = true
            openSubKey = ""
            status = String(localized: "OpenSubtitles key saved to Keychain")
        } catch {
            status = FriendlyError.explain(error)
        }
    }

    private func startRecording(_ id: String) {
        stopRecording()
        editingCommandId = nil
        recordingCommandId = id
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.keyCode == 53 { // Escape cancels recording unless binding escape
                stopRecording()
                return nil
            }
            if let combo = KeyCombo.from(event) {
                keyDraft = combo
                commitBinding(id)
                stopRecording()
                return nil
            }
            return event
        }
    }

    func stopRecording() {
        recordingCommandId = nil
        if let monitor = keyMonitor {
            NSEvent.removeMonitor(monitor)
            keyMonitor = nil
        }
    }

    private struct SettingsSearchItem: Identifiable {
        let id: String
        let title: String
        let section: String
        let tab: SettingsTab
        let keywords: String
    }

    private var searchableSettings: [SettingsSearchItem] {
        [
            SettingsSearchItem(id: "tmdb", title: String(localized: "Metadata (TMDB & Language)"), section: String(localized: "Metadata"), tab: .general, keywords: "tmdb api key language tr en"),
            SettingsSearchItem(id: "ui_lang", title: String(localized: "App Interface Language"), section: String(localized: "Metadata"), tab: .general, keywords: "language turkish english tr en"),
            SettingsSearchItem(id: "spotlight", title: String(localized: "Index Library in macOS Spotlight"), section: String(localized: "Integrations"), tab: .general, keywords: "spotlight search shortcuts siri index"),
            SettingsSearchItem(id: "calendar", title: String(localized: "Upcoming Episode Notifications"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "calendar notification release alert week"),
            SettingsSearchItem(id: "cw_split", title: String(localized: "Split Continue Watching into Movies & Series"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "continue watching movies series split home"),
            SettingsSearchItem(id: "cw_hide", title: String(localized: "Hide Nearly Finished Items in Continue Watching"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "continue watching hide caught up 85"),
            SettingsSearchItem(id: "cw_top", title: String(localized: "Show Continue Watching Above Trending"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "continue watching home order"),
            SettingsSearchItem(id: "addon_rails", title: String(localized: "Addon Catalog Rails on Home"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "home catalog rails limit pinned shelves"),
            SettingsSearchItem(id: "hide_watched", title: String(localized: "Hide Watched & Unreleased in Catalogs"), section: String(localized: "Catalog & Episode Browsing"), tab: .general, keywords: "hide watched unreleased catalog discover"),
            SettingsSearchItem(id: "poster_style", title: String(localized: "Poster Scale, Radius & Badges"), section: String(localized: "Home, Posters & Calendar"), tab: .general, keywords: "poster size scale radius badge imdb watched library"),
            SettingsSearchItem(id: "spoilers", title: String(localized: "Hide Episode Spoilers (Blur Unwatched)"), section: String(localized: "Catalog & Episode Browsing"), tab: .general, keywords: "spoiler blur unwatched episode"),
            SettingsSearchItem(id: "quick_play", title: String(localized: "Enable One-Click Quick Play (Hero & Context Menu)"), section: String(localized: "Catalog & Episode Browsing"), tab: .general, keywords: "quick play hero one click best stream"),
            SettingsSearchItem(id: "theme", title: String(localized: "Accent Color & Liquid Glass"), section: String(localized: "Theme, Glass & Navigation"), tab: .general, keywords: "theme accent amber crimson cyan emerald violet glass tabs"),
            SettingsSearchItem(id: "hold_boost", title: String(localized: "Hold Space / Mouse for Speed Boost"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "hold 2x speed boost space mouse"),
            SettingsSearchItem(id: "trackpad", title: String(localized: "Trackpad Two-Finger Seek & Volume Gestures"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "trackpad scroll swipe gesture volume seek"),
            SettingsSearchItem(id: "haptic", title: String(localized: "Force Touch Haptic Feedback"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "haptic vibrate force touch feedback"),
            SettingsSearchItem(id: "audio_route_pause", title: String(localized: "Pause When Audio Output Changes (Headphone Disconnect)"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "audio headphone airpods disconnect pause output device"),
            SettingsSearchItem(id: "blackout", title: String(localized: "Black Out Other Monitors in Fullscreen"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "blackout dim other monitors displays fullscreen"),
            SettingsSearchItem(id: "auto_pip", title: String(localized: "Auto Mini Player (PiP) When Space/Window Hidden"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "auto pip picture in picture space hidden occlusion"),
            SettingsSearchItem(id: "gamepad", title: String(localized: "Enable Gamepad / Controller Navigation"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "gamepad controller dualsense xbox joystick navigation"),
            SettingsSearchItem(id: "screenshot_sub", title: String(localized: "Include Subtitles in Screenshots"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "screenshot png subtitle include"),
            SettingsSearchItem(id: "ext_player", title: String(localized: "External Player Mode"), section: String(localized: "Playback Behavior"), tab: .playback, keywords: "external player iina infuse vlc mpv url scheme"),
            SettingsSearchItem(id: "hdr", title: String(localized: "Enable macOS EDR HDR Output"), section: String(localized: "Video & Audio"), tab: .playback, keywords: "hdr edr tone mapping bt.2390 peak"),
            SettingsSearchItem(id: "hwdec", title: String(localized: "Hardware Decoding"), section: String(localized: "Video & Audio"), tab: .playback, keywords: "hwdec videotoolbox decoder mpv scaler buffer"),
            SettingsSearchItem(id: "audio_norm", title: String(localized: "Audio Normalization (Dynaudnorm)"), section: String(localized: "Video & Audio"), tab: .playback, keywords: "audio normalize stereo downmix volume boost"),
            SettingsSearchItem(id: "skip", title: String(localized: "Auto-Skip Intro / Outro & Countdown"), section: String(localized: "Automation & Streams"), tab: .playback, keywords: "intro outro recap skip autoplay next episode sleep timer"),
            SettingsSearchItem(id: "stream_sort", title: String(localized: "Stream Sort, Debrid Only & Blocked Keywords"), section: String(localized: "Automation & Streams"), tab: .playback, keywords: "stream sort debrid blocked keyword timeout addon"),
            SettingsSearchItem(id: "sub_langs", title: String(localized: "Preferred Languages & OpenSubtitles"), section: String(localized: "Languages & Search"), tab: .subtitles, keywords: "subtitle audio language dual sdh opensubtitles"),
            SettingsSearchItem(id: "sub_style", title: String(localized: "SRT Appearance, Font & Scale"), section: String(localized: "SRT Appearance"), tab: .subtitles, keywords: "font size color outline background opacity position bold scale"),
            SettingsSearchItem(id: "sub_hdr_peak", title: String(localized: "HDR Subtitle Peak"), section: String(localized: "SRT Appearance"), tab: .subtitles, keywords: "hdr subtitle brightness nits peak"),
            SettingsSearchItem(id: "sub_margins", title: String(localized: "Place Subtitles in Black Bars (Margins)"), section: String(localized: "SRT Appearance"), tab: .subtitles, keywords: "letterbox margin black bar subtitle placement"),
            SettingsSearchItem(id: "vpn", title: String(localized: "VPN Interface Binding"), section: String(localized: "Torrent & VPN"), tab: .downloads, keywords: "vpn interface utun kill switch torrent speed cache"),
            SettingsSearchItem(id: "dl_notify", title: String(localized: "Notify When Download Completes"), section: String(localized: "Folders & Menu Bar"), tab: .downloads, keywords: "download complete notification alert"),
            SettingsSearchItem(id: "clip_magnet", title: String(localized: "Detect Magnet Links in Clipboard on App Focus"), section: String(localized: "Folders & Menu Bar"), tab: .downloads, keywords: "clipboard magnet torrent url catcher"),
            SettingsSearchItem(id: "dl_schedule", title: String(localized: "Download Schedule (Hour Window)"), section: String(localized: "Download Schedule (Hour Window)"), tab: .downloads, keywords: "schedule night download hours pause resume"),
            SettingsSearchItem(id: "local_folders", title: String(localized: "Local Media Folders"), section: String(localized: "Local Media Folders"), tab: .downloads, keywords: "local folder scanner library mkv mp4 nas"),
            SettingsSearchItem(id: "local_socket", title: String(localized: "Local Control Socket (Opt-In)"), section: String(localized: "Local Control Socket (Opt-In)"), tab: .downloads, keywords: "unix socket cinma.sock mcp json rpc api"),
            SettingsSearchItem(id: "anime_filler", title: String(localized: "Auto-Skip Anime Filler Episodes (MyAnimeList)"), section: String(localized: "Automation & Streams"), tab: .playback, keywords: "anime filler mal jikan auto skip"),
            SettingsSearchItem(id: "shortcuts", title: String(localized: "Keyboard Shortcuts & Controller Mapping"), section: String(localized: "Shortcuts"), tab: .shortcuts, keywords: "keyboard shortcut keybinding hotkey frame step gamepad controller"),
            SettingsSearchItem(id: "backup", title: String(localized: "Backup & Sync"), section: String(localized: "Backup"), tab: .backup, keywords: "export import sqlite backup restore"),
        ]
    }

    var settingsSearchResultsView: some View {
        let q = settingsQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let matches = searchableSettings.filter {
            $0.title.lowercased().contains(q)
                || $0.section.lowercased().contains(q)
                || $0.tab.title.lowercased().contains(q)
                || $0.keywords.lowercased().contains(q)
        }
        return List {
            if matches.isEmpty {
                Text(String(localized: "No matching settings found."))
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 12)
            } else {
                ForEach(matches) { item in
                    Button {
                        selectedTab = item.tab
                        settingsQuery = ""
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(item.title)
                                    .font(.body.weight(.medium))
                                Text("\(item.tab.title) › \(item.section)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Image(systemName: "chevron.right")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .listStyle(.inset)
    }

    private func commitBinding(_ id: String) {
        let combo = keyDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            try core.setKeyBinding(id: id, combo: combo.isEmpty ? nil : combo)
            editingCommandId = nil
            reload()
        } catch {
            status = FriendlyError.explain(error)
        }
    }
}
