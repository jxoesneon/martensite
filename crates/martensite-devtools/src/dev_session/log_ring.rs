//! Bounded tracing ring buffer backing the `logs` dev-channel method.
//!
//! `LogRing` stores the most recent `capacity` tracing events so a dev
//! session can drain them with level/target/substring filters.
//! [`LogRing::layer`] adapts a shared ring into a
//! `tracing_subscriber::Layer` the app composes into its subscriber once
//! at startup.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;

/// Maximum characters retained per record message; longer payloads are
/// truncated so one event cannot blow the ring's memory budget.
const MAX_MESSAGE_CHARS: usize = 1024;

/// One captured tracing event.
#[derive(Debug, Clone)]
pub struct LogRecord {
    /// Monotonic capture sequence (oldest evicted first).
    pub seq: u64,
    /// Event level: `TRACE`/`DEBUG`/`INFO`/`WARN`/`ERROR`.
    pub level: String,
    /// `tracing` target (module path).
    pub target: String,
    /// Formatted message (`message` field or `fmt` of all fields).
    pub message: String,
    /// Source file when available.
    pub file: Option<String>,
    /// Source line when available.
    pub line: Option<u32>,
    /// RFC 3339 capture timestamp.
    pub timestamp: String,
}

/// Bounded FIFO buffer of [`LogRecord`]s, oldest evicted at capacity.
#[derive(Debug)]
pub struct LogRing {
    inner: Mutex<VecDeque<LogRecord>>,
    capacity: usize,
    next_seq: Mutex<u64>,
}

impl Default for LogRing {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl LogRing {
    /// Creates a ring holding at most `capacity` records.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(capacity.min(16))),
            capacity: capacity.max(1),
            next_seq: Mutex::new(0),
        }
    }

    /// Appends a record, evicting the oldest at capacity. `seq` is
    /// assigned here; messages longer than `MAX_MESSAGE_CHARS` are
    /// truncated.
    pub fn push(&self, mut record: LogRecord) {
        truncate_message(&mut record.message);
        record.seq = {
            let mut seq = self.next_seq.lock().expect("seq mutex");
            let s = *seq;
            *seq += 1;
            s
        };
        let mut ring = self.inner.lock().expect("log ring mutex");
        if ring.len() >= self.capacity {
            ring.pop_front();
        }
        ring.push_back(record);
    }

    /// Number of buffered records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().expect("log ring mutex").len()
    }

    /// `true` when empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Maximum number of records retained.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Wraps this shared ring as a [`tracing_subscriber::Layer`] the app
    /// composes into its subscriber at startup:
    ///
    /// ```no_run
    /// use std::sync::Arc;
    ///
    /// use martensite_devtools::dev_session::log_ring::LogRing;
    /// use tracing_subscriber::prelude::*;
    ///
    /// let ring = Arc::new(LogRing::new(1024));
    /// tracing_subscriber::registry().with(ring.layer()).init();
    /// ```
    #[must_use]
    pub fn layer(self: &Arc<Self>) -> LogRingLayer {
        LogRingLayer::new(Arc::clone(self))
    }

    /// Drains the newest `limit` records matching the filters.
    /// `level` is a minimum severity (`trace`<`debug`<`info`<`warn`<`error`).
    #[must_use]
    pub fn tail(
        &self,
        limit: usize,
        level: Option<&str>,
        target_prefix: Option<&str>,
        contains: Option<&str>,
    ) -> Vec<LogRecord> {
        let min = level.map(level_rank).unwrap_or(0);
        let needle = contains.map(str::to_ascii_lowercase);
        let ring = self.inner.lock().expect("log ring mutex");
        let mut out: Vec<LogRecord> = ring
            .iter()
            .filter(|r| {
                level_rank(&r.level) >= min
                    && target_prefix.is_none_or(|p| r.target.starts_with(p))
                    && needle
                        .as_ref()
                        .is_none_or(|n| r.message.to_ascii_lowercase().contains(n.as_str()))
            })
            .cloned()
            .collect();
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
        out
    }
}

fn level_rank(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "TRACE" => 0,
        "DEBUG" => 1,
        "INFO" => 2,
        "WARN" | "WARNING" => 3,
        "ERROR" | "CRITICAL" | "FATAL" => 4,
        _ => 2,
    }
}

/// Caps `message` at [`MAX_MESSAGE_CHARS`], marking truncation with a
/// trailing ellipsis. Shared with `runtime_errors` record serialization.
pub(crate) fn truncate_message(message: &mut String) {
    if message.chars().count() > MAX_MESSAGE_CHARS {
        let mut cut: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
        cut.push('…');
        *message = cut;
    }
}

/// `tracing_subscriber::Layer` pushing every event into a shared
/// [`LogRing`]. Obtain one via [`LogRing::layer`] or [`LogRingLayer::new`]
/// and compose it onto `tracing_subscriber::registry()`.
pub struct LogRingLayer {
    ring: Arc<LogRing>,
}

impl LogRingLayer {
    /// Creates a layer writing captured events into `ring`.
    #[must_use]
    pub fn new(ring: Arc<LogRing>) -> Self {
        Self { ring }
    }

    /// The ring this layer writes into.
    #[must_use]
    pub fn ring(&self) -> &Arc<LogRing> {
        &self.ring
    }
}

impl<S> Layer<S> for LogRingLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.ring.push(LogRecord {
            seq: 0, // assigned by `push`
            level: metadata.level().to_string(),
            target: metadata.target().to_string(),
            message: visitor.finish(),
            file: metadata.file().map(str::to_string),
            line: metadata.line(),
            timestamp: super::iso_now(),
        });
    }
}

/// Collects an event's fields into a displayable message: the `message`
/// field when present (followed by the remaining `key=value` fields), or
/// all fields joined when absent.
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

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }
}

impl FieldVisitor {
    fn finish(self) -> String {
        match self.message {
            Some(message) if self.fields.is_empty() => message,
            Some(message) => format!("{message} {}", self.fields.join(" ")),
            None => self.fields.join(" "),
        }
    }
}
