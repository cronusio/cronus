//! Telemetry: opt-in, program-data-only improvement metrics. Off
//! by default; nothing is recorded until the user opts in, and opting out
//! drops whatever was queued — "off" means off, not "silently accumulating
//! for later." Sending itself is a separate, gated step this module does
//! not perform — it hands the caller a drained batch to push through the
//! security egress gate.
//!
//! `MetricPayload` structurally excludes user content: it carries only
//! counters, durations, and booleans — there is no string/free-text field a
//! caller could put project content into. The one string on
//! [`MetricEvent`] (`name`) is checked against a closed allowlist before a
//! record is ever accepted.

/// Known program-metric identifiers (the allowlist filter). A name
/// outside this set is rejected before it ever reaches the queue.
pub const KNOWN_METRIC_NAMES: &[&str] = &[
    "startup",
    "shutdown",
    "model_route",
    "memory_query",
    "doctor_check",
    "backup_create",
    "board_transition",
    "workflow_run",
];

/// The value shape for a metric. Every variant is a plain number/bool —
/// there is no variant that could carry a file path, a prompt, or any other
/// user content (by construction, not by convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricPayload {
    Latency { duration_ms: u64 },
    Count { value: u64 },
    Outcome { success: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricEvent {
    pub name: String,
    pub payload: MetricPayload,
    pub recorded_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownMetric;

/// Local telemetry queue. `opted_in` defaults to `false`; recording
/// itself is gated on opt-in — before opting in there is nothing
/// telemetry-side to inspect or send, which keeps "off" an honest default
/// rather than a silent local accumulation the user never agreed to start.
#[derive(Debug, Default)]
pub struct TelemetryStore {
    opted_in: bool,
    queued: Vec<MetricEvent>,
}

impl TelemetryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_opted_in(&self) -> bool {
        self.opted_in
    }

    /// Flip the master switch. Opting out drops any queued events —
    /// nothing recorded while previously opted in survives a withdrawal of
    /// consent.
    pub fn set_opt_in(&mut self, opted_in: bool) {
        self.opted_in = opted_in;
        if !opted_in {
            self.queued.clear();
        }
    }

    /// Record a program metric. A no-op (returns `Ok(())` without queueing)
    /// while opted out — no telemetry event is ever emitted absent explicit
    /// opt-in. An unknown metric name is rejected regardless of opt-in state.
    pub fn record(
        &mut self,
        name: &str,
        payload: MetricPayload,
        at: u64,
    ) -> Result<(), UnknownMetric> {
        if !KNOWN_METRIC_NAMES.contains(&name) {
            return Err(UnknownMetric);
        }
        if !self.opted_in {
            return Ok(());
        }
        self.queued.push(MetricEvent {
            name: name.to_string(),
            payload,
            recorded_at: at,
        });
        Ok(())
    }

    /// Transparency: exactly what would be sent, inspectable at any time.
    pub fn inspect(&self) -> &[MetricEvent] {
        &self.queued
    }

    /// Drain the queue for sending. Sending is the caller's job (through the
    /// security egress gate) — this only hands over the batch, and
    /// only while opted in.
    pub fn drain_for_send(&mut self) -> Vec<MetricEvent> {
        if !self.opted_in {
            return Vec::new();
        }
        std::mem::take(&mut self.queued)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opted_out_by_default() {
        assert!(!TelemetryStore::new().is_opted_in());
    }

    #[test]
    fn no_event_is_recorded_while_opted_out() {
        let mut store = TelemetryStore::new();
        store
            .record("startup", MetricPayload::Count { value: 1 }, 100)
            .unwrap();
        assert!(
            store.inspect().is_empty(),
            "recording is a no-op absent opt-in"
        );
    }

    #[test]
    fn an_opted_in_event_is_queued_and_inspectable() {
        let mut store = TelemetryStore::new();
        store.set_opt_in(true);
        store
            .record(
                "model_route",
                MetricPayload::Latency { duration_ms: 42 },
                100,
            )
            .unwrap();

        assert_eq!(store.inspect().len(), 1);
        assert_eq!(store.inspect()[0].name, "model_route");
        assert_eq!(
            store.inspect()[0].payload,
            MetricPayload::Latency { duration_ms: 42 }
        );
    }

    #[test]
    fn an_unknown_metric_name_is_rejected_even_when_opted_in() {
        let mut store = TelemetryStore::new();
        store.set_opt_in(true);
        let result = store.record("user_project_path", MetricPayload::Count { value: 1 }, 100);
        assert_eq!(result, Err(UnknownMetric));
        assert!(store.inspect().is_empty());
    }

    #[test]
    fn opting_out_drops_whatever_was_queued() {
        let mut store = TelemetryStore::new();
        store.set_opt_in(true);
        store
            .record("startup", MetricPayload::Count { value: 1 }, 100)
            .unwrap();
        assert_eq!(store.inspect().len(), 1);

        store.set_opt_in(false);
        assert!(store.inspect().is_empty(), "opt-out drops queued events");
    }

    #[test]
    fn drain_for_send_returns_nothing_while_opted_out() {
        let mut store = TelemetryStore::new();
        // Force a queued item via opt-in, then opt back out to simulate a
        // stale drain attempt after withdrawal (queue is already empty by
        // then, but drain must also refuse to hand back anything regardless).
        store.set_opt_in(true);
        store
            .record("shutdown", MetricPayload::Outcome { success: true }, 1)
            .unwrap();
        store.set_opt_in(false);

        assert!(store.drain_for_send().is_empty());
    }

    #[test]
    fn drain_for_send_hands_over_and_empties_the_queue_when_opted_in() {
        let mut store = TelemetryStore::new();
        store.set_opt_in(true);
        store
            .record("doctor_check", MetricPayload::Outcome { success: true }, 1)
            .unwrap();
        store
            .record("doctor_check", MetricPayload::Outcome { success: false }, 2)
            .unwrap();

        let batch = store.drain_for_send();
        assert_eq!(batch.len(), 2);
        assert!(
            store.inspect().is_empty(),
            "drained events are removed from the local queue"
        );
    }

    #[test]
    fn payload_shape_carries_no_free_text_field() {
        // Structural guarantee: every payload variant is a number or
        // bool. This test exists to make the guarantee explicit and to fail
        // loudly (a compile error) if a future edit adds a string field.
        let _latency = MetricPayload::Latency { duration_ms: 0 };
        let _count = MetricPayload::Count { value: 0 };
        let _outcome = MetricPayload::Outcome { success: true };
    }
}
