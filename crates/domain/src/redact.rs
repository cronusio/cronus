//! Output/log redaction: scrub known secret values before rendering.

use cronus_contract::{Outcome, OutcomeValue, Rejection};

/// The mask substituted for any secret occurrence.
pub const MASK: &str = "***";

/// Replace every non-empty secret value in `text` with [`MASK`].
///
/// Apply to anything written to logs, the CLI/TUI, or error reports so secret
/// values never leak into rendered output.
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret, MASK);
        }
    }
    out
}

/// Redact every string a dispatched [`Outcome`] carries — the single place
/// this happens for the invocable dispatch boundary, so every surface
/// receives already-masked output rather than each re-implementing this
/// call (or, as before this existed, omitting it).
pub fn redact_outcome(outcome: Outcome, secrets: &[&str]) -> Outcome {
    match outcome {
        Outcome::Value(value) => Outcome::Value(redact_value(value, secrets)),
        // A stream handle names a channel to subscribe to, not rendered
        // text — nothing here to mask.
        Outcome::Stream(handle) => Outcome::Stream(handle),
        Outcome::Rejected(Rejection {
            binder,
            mode,
            detail,
        }) => Outcome::Rejected(Rejection {
            binder,
            mode,
            detail: redact(&detail, secrets),
        }),
        Outcome::Unavailable { reason } => Outcome::Unavailable {
            reason: redact(&reason, secrets),
        },
    }
}

fn redact_value(value: OutcomeValue, secrets: &[&str]) -> OutcomeValue {
    match value {
        OutcomeValue::Text(text) => OutcomeValue::Text(redact(&text, secrets)),
        OutcomeValue::List(items) => OutcomeValue::List(
            items
                .into_iter()
                .map(|v| redact_value(v, secrets))
                .collect(),
        ),
        OutcomeValue::Record(fields) => OutcomeValue::Record(
            fields
                .into_iter()
                .map(|(k, v)| (k, redact_value(v, secrets)))
                .collect(),
        ),
        // Empty/Integer/Float/Boolean carry no secret-bearing string.
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_known_secrets() {
        let line = "auth=Bearer sk-LIVE-12345 user=alice";
        let red = redact(line, &["sk-LIVE-12345"]);
        assert!(!red.contains("sk-LIVE-12345"));
        assert!(red.contains(MASK));
        assert!(red.contains("user=alice"), "non-secret content preserved");
    }

    #[test]
    fn empty_secret_is_ignored() {
        assert_eq!(redact("unchanged", &[""]), "unchanged");
    }

    #[test]
    fn redact_outcome_masks_text_inside_nested_records_and_lists() {
        let outcome = Outcome::Value(OutcomeValue::Record(vec![(
            "tags".to_string(),
            OutcomeValue::List(vec![OutcomeValue::Text("token=sk-LIVE-777".to_string())]),
        )]));
        let redacted = redact_outcome(outcome, &["sk-LIVE-777"]);
        match redacted {
            Outcome::Value(OutcomeValue::Record(fields)) => match &fields[0].1 {
                OutcomeValue::List(items) => match &items[0] {
                    OutcomeValue::Text(text) => {
                        assert!(!text.contains("sk-LIVE-777"));
                        assert!(text.contains(MASK));
                    }
                    other => panic!("expected Text, got {other:?}"),
                },
                other => panic!("expected List, got {other:?}"),
            },
            other => panic!("expected Value(Record), got {other:?}"),
        }
    }

    #[test]
    fn redact_outcome_masks_rejection_detail_and_unavailable_reason() {
        use cronus_contract::RejectionMode;

        let rejected = Outcome::Rejected(Rejection {
            binder: "token",
            mode: RejectionMode::IllShaped,
            detail: "got sk-LIVE-777".to_string(),
        });
        match redact_outcome(rejected, &["sk-LIVE-777"]) {
            Outcome::Rejected(r) => assert!(!r.detail.contains("sk-LIVE-777")),
            other => panic!("expected Rejected, got {other:?}"),
        }

        let unavailable = Outcome::Unavailable {
            reason: "backend sk-LIVE-777 unreachable".to_string(),
        };
        match redact_outcome(unavailable, &["sk-LIVE-777"]) {
            Outcome::Unavailable { reason } => assert!(!reason.contains("sk-LIVE-777")),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }
}
