import AppKit

/// Environment-driven automation for measurements and smoke tests.
/// Inert unless one of these is set:
///   CINMA_DEV_ADDON=<manifest url>      install before autoplay
///   CINMA_AUTOPLAY=movie:<id> | series:<id> | file:<path>
///   CINMA_AUTOSEEK=0.5,0.8,0.2          fractions of duration, after first frame
///   CINMA_AUTOQUIT=1                    quit after the last seek (or 5 s of playback)
///   CINMA_AUTOQUIT=close                close the player instead and stay running
@MainActor
enum DevAutomation {
    static let environment = ProcessInfo.processInfo.environment

    static var autoSeeks: [Double] {
        (environment["CINMA_AUTOSEEK"] ?? "").split(separator: ",").compactMap { Double($0) }
    }

    static var autoQuit: Bool { environment["CINMA_AUTOQUIT"] != nil }
    static var closeOnly: Bool { environment["CINMA_AUTOQUIT"] == "close" }

    static func run(core: CinmaCore, play: @escaping (PlayRequest) -> Void) {
        guard let target = environment["CINMA_AUTOPLAY"] else { return }
        Task {
            do {
                if let addon = environment["CINMA_DEV_ADDON"] {
                    _ = try await core.installAddon(url: addon)
                }
                if target.hasPrefix("file:") {
                    play(.file(URL(fileURLWithPath: String(target.dropFirst(5)))))
                    return
                }
                let parts = target.split(separator: ":", maxSplits: 1).map(String.init)
                guard parts.count == 2 else { return }
                let streams = try await core.streams(mediaType: parts[0], id: parts[1])
                if let first = streams.first {
                    play(.stream(first))
                } else {
                    log("no streams for \(target)")
                }
            } catch {
                log("autoplay failed: \(error.localizedDescription)")
            }
        }
    }

    static func log(_ message: String) {
        FileHandle.standardError.write(Data("CINMA_DEV \(message)\n".utf8))
    }
}
