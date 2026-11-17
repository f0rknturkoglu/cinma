//! Core → UI event stream. Listeners are called synchronously on whichever
//! thread raised the event and must not block.

use std::sync::Arc;

use crate::{Core, CoreError};

#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
    /// Installed/enabled addons changed (install, deep link, toggle, reorder).
    AddonsChanged,
    /// A key binding was changed; menus should refresh their shortcuts.
    KeyBindingsChanged,
    /// Active profile or profile list changed.
    ProfileChanged,
    /// Library or watch progress changed.
    LibraryChanged,
    /// Download queue changed (added, progressed, paused, completed, removed).
    DownloadsChanged,
    /// Active LAN cast session changed.
    CastChanged,
    /// Tracking adapter state / outbox changed.
    TrackingChanged,
}

pub trait CoreEventListener: Send + Sync {
    fn on_event(&self, event: CoreEvent);
}

impl Core {
    pub fn subscribe(&self, listener: Arc<dyn CoreEventListener>) -> Result<(), CoreError> {
        self.listeners
            .lock()
            .map_err(|_| CoreError::Poisoned)?
            .push(listener);
        Ok(())
    }

    pub(crate) fn emit(&self, event: CoreEvent) {
        let listeners = match self.listeners.lock() {
            Ok(l) => l.clone(),
            Err(_) => return,
        };
        for listener in listeners {
            listener.on_event(event.clone());
        }
    }
}
