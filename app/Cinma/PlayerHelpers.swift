import AVFoundation
import AppKit
import CoreMedia
import MediaPlayer

// MARK: - HDR Surface Detection

enum HDRProbe {
    static func detectIsHdr(for request: PlayRequest, forceHdr: Bool, macEdrEnabled: Bool = true) async -> Bool {
        if forceHdr { return true }
        if !macEdrEnabled { return false }
        switch request {
        case .stream(let stream, let meta):
            if let meta, meta.isHdr { return true }
            let text = "\(stream.name ?? "") \(stream.title ?? "")".lowercased()
            return text.contains("hdr") || text.contains("dolby") || text.contains("hlg")
        case .trailer, .download:
            return false
        case .file(let url):
            let nameLower = url.lastPathComponent.lowercased()
            let nameHintHdr = nameLower.contains("hdr") || nameLower.contains("2160p.dv") || nameLower.contains(".dv.") || nameLower.contains("hlg") || nameLower.contains("pq")
            let asset = AVURLAsset(url: url)
            guard let tracks = try? await asset.loadTracks(withMediaType: .video),
                  let first = tracks.first,
                  let descs = try? await first.load(.formatDescriptions)
            else { return nameHintHdr }
            for desc in descs {
                guard let ext = CMFormatDescriptionGetExtensions(desc) as? [CFString: Any] else { continue }
                if let tf = ext[kCVImageBufferTransferFunctionKey] as? String {
                    let upper = tf.uppercased()
                    if upper.contains("2084") || upper.contains("PQ") || upper.contains("HLG") {
                        return true
                    }
                }
            }
            return false
        }
    }
}

// MARK: - Custom Scrub Bar with Chapter Markers & Hover Thumbnail Tracking

@MainActor
final class ScrubBarView: NSView {
    var duration: Double = 0 { didSet { needsDisplay = true } }
    var position: Double = 0 {
        didSet {
            if !isDragging { needsDisplay = true }
        }
    }
    var chapters: [ChapterInfo] = [] { didSet { needsDisplay = true } }
    var showChapterMarkers = true { didSet { needsDisplay = true } }
    var abLoopA: Double? { didSet { needsDisplay = true } }
    var abLoopB: Double? { didSet { needsDisplay = true } }

    var onSeek: ((Double) -> Void)?
    /// `(seconds, xInSelf)` when hovering; `(nil, 0)` when mouse exits.
    var onHover: ((Double?, CGFloat) -> Void)?
    var onChapterCrossed: (() -> Void)?

    private var isDragging = false
    private var dragPosition: Double = 0
    private var lastDragChapterIdx: Int?
    private var hoverFraction: Double?
    private var tracking: NSTrackingArea?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .mouseMoved, .activeInKeyWindow, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        tracking = area
    }

    override func mouseMoved(with event: NSEvent) {
        updateHover(with: event)
    }

    override func mouseExited(with event: NSEvent) {
        hoverFraction = nil
        needsDisplay = true
        onHover?(nil, 0)
    }

    override func mouseDown(with event: NSEvent) {
        isDragging = true
        lastDragChapterIdx = nil
        updateDrag(with: event)
    }

    override func mouseDragged(with event: NSEvent) {
        updateDrag(with: event)
        updateHover(with: event)
    }

    override func mouseUp(with event: NSEvent) {
        updateDrag(with: event)
        isDragging = false
        lastDragChapterIdx = nil
        onSeek?(dragPosition)
        needsDisplay = true
    }

    private func fractionFor(_ event: NSEvent) -> (Double, CGFloat) {
        let pt = convert(event.locationInWindow, from: nil)
        let pad: CGFloat = 6
        let usable = max(1, bounds.width - pad * 2)
        let clampedX = min(max(pt.x, pad), bounds.width - pad)
        let frac = Double((clampedX - pad) / usable)
        return (min(max(frac, 0), 1), clampedX)
    }

    private func updateHover(with event: NSEvent) {
        guard duration > 0 else { return }
        let (frac, x) = fractionFor(event)
        hoverFraction = frac
        needsDisplay = true
        onHover?(frac * duration, x)
    }

    private func updateDrag(with event: NSEvent) {
        let (frac, _) = fractionFor(event)
        dragPosition = frac * max(duration, 0)
        if !chapters.isEmpty {
            let idx = chapters.lastIndex(where: { $0.startS <= dragPosition })
            if let last = lastDragChapterIdx, last != idx {
                onChapterCrossed?()
            }
            lastDragChapterIdx = idx
        }
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        let pad: CGFloat = 6
        let trackH: CGFloat = hoverFraction != nil || isDragging ? 6 : 4
        let trackW = max(1, bounds.width - pad * 2)
        let trackRect = NSRect(x: pad, y: (bounds.height - trackH) / 2, width: trackW, height: trackH)

        NSColor(white: 1, alpha: 0.20).setFill()
        NSBezierPath(roundedRect: trackRect, xRadius: trackH / 2, yRadius: trackH / 2).fill()

        if duration > 0 {
            if let a = abLoopA, let b = abLoopB, b > a {
                let x0 = pad + trackW * CGFloat(min(max(a / duration, 0), 1))
                let x1 = pad + trackW * CGFloat(min(max(b / duration, 0), 1))
                NSColor(srgbRed: 0x5C / 255, green: 0xC9 / 255, blue: 0x8A / 255, alpha: 0.45).setFill()
                NSBezierPath(rect: NSRect(x: x0, y: trackRect.minY, width: max(2, x1 - x0), height: trackH)).fill()
            }

            let activePos = isDragging ? dragPosition : position
            let frac = CGFloat(min(max(activePos / duration, 0), 1))
            let fillW = max(trackH, trackW * frac)
            CinemaTheme.accent.setFill()
            NSBezierPath(
                roundedRect: NSRect(x: pad, y: trackRect.minY, width: fillW, height: trackH),
                xRadius: trackH / 2,
                yRadius: trackH / 2
            ).fill()

            if showChapterMarkers {
                NSColor(white: 0.05, alpha: 0.75).setFill()
                for ch in chapters where ch.startS > 2 && ch.startS < duration - 2 {
                    let cx = pad + trackW * CGFloat(ch.startS / duration)
                    NSBezierPath(rect: NSRect(x: cx - 1, y: trackRect.minY - 1, width: 2, height: trackH + 2)).fill()
                }
            }

            let knobSize: CGFloat = (hoverFraction != nil || isDragging) ? 14 : 11
            let knobX = pad + trackW * frac - knobSize / 2
            let knobY = (bounds.height - knobSize) / 2
            CinemaTheme.textPrimary.setFill()
            NSBezierPath(ovalIn: NSRect(x: knobX, y: knobY, width: knobSize, height: knobSize)).fill()
        }
    }
}

// MARK: - Seek-Bar Thumbnail Cache + Non-Blocking Decoder

@MainActor
final class ThumbnailPreviewCache {
    private let cache = NSCache<NSNumber, NSImage>()
    private var inFlightBucket: Int?
    private var decodeTask: Task<Void, Never>?
    private let localURL: URL?

    init(localURL: URL?) {
        cache.countLimit = 32
        self.localURL = localURL
    }

    func cachedImage(at seconds: Double) -> NSImage? {
        let bucket = Int((seconds * 2.0).rounded())
        return cache.object(forKey: NSNumber(value: bucket))
    }

    func requestThumbnail(
        at seconds: Double,
        session: PlaybackSession?,
        onReady: @escaping @MainActor (NSImage) -> Void
    ) {
        let bucket = Int((seconds * 2.0).rounded())
        if let hit = cache.object(forKey: NSNumber(value: bucket)) {
            onReady(hit)
            return
        }
        guard inFlightBucket != bucket else { return }
        inFlightBucket = bucket
        decodeTask?.cancel()

        let url = localURL
        decodeTask = Task { [weak self] in
            let image: NSImage? = await Task.detached(priority: .utility) { () -> NSImage? in
                if Task.isCancelled { return nil }
                if let info = session?.thumbnailAt(seconds: Double(bucket) / 2.0),
                   let img = Self.nsImage(fromRgba: info.rgba, width: Int(info.width), height: Int(info.height))
                {
                    return img
                }
                if let url {
                    let gen = AVAssetImageGenerator(asset: AVURLAsset(url: url))
                    gen.maximumSize = CGSize(width: 320, height: 180)
                    gen.appliesPreferredTrackTransform = true
                    gen.requestedTimeToleranceBefore = CMTime(seconds: 2, preferredTimescale: 600)
                    gen.requestedTimeToleranceAfter = CMTime(seconds: 2, preferredTimescale: 600)
                    let time = CMTime(seconds: Double(bucket) / 2.0, preferredTimescale: 600)
                    if let cg = try? await gen.image(at: time).image {
                        return NSImage(cgImage: cg, size: NSSize(width: 160, height: 90))
                    }
                }
                return nil
            }.value

            guard let self, !Task.isCancelled else { return }
            self.inFlightBucket = nil
            if let image {
                self.cache.setObject(image, forKey: NSNumber(value: bucket))
                onReady(image)
            }
        }
    }

    func clear() {
        decodeTask?.cancel()
        cache.removeAllObjects()
    }

    nonisolated static func nsImage(fromRgba rgba: Data, width: Int, height: Int) -> NSImage? {
        guard width > 0, height > 0, rgba.count >= width * height * 4 else { return nil }
        guard let provider = CGDataProvider(data: rgba as CFData) else { return nil }
        let cs = CGColorSpaceCreateDeviceRGB()
        guard let cg = CGImage(
            width: width,
            height: height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: width * 4,
            space: cs,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        ) else { return nil }
        return NSImage(cgImage: cg, size: NSSize(width: width, height: height))
    }
}

// MARK: - macOS Now Playing, Media Keys & Display-Sleep Power Assertion

@MainActor
final class NowPlayingBridge {
    private var sleepAssertion: NSObjectProtocol?
    private var registeredHandlers = false

    var onTogglePause: (() -> Void)?
    var onPlay: (() -> Void)?
    var onPause: (() -> Void)?
    var onNextEpisode: (() -> Void)?
    var onSeekAbsolute: ((Double) -> Void)?

    func installCommands() {
        guard !registeredHandlers else { return }
        registeredHandlers = true
        let cc = MPRemoteCommandCenter.shared()

        cc.playCommand.isEnabled = true
        cc.playCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.onPlay?() }
            return .success
        }
        cc.pauseCommand.isEnabled = true
        cc.pauseCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.onPause?() }
            return .success
        }
        cc.togglePlayPauseCommand.isEnabled = true
        cc.togglePlayPauseCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.onTogglePause?() }
            return .success
        }
        cc.nextTrackCommand.isEnabled = true
        cc.nextTrackCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.onNextEpisode?() }
            return .success
        }
        cc.changePlaybackPositionCommand.isEnabled = true
        cc.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let posEvent = event as? MPChangePlaybackPositionCommandEvent else {
                return .commandFailed
            }
            let target = posEvent.positionTime
            Task { @MainActor in self?.onSeekAbsolute?(target) }
            return .success
        }
    }

    func update(
        title: String,
        subtitle: String?,
        duration: Double,
        position: Double,
        paused: Bool,
        speed: Double
    ) {
        updatePowerAssertion(isPlaying: !paused)
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: title,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: position,
            MPNowPlayingInfoPropertyPlaybackRate: paused ? 0.0 : speed,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.video.rawValue,
        ]
        if duration > 0 {
            info[MPMediaItemPropertyPlaybackDuration] = duration
        }
        if let subtitle {
            info[MPMediaItemPropertyArtist] = subtitle
        }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
        MPNowPlayingInfoCenter.default().playbackState = paused ? .paused : .playing
    }

    func updatePowerAssertion(isPlaying: Bool) {
        if isPlaying {
            if sleepAssertion == nil {
                sleepAssertion = ProcessInfo.processInfo.beginActivity(
                    options: [.idleDisplaySleepDisabled, .userInitiated],
                    reason: "Cinma video playback"
                )
            }
        } else if let token = sleepAssertion {
            ProcessInfo.processInfo.endActivity(token)
            sleepAssertion = nil
        }
    }

    func teardown() {
        updatePowerAssertion(isPlaying: false)
        let cc = MPRemoteCommandCenter.shared()
        cc.playCommand.removeTarget(nil)
        cc.pauseCommand.removeTarget(nil)
        cc.togglePlayPauseCommand.removeTarget(nil)
        cc.nextTrackCommand.removeTarget(nil)
        cc.changePlaybackPositionCommand.removeTarget(nil)
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        MPNowPlayingInfoCenter.default().playbackState = .stopped
        registeredHandlers = false
    }
}
