//! A spoke's side of the wire: one connection per request.

use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use cronus_contract::{Binder, Invocation, Outcome, OutcomeValue};

use super::ledger::WorkItem;
use super::seat::Record;
use super::wire::{Frame, Request, outcome_from_json, parse_frame, read_frame, request_line};

/// How long a spoke waits for the engine to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a spoke waits for the engine to answer a ping or list its work —
/// calls that do no work of their own.
const QUICK_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a request to the engine did not get its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// Nothing accepted the connection.
    Unreachable(String),
    /// The engine would not have the request.
    Refused(String),
    /// The engine said something this build does not understand.
    Protocol(String),
    /// The connection ended before the answer was whole.
    Closed,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::Unreachable(why) => write!(f, "could not reach the engine ({why})"),
            LinkError::Refused(why) => write!(f, "the engine refused the request ({why})"),
            LinkError::Protocol(why) => write!(f, "the engine's answer was not understood ({why})"),
            LinkError::Closed => f.write_str("the engine closed the connection mid-answer"),
        }
    }
}

/// How to reach the engine over one state root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    addr: SocketAddr,
    token: String,
}

/// What an invocation sent to the engine came back as.
pub enum Answer {
    /// The engine does not know the identity.
    Unknown,
    /// A finished result.
    Done(Outcome),
    /// A result still being written.
    Streaming(Streaming),
}

/// One piece of a streamed answer.
#[derive(Debug, PartialEq)]
pub enum Piece {
    Chunk(OutcomeValue),
    End(Outcome),
}

/// The rest of a streamed answer, read as it arrives.
pub struct Streaming {
    reader: BufReader<TcpStream>,
    binders: Vec<Binder>,
    first: Option<Frame>,
}

impl Streaming {
    /// The next piece. After the end, or once the connection has failed, there
    /// is nothing more.
    pub fn next_piece(&mut self) -> Result<Piece, LinkError> {
        let frame = match self.first.take() {
            Some(frame) => frame,
            None => read_reply(&mut self.reader)?,
        };
        match frame {
            Frame::Chunk(value) => Ok(Piece::Chunk(value)),
            Frame::End(json) => outcome_from_json(&json, &self.binders)
                .map(Piece::End)
                .map_err(|e| LinkError::Protocol(e.to_string())),
            Frame::Refused(why) => Err(LinkError::Refused(why)),
            other => Err(LinkError::Protocol(format!(
                "unexpected {other:?} in a stream"
            ))),
        }
    }
}

fn read_reply(reader: &mut BufReader<TcpStream>) -> Result<Frame, LinkError> {
    match read_frame(reader) {
        Ok(Some(line)) => parse_frame(&line).map_err(|e| LinkError::Protocol(e.to_string())),
        Ok(None) => Err(LinkError::Closed),
        Err(err) => Err(LinkError::Unreachable(err.to_string())),
    }
}

impl Link {
    /// The link a published record describes.
    pub fn from_record(record: &Record) -> Result<Self, LinkError> {
        let addr = record
            .addr
            .to_socket_addrs()
            .map_err(|e| LinkError::Protocol(format!("the record's address: {e}")))?
            .find(|addr| addr.ip().is_loopback())
            .ok_or_else(|| LinkError::Protocol("the record's address is not loopback".into()))?;
        Ok(Link {
            addr,
            token: record.token.clone(),
        })
    }

    /// Where the engine listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn open(&self, request: &Request) -> Result<(TcpStream, BufReader<TcpStream>), LinkError> {
        let mut stream = TcpStream::connect_timeout(&self.addr, CONNECT_TIMEOUT)
            .map_err(|e| LinkError::Unreachable(e.to_string()))?;
        let line =
            request_line(&self.token, request).map_err(|e| LinkError::Protocol(e.to_string()))?;
        writeln!(stream, "{line}")
            .and_then(|()| stream.flush())
            .map_err(|e| LinkError::Unreachable(e.to_string()))?;
        let reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|e| LinkError::Unreachable(e.to_string()))?,
        );
        Ok((stream, reader))
    }

    /// Which release answers, if anything does.
    pub fn ping(&self) -> Result<String, LinkError> {
        let (stream, mut reader) = self.open(&Request::Ping)?;
        stream
            .set_read_timeout(Some(QUICK_TIMEOUT))
            .map_err(|e| LinkError::Unreachable(e.to_string()))?;
        match read_reply(&mut reader)? {
            Frame::Pong { version } => Ok(version),
            Frame::Refused(why) => Err(LinkError::Refused(why)),
            other => Err(LinkError::Protocol(format!("unexpected {other:?}"))),
        }
    }

    /// What the engine is running.
    pub fn work(&self) -> Result<Vec<WorkItem>, LinkError> {
        let (stream, mut reader) = self.open(&Request::Work)?;
        stream
            .set_read_timeout(Some(QUICK_TIMEOUT))
            .map_err(|e| LinkError::Unreachable(e.to_string()))?;
        match read_reply(&mut reader)? {
            Frame::Work(items) => Ok(items),
            Frame::Refused(why) => Err(LinkError::Refused(why)),
            other => Err(LinkError::Protocol(format!("unexpected {other:?}"))),
        }
    }

    /// Run `invocation` at the engine, for a caller standing in `cwd`. `binders`
    /// are the invoked identity's declared binders, from which a rejection's
    /// binder name is restored.
    pub fn invoke(
        &self,
        invocation: &Invocation,
        binders: &[Binder],
        cwd: &Path,
    ) -> Result<Answer, LinkError> {
        let (_stream, mut reader) = self.open(&Request::Invoke {
            id: invocation.id.clone(),
            caller: invocation.caller,
            args: invocation.args.clone(),
            cwd: cwd.to_path_buf(),
        })?;
        match read_reply(&mut reader)? {
            Frame::Unknown => Ok(Answer::Unknown),
            Frame::Outcome(json) => outcome_from_json(&json, binders)
                .map(Answer::Done)
                .map_err(|e| LinkError::Protocol(e.to_string())),
            first @ (Frame::Chunk(_) | Frame::End(_)) => Ok(Answer::Streaming(Streaming {
                reader,
                binders: binders.to_vec(),
                first: Some(first),
            })),
            Frame::Refused(why) => Err(LinkError::Refused(why)),
            other => Err(LinkError::Protocol(format!("unexpected {other:?}"))),
        }
    }
}
