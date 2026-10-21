//! The wire: how an invocation and its answer are written between an engine
//! and its spokes.
//!
//! One request is one JSON object on one line; a reply is one or more such
//! lines. The contract types derive no deserialization on purpose (their names
//! and binders are `'static`), so the mapping is written out in both
//! directions here, and a rejection's binder is restored from the declared
//! binders of the invocable that was called. Values are tagged by kind and a
//! record is an ordered list of pairs, so nothing depends on object key order.

use std::io::{self, BufRead};
use std::path::PathBuf;

use cronus_contract::{
    ArgValue, ArgValues, Binder, InvocableId, Outcome, OutcomeValue, Rejection, RejectionMode,
    Surface,
};
use serde_json::{Map, Number, Value, json};

use super::ledger::{WorkItem, WorkKind};

/// The protocol version this build speaks.
pub const PROTOCOL: u64 = 1;

/// The longest frame either side accepts, in bytes.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// A frame that cannot be written or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// Not what the protocol says a frame is; the string says what was wrong.
    Malformed(String),
    /// A number that has no JSON form (NaN, infinity). Refused at the sender
    /// rather than written as something a reader would misread.
    NotFinite,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Malformed(what) => write!(f, "malformed frame: {what}"),
            WireError::NotFinite => f.write_str("a number that is not finite cannot be sent"),
        }
    }
}

fn bad(what: impl Into<String>) -> WireError {
    WireError::Malformed(what.into())
}

/// The single `(tag, payload)` of a tagged value.
fn tagged(value: &Value) -> Result<(&str, &Value), WireError> {
    let object = value
        .as_object()
        .ok_or_else(|| bad("a value is an object"))?;
    let mut entries = object.iter();
    match (entries.next(), entries.next()) {
        (Some((tag, payload)), None) => Ok((tag.as_str(), payload)),
        _ => Err(bad("a value has exactly one tag")),
    }
}

fn number(n: f64) -> Result<Value, WireError> {
    Number::from_f64(n)
        .map(Value::Number)
        .ok_or(WireError::NotFinite)
}

// ─── outcome values ──────────────────────────────────────────────────────────

/// A result value as JSON.
pub fn value_to_json(value: &OutcomeValue) -> Result<Value, WireError> {
    Ok(match value {
        OutcomeValue::Empty => json!({ "empty": true }),
        OutcomeValue::Text(text) => json!({ "text": text }),
        OutcomeValue::Integer(n) => json!({ "int": n }),
        OutcomeValue::Float(n) => json!({ "float": number(*n)? }),
        OutcomeValue::Boolean(b) => json!({ "bool": b }),
        OutcomeValue::List(items) => json!({
            "list": items.iter().map(value_to_json).collect::<Result<Vec<_>, _>>()?
        }),
        OutcomeValue::Record(fields) => json!({
            "record": fields
                .iter()
                .map(|(name, field)| Ok(json!([name, value_to_json(field)?])))
                .collect::<Result<Vec<_>, WireError>>()?
        }),
    })
}

/// A result value from JSON.
pub fn value_from_json(value: &Value) -> Result<OutcomeValue, WireError> {
    let (tag, payload) = tagged(value)?;
    match tag {
        "empty" => Ok(OutcomeValue::Empty),
        "text" => Ok(OutcomeValue::Text(
            payload
                .as_str()
                .ok_or_else(|| bad("text is a string"))?
                .to_string(),
        )),
        "int" => Ok(OutcomeValue::Integer(
            payload.as_i64().ok_or_else(|| bad("int is an integer"))?,
        )),
        "float" => Ok(OutcomeValue::Float(
            payload.as_f64().ok_or_else(|| bad("float is a number"))?,
        )),
        "bool" => Ok(OutcomeValue::Boolean(
            payload.as_bool().ok_or_else(|| bad("bool is a boolean"))?,
        )),
        "list" => Ok(OutcomeValue::List(
            payload
                .as_array()
                .ok_or_else(|| bad("list is an array"))?
                .iter()
                .map(value_from_json)
                .collect::<Result<_, _>>()?,
        )),
        "record" => {
            let mut fields = Vec::new();
            for pair in payload
                .as_array()
                .ok_or_else(|| bad("record is an array"))?
            {
                let pair = pair.as_array().ok_or_else(|| bad("a field is a pair"))?;
                let [name, field] = pair.as_slice() else {
                    return Err(bad("a field is a name and a value"));
                };
                fields.push((
                    name.as_str()
                        .ok_or_else(|| bad("a field name is a string"))?
                        .to_string(),
                    value_from_json(field)?,
                ));
            }
            Ok(OutcomeValue::Record(fields))
        }
        other => Err(bad(format!("unknown value tag {other:?}"))),
    }
}

// ─── arguments ───────────────────────────────────────────────────────────────

fn arg_to_json(arg: &ArgValue) -> Result<Value, WireError> {
    Ok(match arg {
        ArgValue::Text(text) => json!({ "text": text }),
        ArgValue::Integer(n) => json!({ "int": n }),
        ArgValue::Boolean(b) => json!({ "bool": b }),
        ArgValue::Flag => json!({ "flag": true }),
        ArgValue::Float(n) => json!({ "float": number(*n)? }),
        ArgValue::List(items) => json!({ "strings": items }),
    })
}

fn arg_from_json(value: &Value) -> Result<ArgValue, WireError> {
    let (tag, payload) = tagged(value)?;
    match tag {
        "text" => Ok(ArgValue::Text(
            payload
                .as_str()
                .ok_or_else(|| bad("text is a string"))?
                .to_string(),
        )),
        "int" => Ok(ArgValue::Integer(
            payload.as_i64().ok_or_else(|| bad("int is an integer"))?,
        )),
        "bool" => Ok(ArgValue::Boolean(
            payload.as_bool().ok_or_else(|| bad("bool is a boolean"))?,
        )),
        "flag" => Ok(ArgValue::Flag),
        "float" => Ok(ArgValue::Float(
            payload.as_f64().ok_or_else(|| bad("float is a number"))?,
        )),
        "strings" => Ok(ArgValue::List(
            payload
                .as_array()
                .ok_or_else(|| bad("strings is an array"))?
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| bad("strings holds strings"))
                })
                .collect::<Result<_, _>>()?,
        )),
        other => Err(bad(format!("unknown argument tag {other:?}"))),
    }
}

/// The arguments of an invocation as a JSON object keyed by binder name.
pub fn args_to_json(args: &ArgValues) -> Result<Value, WireError> {
    let mut object = Map::new();
    for (name, arg) in args.iter() {
        object.insert(name.to_string(), arg_to_json(arg)?);
    }
    Ok(Value::Object(object))
}

/// The arguments of an invocation from a JSON object.
pub fn args_from_json(value: &Value) -> Result<ArgValues, WireError> {
    let object = value.as_object().ok_or_else(|| bad("args is an object"))?;
    let mut args = ArgValues::new();
    for (name, arg) in object {
        args.insert(name.clone(), arg_from_json(arg)?);
    }
    Ok(args)
}

// ─── outcomes ────────────────────────────────────────────────────────────────

fn mode_word(mode: RejectionMode) -> &'static str {
    match mode {
        RejectionMode::Absent => "absent",
        RejectionMode::Unreadable => "unreadable",
        RejectionMode::Malformed => "malformed",
        RejectionMode::IllShaped => "ill-shaped",
    }
}

fn mode_from_word(word: &str) -> Option<RejectionMode> {
    match word {
        "absent" => Some(RejectionMode::Absent),
        "unreadable" => Some(RejectionMode::Unreadable),
        "malformed" => Some(RejectionMode::Malformed),
        "ill-shaped" => Some(RejectionMode::IllShaped),
        _ => None,
    }
}

/// A finished outcome as JSON. A stream handle has no wire form: the engine
/// reads the stream itself and relays it as frames.
pub fn outcome_to_json(outcome: &Outcome) -> Result<Value, WireError> {
    Ok(match outcome {
        Outcome::Value(value) => json!({ "value": value_to_json(value)? }),
        Outcome::Unavailable { reason } => json!({ "unavailable": reason }),
        Outcome::Rejected(rejection) => json!({
            "rejected": {
                "binder": rejection.binder,
                "mode": mode_word(rejection.mode),
                "detail": rejection.detail,
            }
        }),
        Outcome::Stream(_) => return Err(bad("a stream has no wire form")),
    })
}

/// The binder a rejection from a spoke names when the engine named one the
/// invoked identity does not declare. Only a handler that rejects on a name of
/// its own can produce it.
const UNDECLARED_BINDER: &str = "argument";

/// A finished outcome from JSON. A rejection's binder is restored to the
/// `'static` name the invocable declares, from `binders`.
pub fn outcome_from_json(value: &Value, binders: &[Binder]) -> Result<Outcome, WireError> {
    let (tag, payload) = tagged(value)?;
    match tag {
        "value" => Ok(Outcome::Value(value_from_json(payload)?)),
        "unavailable" => Ok(Outcome::Unavailable {
            reason: payload
                .as_str()
                .ok_or_else(|| bad("unavailable carries a reason"))?
                .to_string(),
        }),
        "rejected" => {
            let named = payload
                .get("binder")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("a rejection names its binder"))?;
            let mode = payload
                .get("mode")
                .and_then(Value::as_str)
                .and_then(mode_from_word)
                .ok_or_else(|| bad("a rejection has a known mode"))?;
            let detail = payload
                .get("detail")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("a rejection has a detail"))?
                .to_string();
            let binder = binders
                .iter()
                .find(|b| b.name == named)
                .map(|b| b.name)
                .unwrap_or(UNDECLARED_BINDER);
            Ok(Outcome::Rejected(Rejection {
                binder,
                mode,
                detail,
            }))
        }
        other => Err(bad(format!("unknown outcome tag {other:?}"))),
    }
}

// ─── surfaces ────────────────────────────────────────────────────────────────

fn surface_word(surface: Surface) -> &'static str {
    match surface {
        Surface::Cli => "cli",
        Surface::Tui => "tui",
        Surface::Desktop => "desktop",
    }
}

fn surface_from_word(word: &str) -> Option<Surface> {
    match word {
        "cli" => Some(Surface::Cli),
        "tui" => Some(Surface::Tui),
        "desktop" => Some(Surface::Desktop),
        _ => None,
    }
}

// ─── requests ────────────────────────────────────────────────────────────────

/// What a spoke asks of the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Run an invocation.
    Invoke {
        id: InvocableId,
        caller: Surface,
        args: ArgValues,
        /// Where the caller stands, so a relative path or "here" in the
        /// invocation means the caller's and not the engine's.
        cwd: PathBuf,
    },
    /// What is running.
    Work,
    /// Is anyone there, and which release.
    Ping,
}

/// A request as the one line to send, carrying `token`.
pub fn request_line(token: &str, request: &Request) -> Result<String, WireError> {
    let mut frame = json!({ "v": PROTOCOL, "token": token });
    let object = frame
        .as_object_mut()
        .ok_or_else(|| bad("a request is an object"))?;
    match request {
        Request::Invoke {
            id,
            caller,
            args,
            cwd,
        } => {
            object.insert("op".into(), json!("invoke"));
            object.insert(
                "cwd".into(),
                json!(
                    cwd.to_str()
                        .ok_or_else(|| bad("the current directory is not valid text"))?
                ),
            );
            object.insert("id".into(), json!(id.as_str()));
            object.insert("caller".into(), json!(surface_word(*caller)));
            object.insert("args".into(), args_to_json(args)?);
        }
        Request::Work => {
            object.insert("op".into(), json!("work"));
        }
        Request::Ping => {
            object.insert("op".into(), json!("ping"));
        }
    }
    Ok(frame.to_string())
}

/// What a received request line says: the version and token it presented, and
/// the request. The token is returned for the server to check; nothing here
/// decides whether it is welcome.
pub struct Received {
    pub version: u64,
    pub token: String,
    pub request: Request,
}

/// Parse a request line.
pub fn parse_request(line: &str) -> Result<Received, WireError> {
    let value: Value = serde_json::from_str(line).map_err(|e| bad(e.to_string()))?;
    let version = value
        .get("v")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("a request has a version"))?;
    let token = value
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("a request has a token"))?
        .to_string();
    let op = value
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("a request has an op"))?;
    let request = match op {
        "invoke" => {
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("invoke names an identity"))?;
            let caller = value
                .get("caller")
                .and_then(Value::as_str)
                .and_then(surface_from_word)
                .ok_or_else(|| bad("invoke names a known caller"))?;
            let args = args_from_json(value.get("args").ok_or_else(|| bad("invoke has args"))?)?;
            let cwd = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or_else(|| bad("invoke says where the caller stands"))?;
            Request::Invoke {
                id: InvocableId::new(id).map_err(|_| bad("invoke names a well-formed identity"))?,
                caller,
                args,
                cwd,
            }
        }
        "work" => Request::Work,
        "ping" => Request::Ping,
        other => return Err(bad(format!("unknown op {other:?}"))),
    };
    Ok(Received {
        version,
        token,
        request,
    })
}

// ─── replies ─────────────────────────────────────────────────────────────────

/// One line of an engine's reply.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// The identity named nothing.
    Unknown,
    /// A finished result (kept as JSON until the caller knows the binders).
    Outcome(Value),
    /// A piece of a streamed result.
    Chunk(OutcomeValue),
    /// The settled result of a stream.
    End(Value),
    /// What is running.
    Work(Vec<WorkItem>),
    /// An answer to a ping.
    Pong { version: String },
    /// The request was not welcome or not understood.
    Refused(String),
}

fn work_to_json(items: &[WorkItem]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| {
                json!({ "kind": item.kind.as_str(), "label": item.label, "since": item.since })
            })
            .collect(),
    )
}

fn work_from_json(value: &Value) -> Result<Vec<WorkItem>, WireError> {
    value
        .as_array()
        .ok_or_else(|| bad("work is an array"))?
        .iter()
        .map(|item| {
            Ok(WorkItem {
                kind: item
                    .get("kind")
                    .and_then(Value::as_str)
                    .and_then(WorkKind::parse)
                    .ok_or_else(|| bad("work has a known kind"))?,
                label: item
                    .get("label")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("work has a label"))?
                    .to_string(),
                since: item
                    .get("since")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| bad("work has a start"))?,
            })
        })
        .collect()
}

/// A reply frame as the one line to send.
pub fn frame_line(frame: &Frame) -> Result<String, WireError> {
    let value = match frame {
        Frame::Unknown => json!({ "t": "unknown" }),
        Frame::Outcome(outcome) => json!({ "t": "outcome", "outcome": outcome }),
        Frame::Chunk(value) => json!({ "t": "chunk", "value": value_to_json(value)? }),
        Frame::End(outcome) => json!({ "t": "end", "outcome": outcome }),
        Frame::Work(items) => json!({ "t": "work", "work": work_to_json(items) }),
        Frame::Pong { version } => json!({ "t": "pong", "version": version }),
        Frame::Refused(reason) => json!({ "t": "refused", "reason": reason }),
    };
    Ok(value.to_string())
}

/// Parse a reply line.
pub fn parse_frame(line: &str) -> Result<Frame, WireError> {
    let value: Value = serde_json::from_str(line).map_err(|e| bad(e.to_string()))?;
    let tag = value
        .get("t")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("a reply has a tag"))?;
    let outcome = || {
        value
            .get("outcome")
            .cloned()
            .ok_or_else(|| bad("this reply carries an outcome"))
    };
    Ok(match tag {
        "unknown" => Frame::Unknown,
        "outcome" => Frame::Outcome(outcome()?),
        "chunk" => Frame::Chunk(value_from_json(
            value
                .get("value")
                .ok_or_else(|| bad("a chunk carries a value"))?,
        )?),
        "end" => Frame::End(outcome()?),
        "work" => Frame::Work(work_from_json(
            value
                .get("work")
                .ok_or_else(|| bad("work carries the work"))?,
        )?),
        "pong" => Frame::Pong {
            version: value
                .get("version")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("pong carries a version"))?
                .to_string(),
        },
        "refused" => Frame::Refused(
            value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("refused")
                .to_string(),
        ),
        other => return Err(bad(format!("unknown reply tag {other:?}"))),
    })
}

// ─── framing ─────────────────────────────────────────────────────────────────

/// Read one line, at most [`MAX_FRAME`] bytes of it. `None` at end of input.
/// A line longer than the cap is an error, never a truncated read: the rest of
/// it would be read as the next frame.
pub fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = String::new();
    let mut limited = io::Read::take(&mut *reader, (MAX_FRAME + 1) as u64);
    let read = limited.read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if !line.ends_with('\n') && line.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a frame is longer than the protocol allows",
        ));
    }
    Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
}

#[cfg(test)]
mod tests {
    use cronus_contract::BinderKind;

    use super::*;

    fn round_trip(value: OutcomeValue) -> OutcomeValue {
        let json = value_to_json(&value).expect("writes");
        // Through a string, as on the wire.
        let reread: Value = serde_json::from_str(&json.to_string()).expect("reads");
        value_from_json(&reread).expect("parses")
    }

    #[test]
    fn every_kind_of_result_value_survives_the_wire() {
        let value = OutcomeValue::Record(vec![
            ("empty".to_string(), OutcomeValue::Empty),
            (
                "text".to_string(),
                OutcomeValue::Text("a \"quoted\"\nline ünï".to_string()),
            ),
            ("int".to_string(), OutcomeValue::Integer(i64::MIN)),
            ("float".to_string(), OutcomeValue::Float(-0.25)),
            ("bool".to_string(), OutcomeValue::Boolean(false)),
            (
                "list".to_string(),
                OutcomeValue::List(vec![
                    OutcomeValue::Integer(1),
                    OutcomeValue::List(Vec::new()),
                ]),
            ),
        ]);
        assert_eq!(round_trip(value.clone()), value);
    }

    #[test]
    fn an_empty_result_a_zero_item_list_and_an_empty_text_stay_three_things() {
        for value in [
            OutcomeValue::Empty,
            OutcomeValue::List(Vec::new()),
            OutcomeValue::Text(String::new()),
        ] {
            assert_eq!(round_trip(value.clone()), value);
        }
    }

    #[test]
    fn a_record_keeps_its_field_order() {
        let fields: Vec<(String, OutcomeValue)> = ["z", "a", "m"]
            .iter()
            .map(|name| (name.to_string(), OutcomeValue::Integer(1)))
            .collect();
        let back = round_trip(OutcomeValue::Record(fields.clone()));
        assert_eq!(back, OutcomeValue::Record(fields));
    }

    #[test]
    fn a_number_with_no_json_form_is_refused_at_the_sender() {
        for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                value_to_json(&OutcomeValue::Float(n)),
                Err(WireError::NotFinite)
            );
        }
    }

    #[test]
    fn arguments_of_every_kind_survive_the_wire() {
        let mut args = ArgValues::new();
        args.insert("text", ArgValue::Text("x".into()));
        args.insert("int", ArgValue::Integer(-7));
        args.insert("bool", ArgValue::Boolean(true));
        args.insert("flag", ArgValue::Flag);
        args.insert("float", ArgValue::Float(2.5));
        args.insert("many", ArgValue::List(vec!["a".into(), "b".into()]));
        let json = args_to_json(&args).expect("writes");
        let reread: Value = serde_json::from_str(&json.to_string()).expect("reads");
        assert_eq!(args_from_json(&reread).expect("parses"), args);
    }

    #[test]
    fn outcomes_round_trip_and_a_rejection_gets_its_static_binder_back() {
        let binders = [Binder {
            name: "id",
            kind: BinderKind::Text,
            optional: false,
        }];
        let rejected = Outcome::Rejected(Rejection {
            binder: "id",
            mode: RejectionMode::IllShaped,
            detail: "no".to_string(),
        });
        for outcome in [
            Outcome::Value(OutcomeValue::Text("ok".into())),
            Outcome::Unavailable {
                reason: "down".into(),
            },
            rejected,
        ] {
            let json = outcome_to_json(&outcome).expect("writes");
            assert_eq!(outcome_from_json(&json, &binders).expect("parses"), outcome);
        }
    }

    #[test]
    fn a_binder_the_invocable_does_not_declare_is_named_as_an_argument() {
        let json = json!({ "rejected": { "binder": "mystery", "mode": "absent", "detail": "d" } });
        match outcome_from_json(&json, &[]).expect("parses") {
            Outcome::Rejected(rejection) => assert_eq!(rejection.binder, "argument"),
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn a_stream_handle_has_no_wire_form() {
        let stream = Outcome::Stream(cronus_contract::StreamHandle {
            channel: "c".to_string(),
        });
        assert!(matches!(
            outcome_to_json(&stream),
            Err(WireError::Malformed(_))
        ));
    }

    #[test]
    fn a_request_round_trips_with_its_token() {
        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("s-1".into()));
        let request = Request::Invoke {
            id: InvocableId::new("core:session.show").expect("well-formed"),
            caller: Surface::Tui,
            args,
            cwd: PathBuf::from("/work/project"),
        };
        let line = request_line("t0k3n", &request).expect("writes");
        assert!(!line.contains('\n'), "one request is one line");
        let received = parse_request(&line).expect("parses");
        assert_eq!(received.version, PROTOCOL);
        assert_eq!(received.token, "t0k3n");
        assert_eq!(received.request, request);
        for simple in [Request::Work, Request::Ping] {
            let line = request_line("t", &simple).expect("writes");
            assert_eq!(parse_request(&line).expect("parses").request, simple);
        }
    }

    #[test]
    fn requests_that_are_not_requests_are_malformed() {
        for line in [
            "",
            "not json",
            "{}",
            r#"{"v":1,"token":"t"}"#,
            r#"{"v":1,"token":"t","op":"explode"}"#,
            r#"{"v":1,"token":"t","op":"invoke","id":"nope","caller":"cli","args":{}}"#,
            r#"{"v":1,"token":"t","op":"invoke","id":"core:x.y","caller":"mars","args":{}}"#,
        ] {
            assert!(parse_request(line).is_err(), "{line:?} should not parse");
        }
    }

    #[test]
    fn every_reply_frame_round_trips() {
        let item = WorkItem {
            kind: WorkKind::AgentTurn,
            label: "s-1".to_string(),
            since: 12,
        };
        for frame in [
            Frame::Unknown,
            Frame::Outcome(json!({ "unavailable": "down" })),
            Frame::Chunk(OutcomeValue::Text("tok".into())),
            Frame::End(json!({ "value": { "empty": true } })),
            Frame::Work(vec![item]),
            Frame::Pong {
                version: "1.0".into(),
            },
            Frame::Refused("because".into()),
        ] {
            let line = frame_line(&frame).expect("writes");
            assert!(!line.contains('\n'));
            assert_eq!(parse_frame(&line).expect("parses"), frame);
        }
    }

    #[test]
    fn frames_are_read_a_line_at_a_time_with_the_newline_taken_off() {
        let mut input = io::Cursor::new(b"first\r\nsecond\n".to_vec());
        assert_eq!(
            read_frame(&mut input).expect("reads"),
            Some("first".to_string())
        );
        assert_eq!(
            read_frame(&mut input).expect("reads"),
            Some("second".to_string())
        );
        assert_eq!(read_frame(&mut input).expect("reads"), None);
    }

    #[test]
    fn a_frame_longer_than_the_cap_is_an_error_not_a_truncated_read() {
        let mut huge = vec![b'x'; MAX_FRAME + 10];
        huge.push(b'\n');
        let mut input = io::Cursor::new(huge);
        assert!(read_frame(&mut input).is_err());
    }
}
