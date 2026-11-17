//! Command registry (§5.11): every menu item, hotkey and deep link maps to a
//! command id, so remapping keys or adding a link is configuration, not code.
//! The UI owns *how* a command runs; the registry owns ids, contexts, key
//! bindings (defaults + user overrides in `app_setting`) and link parsing.

use std::collections::HashMap;

use crate::events::CoreEvent;
use crate::{Core, CoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandContext {
    /// Menu bar / whole app.
    Global,
    /// Only while the player has focus.
    Player,
}

impl CommandContext {
    pub fn name(self) -> &'static str {
        match self {
            CommandContext::Global => "global",
            CommandContext::Player => "player",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "global" => Some(Self::Global),
            "player" => Some(Self::Player),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub context: CommandContext,
    pub default_key: Option<&'static str>,
}

const fn global(id: &'static str, key: Option<&'static str>) -> CommandSpec {
    CommandSpec {
        id,
        context: CommandContext::Global,
        default_key: key,
    }
}

const fn player(id: &'static str, key: &'static str) -> CommandSpec {
    CommandSpec {
        id,
        context: CommandContext::Player,
        default_key: Some(key),
    }
}

/// All commands, in menu order. Only commands with a consumer live here.
pub const COMMANDS: &[CommandSpec] = &[
    global("app.about", None),
    global("app.hide", Some("cmd+h")),
    global("app.hide_others", Some("cmd+alt+h")),
    global("app.show_all", None),
    global("app.quit", Some("cmd+q")),
    global("file.open", Some("cmd+o")),
    global("edit.undo", Some("cmd+z")),
    global("edit.redo", Some("cmd+shift+z")),
    global("edit.cut", Some("cmd+x")),
    global("edit.copy", Some("cmd+c")),
    global("edit.paste", Some("cmd+v")),
    global("edit.select_all", Some("cmd+a")),
    global("window.minimize", Some("cmd+m")),
    global("window.close", Some("cmd+w")),
    player("player.toggle_pause", "space"),
    player("player.seek_back", "left"),
    player("player.seek_forward", "right"),
    player("player.close", "escape"),
    // Reached through deep links only.
    global("addon.install", None),
];

pub fn command(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.id == id)
}

const MODIFIERS: [&str; 4] = ["cmd", "ctrl", "alt", "shift"];
const NAMED_KEYS: &[&str] = &[
    "space", "left", "right", "up", "down", "escape", "return", "tab", "delete", "f1", "f2", "f3",
    "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
];

/// Canonical key combo: modifiers in a fixed order, then one key.
/// `"Shift+CMD+Z"` → `"cmd+shift+z"`. `None` if malformed.
pub fn normalize_combo(input: &str) -> Option<String> {
    let parts: Vec<String> = input
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    let (key, modifiers) = parts.split_last()?;
    let key = match key.as_str() {
        "esc" => "escape".to_owned(),
        "option" | "opt" => return None,
        k if k.chars().count() == 1 && k != "+" => k.to_owned(),
        k if NAMED_KEYS.contains(&k) => k.to_owned(),
        _ => return None,
    };
    let mut seen = [false; 4];
    for m in modifiers {
        let m = match m.as_str() {
            "command" => "cmd",
            "control" => "ctrl",
            "option" | "opt" => "alt",
            other => other,
        };
        let index = MODIFIERS.iter().position(|x| *x == m)?;
        if seen[index] {
            return None;
        }
        seen[index] = true;
    }
    let mut out: Vec<&str> = MODIFIERS
        .iter()
        .zip(seen)
        .filter(|(_, s)| *s)
        .map(|(m, _)| *m)
        .collect();
    out.push(&key);
    Some(out.join("+"))
}

fn binding_key(id: &str) -> String {
    format!("keybinding.{id}")
}

/// A parsed deep link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub command: &'static str,
    pub argument: Option<String>,
}

/// `stremio://host/…/manifest.json` → install that addon (over HTTPS).
pub fn parse_deep_link(url: &str) -> Option<Invocation> {
    let rest = url.trim().strip_prefix("stremio://")?;
    if rest.is_empty() || !rest.ends_with("manifest.json") {
        return None;
    }
    Some(Invocation {
        command: "addon.install",
        argument: Some(format!("https://{rest}")),
    })
}

impl Core {
    /// Effective binding per command (user override or default).
    pub fn key_bindings(&self) -> Result<Vec<(CommandSpec, Option<String>)>, CoreError> {
        let overrides: HashMap<String, String> = self
            .db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .app_settings_with_prefix("keybinding.")?
            .into_iter()
            .collect();
        Ok(COMMANDS
            .iter()
            .map(|spec| {
                let key = match overrides.get(&binding_key(spec.id)) {
                    Some(v) if v.is_empty() => None, // explicitly unbound
                    Some(v) => normalize_combo(v),
                    None => spec.default_key.map(str::to_owned),
                };
                (*spec, key)
            })
            .collect())
    }

    /// The command bound to `combo` in `context` (player falls back to global).
    pub fn resolve_key(
        &self,
        context: CommandContext,
        combo: &str,
    ) -> Result<Option<&'static str>, CoreError> {
        let Some(combo) = normalize_combo(combo) else {
            return Ok(None);
        };
        let bindings = self.key_bindings()?;
        let find = |ctx: CommandContext| {
            bindings
                .iter()
                .find(|(spec, key)| spec.context == ctx && key.as_deref() == Some(combo.as_str()))
                .map(|(spec, _)| spec.id)
        };
        Ok(match context {
            CommandContext::Player => {
                find(CommandContext::Player).or_else(|| find(CommandContext::Global))
            }
            CommandContext::Global => find(CommandContext::Global),
        })
    }

    /// Rebinds a command (`None` = unbound). Rejects a combo already used in
    /// the same context.
    pub fn set_key_binding(&self, id: &str, combo: Option<&str>) -> Result<(), CoreError> {
        let spec =
            command(id).ok_or_else(|| CoreError::Invalid(format!("unknown command {id}")))?;
        let combo = match combo {
            Some(c) => Some(
                normalize_combo(c)
                    .ok_or_else(|| CoreError::Invalid(format!("invalid key combo {c:?}")))?,
            ),
            None => None,
        };
        if let Some(combo) = &combo
            && let Some((other, _)) = self.key_bindings()?.into_iter().find(|(s, k)| {
                s.id != id && s.context == spec.context && k.as_deref() == Some(combo.as_str())
            })
        {
            return Err(CoreError::Invalid(format!(
                "{combo} is already bound to {}",
                other.id
            )));
        }
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .set_app_setting(&binding_key(id), combo.as_deref().unwrap_or(""))?;
        self.emit(CoreEvent::KeyBindingsChanged);
        Ok(())
    }

    pub fn reset_key_binding(&self, id: &str) -> Result<(), CoreError> {
        self.db
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .delete_app_setting(&binding_key(id))?;
        self.emit(CoreEvent::KeyBindingsChanged);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn combos_normalize() {
        assert_eq!(
            normalize_combo("Shift+CMD+Z").as_deref(),
            Some("cmd+shift+z")
        );
        assert_eq!(
            normalize_combo("option+command+h").as_deref(),
            Some("cmd+alt+h")
        );
        assert_eq!(normalize_combo("Esc").as_deref(), Some("escape"));
        assert_eq!(normalize_combo("space").as_deref(), Some("space"));
        assert_eq!(normalize_combo("cmd+cmd+a"), None);
        assert_eq!(normalize_combo("hyper+a"), None);
        assert_eq!(normalize_combo("cmd+"), None);
        assert_eq!(normalize_combo("cmd+pageflip"), None);
    }

    #[test]
    fn default_bindings_are_unique_per_context_and_valid() {
        for (i, a) in COMMANDS.iter().enumerate() {
            if let Some(key) = a.default_key {
                assert_eq!(
                    normalize_combo(key).as_deref(),
                    Some(key),
                    "{} not canonical",
                    a.id
                );
                for b in &COMMANDS[i + 1..] {
                    assert!(
                        !(a.context == b.context && a.default_key == b.default_key),
                        "{} vs {}",
                        a.id,
                        b.id
                    );
                }
            }
            assert!(COMMANDS.iter().filter(|c| c.id == a.id).count() == 1);
        }
    }

    #[test]
    fn rebinding_persists_resolves_and_rejects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::open(dir.path()).unwrap();
        assert_eq!(
            core.resolve_key(CommandContext::Player, "space").unwrap(),
            Some("player.toggle_pause")
        );
        assert_eq!(
            core.resolve_key(CommandContext::Player, "cmd+q").unwrap(),
            Some("app.quit"),
            "falls back to global"
        );
        assert_eq!(
            core.resolve_key(CommandContext::Global, "space").unwrap(),
            None
        );

        core.set_key_binding("player.toggle_pause", Some("K"))
            .unwrap();
        assert_eq!(
            core.resolve_key(CommandContext::Player, "k").unwrap(),
            Some("player.toggle_pause")
        );
        assert_eq!(
            core.resolve_key(CommandContext::Player, "space").unwrap(),
            None
        );
        assert!(
            core.set_key_binding("player.close", Some("k")).is_err(),
            "conflict in same context"
        );
        assert!(core.set_key_binding("nope", Some("k")).is_err());

        core.set_key_binding("app.quit", None).unwrap();
        assert_eq!(
            core.resolve_key(CommandContext::Global, "cmd+q").unwrap(),
            None
        );
        drop(core);
        let core = Core::open(dir.path()).unwrap();
        assert_eq!(
            core.resolve_key(CommandContext::Player, "k").unwrap(),
            Some("player.toggle_pause")
        );
        core.reset_key_binding("app.quit").unwrap();
        assert_eq!(
            core.resolve_key(CommandContext::Global, "cmd+q").unwrap(),
            Some("app.quit")
        );
    }

    #[test]
    fn deep_links() {
        assert_eq!(
            parse_deep_link("stremio://addon.example.org/cfg/manifest.json"),
            Some(Invocation {
                command: "addon.install",
                argument: Some("https://addon.example.org/cfg/manifest.json".into())
            })
        );
        assert_eq!(parse_deep_link("stremio://addon.example.org/catalog"), None);
        assert_eq!(parse_deep_link("https://x/manifest.json"), None);
    }
}
