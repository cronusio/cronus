//! What an event looks like once it leaves the code that emitted it.
//!
//! Redaction, the payload policy and folding happen here, where a projection
//! is rendered, and not where an event is emitted: a library crate that logs
//! cannot forget them, because it never calls them.
//!
//! **What protects today.** Two protections work: a field whose name looks like
//! a secret has its text replaced, and an invocable declared "do not journal
//! input" never has its arguments logged. The third — masking values against
//! the live secret list — is wired and **inert**: the list every surface hands
//! the dispatcher is empty, because the core does not yet expose its secret
//! store to the dispatch path. A projector
//! given an empty list masks nothing by value, and the log must not be read as
//! if it did. [`Projector::set_secrets`] is the seam the same list will arrive
//! through.

use std::sync::RwLock;

use cronus_domain::redact::redact;
use serde_json::{Map, Value};

use super::is_secret_name;
use super::line::Fields;

/// What replaces the text of a secret-named field.
pub const REDACTED: &str = "<redacted>";

/// How much of a large text field a projection keeps (`log.payload`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadPolicy {
    /// Sizes, not bodies: a large field renders as `<N bytes>`. The default.
    Descriptor,
    /// A bounded prefix, with the original size beside it in
    /// `<field>.truncated_from`.
    Excerpt,
    /// Everything.
    Full,
}

impl PayloadPolicy {
    /// Parse the `log.payload` setting's value.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "descriptor" => Some(PayloadPolicy::Descriptor),
            "excerpt" => Some(PayloadPolicy::Excerpt),
            "full" => Some(PayloadPolicy::Full),
            _ => None,
        }
    }
}

/// The payload policy and the sizes it applies at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Payload {
    /// What to do with a field larger than `threshold`.
    pub policy: PayloadPolicy,
    /// Text longer than this many bytes is a body, not a value.
    pub threshold: usize,
    /// Bytes kept by [`PayloadPolicy::Excerpt`].
    pub excerpt: usize,
}

impl Default for Payload {
    fn default() -> Self {
        Payload {
            policy: PayloadPolicy::Descriptor,
            threshold: 1024,
            excerpt: 256,
        }
    }
}

/// Redacts and reduces an event's fields for a projection.
#[derive(Debug, Default)]
pub struct Projector {
    secrets: RwLock<Vec<String>>,
    payload: RwLock<Payload>,
}

impl Projector {
    /// Replace the live secret list. Masking by value applies to every string
    /// field and to the message.
    pub fn set_secrets(&self, secrets: Vec<String>) {
        let mut held = self
            .secrets
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *held = secrets;
    }

    /// Replace the payload policy (`log.payload`).
    pub fn set_payload(&self, payload: Payload) {
        let mut held = self
            .payload
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *held = payload;
    }

    /// The payload settings now in force.
    pub fn payload(&self) -> Payload {
        *self
            .payload
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Project an event's fields. `policy` overrides the configured payload
    /// policy for this projection when given (the console always reduces).
    pub(crate) fn project(&self, fields: Fields, policy: Option<PayloadPolicy>) -> Fields {
        let secrets = self
            .secrets
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let masks: Vec<&str> = secrets.iter().map(String::as_str).collect();
        let mut payload = self.payload();
        if let Some(policy) = policy {
            payload.policy = policy;
        }

        let mut map = Map::new();
        for (key, value) in fields.map {
            let Value::String(text) = value else {
                map.insert(key, value);
                continue;
            };
            if is_secret_name(&key) {
                map.insert(key, Value::String(REDACTED.to_owned()));
                continue;
            }
            let original_len = text.len();
            // Mask before reducing, so a cut can never leave half a secret.
            let masked = redact(&text, &masks);
            if payload.policy == PayloadPolicy::Full || original_len <= payload.threshold {
                map.insert(key, Value::String(masked));
                continue;
            }
            match payload.policy {
                PayloadPolicy::Excerpt => {
                    map.insert(
                        format!("{key}.truncated_from"),
                        Value::from(original_len as u64),
                    );
                    map.insert(key, Value::String(prefix(&masked, payload.excerpt)));
                }
                _ => {
                    map.insert(key, Value::String(format!("<{original_len} bytes>")));
                }
            }
        }
        Fields {
            message: fields.message.map(|text| redact(&text, &masks)),
            map,
        }
    }
}

/// At most `max` bytes of `text`, cut on a character boundary.
fn prefix(text: &str, max: usize) -> String {
    let end = text
        .char_indices()
        .map(|(index, ch)| index + ch.len_utf8())
        .take_while(|end| *end <= max)
        .last()
        .unwrap_or(0);
    text[..end].to_owned()
}

/// An event reduced to what decides whether two of them are "the same".
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub(crate) level: String,
    pub(crate) target: String,
    pub(crate) msg: String,
    pub(crate) fields: Map<String, Value>,
}

/// A run of identical consecutive entries and when it was seen.
#[derive(Debug, Clone)]
pub(crate) struct Folded<M> {
    pub(crate) entry: Entry,
    pub(crate) first: M,
    pub(crate) last: M,
    pub(crate) count: u64,
}

/// Folds identical consecutive entries into one.
///
/// It holds the newest run back until a different entry arrives or the caller
/// takes it: [`Folder::offer`] hands back the run that just ended, and
/// [`Folder::take_if`] hands back the held one when a caller decides it has
/// waited long enough. `M` is whatever the caller wants to remember about when
/// an entry was seen.
#[derive(Debug)]
pub(crate) struct Folder<M> {
    held: Option<Folded<M>>,
}

impl<M: Clone> Folder<M> {
    pub(crate) fn new() -> Self {
        Folder { held: None }
    }

    /// Whether a run is being held.
    pub(crate) fn is_empty(&self) -> bool {
        self.held.is_none()
    }

    /// Offer the next entry. An identical one extends the held run and returns
    /// nothing; a different one starts a new run and returns the run it ended.
    pub(crate) fn offer(&mut self, entry: Entry, at: M) -> Option<Folded<M>> {
        if let Some(held) = self.held.as_mut()
            && held.entry == entry
        {
            held.count += 1;
            held.last = at;
            return None;
        }
        self.held.replace(Folded {
            entry,
            first: at.clone(),
            last: at,
            count: 1,
        })
    }

    /// Take the held run unconditionally.
    pub(crate) fn take(&mut self) -> Option<Folded<M>> {
        self.held.take()
    }

    /// Take the held run if `due` says it has been held long enough.
    pub(crate) fn take_if(&mut self, due: impl FnOnce(&Folded<M>) -> bool) -> Option<Folded<M>> {
        if self.held.as_ref().is_some_and(due) {
            self.held.take()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, Value)]) -> Fields {
        let mut map = Map::new();
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        Fields {
            message: Some("m".to_owned()),
            map,
        }
    }

    fn text(value: &str) -> Value {
        Value::String(value.to_owned())
    }

    #[test]
    fn fields_named_like_secrets_are_replaced_whatever_they_hold() {
        let projector = Projector::default();
        let projected = projector.project(
            fields(&[
                ("api_token", text("abc")),
                ("client_secret", text("def")),
                ("password", text("ghi")),
                ("authorization", text("Bearer jkl")),
                ("Proxy-Authorization", text("mno")),
                ("name", text("kept")),
            ]),
            None,
        );

        for key in [
            "api_token",
            "client_secret",
            "password",
            "authorization",
            "Proxy-Authorization",
        ] {
            assert_eq!(projected.map[key], REDACTED, "{key}");
        }
        assert_eq!(projected.map["name"], "kept");
    }

    #[test]
    fn a_count_named_like_a_secret_is_not_text_and_is_kept() {
        let projector = Projector::default();
        let projected = projector.project(
            fields(&[("input_tokens", Value::from(120)), ("token", text("abc"))]),
            None,
        );
        assert_eq!(projected.map["input_tokens"], 120);
        assert_eq!(projected.map["token"], REDACTED);
    }

    #[test]
    fn secret_values_are_masked_in_fields_and_in_the_message() {
        let projector = Projector::default();
        projector.set_secrets(vec!["sk-LIVE-777".to_owned()]);
        let mut input = fields(&[("detail", text("failed with key sk-LIVE-777 today"))]);
        input.message = Some("calling with sk-LIVE-777".to_owned());
        let projected = projector.project(input, None);

        assert_eq!(projected.map["detail"], "failed with key *** today");
        assert_eq!(projected.message.as_deref(), Some("calling with ***"));
    }

    #[test]
    fn with_an_empty_secret_list_values_are_not_masked() {
        // The honest statement of today's coverage: the list is empty, so only
        // the name denylist protects.
        let projector = Projector::default();
        let projected = projector.project(fields(&[("detail", text("sk-LIVE-777"))]), None);
        assert_eq!(projected.map["detail"], "sk-LIVE-777");
    }

    #[test]
    fn a_large_body_renders_as_its_size_by_default() {
        let projector = Projector::default();
        let body = "x".repeat(4096);
        let projected = projector.project(fields(&[("body", text(&body))]), None);

        assert_eq!(projected.map["body"], "<4096 bytes>");
        assert!(!projected.map.contains_key("body.truncated_from"));
    }

    #[test]
    fn excerpt_keeps_a_prefix_and_states_the_original_size() {
        let projector = Projector::default();
        projector.set_payload(Payload {
            policy: PayloadPolicy::Excerpt,
            ..Payload::default()
        });
        let body = "y".repeat(4096);
        let projected = projector.project(fields(&[("body", text(&body))]), None);

        assert_eq!(projected.map["body"].as_str().map(str::len), Some(256));
        assert_eq!(projected.map["body.truncated_from"], 4096);
    }

    #[test]
    fn full_keeps_everything_and_a_small_field_is_never_reduced() {
        let projector = Projector::default();
        let body = "z".repeat(4096);
        projector.set_payload(Payload {
            policy: PayloadPolicy::Full,
            ..Payload::default()
        });
        let full = projector.project(fields(&[("body", text(&body))]), None);
        assert_eq!(full.map["body"].as_str().map(str::len), Some(4096));

        projector.set_payload(Payload::default());
        let small = projector.project(fields(&[("body", text("short"))]), None);
        assert_eq!(small.map["body"], "short");
    }

    #[test]
    fn a_projection_can_force_the_descriptor_form() {
        let projector = Projector::default();
        projector.set_payload(Payload {
            policy: PayloadPolicy::Full,
            ..Payload::default()
        });
        let body = "w".repeat(2000);
        let projected = projector.project(
            fields(&[("body", text(&body))]),
            Some(PayloadPolicy::Descriptor),
        );
        assert_eq!(projected.map["body"], "<2000 bytes>");
    }

    #[test]
    fn a_secret_is_masked_before_the_excerpt_is_cut() {
        let projector = Projector::default();
        projector.set_secrets(vec!["TOPSECRETVALUE".to_owned()]);
        projector.set_payload(Payload {
            policy: PayloadPolicy::Excerpt,
            threshold: 10,
            excerpt: 8,
        });
        let projected = projector.project(
            fields(&[("body", text("TOPSECRETVALUE and then a long tail of text"))]),
            None,
        );
        let kept = projected.map["body"].as_str().expect("text").to_owned();
        assert!(
            !kept.contains("TOPSECRET"),
            "no fragment of the secret: {kept}"
        );
    }

    #[test]
    fn an_excerpt_never_cuts_inside_a_character() {
        assert_eq!(prefix("aé€", 1), "a");
        assert_eq!(prefix("aé€", 2), "a");
        assert_eq!(prefix("aé€", 3), "aé");
        assert_eq!(prefix("aé€", 6), "aé€");
        assert_eq!(prefix("é", 0), "");
    }

    #[test]
    fn the_payload_setting_parses_its_three_values() {
        assert_eq!(
            PayloadPolicy::parse("Descriptor"),
            Some(PayloadPolicy::Descriptor)
        );
        assert_eq!(
            PayloadPolicy::parse(" excerpt "),
            Some(PayloadPolicy::Excerpt)
        );
        assert_eq!(PayloadPolicy::parse("full"), Some(PayloadPolicy::Full));
        assert_eq!(PayloadPolicy::parse("everything"), None);
    }

    fn entry(msg: &str, detail: &str) -> Entry {
        let mut fields = Map::new();
        fields.insert("detail".to_owned(), Value::from(detail));
        Entry {
            level: "INFO".to_owned(),
            target: "cronus_core::x".to_owned(),
            msg: msg.to_owned(),
            fields,
        }
    }

    #[test]
    fn three_identical_consecutive_entries_fold_into_one_run_of_three() {
        let mut folder = Folder::new();
        assert!(folder.offer(entry("a", "1"), 1).is_none());
        assert!(folder.offer(entry("a", "1"), 2).is_none());
        assert!(folder.offer(entry("a", "1"), 3).is_none());

        let ended = folder.offer(entry("b", "1"), 4).expect("the run ended");
        assert_eq!(ended.count, 3);
        assert_eq!((ended.first, ended.last), (1, 3));
        assert_eq!(ended.entry.msg, "a");
    }

    #[test]
    fn a_different_entry_between_two_identical_ones_breaks_the_fold() {
        let mut folder = Folder::new();
        assert!(folder.offer(entry("a", "1"), 1).is_none());
        let first = folder.offer(entry("a", "2"), 2).expect("fields differ");
        assert_eq!(first.count, 1);
        let second = folder
            .offer(entry("a", "1"), 3)
            .expect("fields differ again");
        assert_eq!(second.count, 1);
        assert_eq!(folder.take().map(|run| run.count), Some(1));
        assert!(folder.is_empty());
    }

    #[test]
    fn level_and_target_take_part_in_the_comparison() {
        let mut folder = Folder::new();
        assert!(folder.offer(entry("a", "1"), 1).is_none());
        let mut warn = entry("a", "1");
        warn.level = "WARN".to_owned();
        assert!(folder.offer(warn, 2).is_some());
        let mut other = entry("a", "1");
        other.level = "WARN".to_owned();
        other.target = "cronus_core::y".to_owned();
        assert!(folder.offer(other, 3).is_some());
    }

    #[test]
    fn a_held_run_is_taken_only_when_due() {
        let mut folder = Folder::new();
        assert!(folder.offer(entry("a", "1"), 10).is_none());
        assert!(folder.take_if(|run| run.first >= 20).is_none());
        assert!(!folder.is_empty());
        assert_eq!(
            folder.take_if(|run| run.first >= 10).map(|run| run.count),
            Some(1)
        );
        assert!(folder.is_empty());
    }
}
