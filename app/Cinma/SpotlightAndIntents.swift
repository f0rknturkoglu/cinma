import AppIntents
@preconcurrency import CoreSpotlight
import Foundation
import UniformTypeIdentifiers

/// Lazy CoreSpotlight indexing for the user's library (`ROADMAP.md` §8 priority 12).
/// Never runs during launch first-frame critical path.
@MainActor
enum SpotlightLibraryIndexer {
    private static let domain = "app.cinma.library"
    private static let enabledKey = "cinma.spotlight.enabled"

    static var isEnabled: Bool {
        get { UserDefaults.standard.bool(forKey: enabledKey) }
        set {
            UserDefaults.standard.set(newValue, forKey: enabledKey)
            if !newValue {
                CSSearchableIndex.default().deleteSearchableItems(withDomainIdentifiers: [domain])
            }
        }
    }

    static func indexLibraryIfEnabled(core: CinmaCore) {
        guard isEnabled else { return }
        let items = (try? core.libraryItems()) ?? []
        let specs: [(id: String, metaId: String, mediaType: String, name: String, poster: String?)] = items.map {
            ("cinma:\($0.mediaType):\($0.metaId)", $0.metaId, $0.mediaType, $0.name, $0.poster)
        }
        let domain = Self.domain
        Task.detached(priority: .utility) {
            let searchable: [CSSearchableItem] = specs.map { spec in
                let attrs = CSSearchableItemAttributeSet(contentType: UTType.movie)
                attrs.title = spec.name
                attrs.contentDescription = spec.mediaType == "series"
                    ? String(localized: "Series in Cinma Library")
                    : String(localized: "Movie in Cinma Library")
                attrs.identifier = spec.metaId
                if let poster = spec.poster, let url = URL(string: poster) {
                    attrs.thumbnailURL = url
                }
                return CSSearchableItem(
                    uniqueIdentifier: spec.id,
                    domainIdentifier: domain,
                    attributeSet: attrs
                )
            }
            try? await CSSearchableIndex.default().deleteSearchableItems(withDomainIdentifiers: [domain])
            if !searchable.isEmpty {
                try? await CSSearchableIndex.default().indexSearchableItems(searchable)
            }
        }
    }
}

struct OpenCinmaSearchIntent: AppIntent {
    static let title: LocalizedStringResource = "Open Cinma Title"
    static let description = IntentDescription("Opens a movie or series in Cinma by IMDb/TMDB ID.")
    static let openAppWhenRun = true

    @Parameter(title: "Title ID (e.g. tt0133093)")
    var titleId: String

    @Parameter(title: "Type", default: "movie")
    var mediaType: String

    @MainActor
    func perform() async throws -> some IntentResult {
        if let url = URL(string: "cinma://open/\(mediaType)/\(titleId)") {
            NotificationCenter.default.post(name: .cinmaExternalRoute, object: url)
        }
        return .result()
    }
}

struct PlayContinueWatchingIntent: AppIntent {
    static let title: LocalizedStringResource = "Continue Watching in Cinma"
    static let description = IntentDescription("Opens your most recently watched title in Cinma.")
    static let openAppWhenRun = true

    @MainActor
    func perform() async throws -> some IntentResult {
        if let url = URL(string: "cinma://continue") {
            NotificationCenter.default.post(name: .cinmaExternalRoute, object: url)
        }
        return .result()
    }
}

extension Notification.Name {
    static let cinmaExternalRoute = Notification.Name("cinmaExternalRoute")
    static let cinmaIptvVisibilityChanged = Notification.Name("cinmaIptvVisibilityChanged")
}
