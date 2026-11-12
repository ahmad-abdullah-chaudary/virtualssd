//! Real backend engine log capture and streaming.
//!
//! A bounded in-memory ring buffer is attached to the global `tracing`
//! subscriber. Every event emitted by this backend — startup, provider, disk,
//! snapshot, encryption, integrity, and the shared storage-core crates — is
//! stored and, once the Tauri application is running, emitted to the frontend
//! as an `engine-log` event.
//!
//! The frontend never synthesizes entries for this stream: it displays exactly
//! what the Rust backend recorded.

use chrono::Local;
use parking_lot::Mutex;
use serde::Serialize;
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Maximum number of entries kept in memory and offered to the frontend.
pub const ENGINE_LOG_CAPACITY: usize = 500;
pub const ENGINE_LOG_EVENT: &str = "engine-log";
const MAX_MESSAGE_CHARS: usize = 4096;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EngineLogEntry {
    /// Monotonic per-process id so the UI can de-duplicate the initial fetch
    /// against entries delivered by the live event subscription.
    pub id: u64,
    /// Local wall-clock time for the console column.
    pub ts: String,
    /// RFC 3339 timestamp for diagnostics/export.
    pub timestamp: String,
    pub level: String,
    pub module: String,
    pub msg: String,
}

/// Bounded, newest-first log buffer. Split out from the Tauri plumbing so the
/// retention and ordering rules can be unit tested without a running app.
pub struct LogRing {
    entries: Mutex<VecDeque<EngineLogEntry>>,
    next_id: AtomicU64,
    capacity: usize,
}

impl LogRing {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            next_id: AtomicU64::new(1),
            capacity: capacity.max(1),
        }
    }

    /// Appends an entry, evicting the oldest one when the buffer is full, and
    /// returns the stored entry with its assigned id.
    fn push(&self, mut entry: EngineLogEntry) -> EngineLogEntry {
        entry.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut entries = self.entries.lock();
        while entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry.clone());
        entry
    }

    /// Newest-first snapshot, matching the console layout.
    fn snapshot(&self) -> Vec<EngineLogEntry> {
        self.entries.lock().iter().rev().cloned().collect()
    }

    fn clear(&self) {
        self.entries.lock().clear();
    }
}

pub struct EngineLogStore {
    ring: LogRing,
    app_handle: OnceLock<tauri::AppHandle>,
}

impl EngineLogStore {
    fn new() -> Self {
        Self {
            ring: LogRing::with_capacity(ENGINE_LOG_CAPACITY),
            app_handle: OnceLock::new(),
        }
    }

    /// Attaches the Tauri app handle so new entries are pushed live. Events
    /// recorded before this point remain in the ring buffer and are returned
    /// by the initial `get_engine_logs` fetch.
    pub fn attach_app(&self, handle: tauri::AppHandle) {
        let _ = self.app_handle.set(handle);
    }

    fn push(&self, entry: EngineLogEntry) {
        let stored = self.ring.push(entry);

        // Emission is best-effort: a closed window must not break logging.
        // The re-entrancy guard stops a log event raised by the event system
        // itself from recursing back into `emit`.
        if let Some(app) = self.app_handle.get() {
            use tauri::Emitter;
            if !EMIT_GUARD.with(|guard| guard.replace(true)) {
                let _ = app.emit(ENGINE_LOG_EVENT, &stored);
                EMIT_GUARD.with(|guard| guard.set(false));
            }
        }
    }

    /// Newest-first snapshot, matching the console layout.
    pub fn snapshot(&self) -> Vec<EngineLogEntry> {
        self.ring.snapshot()
    }

    pub fn clear(&self) {
        self.ring.clear();
    }
}

static STORE: OnceLock<EngineLogStore> = OnceLock::new();

thread_local! {
    /// Set while an event is being emitted, so a log event raised by the
    /// event system itself cannot recurse back into `emit`.
    static EMIT_GUARD: Cell<bool> = const { Cell::new(false) };
}

pub fn store() -> &'static EngineLogStore {
    STORE.get_or_init(EngineLogStore::new)
}

/// `tracing` layer that mirrors every filtered event into the ring buffer.
pub struct EngineLogLayer;

impl<S: Subscriber> Layer<S> for EngineLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let mut msg = match (visitor.message, visitor.fields.is_empty()) {
            (Some(message), true) => message,
            (Some(message), false) => format!("{message} [{}]", visitor.fields.join(", ")),
            (None, true) => "(no message)".to_string(),
            (None, false) => visitor.fields.join(", "),
        };
        if msg.chars().count() > MAX_MESSAGE_CHARS {
            msg = msg.chars().take(MAX_MESSAGE_CHARS).collect::<String>() + "…";
        }

        let now = Local::now();
        let level = match *metadata.level() {
            tracing::Level::TRACE | tracing::Level::DEBUG => "DEBUG",
            tracing::Level::INFO => "INFO",
            tracing::Level::WARN => "WARN",
            tracing::Level::ERROR => "ERROR",
        };

        store().push(EngineLogEntry {
            id: 0,
            ts: now.format("%H:%M:%S").to_string(),
            timestamp: now.to_rfc3339(),
            level: level.to_string(),
            module: display_module(metadata.target()),
            msg,
        });
    }
}

/// Converts `virtualssd_storage_core::crypto` into `storage-core::crypto`
/// while leaving third-party targets intact.
fn display_module(target: &str) -> String {
    let mut parts = target.split("::");
    let crate_name = parts.next().unwrap_or(target);
    let crate_name = match crate_name.strip_prefix("virtualssd_") {
        Some(stripped) => stripped.replace('_', "-"),
        None => crate_name.replace('_', "-"),
    };
    match parts.next() {
        Some(module) => format!("{crate_name}::{module}"),
        None => crate_name,
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(module: &str, msg: &str) -> EngineLogEntry {
        EngineLogEntry {
            id: 0,
            ts: "00:00:00".to_string(),
            timestamp: "2026-01-01T00:00:00+00:00".to_string(),
            level: "INFO".to_string(),
            module: module.to_string(),
            msg: msg.to_string(),
        }
    }

    #[test]
    fn ring_returns_entries_newest_first() {
        let ring = LogRing::with_capacity(10);
        ring.push(entry("a", "first"));
        ring.push(entry("b", "second"));
        ring.push(entry("c", "third"));

        let snapshot = ring.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].msg, "third");
        assert_eq!(snapshot[2].msg, "first");
    }

    #[test]
    fn ring_ids_are_monotonic_and_unique() {
        let ring = LogRing::with_capacity(10);
        let first = ring.push(entry("a", "first"));
        let second = ring.push(entry("a", "second"));
        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
    }

    #[test]
    fn ring_evicts_oldest_entries_at_capacity() {
        let ring = LogRing::with_capacity(3);
        for i in 0..5 {
            ring.push(entry("a", &format!("entry {i}")));
        }

        let snapshot = ring.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].msg, "entry 4");
        assert_eq!(snapshot[2].msg, "entry 2");
    }

    #[test]
    fn ring_clear_empties_the_buffer() {
        let ring = LogRing::with_capacity(10);
        ring.push(entry("a", "first"));
        ring.clear();
        assert!(ring.snapshot().is_empty());
    }

    #[test]
    fn module_names_drop_the_virtualssd_prefix() {
        assert_eq!(display_module("virtualssd_storage_core"), "storage-core");
        assert_eq!(
            display_module("virtualssd_storage_core::crypto"),
            "storage-core::crypto"
        );
        assert_eq!(
            display_module("virtualssd_provider_gdrive::upload"),
            "provider-gdrive::upload"
        );
        assert_eq!(display_module("virtualssd_desktop"), "desktop");
    }

    #[test]
    fn third_party_targets_keep_their_name() {
        assert_eq!(display_module("reqwest::connect"), "reqwest::connect");
    }
}
