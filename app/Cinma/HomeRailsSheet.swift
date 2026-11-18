import SwiftUI

/// Chooses which addon catalogs appear as rails on Home. Rails follow addon
/// order (Settings → Addons); "Automatic" shows the first ones in that order.
struct HomeRailsSheet: View {
    let catalogs: [CatalogInfo]
    let onSave: ([String]) -> Void
    let onCancel: () -> Void
    @State private var selected: Set<String>
    @State private var filter = ""

    init(catalogs: [CatalogInfo], selected: Set<String>, onSave: @escaping ([String]) -> Void, onCancel: @escaping () -> Void) {
        self.catalogs = catalogs
        self.onSave = onSave
        self.onCancel = onCancel
        _selected = State(initialValue: selected)
    }

    private struct AddonGroup: Identifiable {
        let id: String
        let name: String
        let catalogs: [CatalogInfo]
    }

    private var groups: [AddonGroup] {
        var order: [String] = []
        var byAddon: [String: [CatalogInfo]] = [:]
        var names: [String: String] = [:]
        let needle = filter.trimmingCharacters(in: .whitespaces).lowercased()
        for c in catalogs {
            if !needle.isEmpty,
               !c.name.lowercased().contains(needle),
               !c.addonName.lowercased().contains(needle)
            {
                continue
            }
            if byAddon[c.addonId] == nil { order.append(c.addonId) }
            byAddon[c.addonId, default: []].append(c)
            names[c.addonId] = c.addonName
        }
        return order.map { AddonGroup(id: $0, name: names[$0] ?? $0, catalogs: byAddon[$0] ?? []) }
    }

    private func typeLabel(_ t: String) -> String {
        switch t {
        case "movie": String(localized: "Movies")
        case "series": String(localized: "Series")
        default: t.capitalized
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(String(localized: "Home Rails"))
                .font(.title2.bold())
            Text(String(localized: "Pick the addon catalogs shown on Home. They appear in addon order; change the order in Settings → Addons."))
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            TextField(String(localized: "Filter catalogs…"), text: $filter)
                .textFieldStyle(.roundedBorder)
            List {
                ForEach(groups) { group in
                    Section(group.name) {
                        ForEach(group.catalogs, id: \.self) { c in
                            let key = HomeBrowserViewController.catalogKey(c)
                            Toggle(isOn: Binding(
                                get: { selected.contains(key) },
                                set: { on in
                                    if on { selected.insert(key) } else { selected.remove(key) }
                                }
                            )) {
                                HStack {
                                    Text(c.name)
                                    Spacer()
                                    Text(typeLabel(c.mediaType))
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }
            }
            .frame(minHeight: 360)
            HStack {
                Text(String(localized: "\(selected.count) rails"))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                Spacer()
                Button(String(localized: "Automatic")) { onSave([]) }
                    .help(String(localized: "Show the first catalogs in addon order"))
                Button(String(localized: "Cancel"), role: .cancel) { onCancel() }
                    .keyboardShortcut(.cancelAction)
                Button(String(localized: "Save")) {
                    // Keep addon order for the saved selection.
                    let keys = catalogs.map(HomeBrowserViewController.catalogKey).filter { selected.contains($0) }
                    onSave(keys)
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 560, height: 620)
    }
}
