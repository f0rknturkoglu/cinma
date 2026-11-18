import Foundation
import UserNotifications

/// Local-only upcoming episode alerts (`ROADMAP.md` §8 priority 5).
/// Off by default; schedules at most 32 pending local notifications inside the
/// app process when enabled — zero background agent or LaunchAgent.
@MainActor
enum CalendarNotifications {
    private static let enabledKey = "cinma.calendar.notifications.enabled"

    static var isEnabled: Bool {
        get { UserDefaults.standard.bool(forKey: enabledKey) }
        set { UserDefaults.standard.set(newValue, forKey: enabledKey) }
    }

    static func setEnabled(_ enabled: Bool, core: CinmaCore) async -> Bool {
        isEnabled = enabled
        let center = UNUserNotificationCenter.current()
        if !enabled {
            center.removeAllPendingNotificationRequests()
            return false
        }
        let granted = (try? await center.requestAuthorization(options: [.alert, .sound])) ?? false
        if !granted {
            isEnabled = false
            return false
        }
        await refreshSchedule(core: core)
        return true
    }

    static func refreshScheduleIfEnabled(core: CinmaCore) {
        guard isEnabled else { return }
        Task { @MainActor in
            await refreshSchedule(core: core)
        }
    }

    static func refreshSchedule(core: CinmaCore) async {
        guard isEnabled else { return }
        let center = UNUserNotificationCenter.current()
        let settings = await center.notificationSettings()
        guard settings.authorizationStatus == .authorized || settings.authorizationStatus == .provisional else {
            return
        }
        let items = (try? await core.calendarEntries()) ?? []
        center.removeAllPendingNotificationRequests()

        let parser = DateFormatter()
        parser.dateFormat = "yyyy-MM-dd"
        parser.locale = Locale(identifier: "en_US_POSIX")
        parser.timeZone = .current
        let now = Date()
        let horizon = now.addingTimeInterval(14 * 86_400)

        var scheduled = 0
        for ep in items where scheduled < 32 {
            guard let day = parser.date(from: ep.airDate) else { continue }
            var comps = Calendar.current.dateComponents([.year, .month, .day], from: day)
            comps.hour = 19
            comps.minute = 0
            guard let fireDate = Calendar.current.date(from: comps),
                  fireDate > now,
                  fireDate <= horizon
            else { continue }

            let content = UNMutableNotificationContent()
            content.title = ep.showName
            let epTag = String(format: "S%02dE%02d", ep.season, ep.episode)
            content.body = "\(epTag) · \(ep.episodeName)"
            content.sound = .default

            let trigger = UNCalendarNotificationTrigger(dateMatching: comps, repeats: false)
            let req = UNNotificationRequest(
                identifier: "cinma.cal.\(ep.showId).\(ep.season).\(ep.episode)",
                content: content,
                trigger: trigger
            )
            try? await center.add(req)
            scheduled += 1
        }
    }
}
