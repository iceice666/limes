use std::sync::{Arc, Mutex};

use limes_proto::LimesEvent;

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &LimesEvent);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StderrEventSink;

impl EventSink for StderrEventSink {
    fn emit(&self, event: &LimesEvent) {
        eprintln!("limes event: {event:?}");
    }
}

#[derive(Clone, Default)]
pub struct EventBus {
    sinks: Arc<Mutex<Vec<Arc<dyn EventSink>>>>,
}

impl EventBus {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_env() -> Self {
        let events = Self::new();
        if std::env::var_os("LIMES_LOG_EVENTS").is_some() {
            events.subscribe(Arc::new(StderrEventSink));
        }
        events
    }

    pub fn subscribe(&self, sink: Arc<dyn EventSink>) {
        if let Ok(mut sinks) = self.sinks.lock() {
            sinks.push(sink);
        }
    }

    pub fn emit(&self, event: LimesEvent) {
        let sinks = self
            .sinks
            .lock()
            .map(|sinks| sinks.clone())
            .unwrap_or_default();

        for sink in sinks {
            sink.emit(&event);
        }
    }
}
