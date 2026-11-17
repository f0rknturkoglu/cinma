import AppKit
import CryptoKit
import ImageIO

/// Downsampling + two-tier image pipeline (§5.15, §5.16):
///   1. Memory `NSCache` with byte cost limit (dropped on window close).
///   2. Capped disk cache under `Caches/Cinma/images` (150 MB LRU cap).
///   3. `ImageIO` thumbnail downsampling to exact display pixel size.
@MainActor
final class ImagePipeline {
    static let shared = ImagePipeline()

    private let memoryCache: NSCache<NSString, NSImage> = {
        let cache = NSCache<NSString, NSImage>()
        cache.totalCostLimit = 64 * 1024 * 1024
        cache.countLimit = 250
        return cache
    }()

    private let diskDirectory: URL
    private let maxDiskBytes: UInt64 = 150 * 1024 * 1024
    private let session: URLSession

    private init() {
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? URL.temporaryDirectory
        diskDirectory = base.appending(path: "Cinma/images", directoryHint: .isDirectory)
        try? FileManager.default.createDirectory(at: diskDirectory, withIntermediateDirectories: true)

        let config = URLSessionConfiguration.default
        config.urlCache = nil
        config.timeoutIntervalForRequest = 15
        config.httpMaximumConnectionsPerHost = 6
        session = URLSession(configuration: config)
    }

    func clearMemoryCache() {
        memoryCache.removeAllObjects()
    }

    /// Loads and downsamples `urlString` to fit `pointSize` on the current display scale.
    func load(urlString: String?, pointSize: NSSize, scale: CGFloat = 2.0) async -> NSImage? {
        guard let urlString, !urlString.isEmpty, let url = URL(string: urlString) else {
            return nil
        }
        let maxPixels = max(64, Int((max(pointSize.width, pointSize.height) * scale).rounded(.up)))
        let key = "\(urlString)#\(maxPixels)" as NSString
        if let cached = memoryCache.object(forKey: key) {
            return cached
        }

        let diskPath = diskFile(for: urlString)
        let session = self.session
        let maxDisk = self.maxDiskBytes
        let dir = self.diskDirectory

        let decoded: (NSImage, Int)? = await Task.detached(priority: .utility) {
            let data: Data
            if let existing = try? Data(contentsOf: diskPath), !existing.isEmpty {
                try? FileManager.default.setAttributes(
                    [.modificationDate: Date()],
                    ofItemAtPath: diskPath.path
                )
                data = existing
            } else {
                guard let (fetched, response) = try? await session.data(from: url),
                      let http = response as? HTTPURLResponse,
                      (200..<300).contains(http.statusCode),
                      !fetched.isEmpty
                else {
                    return nil
                }
                try? fetched.write(to: diskPath, options: .atomic)
                Self.pruneDiskCacheIfNeeded(in: dir, limit: maxDisk)
                data = fetched
            }

            guard let cgImage = Self.downsample(data: data, maxPixelSize: maxPixels) else {
                return nil
            }
            let image = NSImage(
                cgImage: cgImage,
                size: NSSize(width: CGFloat(cgImage.width) / scale, height: CGFloat(cgImage.height) / scale)
            )
            let cost = cgImage.bytesPerRow * cgImage.height
            return (image, cost)
        }.value

        guard !Task.isCancelled, let (image, cost) = decoded else {
            return nil
        }
        memoryCache.setObject(image, forKey: key, cost: cost)
        return image
    }

    private func diskFile(for urlString: String) -> URL {
        let digest = SHA256.hash(data: Data(urlString.utf8))
        let hex = digest.map { String(format: "%02x", $0) }.joined()
        return diskDirectory.appending(path: hex)
    }

    nonisolated private static func downsample(data: Data, maxPixelSize: Int) -> CGImage? {
        let sourceOpts: [CFString: Any] = [kCGImageSourceShouldCache: false]
        guard let source = CGImageSourceCreateWithData(data as CFData, sourceOpts as CFDictionary) else {
            return nil
        }
        let thumbOpts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixelSize,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, thumbOpts as CFDictionary)
    }

    nonisolated private static func pruneDiskCacheIfNeeded(in dir: URL, limit: UInt64) {
        let fm = FileManager.default
        let keys: Set<URLResourceKey> = [.fileSizeKey, .contentModificationDateKey]
        guard let urls = try? fm.contentsOfDirectory(
            at: dir,
            includingPropertiesForKeys: Array(keys),
            options: [.skipsHiddenFiles]
        ) else {
            return
        }
        var total: UInt64 = 0
        var entries: [(URL, UInt64, Date)] = []
        for url in urls {
            guard let values = try? url.resourceValues(forKeys: keys),
                  let size = values.fileSize.map({ UInt64(max(0, $0)) })
            else { continue }
            let date = values.contentModificationDate ?? .distantPast
            total += size
            entries.append((url, size, date))
        }
        guard total > limit else { return }
        entries.sort { $0.2 < $1.2 }
        let target = (limit * 3) / 4
        for (url, size, _) in entries {
            guard total > target else { break }
            try? fm.removeItem(at: url)
            total = total > size ? total - size : 0
        }
    }
}

@MainActor
extension NSImageView {
    /// Cancels any prior load on this image view and loads `url` progressively.
    func setRemoteImage(
        _ url: String?,
        pointSize: NSSize,
        taskStorage: inout Task<Void, Never>?
    ) {
        taskStorage?.cancel()
        image = nil
        guard let url, !url.isEmpty else { return }
        let scale = window?.backingScaleFactor ?? 2.0
        taskStorage = Task { [weak self] in
            let loaded = await ImagePipeline.shared.load(urlString: url, pointSize: pointSize, scale: scale)
            guard !Task.isCancelled, let self else { return }
            if let loaded {
                if !CinemaTheme.reduceMotion {
                    self.alphaValue = 0
                    self.image = loaded
                    NSAnimationContext.runAnimationGroup({ ctx in
                        ctx.duration = 0.18
                        self.animator().alphaValue = 1
                    }, completionHandler: nil)
                } else {
                    self.alphaValue = 1
                    self.image = loaded
                }
            }
        }
    }
}
