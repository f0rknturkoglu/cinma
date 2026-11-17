import AppKit
import OpenGL.GL3

/// mpv draws into this layer through the libmpv OpenGL render API (ADR 0004).
/// `isAsynchronous` is off, so every callback below runs on the main thread;
/// that is what makes the `@unchecked Sendable` sound.
final class VideoLayer: CAOpenGLLayer, @unchecked Sendable {
    private(set) var session: PlaybackSession?
    private var renderReady = false
    private var context: CGLContextObj?
    private var pixelFormat: CGLPixelFormatObj?

    override init() {
        super.init()
        isAsynchronous = false
        isOpaque = true
        needsDisplayOnBoundsChange = true
        backgroundColor = NSColor.black.cgColor
        wantsExtendedDynamicRangeContent = true
        colorspace = CGColorSpace(name: CGColorSpace.sRGB)
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

    func attach(_ session: PlaybackSession) {
        self.session = session
        setNeedsDisplay()
    }

    /// Switches between an HDR (PQ, EDR) and an SDR output surface.
    func setHDR(_ hdr: Bool) {
        colorspace = CGColorSpace(name: hdr ? CGColorSpace.itur_2100_PQ : CGColorSpace.sRGB)
        setNeedsDisplay()
    }

    /// Frees mpv's render context with our GL context current. Must run
    /// before `PlaybackSession.stop()`.
    func detach() {
        if renderReady, let session, let context {
            CGLSetCurrentContext(context)
            session.renderFree()
            CGLSetCurrentContext(nil)
        }
        renderReady = false
        session = nil
    }

    override func copyCGLPixelFormat(forDisplayMask mask: UInt32) -> CGLPixelFormatObj {
        if let pixelFormat { return CGLRetainPixelFormat(pixelFormat) }
        let base: [CGLPixelFormatAttribute] = [
            kCGLPFAOpenGLProfile, CGLPixelFormatAttribute(kCGLOGLPVersion_3_2_Core.rawValue),
            kCGLPFAAccelerated, kCGLPFADoubleBuffer, kCGLPFAAllowOfflineRenderers,
        ]
        // 16-bit float framebuffer for EDR; fall back to 8-bit if unavailable.
        for extra in [[kCGLPFAColorSize, CGLPixelFormatAttribute(64), kCGLPFAColorFloat], []] {
            var attributes = base + extra + [CGLPixelFormatAttribute(0)]
            var format: CGLPixelFormatObj?
            var count: GLint = 0
            if CGLChoosePixelFormat(&attributes, &format, &count) == kCGLNoError, let format {
                pixelFormat = format
                return CGLRetainPixelFormat(format)
            }
        }
        return super.copyCGLPixelFormat(forDisplayMask: mask)
    }

    override func copyCGLContext(forPixelFormat format: CGLPixelFormatObj) -> CGLContextObj {
        // One context for the layer's lifetime: mpv's render context is bound to it.
        if let context { return CGLRetainContext(context) }
        let created = super.copyCGLContext(forPixelFormat: format)
        context = CGLRetainContext(created)
        return created
    }

    override func canDraw(
        inCGLContext ctx: CGLContextObj, pixelFormat pf: CGLPixelFormatObj,
        forLayerTime t: CFTimeInterval, displayTime ts: UnsafePointer<CVTimeStamp>?
    ) -> Bool {
        true
    }

    override func draw(
        inCGLContext ctx: CGLContextObj, pixelFormat pf: CGLPixelFormatObj,
        forLayerTime t: CFTimeInterval, displayTime ts: UnsafePointer<CVTimeStamp>?
    ) {
        guard let session else {
            glClearColor(0, 0, 0, 1)
            glClear(GLbitfield(GL_COLOR_BUFFER_BIT))
            return
        }
        if !renderReady {
            // Drain errors left by Core Animation's own GL calls so mpv's
            // post-init error check doesn't report them as its own.
            while glGetError() != GLenum(GL_NO_ERROR) {}
            var bound: GLint = 0
            glGetIntegerv(GLenum(GL_FRAMEBUFFER_BINDING), &bound)
            glBindFramebuffer(GLenum(GL_FRAMEBUFFER), 0)
            defer { glBindFramebuffer(GLenum(GL_FRAMEBUFFER), GLuint(bound)) }
            do {
                try session.renderInit(observer: RedrawRequest(layer: self))
                renderReady = true
            } catch {
                NSLog("cinma: render init failed: %@", error.localizedDescription)
                self.session = nil
                return
            }
        }
        var fbo: GLint = 0
        glGetIntegerv(GLenum(GL_FRAMEBUFFER_BINDING), &fbo)
        var viewport = [GLint](repeating: 0, count: 4)
        glGetIntegerv(GLenum(GL_VIEWPORT), &viewport)
        try? session.render(fbo: fbo, width: viewport[2], height: viewport[3])
        glFlush()
    }
}

/// Bridges mpv's "new frame" callback (mpv thread) to a main-thread redraw.
private final class RedrawRequest: RenderObserver, @unchecked Sendable {
    private weak var layer: VideoLayer?

    init(layer: VideoLayer) {
        self.layer = layer
    }

    func needsDisplay() {
        DispatchQueue.main.async { [weak layer] in layer?.setNeedsDisplay() }
    }
}
