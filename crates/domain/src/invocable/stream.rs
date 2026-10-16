//! Streamed results: an invocation that answers over time rather than all at
//! once.
//!
//! A handler that streams opens a channel on the dispatcher's [`StreamHub`],
//! hands the work to a thread of its own, and returns
//! [`cronus_contract::Outcome::Stream`] naming the channel at once. The work
//! publishes **chunks** (a token, a progress line) through its [`StreamSink`]
//! and finishes with one **end** outcome — the settled result, exactly what a
//! non-streaming invocation would have returned. A surface that can show
//! chunks as they arrive subscribes; one that cannot [`Subscription::collect`]s
//! the stream and takes the end outcome, so streaming is an improvement a
//! surface may decline, never a second behavior.
//!
//! Secret masking happens here, at the single boundary every surface reads
//! from, and it holds across chunk boundaries: a text chunk is held back by as
//! many characters as the longest secret is long, less one, so a secret
//! split between two chunks is still found whole before anything is released.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use cronus_contract::{Outcome, OutcomeValue, StreamHandle};

use crate::redact::{redact, redact_outcome};

/// One thing a stream carries.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamItem {
    /// Part of the result, in order: a token of text, a progress line.
    Chunk(OutcomeValue),
    /// The settled result; nothing follows it.
    End(Outcome),
}

/// The open channels of one dispatcher, waiting for their subscriber.
#[derive(Default)]
pub struct StreamHub {
    slots: Mutex<HashMap<String, Receiver<StreamItem>>>,
    next: AtomicU64,
}

impl StreamHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a channel: the handle goes back to the caller as the invocation's
    /// outcome, the sink to the work that publishes into it.
    pub fn open(&self) -> (StreamHandle, StreamSink) {
        let (sender, receiver) = mpsc::channel();
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let channel = format!("stream-{}-{n}", std::process::id());
        self.slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(channel.clone(), receiver);
        (
            StreamHandle { channel },
            StreamSink {
                sender: Some(sender),
            },
        )
    }

    /// Take the subscription to `handle`'s channel. A channel has exactly one
    /// subscriber: a second subscription, or one to a channel that was never
    /// opened, is `None`.
    pub fn subscribe(&self, handle: &StreamHandle, secrets: &[String]) -> Option<Subscription> {
        let receiver = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&handle.channel)?;
        Some(Subscription {
            receiver,
            secrets: secrets.iter().filter(|s| !s.is_empty()).cloned().collect(),
            carry: String::new(),
            ready: VecDeque::new(),
            finished: false,
        })
    }
}

/// The publishing end of a channel, held by the work that produces the result.
pub struct StreamSink {
    sender: Option<Sender<StreamItem>>,
}

impl StreamSink {
    /// Publish a chunk. `false` means nobody is listening any more — the work
    /// may stop early, but need not.
    pub fn chunk(&self, value: OutcomeValue) -> bool {
        self.sender
            .as_ref()
            .is_some_and(|sender| sender.send(StreamItem::Chunk(value)).is_ok())
    }

    /// Finish with the settled result.
    pub fn end(mut self, outcome: Outcome) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(StreamItem::End(outcome));
        }
    }
}

impl Drop for StreamSink {
    /// Work that dies without finishing still ends its stream, saying so: a
    /// subscriber is never left waiting for a result that cannot come.
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(StreamItem::End(Outcome::Unavailable {
                reason: "the stream ended without a result".to_string(),
            }));
        }
    }
}

/// What reading a subscription without waiting found.
#[derive(Debug, Clone, PartialEq)]
pub enum Polled {
    /// The next item, already masked.
    Item(StreamItem),
    /// Nothing yet.
    Empty,
    /// The end has been delivered; nothing more will come.
    Finished,
}

/// The receiving end of a channel. Every item it yields is masked.
pub struct Subscription {
    receiver: Receiver<StreamItem>,
    secrets: Vec<String>,
    /// Text held back until it can no longer be the start of a secret.
    carry: String,
    ready: VecDeque<StreamItem>,
    finished: bool,
}

impl Subscription {
    /// The next item, waiting for it; `None` once the end was delivered.
    pub fn next_item(&mut self) -> Option<StreamItem> {
        loop {
            if let Some(item) = self.ready.pop_front() {
                return Some(item);
            }
            if self.finished {
                return None;
            }
            match self.receiver.recv() {
                Ok(raw) => self.take(raw),
                Err(_) => self.take(lost()),
            }
        }
    }

    /// The next item if there is one now.
    pub fn poll(&mut self) -> Polled {
        loop {
            if let Some(item) = self.ready.pop_front() {
                return Polled::Item(item);
            }
            if self.finished {
                return Polled::Finished;
            }
            match self.receiver.try_recv() {
                Ok(raw) => self.take(raw),
                Err(TryRecvError::Empty) => return Polled::Empty,
                Err(TryRecvError::Disconnected) => self.take(lost()),
            }
        }
    }

    /// Wait for the end and return it, setting the chunks aside — what a
    /// surface that cannot show a stream takes instead.
    pub fn collect(mut self) -> Outcome {
        while let Some(item) = self.next_item() {
            if let StreamItem::End(outcome) = item {
                return outcome;
            }
        }
        lost_outcome()
    }

    fn secrets(&self) -> Vec<&str> {
        self.secrets.iter().map(String::as_str).collect()
    }

    fn take(&mut self, raw: StreamItem) {
        match raw {
            StreamItem::Chunk(OutcomeValue::Text(text)) => {
                self.carry.push_str(&text);
                let masked = redact(&self.carry, &self.secrets());
                let hold = self
                    .secrets
                    .iter()
                    .map(|s| s.chars().count().saturating_sub(1))
                    .max()
                    .unwrap_or(0);
                let total = masked.chars().count();
                if total > hold {
                    let release: String = masked.chars().take(total - hold).collect();
                    self.carry = masked.chars().skip(total - hold).collect();
                    self.ready
                        .push_back(StreamItem::Chunk(OutcomeValue::Text(release)));
                } else {
                    self.carry = masked;
                }
            }
            StreamItem::Chunk(other) => {
                self.flush();
                let masked = redact_outcome(Outcome::Value(other), &self.secrets());
                if let Outcome::Value(value) = masked {
                    self.ready.push_back(StreamItem::Chunk(value));
                }
            }
            StreamItem::End(outcome) => {
                self.flush();
                let masked = redact_outcome(outcome, &self.secrets());
                self.ready.push_back(StreamItem::End(masked));
                self.finished = true;
            }
        }
    }

    /// Release the held-back text: nothing more is coming that could complete
    /// a secret across it.
    fn flush(&mut self) {
        if !self.carry.is_empty() {
            let text = redact(&std::mem::take(&mut self.carry), &self.secrets());
            self.ready
                .push_back(StreamItem::Chunk(OutcomeValue::Text(text)));
        }
    }
}

fn lost_outcome() -> Outcome {
    Outcome::Unavailable {
        reason: "the stream ended without a result".to_string(),
    }
}

fn lost() -> StreamItem {
    StreamItem::End(lost_outcome())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> OutcomeValue {
        OutcomeValue::Text(s.to_string())
    }

    fn drain(mut subscription: Subscription) -> (String, Outcome) {
        let mut said = String::new();
        while let Some(item) = subscription.next_item() {
            match item {
                StreamItem::Chunk(OutcomeValue::Text(t)) => said.push_str(&t),
                StreamItem::Chunk(other) => panic!("unexpected chunk {other:?}"),
                StreamItem::End(outcome) => return (said, outcome),
            }
        }
        panic!("the stream ended without an end item");
    }

    #[test]
    fn chunks_arrive_in_order_and_the_end_closes_the_stream() {
        let hub = StreamHub::new();
        let (handle, sink) = hub.open();
        let subscription = hub.subscribe(&handle, &[]).expect("subscribes");
        sink.chunk(text("a"));
        sink.chunk(text("b"));
        sink.end(Outcome::Value(text("ab")));
        let (said, end) = drain(subscription);
        assert_eq!(said, "ab");
        assert_eq!(end, Outcome::Value(text("ab")));
    }

    #[test]
    fn a_channel_has_one_subscriber_and_an_unknown_one_has_none() {
        let hub = StreamHub::new();
        let (handle, _sink) = hub.open();
        assert!(hub.subscribe(&handle, &[]).is_some());
        assert!(hub.subscribe(&handle, &[]).is_none(), "taken once");
        let stranger = StreamHandle {
            channel: "never-opened".to_string(),
        };
        assert!(hub.subscribe(&stranger, &[]).is_none());
    }

    #[test]
    fn work_that_dies_without_finishing_still_ends_its_stream_and_says_so() {
        let hub = StreamHub::new();
        let (handle, sink) = hub.open();
        let subscription = hub.subscribe(&handle, &[]).expect("subscribes");
        sink.chunk(text("half"));
        drop(sink);
        let (said, end) = drain(subscription);
        assert_eq!(said, "half");
        assert!(matches!(end, Outcome::Unavailable { .. }));
    }

    #[test]
    fn collecting_takes_the_end_and_sets_the_chunks_aside() {
        let hub = StreamHub::new();
        let (handle, sink) = hub.open();
        let subscription = hub.subscribe(&handle, &[]).expect("subscribes");
        sink.chunk(text("x"));
        sink.end(Outcome::Value(OutcomeValue::Integer(7)));
        assert_eq!(
            subscription.collect(),
            Outcome::Value(OutcomeValue::Integer(7))
        );
    }

    #[test]
    fn a_secret_split_across_chunks_is_masked_whole() {
        let hub = StreamHub::new();
        let (handle, sink) = hub.open();
        let subscription = hub
            .subscribe(&handle, &["hunter-two-secret".to_string()])
            .expect("subscribes");
        for piece in ["token is hunt", "er-two-sec", "ret ok"] {
            sink.chunk(text(piece));
        }
        sink.end(Outcome::Value(text("token is hunter-two-secret ok")));
        let (said, end) = drain(subscription);
        assert_eq!(said, "token is *** ok", "{said}");
        assert_eq!(
            end,
            Outcome::Value(text("token is *** ok")),
            "the end is masked too"
        );
    }

    #[test]
    fn masking_holds_back_only_what_could_still_become_a_secret() {
        let hub = StreamHub::new();
        let (handle, sink) = hub.open();
        let mut subscription = hub
            .subscribe(&handle, &["secret".to_string()])
            .expect("subscribes");
        sink.chunk(text("a long harmless sentence"));
        // At most `len(secret) - 1` trailing characters are held back.
        match subscription.poll() {
            Polled::Item(StreamItem::Chunk(OutcomeValue::Text(released))) => {
                assert_eq!(released, "a long harmless sen");
            }
            other => panic!("expected the released text, got {other:?}"),
        }
        assert_eq!(subscription.poll(), Polled::Empty);
        sink.end(Outcome::Value(OutcomeValue::Empty));
        // The held-back tail is released when the end arrives.
        assert_eq!(
            subscription.poll(),
            Polled::Item(StreamItem::Chunk(text("tence")))
        );
        assert!(matches!(
            subscription.poll(),
            Polled::Item(StreamItem::End(_))
        ));
        assert_eq!(subscription.poll(), Polled::Finished);
    }
}
