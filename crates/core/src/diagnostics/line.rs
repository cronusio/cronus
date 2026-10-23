//! The diagnostic log's line format: one JSON object per event, schema `v` 1.
//!
//! Every line starts with the same attribution keys in the same order —
//! `v`, `ts`, `level`, `target`, `kind`, `session`, `msg` — followed by the
//! event's own fields in name order. A folded run of identical events also
//! carries `repeat` and `last_ts`. A field an event names like one of those
//! keys is kept under `field.<name>`, so a caller can never overwrite who wrote
//! the line, when, or how many times.

use std::fmt::Debug;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use tracing::field::{Field, Visit};

/// Schema version carried by every line (`v`). A change to the line shape
/// bumps it; readers handle every version they know.
pub const SCHEMA_VERSION: u64 = 1;

/// Keys the line itself owns.
const RESERVED: [&str; 9] = [
    "v", "ts", "level", "target", "kind", "session", "msg", "repeat", "last_ts",
];

/// The fields of one event, split into its message and everything else.
#[derive(Debug, Default)]
pub(crate) struct Fields {
    pub(crate) message: Option<String>,
    pub(crate) map: Map<String, Value>,
}

impl Fields {
    fn put(&mut self, field: &Field, value: Value) {
        let name = field.name();
        if name == "message" {
            self.message = Some(match value {
                Value::String(text) => text,
                other => other.to_string(),
            });
            return;
        }
        let key = if RESERVED.contains(&name) {
            format!("field.{name}")
        } else {
            name.to_owned()
        };
        self.map.insert(key, value);
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.put(field, Value::String(format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::String(value.to_owned()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::Bool(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        // JSON has no NaN or infinity; those stay readable as text.
        match serde_json::Number::from_f64(value) {
            Some(number) => self.put(field, Value::Number(number)),
            None => self.put(field, Value::String(value.to_string())),
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.put(field, Value::String(value.to_string()));
    }
}

/// Who a line is attributed to: the process kind and the run id.
#[derive(Debug, Clone)]
pub(crate) struct Attribution {
    pub(crate) kind: String,
    pub(crate) session: String,
}

fn push_json_string(out: &mut String, text: &str) {
    // `Value`'s `Display` is infallible, unlike `serde_json::to_string`.
    out.push_str(&Value::String(text.to_owned()).to_string());
}

fn push_field(out: &mut String, key: &str, value: &str) {
    out.push(',');
    push_json_string(out, key);
    out.push(':');
    push_json_string(out, value);
}

/// Render one line, newline included, from its parts.
pub(crate) fn render_parts(
    who: &Attribution,
    level: &str,
    target: &str,
    msg: &str,
    fields: Map<String, Value>,
    now: SystemTime,
) -> String {
    let mut out = String::with_capacity(192);
    out.push_str("{\"v\":");
    out.push_str(&SCHEMA_VERSION.to_string());
    push_field(&mut out, "ts", &utc_millis(now));
    push_field(&mut out, "level", level);
    push_field(&mut out, "target", target);
    push_field(&mut out, "kind", &who.kind);
    push_field(&mut out, "session", &who.session);
    push_field(&mut out, "msg", msg);
    for (key, value) in fields {
        out.push(',');
        push_json_string(&mut out, &key);
        out.push(':');
        out.push_str(&value.to_string());
    }
    out.push_str("}\n");
    out
}

/// `2026-10-22T09:14:03.120Z` — UTC, millisecond precision.
pub(crate) fn utc_millis(time: SystemTime) -> String {
    let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX / 2);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let in_day = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3600,
        (in_day % 3600) / 60,
        in_day % 60,
        since_epoch.subsec_millis()
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(seconds: u64, millis: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(seconds * 1000 + millis)
    }

    #[test]
    fn timestamps_are_utc_with_milliseconds() {
        assert_eq!(utc_millis(at(0, 0)), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            utc_millis(at(1_700_000_000, 123)),
            "2023-11-14T22:13:20.123Z"
        );
        assert_eq!(utc_millis(at(1_709_164_800, 0)), "2024-02-29T00:00:00.000Z");
        assert_eq!(
            utc_millis(at(1_709_251_199, 999)),
            "2024-02-29T23:59:59.999Z"
        );
        assert_eq!(utc_millis(at(4_102_444_800, 5)), "2100-01-01T00:00:00.005Z");
    }

    #[test]
    fn a_time_before_the_epoch_does_not_panic() {
        let before = UNIX_EPOCH - Duration::from_secs(10);
        assert_eq!(utc_millis(before), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn attribution_keys_come_first_and_an_event_cannot_overwrite_them() {
        let who = Attribution {
            kind: "cli".to_owned(),
            session: "abc".to_owned(),
        };
        let mut map = Map::new();
        map.insert("bytes".to_owned(), Value::from(5));
        map.insert("field.ts".to_owned(), Value::from("kept"));
        let line = render_parts(&who, "INFO", "cronus_core::x", "hello", map, at(0, 0));

        assert!(line.starts_with(
            "{\"v\":1,\"ts\":\"1970-01-01T00:00:00.000Z\",\"level\":\"INFO\",\
             \"target\":\"cronus_core::x\",\"kind\":\"cli\",\"session\":\"abc\",\"msg\":\"hello\""
        ));
        assert!(line.ends_with("}\n"));
        let parsed: Value = serde_json::from_str(line.trim_end()).expect("valid JSON");
        assert_eq!(parsed["bytes"], 5);
        assert_eq!(parsed["field.ts"], "kept");
        assert_eq!(parsed["ts"], "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn text_with_quotes_and_newlines_stays_one_valid_line() {
        let who = Attribution {
            kind: "cli".to_owned(),
            session: "abc".to_owned(),
        };
        let mut map = Map::new();
        map.insert("path".to_owned(), Value::from("C:\\a \"b\"\nc"));
        let line = render_parts(&who, "WARN", "t", "m\nsecond", map, at(0, 0));

        assert_eq!(
            line.matches('\n').count(),
            1,
            "exactly the terminating newline"
        );
        let parsed: Value = serde_json::from_str(line.trim_end()).expect("valid JSON");
        assert_eq!(parsed["path"], "C:\\a \"b\"\nc");
        assert_eq!(parsed["msg"], "m\nsecond");
    }
}
