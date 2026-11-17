import AppKit
import OpenGL.GL3

/// mpv draws into this layer through the libmpv OpenGL render API (ADR 0004 & ADR 0011).
/// `isAsynchronous = true` lets Core Animation invoke `canDraw` / `draw` on its
/// background display-link thread (`CVDisplayLink`), keeping the main AppKit thread
/// free for 120 Hz HUD & thumbnail hover responsiveness.
///
/// `hdrMode` must be configured before `attach(_:)`: HDR streams use a 16-bit float
/// PQ/BT.2020 EDR surface, while known SDR content uses an 8-bit BT.709 surface to
/// avoid float framebuffer bandwidth overhead.
final class VideoLayer: CAOpenGLLayer {
    private let stateLock = NSLock()
    private var _session: PlaybackSession?
    private var _onRenderReady: (@Sendable () -> Void)?
    private var renderReady = false
    private var hdrSurface = false
    private var context: CGLContextObj?
    private var pixelFormat: CGLPixelFormatObj?

    var session: PlaybackSession? {
        stateLock.withLock { _session }
    }

    override init() {
        super.init()
        isAsynchronous = true
        isOpaque = true
        needsDisplayOnBoundsChange = true
        backgroundColor = NSColor.black.cgColor
        applySurfaceColorSpace(hdr: false)
    }

    /// Used by Core Animation for presentation copies.
    override init(layer: Any) {
        super.init(layer: layer)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }

    deinit {
        if let context { CGLReleaseContext(context) }
        if let pixelFormat { CGLReleasePixelFormat(pixelFormat) }
    }

    func configureSurface(hdr: Bool) {
        stateLock.withLock {
            guard !renderReady else { return }
            hdrSurface = hdr
        }
        applySurfaceColorSpace(hdr: hdr)
    }

    private func applySurfaceColorSpace(hdr: Bool) {
        wantsExtendedDynamicRangeContent = hdr
        colorspace = CGColorSpace(
            name: hdr ? CGColorSpace.itur_2100_PQ : CGColorSpace.itur_709
        )
    }

    func attach(_ session: PlaybackSession, onReady: (@Sendable () -> Void)? = nil) {
        stateLock.withLock {
            _session = session
            _onRenderReady = onReady
        }
        setNeedsDisplay()
    }

    /// Frees mpv's render context with our GL context current and locked.
    /// Must run before `PlaybackSession.stop()`.
    func detach() {
        let (sess, ready, ctx) = stateLock.withLock { () -> (PlaybackSession?, Bool, CGLContextObj?) in
            let s = _session
            let r = renderReady
            let c = context
            _session = nil
            _onRenderReady = nil
            renderReady = false
            return (s, r, c)
        }
        guard ready, let sess, let ctx else { return }
        CGLLockContext(ctx)
        CGLSetCurrentContext(ctx)
        sess.renderFree()
        CGLSetCurrentContext(nil)
        CGLUnlockContext(ctx)
    }

    override func copyCGLPixelFormat(forDisplayMask mask: UInt32) -> CGLPixelFormatObj {
        let existing = stateLock.withLock { pixelFormat }
        if let existing { return CGLRetainPixelFormat(existing) }
        let useHdr = stateLock.withLock { hdrSurface }

        let base: [CGLPixelFormatAttribute] = [
            kCGLPFAOpenGLProfile, CGLPixelFormatAttribute(kCGLOGLPVersion_3_2_Core.rawValue),
            kCGLPFAAccelerated, kCGLPFADoubleBuffer, kCGLPFAAllowOfflineRenderers,
        ]
        // 16-bit float framebuffer for HDR/EDR; 8-bit surface for SDR avoids float cost.
        let candidates: [[CGLPixelFormatAttribute]] = useHdr
            ? [[kCGLPFAColorSize, CGLPixelFormatAttribute(64), kCGLPFAColorFloat], []]
            : [[kCGLPFAColorSize, CGLPixelFormatAttribute(32)], []]

        for extra in candidates {
            var attributes = base + extra + [CGLPixelFormatAttribute(0)]
            var format: CGLPixelFormatObj?
            var count: GLint = 0
            if CGLChoosePixelFormat(&attributes, &format, &count) == kCGLNoError, let format {
                stateLock.withLock { pixelFormat = format }
                return CGLRetainPixelFormat(format)
            }
        }
        return super.copyCGLPixelFormat(forDisplayMask: mask)
    }

    override func copyCGLContext(forPixelFormat format: CGLPixelFormatObj) -> CGLContextObj {
        let existing = stateLock.withLock { context }
        if let existing { return CGLRetainContext(existing) }
        let created = super.copyCGLContext(forPixelFormat: format)
        stateLock.withLock { context = CGLRetainContext(created) }
        return created
    }

    override func canDraw(
        inCGLContext ctx: CGLContextObj, pixelFormat pf: CGLPixelFormatObj,
        forLayerTime t: CFTimeInterval, displayTime ts: UnsafePointer<CVTimeStamp>?
    ) -> Bool {
        let (sess, ready) = stateLock.withLock { (_session, renderReady) }
        guard let sess else { return false }
        if !ready { return true }
        return sess.renderShouldDraw()
    }

    override func draw(
        inCGLContext ctx: CGLContextObj, pixelFormat pf: CGLPixelFormatObj,
        forLayerTime t: CFTimeInterval, displayTime ts: UnsafePointer<CVTimeStamp>?
    ) {
        CGLLockContext(ctx)
        defer { CGLUnlockContext(ctx) }

        let (sess, ready) = stateLock.withLock { (_session, renderReady) }
        guard let sess else {
            glClearColor(0, 0, 0, 1)
            glClear(GLbitfield(GL_COLOR_BUFFER_BIT))
            return
        }
        if !ready {
            // Drain errors left by Core Animation's own GL calls so mpv's
            // post-init error check doesn't report them as its own.
            while glGetError() != GLenum(GL_NO_ERROR) {}
            var bound: GLint = 0
            glGetIntegerv(GLenum(GL_FRAMEBUFFER_BINDING), &bound)
            glBindFramebuffer(GLenum(GL_FRAMEBUFFER), 0)
            defer { glBindFramebuffer(GLenum(GL_FRAMEBUFFER), GLuint(bound)) }
            do {
                try sess.renderInit(observer: NoOpRenderObserver())
                let readyCb = stateLock.withLock { () -> (@Sendable () -> Void)? in
                    renderReady = true
                    let cb = _onRenderReady
                    _onRenderReady = nil
                    return cb
                }
                if let readyCb {
                    DispatchQueue.main.async { readyCb() }
                }
            } catch {
                NSLog("cinma: render init failed: %@", error.localizedDescription)
                stateLock.withLock { _session = nil }
                return
            }
        }
        var fbo: GLint = 0
        glGetIntegerv(GLenum(GL_FRAMEBUFFER_BINDING), &fbo)
        var viewport = [GLint](repeating: 0, count: 4)
        glGetIntegerv(GLenum(GL_VIEWPORT), &viewport)
        try? sess.renderFrame(fbo: fbo, width: viewport[2], height: viewport[3], blockForTargetTime: false)
        glFlush()
    }
}

/// Asynchronous `CAOpenGLLayer` polls `mpv_render_context_update` inside `canDraw`,
/// so no main-thread redraw hop is needed per frame.
private final class NoOpRenderObserver: RenderObserver, @unchecked Sendable {
    func needsDisplay() {}
}
