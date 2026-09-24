//! `cronus-model-local` — the streaming REST transport realizing
//! Endpoint profiles over the federated local provider
//! catalog, the streaming generate call,
//! plus (in a later phase task) embed/describe/pull and failure mapping.
//!
//! This module's scope so far: the endpoint-profile model, its reachability
//! probe, and the streaming generate call. The
//! *address* (`api_base`) always comes from the caller (the router's
//! policy, owned by the router) — this crate never invents or looks one up;
//! it only adds the how-to-talk layer (protocol family, capability flags,
//! probe rules, wire framing) over that address.
//!
//! **On the HTTP client:** the loopback catalog (the only profiles
//! `EndpointProfile::new` constructs today) needs no TLS, and the
//! cooperative-cancellation contract (poll a short read timeout, check the
//! `CancelHandle`, retry) does not map cleanly onto a higher-level client's
//! whole-body timeout model — so the streaming call below drives a raw
//! `TcpStream` directly. A real HTTP+TLS client
//! is deferred to the remote/egress-gated
//! profile path, where TLS is genuinely required; adding it before that
//! path exists would be an unused dependency.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;
use std::time::Duration;

use cronus_contract::{
    CancelHandle, GenerateRequest, InferenceBackend, InferenceError, ModelDescriptor, PullProgress,
    ResidencyHint, StreamEvent,
};

/// The stack's probe discipline: no probe blocks longer than this by
/// default. Tests override via `probe_with_timeout`.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_millis(800);

/// Which request/response shape a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolFamily {
    /// OpenAI-compatible `/v1/...` surface (llama.cpp, MLX, vLLM, LM Studio,
    /// Docker Model Runner).
    OpenAiCompatible,
    /// Provider-native REST (e.g. Ollama's `/api/*`).
    Native,
}

/// Capability flags an endpoint declares — data, not a
/// promise: a capability absent here is reported to the caller as absent,
/// never silently emulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub streaming: bool,
    pub embeddings: bool,
    pub pull: bool,
    pub residency_control: bool,
    pub digest_reporting: bool,
}

/// Why constructing an `EndpointProfile` was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// `api_base` did not parse into a connectable host[:port].
    InvalidAddress(String),
    /// The host is not loopback, and `EndpointProfile::new` is loopback-only
    /// by default — a remote endpoint must be built with
    /// [`EndpointProfile::new_remote`], which requires an [`EgressGrant`].
    NotLoopback(String),
    /// The host is a wildcard bind address (`0.0.0.0` / `::`), never a
    /// valid connect target.
    WildcardAddress(String),
    /// A remote profile's [`EgressGrant`] authorized a different endpoint
    /// than the one being constructed — the grant is endpoint-scoped and is
    /// not a blanket egress permit.
    GrantEndpointMismatch { granted: String, requested: String },
}

/// Resolves the credential for a remote endpoint **at call time**, so the
/// secret is never cached in the profile or in config. The
/// concrete implementation wraps the secret store; the transport only calls
/// `resolve` when it is about to build a request and forgets the result
/// immediately after attaching it. `None` means "no credential available"
/// — the call proceeds unauthenticated and the endpoint's own auth failure
/// (a 401/403) surfaces through the normal taxonomy.
pub trait CredentialResolver: Send + Sync {
    /// Return the bearer credential for `endpoint`, freshly, per call.
    fn resolve(&self, endpoint: &str) -> Option<String>;
}

/// Proof that the security egress gate authorized reaching a
/// specific remote endpoint. Minted by the security layer that owns the
/// egress decision — **never** by the transport itself (the agent's
/// execution plane cannot self-authorize egress). Requiring one to build a
/// remote profile makes "no remote call without an egress grant" a
/// compile-time property: [`EndpointProfile::new_remote`] cannot be called
/// without it. The grant is endpoint-scoped, not a blanket permit.
#[derive(Debug, Clone)]
pub struct EgressGrant {
    endpoint: String,
}

impl EgressGrant {
    /// Mint a grant for `endpoint`. Called by the security layer once its
    /// egress gate has approved reaching `endpoint`; the transport receives
    /// the minted grant, it does not construct one on its own behalf.
    pub fn for_endpoint(endpoint: impl Into<String>) -> Self {
        EgressGrant {
            endpoint: endpoint.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// Outcome of a reachability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    Reachable,
    ConnectionRefused,
    Timeout,
    InvalidAddress,
}

/// How to talk to one catalog provider — data, not code. The address
/// (`api_base`) is supplied by the caller (the router's policy); this
/// profile adds only the how-to-talk layer, never a parallel address
/// registry.
///
/// Not `PartialEq`/`Eq`: a remote profile carries a credential resolver
/// (`Arc<dyn CredentialResolver>`) that has no meaningful equality, and
/// comparing profiles is not a use this crate needs. `Debug` is hand-written
/// to redact the resolver — a secret source must never reach a log.
#[derive(Clone)]
pub struct EndpointProfile {
    api_base: String,
    protocol: ProtocolFamily,
    capabilities: Capabilities,
    /// `Some` only for remote (non-loopback) profiles built via
    /// [`Self::new_remote`]. Holds the call-time credential resolver, never
    /// a resolved secret.
    remote_auth: Option<Arc<dyn CredentialResolver>>,
}

impl fmt::Debug for EndpointProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EndpointProfile")
            .field("api_base", &self.api_base)
            .field("protocol", &self.protocol)
            .field("capabilities", &self.capabilities)
            .field(
                "remote_auth",
                &self.remote_auth.as_ref().map(|_| "<credential-resolver>"),
            )
            .finish()
    }
}

impl EndpointProfile {
    /// Construct a loopback-only profile — the default. Rejects a
    /// non-loopback host and a wildcard bind address; a remote profile is a
    /// distinct, egress-gated construction path ([`Self::new_remote`]).
    pub fn new(
        api_base: impl Into<String>,
        protocol: ProtocolFamily,
        capabilities: Capabilities,
    ) -> Result<Self, ProfileError> {
        let api_base = api_base.into();
        let host =
            host_of(&api_base).ok_or_else(|| ProfileError::InvalidAddress(api_base.clone()))?;

        if !host.eq_ignore_ascii_case("localhost") {
            match host.parse::<IpAddr>() {
                Ok(ip) if ip.is_unspecified() => {
                    return Err(ProfileError::WildcardAddress(api_base));
                }
                Ok(ip) if ip.is_loopback() => {}
                _ => return Err(ProfileError::NotLoopback(api_base)),
            }
        }

        Ok(EndpointProfile {
            api_base,
            protocol,
            capabilities,
            remote_auth: None,
        })
    }

    /// Construct a **remote** (non-loopback-permitted) profile. Requires an
    /// [`EgressGrant`] scoped to this exact `api_base` — the type
    /// makes "no remote profile without an egress grant" a compile-time
    /// guarantee. The credential is not passed here: a [`CredentialResolver`]
    /// is stored and invoked per call so the secret is never cached.
    /// A wildcard bind address is still rejected.
    pub fn new_remote(
        api_base: impl Into<String>,
        protocol: ProtocolFamily,
        capabilities: Capabilities,
        grant: &EgressGrant,
        credential: Arc<dyn CredentialResolver>,
    ) -> Result<Self, ProfileError> {
        let api_base = api_base.into();
        if grant.endpoint() != api_base {
            return Err(ProfileError::GrantEndpointMismatch {
                granted: grant.endpoint().to_string(),
                requested: api_base,
            });
        }
        let host =
            host_of(&api_base).ok_or_else(|| ProfileError::InvalidAddress(api_base.clone()))?;
        if let Ok(ip) = host.parse::<IpAddr>()
            && ip.is_unspecified()
        {
            return Err(ProfileError::WildcardAddress(api_base));
        }

        Ok(EndpointProfile {
            api_base,
            protocol,
            capabilities,
            remote_auth: Some(credential),
        })
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    pub fn protocol(&self) -> ProtocolFamily {
        self.protocol
    }

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    /// Reachability probe with the stack's default 800ms budget.
    pub fn probe(&self) -> ProbeOutcome {
        self.probe_with_timeout(DEFAULT_PROBE_TIMEOUT)
    }

    /// Reachability probe with an explicit timeout (test seam) — a pure TCP
    /// connectivity check, no wire protocol spoken.
    pub fn probe_with_timeout(&self, timeout: Duration) -> ProbeOutcome {
        let Some(addr) = resolve_one(&self.api_base) else {
            return ProbeOutcome::InvalidAddress;
        };
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(_) => ProbeOutcome::Reachable,
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                ProbeOutcome::ConnectionRefused
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => ProbeOutcome::Timeout,
            // A conservative default: any other connect failure (e.g. "no
            // route to host", host-OS-dependent) is reported as Timeout —
            // "not reachable within budget" is the honest characterization
            // shared by every non-refused failure mode.
            Err(_) => ProbeOutcome::Timeout,
        }
    }

    /// Stream a generation call with the default bounded channel capacity.
    /// Blocking pull-iterator: a worker thread owns the HTTP
    /// connection; the caller drives it by advancing the returned
    /// `StreamReceiver`.
    pub fn generate_stream(
        &self,
        request: &GenerateRequest,
        cancel: CancelHandle,
    ) -> StreamReceiver {
        self.generate_stream_with_capacity(request, cancel, DEFAULT_CHANNEL_CAPACITY)
    }

    /// Same as [`Self::generate_stream`] with an explicit bounded-channel
    /// capacity — a tuning/test seam, not a per-call protocol concern.
    pub fn generate_stream_with_capacity(
        &self,
        request: &GenerateRequest,
        cancel: CancelHandle,
        capacity: usize,
    ) -> StreamReceiver {
        let (tx, rx) = sync_channel(capacity);
        let api_base = self.api_base.clone();
        let protocol = self.protocol;
        let request = request.clone();
        // Resolve the credential now (at call time), never earlier: the
        // profile caches only the resolver, and the resolved secret lives
        // only for this call's worker.
        let auth_header = self.resolve_auth_header();
        thread::spawn(move || {
            run_generate_worker(&api_base, protocol, &request, auth_header, &cancel, &tx)
        });
        StreamReceiver {
            rx,
            terminated: false,
        }
    }

    /// Resolve this profile's bearer credential fresh, per call. `None` for
    /// a loopback profile (no auth) or when the resolver has no credential.
    fn resolve_auth_header(&self) -> Option<String> {
        self.remote_auth
            .as_ref()
            .and_then(|r| r.resolve(&self.api_base))
            .map(|secret| format!("Bearer {secret}"))
    }

    /// Embed `input` with `model`. Capability-gated: a profile whose
    /// `embeddings` flag is unset reports `Unsupported` rather than emulating.
    /// One request, no retry.
    pub fn embed(&self, model: &str, input: &str) -> Result<Vec<f32>, InferenceError> {
        if !self.capabilities.embeddings {
            return Err(InferenceError::Unsupported);
        }
        let path = match self.protocol {
            ProtocolFamily::OpenAiCompatible => "/v1/embeddings",
            ProtocolFamily::Native => "/api/embeddings",
        };
        let body = match self.protocol {
            ProtocolFamily::OpenAiCompatible => {
                serde_json::json!({ "model": model, "input": input }).to_string()
            }
            ProtocolFamily::Native => {
                serde_json::json!({ "model": model, "prompt": input }).to_string()
            }
        };
        let (_status, bytes) = self.http_call("POST", path, Some(&body))?;
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| InferenceError::MalformedStream(e.to_string()))?;
        // OpenAI: {data:[{embedding:[...]}]}; Ollama: {embedding:[...]}.
        let vec = json
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("embedding"))
            .or_else(|| json.get("embedding"))
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect::<Vec<f32>>()
            })
            .ok_or_else(|| {
                InferenceError::MalformedStream("no embedding array in response".to_string())
            })?;
        Ok(vec)
    }

    /// Describe `model`: surface whatever static facts the
    /// serving backend reports (name/digest/size/parameters), missing fields
    /// left `None` rather than fabricated. One request, no retry.
    pub fn describe(&self, model: &str) -> Result<ModelDescriptor, InferenceError> {
        let (path, method, body) = match self.protocol {
            ProtocolFamily::OpenAiCompatible => (format!("/v1/models/{model}"), "GET", None),
            ProtocolFamily::Native => (
                "/api/show".to_string(),
                "POST",
                Some(serde_json::json!({ "name": model }).to_string()),
            ),
        };
        let (_status, bytes) = self.http_call(method, &path, body.as_deref())?;
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| InferenceError::MalformedStream(e.to_string()))?;
        let digest = json
            .get("digest")
            .or_else(|| json.pointer("/details/digest"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let size_bytes = json.get("size").and_then(|v| v.as_u64());
        let parameters = json
            .pointer("/details/parameter_size")
            .or_else(|| json.get("parameter_size"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(ModelDescriptor {
            name: model.to_string(),
            digest,
            size_bytes,
            parameters,
        })
    }

    /// Set a residency hint for `model`. Capability-gated: a profile
    /// without `residency_control` reports `Unsupported` rather than
    /// pretending. When supported, maps to a native keep-alive request.
    pub fn set_residency(&self, model: &str, hint: ResidencyHint) -> Result<(), InferenceError> {
        if !self.capabilities.residency_control {
            return Err(InferenceError::Unsupported);
        }
        let keep_alive = match hint {
            ResidencyHint::KeepAliveSecs(s) => s as i64,
            ResidencyHint::UnloadNow => 0,
        };
        let body = serde_json::json!({ "model": model, "keep_alive": keep_alive }).to_string();
        self.http_call("POST", "/api/generate", Some(&body))?;
        Ok(())
    }

    /// Acquire `model` by name, progress-streamed. Capability-gated:
    /// a profile without `pull` yields a single `Error(Unsupported)`.
    pub fn pull(&self, model: &str) -> PullReceiver {
        if !self.capabilities.pull {
            let (tx, rx) = sync_channel(1);
            let _ = tx.send(PullProgress::Error(InferenceError::Unsupported));
            return PullReceiver {
                rx,
                terminated: false,
            };
        }
        let (tx, rx) = sync_channel(DEFAULT_CHANNEL_CAPACITY);
        let api_base = self.api_base.clone();
        let model = model.to_string();
        let auth_header = self.resolve_auth_header();
        thread::spawn(move || run_pull_worker(&api_base, &model, auth_header, &tx));
        PullReceiver {
            rx,
            terminated: false,
        }
    }

    /// A single non-streaming HTTP request/response over a raw `TcpStream`,
    /// with the full wire-failure taxonomy and **no internal retry**.
    /// Returns the status code and the full body bytes for 2xx; a non-2xx
    /// status maps to `ClientError`/`ServerError`.
    fn http_call(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, Vec<u8>), InferenceError> {
        let addr = resolve_one(&self.api_base).ok_or_else(|| {
            InferenceError::MalformedStream(format!("cannot resolve {}", self.api_base))
        })?;
        let mut reader =
            FrameReader::connect(addr, DEFAULT_PROBE_TIMEOUT).map_err(|e| map_connect_error(&e))?;
        let never = CancelHandle::new();
        let request = build_http_request(
            method,
            path,
            &host_of(&self.api_base).unwrap_or_default(),
            self.resolve_auth_header().as_deref(),
            body,
        );
        reader
            .stream
            .write_all(request.as_bytes())
            .map_err(|e| InferenceError::MalformedStream(e.to_string()))?;
        let status = read_http_status(&mut reader, &never)?;
        if !(200..300).contains(&status) {
            return Err(if (400..500).contains(&status) {
                InferenceError::ClientError(status)
            } else {
                InferenceError::ServerError(status)
            });
        }
        let bytes = reader.read_to_end(&never)?;
        Ok((status, bytes))
    }
}

/// A blocking pull-iterator over a model-acquisition call.
pub struct PullReceiver {
    rx: Receiver<PullProgress>,
    terminated: bool,
}

impl Iterator for PullReceiver {
    type Item = PullProgress;

    fn next(&mut self) -> Option<PullProgress> {
        if self.terminated {
            return None;
        }
        match self.rx.recv() {
            Ok(progress) => {
                if matches!(progress, PullProgress::Done { .. } | PullProgress::Error(_)) {
                    self.terminated = true;
                }
                Some(progress)
            }
            Err(_) => {
                self.terminated = true;
                Some(PullProgress::Error(InferenceError::MalformedStream(
                    "pull worker ended without a terminal event".to_string(),
                )))
            }
        }
    }
}

impl InferenceBackend for EndpointProfile {
    fn generate_stream(
        &self,
        request: GenerateRequest,
        cancel: CancelHandle,
    ) -> Box<dyn Iterator<Item = StreamEvent> + Send> {
        Box::new(EndpointProfile::generate_stream(self, &request, cancel))
    }

    fn embed(&self, model: &str, input: &str) -> Result<Vec<f32>, InferenceError> {
        EndpointProfile::embed(self, model, input)
    }

    fn describe(&self, model: &str) -> Result<ModelDescriptor, InferenceError> {
        EndpointProfile::describe(self, model)
    }

    fn pull(&self, model: &str) -> Box<dyn Iterator<Item = PullProgress> + Send> {
        Box::new(EndpointProfile::pull(self, model))
    }

    fn set_residency(&self, model: &str, hint: ResidencyHint) -> Result<(), InferenceError> {
        EndpointProfile::set_residency(self, model, hint)
    }
}

/// The stack's discipline applied to the streaming call: how often the
/// worker's blocking read wakes up to check `CancelHandle` — bounds
/// cancellation latency without busy-spinning.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Default bounded-channel capacity (backpressure): the worker blocks
/// on `send` once this many events are buffered and unconsumed, rather than
/// growing memory without limit for a slow consumer.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 32;

/// A blocking pull-iterator over a `generate_stream` call. Yields
/// events in order; the first `Done` or `Error` is terminal — no further
/// polling of the underlying channel occurs after it.
pub struct StreamReceiver {
    rx: Receiver<StreamEvent>,
    terminated: bool,
}

impl Iterator for StreamReceiver {
    type Item = StreamEvent;

    fn next(&mut self) -> Option<StreamEvent> {
        if self.terminated {
            return None;
        }
        match self.rx.recv() {
            Ok(event) => {
                if matches!(event, StreamEvent::Done | StreamEvent::Error(_)) {
                    self.terminated = true;
                }
                Some(event)
            }
            // The worker ended (e.g. panicked) without sending a terminal
            // event — surfaced honestly rather than silently ending the
            // stream as if it had completed normally.
            Err(_) => {
                self.terminated = true;
                Some(StreamEvent::Error(InferenceError::MalformedStream(
                    "worker thread ended without a terminal event".to_string(),
                )))
            }
        }
    }
}

/// A byte-buffered line reader over a `TcpStream` with a short read timeout
/// (`POLL_INTERVAL`), so a blocked read periodically returns control to the
/// caller to check `CancelHandle` — the cooperative-cancellation mechanism,
/// with no async runtime. A timeout is not an error here: it is retried,
/// with any partially-read bytes preserved across retries.
struct FrameReader {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl FrameReader {
    fn connect(addr: SocketAddr, connect_timeout: Duration) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, connect_timeout)?;
        stream.set_read_timeout(Some(POLL_INTERVAL))?;
        Ok(FrameReader {
            stream,
            buf: Vec::new(),
        })
    }

    /// Read one line (without its trailing `\r\n`/`\n`), or `Ok(None)` on a
    /// clean end-of-stream with no more buffered bytes.
    fn read_line(&mut self, cancel: &CancelHandle) -> Result<Option<String>, InferenceError> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                line.pop(); // trailing '\n'
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            if cancel.is_cancelled() {
                return Err(InferenceError::Cancelled);
            }
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    return if self.buf.is_empty() {
                        Ok(None)
                    } else {
                        let line = String::from_utf8_lossy(&self.buf).into_owned();
                        self.buf.clear();
                        Ok(Some(line))
                    };
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    continue; // poll again — re-checks `cancel` at the top
                }
                Err(e) => return Err(InferenceError::MalformedStream(e.to_string())),
            }
        }
    }

    /// Read everything remaining (buffered bytes + the rest of the stream)
    /// until a clean EOF — for a non-streaming response body. Same
    /// timeout-poll/cancel discipline as `read_line`.
    fn read_to_end(&mut self, cancel: &CancelHandle) -> Result<Vec<u8>, InferenceError> {
        let mut out = std::mem::take(&mut self.buf);
        loop {
            if cancel.is_cancelled() {
                return Err(InferenceError::Cancelled);
            }
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Ok(out),
                Ok(n) => out.extend_from_slice(&chunk[..n]),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => return Err(InferenceError::MalformedStream(e.to_string())),
            }
        }
    }
}

/// Read the HTTP status line and consume headers up to the blank-line
/// terminator, returning the status code. Leaves `reader` positioned at the
/// start of the response body (any bytes already read past the header
/// terminator stay buffered in `reader`, not discarded).
fn read_http_status(
    reader: &mut FrameReader,
    cancel: &CancelHandle,
) -> Result<u16, InferenceError> {
    let status_line = reader.read_line(cancel)?.ok_or_else(|| {
        InferenceError::MalformedStream("connection closed before a status line".to_string())
    })?;
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| {
            InferenceError::MalformedStream(format!("unparseable status line: {status_line}"))
        })?;
    loop {
        match reader.read_line(cancel)? {
            None => {
                return Err(InferenceError::MalformedStream(
                    "connection closed mid-headers".to_string(),
                ));
            }
            Some(line) if line.is_empty() => break,
            Some(_) => continue,
        }
    }
    Ok(code)
}

/// Read one SSE event — a run of `data:` lines terminated by a blank line —
/// as its concatenated payload text (multi-line `data:` events join with
/// `\n`, per the SSE spec). Non-`data:` lines are ignored; this transport
/// only needs the data payload, not SSE's full field set.
fn read_sse_event(
    reader: &mut FrameReader,
    cancel: &CancelHandle,
) -> Result<Option<String>, InferenceError> {
    let mut payload_lines: Vec<String> = Vec::new();
    loop {
        match reader.read_line(cancel)? {
            None => {
                return if payload_lines.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(payload_lines.join("\n")))
                };
            }
            Some(line) if line.is_empty() => {
                if !payload_lines.is_empty() {
                    return Ok(Some(payload_lines.join("\n")));
                }
                // A blank line before any `data:` line — ignore, keep reading.
            }
            Some(line) => {
                if let Some(data) = line.strip_prefix("data:") {
                    payload_lines.push(data.trim_start().to_string());
                }
                // Non-`data:` SSE fields/comments — ignore.
            }
        }
    }
}

/// Project one SSE payload onto a `StreamEvent`. `None` means the frame
/// carried no event worth surfacing (e.g. a role-only opener delta with no
/// text content) — honestly distinct from a malformed frame, which is a
/// `StreamEvent::Error`, not a silent skip disguised as one.
fn map_sse_payload(payload: &str) -> Option<StreamEvent> {
    if payload == "[DONE]" {
        return Some(StreamEvent::Done);
    }
    let json: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            return Some(StreamEvent::Error(InferenceError::MalformedStream(
                e.to_string(),
            )));
        }
    };
    let delta = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("delta"));
    if let Some(content) = delta
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
        && !content.is_empty()
    {
        return Some(StreamEvent::Token(content.to_string()));
    }
    None
}

fn map_connect_error(e: &io::Error) -> InferenceError {
    match e.kind() {
        io::ErrorKind::ConnectionRefused => InferenceError::ConnectRefused,
        io::ErrorKind::TimedOut => InferenceError::Timeout,
        _ => InferenceError::Timeout,
    }
}

fn build_request_body(request: &GenerateRequest) -> String {
    // `parameters` is not yet folded into the request body — how to map
    // opaque string key/value pairs onto a provider's typed JSON fields
    // (numeric vs. string coercion) is deferred to when a concrete `/v1`
    // request-shape mapping is needed; forwarding them incorrectly now
    // would be worse than not forwarding them yet.
    serde_json::json!({
        "model": request.model,
        "messages": [{"role": "user", "content": request.prompt}],
        "stream": true,
    })
    .to_string()
}

/// Build a raw HTTP/1.1 request string. `auth` is attached as an
/// `Authorization` header only when present (a loopback profile passes
/// `None`); the resolved secret lives only for this string's lifetime.
fn build_http_request(
    method: &str,
    path: &str,
    host: &str,
    auth: Option<&str>,
    body: Option<&str>,
) -> String {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(auth) = auth {
        req.push_str(&format!("Authorization: {auth}\r\n"));
    }
    match body {
        Some(body) => {
            req.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ));
        }
        None => req.push_str("\r\n"),
    }
    req
}

fn run_generate_worker(
    api_base: &str,
    protocol: ProtocolFamily,
    request: &GenerateRequest,
    auth_header: Option<String>,
    cancel: &CancelHandle,
    tx: &SyncSender<StreamEvent>,
) {
    let Some(addr) = resolve_one(api_base) else {
        let _ = tx.send(StreamEvent::Error(InferenceError::MalformedStream(
            format!("cannot resolve address: {api_base}"),
        )));
        return;
    };
    let mut reader = match FrameReader::connect(addr, DEFAULT_PROBE_TIMEOUT) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(StreamEvent::Error(map_connect_error(&e)));
            return;
        }
    };

    let path = match protocol {
        ProtocolFamily::OpenAiCompatible => "/v1/chat/completions",
        ProtocolFamily::Native => "/api/generate",
    };
    let host = host_of(api_base).unwrap_or_default();
    let body = build_request_body(request);
    let http_request = build_http_request("POST", path, &host, auth_header.as_deref(), Some(&body));
    if let Err(e) = reader.stream.write_all(http_request.as_bytes()) {
        let _ = tx.send(StreamEvent::Error(InferenceError::MalformedStream(
            e.to_string(),
        )));
        return;
    }

    let status = match read_http_status(&mut reader, cancel) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(StreamEvent::Error(e));
            return;
        }
    };
    if !(200..300).contains(&status) {
        let err = if (400..500).contains(&status) {
            InferenceError::ClientError(status)
        } else {
            InferenceError::ServerError(status)
        };
        let _ = tx.send(StreamEvent::Error(err));
        return;
    }

    loop {
        match read_sse_event(&mut reader, cancel) {
            Ok(Some(payload)) => match map_sse_payload(&payload) {
                Some(event) => {
                    let terminal = matches!(event, StreamEvent::Done | StreamEvent::Error(_));
                    if tx.send(event).is_err() {
                        return; // receiver dropped — caller gave up
                    }
                    if terminal {
                        return;
                    }
                }
                None => continue,
            },
            Ok(None) => {
                // Clean EOF with no explicit [DONE] sentinel — some
                // providers close the connection instead of sending one.
                let _ = tx.send(StreamEvent::Done);
                return;
            }
            Err(e) => {
                let _ = tx.send(StreamEvent::Error(e));
                return; // drops `reader` (and its TcpStream), closing the socket
            }
        }
    }
}

/// Native model acquisition (`/api/pull`), streamed as newline-delimited
/// JSON progress objects `{status, completed, total}` mapped onto
/// `PullProgress`. One request, no retry.
fn run_pull_worker(
    api_base: &str,
    model: &str,
    auth_header: Option<String>,
    tx: &SyncSender<PullProgress>,
) {
    let Some(addr) = resolve_one(api_base) else {
        let _ = tx.send(PullProgress::Error(InferenceError::MalformedStream(
            format!("cannot resolve address: {api_base}"),
        )));
        return;
    };
    let mut reader = match FrameReader::connect(addr, DEFAULT_PROBE_TIMEOUT) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(PullProgress::Error(map_connect_error(&e)));
            return;
        }
    };
    let host = host_of(api_base).unwrap_or_default();
    let body = serde_json::json!({ "name": model, "stream": true }).to_string();
    let http_request = build_http_request(
        "POST",
        "/api/pull",
        &host,
        auth_header.as_deref(),
        Some(&body),
    );
    if let Err(e) = reader.stream.write_all(http_request.as_bytes()) {
        let _ = tx.send(PullProgress::Error(InferenceError::MalformedStream(
            e.to_string(),
        )));
        return;
    }

    let never = CancelHandle::new();
    let status = match read_http_status(&mut reader, &never) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(PullProgress::Error(e));
            return;
        }
    };
    if !(200..300).contains(&status) {
        let _ = tx.send(PullProgress::Error(if (400..500).contains(&status) {
            InferenceError::ClientError(status)
        } else {
            InferenceError::ServerError(status)
        }));
        return;
    }

    loop {
        match reader.read_line(&never) {
            Ok(Some(line)) if line.trim().is_empty() => continue,
            Ok(Some(line)) => {
                let json: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(PullProgress::Error(InferenceError::MalformedStream(
                            e.to_string(),
                        )));
                        return;
                    }
                };
                let status_field = json.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status_field.eq_ignore_ascii_case("success") {
                    let digest = json
                        .get("digest")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let _ = tx.send(PullProgress::Done { digest });
                    return;
                }
                let progress = PullProgress::Downloading {
                    bytes_done: json.get("completed").and_then(|v| v.as_u64()).unwrap_or(0),
                    bytes_total: json.get("total").and_then(|v| v.as_u64()),
                };
                if tx.send(progress).is_err() {
                    return;
                }
            }
            Ok(None) => {
                // Clean EOF without an explicit success line — treat as done.
                let _ = tx.send(PullProgress::Done { digest: None });
                return;
            }
            Err(e) => {
                let _ = tx.send(PullProgress::Error(e));
                return;
            }
        }
    }
}

/// Strip an optional `scheme://` prefix and any path/query suffix, leaving
/// just the `host[:port]` (or bracketed-IPv6 `[host]:port`) portion.
fn strip_scheme_and_path(api_base: &str) -> &str {
    let without_scheme = api_base
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(api_base);
    without_scheme.split('/').next().unwrap_or(without_scheme)
}

/// Extract just the host (no port, no brackets) from an `api_base` string.
fn host_of(api_base: &str) -> Option<String> {
    let host_port = strip_scheme_and_path(api_base);
    if let Some(rest) = host_port.strip_prefix('[') {
        // Bracketed IPv6: "[::1]:8080" or "[::1]".
        let host = rest.split(']').next()?;
        return if host.is_empty() {
            None
        } else {
            Some(host.to_string())
        };
    }
    // "host:port" or bare "host" — a literal IPv4/hostname has at most one
    // colon (the port separator), so splitting on the last colon is safe.
    let host = host_port
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(host_port);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Resolve `api_base` to one connectable `SocketAddr`.
fn resolve_one(api_base: &str) -> Option<SocketAddr> {
    let host_port = strip_scheme_and_path(api_base);
    host_port.to_socket_addrs().ok()?.next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn caps() -> Capabilities {
        Capabilities {
            streaming: true,
            ..Default::default()
        }
    }

    fn req() -> GenerateRequest {
        GenerateRequest {
            model: "test-model".to_string(),
            prompt: "hi".to_string(),
            parameters: vec![],
        }
    }

    /// A minimal, hand-rolled HTTP/SSE mock server for hermetic streaming
    /// tests — never talks to a real network peer, no framework, just
    /// enough wire protocol to drive this crate's client logic. Shared
    /// fixture, reused by later tests.
    struct MockSseServer {
        listener: TcpListener,
    }

    impl MockSseServer {
        fn bind() -> Self {
            MockSseServer {
                listener: TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port"),
            }
        }

        fn api_base(&self) -> String {
            format!(
                "http://127.0.0.1:{}",
                self.listener.local_addr().expect("local addr").port()
            )
        }

        /// Accept exactly one connection on its own thread and hand it to
        /// `script` to drive (write the response, optionally probe for
        /// connection close, etc).
        fn accept_and_run(
            self,
            script: impl FnOnce(TcpStream) + Send + 'static,
        ) -> thread::JoinHandle<()> {
            thread::spawn(move || {
                let (stream, _) = self.listener.accept().expect("accept a connection");
                script(stream);
            })
        }
    }

    /// Read and discard an incoming HTTP request up to its header
    /// terminator — this fixture doesn't need to inspect request content.
    fn drain_request(stream: &mut TcpStream) {
        let mut buf = [0u8; 4096];
        let mut acc: Vec<u8> = Vec::new();
        loop {
            if acc.windows(4).any(|w| w == b"\r\n\r\n") {
                return;
            }
            let n = stream.read(&mut buf).expect("read request");
            if n == 0 {
                return;
            }
            acc.extend_from_slice(&buf[..n]);
        }
    }

    fn write_sse_preamble(stream: &mut TcpStream) {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n")
            .expect("write preamble");
    }

    fn write_sse_token(stream: &mut TcpStream, content: &str) {
        let payload = serde_json::json!({"choices":[{"delta":{"content": content}}]});
        write!(stream, "data: {payload}\n\n").expect("write token");
        stream.flush().expect("flush");
    }

    fn write_sse_done(stream: &mut TcpStream) {
        stream.write_all(b"data: [DONE]\n\n").expect("write done");
        let _ = stream.flush();
    }

    /// Read the incoming request's header block (up to `\r\n\r\n`) and
    /// return it as a string so a test can assert on headers.
    fn read_request_headers(stream: &mut TcpStream) -> String {
        let mut acc: Vec<u8> = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            if let Some(pos) = acc.windows(4).position(|w| w == b"\r\n\r\n") {
                return String::from_utf8_lossy(&acc[..pos]).into_owned();
            }
            let n = stream.read(&mut buf).expect("read request");
            if n == 0 {
                return String::from_utf8_lossy(&acc).into_owned();
            }
            acc.extend_from_slice(&buf[..n]);
        }
    }

    fn write_json_response(stream: &mut TcpStream, status: u16, json: &serde_json::Value) {
        let body = json.to_string();
        let reason = if status == 200 { "OK" } else { "ERR" };
        write!(
            stream,
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write json response");
        let _ = stream.flush();
    }

    fn write_status_only(stream: &mut TcpStream, status: u16) {
        write!(
            stream,
            "HTTP/1.1 {status} ERR\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .expect("write status");
        let _ = stream.flush();
    }

    /// A loopback server that accepts connections in a loop and counts them,
    /// so a test can assert "exactly one request was made" (no retry). Runs
    /// each accepted connection through `script`. Stops on `Drop`.
    struct CountingServer {
        api_base: String,
        count: Arc<std::sync::atomic::AtomicUsize>,
        stop: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl CountingServer {
        fn start(script: impl Fn(TcpStream) + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
            let api_base = format!(
                "http://127.0.0.1:{}",
                listener.local_addr().expect("local addr").port()
            );
            listener
                .set_nonblocking(true)
                .expect("set listener nonblocking");
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let count_t = count.clone();
            let stop_t = stop.clone();
            let handle = thread::spawn(move || {
                while !stop_t.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            count_t.fetch_add(1, Ordering::SeqCst);
                            script(stream);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => return,
                    }
                }
            });
            CountingServer {
                api_base,
                count,
                stop,
                handle: Some(handle),
            }
        }

        fn api_base(&self) -> &str {
            &self.api_base
        }

        fn connection_count(&self) -> usize {
            self.count.load(Ordering::SeqCst)
        }
    }

    impl Drop for CountingServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    fn embeddable_caps() -> Capabilities {
        Capabilities {
            streaming: true,
            embeddings: true,
            ..Default::default()
        }
    }

    #[test]
    fn accepts_the_real_default_localhost_endpoints() {
        // The stack catalog's actual defaults use the literal
        // hostname "localhost", not a numeric loopback IP.
        for api_base in [
            "http://localhost:11434", // Ollama
            "http://localhost:8080",  // llama.cpp / MLX
            "http://localhost:8000",  // vLLM
            "http://localhost:1234",  // LM Studio
            "http://localhost:12434", // Docker Model Runner
        ] {
            let profile = EndpointProfile::new(api_base, ProtocolFamily::OpenAiCompatible, caps())
                .unwrap_or_else(|e| panic!("{api_base} should be accepted, got {e:?}"));
            assert_eq!(profile.api_base(), api_base);
        }
    }

    #[test]
    fn accepts_numeric_loopback_hosts() {
        assert!(
            EndpointProfile::new("http://127.0.0.1:8080", ProtocolFamily::Native, caps()).is_ok()
        );
        assert!(EndpointProfile::new("http://[::1]:8080", ProtocolFamily::Native, caps()).is_ok());
    }

    #[test]
    fn rejects_wildcard_bind_addresses() {
        assert!(matches!(
            EndpointProfile::new("http://0.0.0.0:8080", ProtocolFamily::Native, caps()),
            Err(ProfileError::WildcardAddress(s)) if s == "http://0.0.0.0:8080"
        ));
        assert!(matches!(
            EndpointProfile::new("http://[::]:8080", ProtocolFamily::Native, caps()),
            Err(ProfileError::WildcardAddress(s)) if s == "http://[::]:8080"
        ));
    }

    #[test]
    fn rejects_non_loopback_hosts_by_default() {
        assert!(matches!(
            EndpointProfile::new("http://192.168.1.5:8080", ProtocolFamily::Native, caps()),
            Err(ProfileError::NotLoopback(s)) if s == "http://192.168.1.5:8080"
        ));
        assert!(matches!(
            EndpointProfile::new("http://example.com:8080", ProtocolFamily::Native, caps()),
            Err(ProfileError::NotLoopback(s)) if s == "http://example.com:8080"
        ));
    }

    #[test]
    fn probe_classifies_reachable_against_a_real_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        let profile = EndpointProfile::new(
            format!("http://127.0.0.1:{port}"),
            ProtocolFamily::Native,
            caps(),
        )
        .expect("loopback profile");

        assert_eq!(profile.probe(), ProbeOutcome::Reachable);
    }

    #[test]
    fn probe_classifies_a_closed_port_as_definitively_unreachable() {
        // Bind to let the OS assign a free port, then drop the listener —
        // the port is now provably closed on this host.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);

        let profile = EndpointProfile::new(
            format!("http://127.0.0.1:{port}"),
            ProtocolFamily::Native,
            caps(),
        )
        .expect("loopback profile");

        // A closed port normally resets the connection immediately
        // (`ConnectionRefused`). Some sandboxed/virtualized network layers
        // (confirmed empirically for this execution environment) instead
        // suppress the RST and let the connect attempt run out the clock,
        // which this probe honestly reports as `Timeout` rather than
        // fabricating a refusal it never observed (see the `Err(_) =>
        // Timeout` fallback above) — both outcomes agree the endpoint is
        // not serving, which is what this test asserts.
        let outcome = profile.probe_with_timeout(Duration::from_millis(300));
        assert!(
            matches!(
                outcome,
                ProbeOutcome::ConnectionRefused | ProbeOutcome::Timeout
            ),
            "a closed port must classify as refused or (honestly) timed-out, got {outcome:?}"
        );
    }

    #[test]
    fn probe_classifies_timeout_within_the_given_budget() {
        // RFC 5737 TEST-NET-1: reserved for documentation, never routed —
        // a deterministic, hermetic non-responder. Uses a short timeout
        // (not the 800ms default) to keep the test fast.
        let profile = EndpointProfile {
            api_base: "192.0.2.1:80".to_string(),
            protocol: ProtocolFamily::Native,
            capabilities: caps(),
            remote_auth: None,
        };

        let start = std::time::Instant::now();
        let outcome = profile.probe_with_timeout(Duration::from_millis(300));
        let elapsed = start.elapsed();

        assert_eq!(outcome, ProbeOutcome::Timeout);
        assert!(
            elapsed < Duration::from_millis(800),
            "probe must honor the given budget, took {elapsed:?}"
        );
    }

    #[test]
    fn generate_stream_yields_ordered_tokens_then_done() {
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let handle = server.accept_and_run(|mut stream| {
            drain_request(&mut stream);
            write_sse_preamble(&mut stream);
            write_sse_token(&mut stream, "Hello");
            write_sse_token(&mut stream, ", world");
            write_sse_done(&mut stream);
        });

        let profile = EndpointProfile::new(api_base, ProtocolFamily::OpenAiCompatible, caps())
            .expect("loopback profile");
        let cancel = CancelHandle::new();

        let events: Vec<StreamEvent> = profile.generate_stream(&req(), cancel).collect();

        handle.join().expect("server thread must not panic");
        assert_eq!(
            events,
            vec![
                StreamEvent::Token("Hello".to_string()),
                StreamEvent::Token(", world".to_string()),
                StreamEvent::Done,
            ]
        );
    }

    #[test]
    fn generate_stream_treats_clean_eof_without_done_sentinel_as_done() {
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let handle = server.accept_and_run(|mut stream| {
            drain_request(&mut stream);
            write_sse_preamble(&mut stream);
            write_sse_token(&mut stream, "Hello");
            // No `[DONE]` sentinel — the server just closes, as some real
            // providers do; the worker must treat a clean EOF as `Done`.
        });

        let profile = EndpointProfile::new(api_base, ProtocolFamily::OpenAiCompatible, caps())
            .expect("loopback profile");
        let cancel = CancelHandle::new();

        let events: Vec<StreamEvent> = profile.generate_stream(&req(), cancel).collect();

        handle.join().expect("server thread must not panic");
        assert_eq!(
            events,
            vec![StreamEvent::Token("Hello".to_string()), StreamEvent::Done]
        );
    }

    #[test]
    fn cancel_mid_stream_yields_error_and_server_observes_socket_close() {
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let server_saw_close = Arc::new(AtomicBool::new(false));
        let server_saw_close_thread = server_saw_close.clone();

        let handle = server.accept_and_run(move |mut stream| {
            drain_request(&mut stream);
            write_sse_preamble(&mut stream);
            write_sse_token(&mut stream, "Hello");
            // Wait for the client to cancel and close its half of the
            // connection; detect it via a bounded read (never hangs the
            // test even if cancellation somehow failed to propagate).
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut buf = [0u8; 16];
            match stream.read(&mut buf) {
                Ok(0) => server_saw_close_thread.store(true, Ordering::SeqCst),
                // A genuine close-related error (reset/aborted) counts; a
                // mere read timeout does NOT — that would mean the client
                // never actually closed the connection within the 2s
                // budget, which must fail the assertion below, not
                // masquerade as a detected close.
                Err(e)
                    if e.kind() != io::ErrorKind::WouldBlock
                        && e.kind() != io::ErrorKind::TimedOut =>
                {
                    server_saw_close_thread.store(true, Ordering::SeqCst)
                }
                _ => {}
            }
        });

        let profile = EndpointProfile::new(api_base, ProtocolFamily::OpenAiCompatible, caps())
            .expect("loopback profile");
        let cancel = CancelHandle::new();
        let mut events = profile.generate_stream(&req(), cancel.clone());

        assert_eq!(events.next(), Some(StreamEvent::Token("Hello".to_string())));
        cancel.cancel();
        assert_eq!(
            events.next(),
            Some(StreamEvent::Error(InferenceError::Cancelled))
        );
        assert_eq!(
            events.next(),
            None,
            "no events after the terminal Cancelled"
        );

        handle.join().expect("server thread must not panic");
        assert!(
            server_saw_close.load(Ordering::SeqCst),
            "server must observe the client closing the connection after cancel"
        );
    }

    #[test]
    fn generate_stream_channel_is_bounded_not_unbounded_buffering() {
        const CAPACITY: usize = 4;
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let handle = server.accept_and_run(|mut stream| {
            drain_request(&mut stream);
            write_sse_preamble(&mut stream);
            for i in 0..50 {
                write_sse_token(&mut stream, &format!("t{i}"));
            }
            write_sse_done(&mut stream);
        });

        let profile = EndpointProfile::new(api_base, ProtocolFamily::OpenAiCompatible, caps())
            .expect("loopback profile");
        let cancel = CancelHandle::new();
        let receiver = profile.generate_stream_with_capacity(&req(), cancel, CAPACITY);

        // Deliberately don't drain — give the worker time to race ahead if
        // it were (wrongly) buffering unbounded events instead of blocking
        // on a full bounded channel.
        thread::sleep(Duration::from_millis(200));

        let mut buffered = 0usize;
        while receiver.rx.try_recv().is_ok() {
            buffered += 1;
        }

        assert!(
            buffered <= CAPACITY + 1,
            "worker must block once the bounded channel is full, not buffer all 50 events; observed {buffered}"
        );

        // Drain the rest so the worker (and the server, if its own OS
        // socket send buffer were ever small enough to fill) can finish —
        // keeps the test robust regardless of the platform's actual buffer
        // size, without weakening the backpressure assertion taken above.
        for _ in receiver {}

        handle.join().expect("server thread must not panic");
    }

    // ── embed / describe / pull / residency + failure map + egress ──

    #[test]
    fn embed_is_capability_gated_off_reports_unsupported() {
        // Default caps have embeddings=false — no network call is even made.
        let profile = EndpointProfile::new(
            "http://127.0.0.1:1",
            ProtocolFamily::OpenAiCompatible,
            caps(),
        )
        .expect("loopback profile");
        assert_eq!(profile.embed("m", "hi"), Err(InferenceError::Unsupported));
    }

    #[test]
    fn embed_parses_openai_shaped_embedding() {
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let handle = server.accept_and_run(|mut stream| {
            drain_request(&mut stream);
            write_json_response(
                &mut stream,
                200,
                &serde_json::json!({"data":[{"embedding":[0.1, 0.2, 0.3]}]}),
            );
        });

        let profile = EndpointProfile::new(
            api_base,
            ProtocolFamily::OpenAiCompatible,
            embeddable_caps(),
        )
        .expect("loopback profile");
        let vec = profile.embed("m", "hi").expect("embedding");

        handle.join().expect("server thread must not panic");
        assert_eq!(vec, vec![0.1_f32, 0.2, 0.3]);
    }

    #[test]
    fn describe_surfaces_present_fields_and_leaves_missing_ones_none() {
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let handle = server.accept_and_run(|mut stream| {
            drain_request(&mut stream);
            write_json_response(
                &mut stream,
                200,
                &serde_json::json!({"digest":"sha256:abc","size":42}),
            );
        });

        let profile = EndpointProfile::new(api_base, ProtocolFamily::Native, caps())
            .expect("loopback profile");
        let d = profile.describe("llama3").expect("descriptor");

        handle.join().expect("server thread must not panic");
        assert_eq!(d.name, "llama3");
        assert_eq!(d.digest.as_deref(), Some("sha256:abc"));
        assert_eq!(d.size_bytes, Some(42));
        assert_eq!(d.parameters, None); // absent field stays None, not fabricated
    }

    #[test]
    fn wire_failures_map_onto_the_taxonomy() {
        // 404 → ClientError.
        {
            let server = MockSseServer::bind();
            let api_base = server.api_base();
            let handle = server.accept_and_run(|mut stream| {
                drain_request(&mut stream);
                write_status_only(&mut stream, 404);
            });
            let profile = EndpointProfile::new(api_base, ProtocolFamily::Native, caps())
                .expect("loopback profile");
            assert_eq!(profile.describe("m"), Err(InferenceError::ClientError(404)));
            handle.join().expect("server thread must not panic");
        }
        // 503 → ServerError.
        {
            let server = MockSseServer::bind();
            let api_base = server.api_base();
            let handle = server.accept_and_run(|mut stream| {
                drain_request(&mut stream);
                write_status_only(&mut stream, 503);
            });
            let profile = EndpointProfile::new(api_base, ProtocolFamily::Native, caps())
                .expect("loopback profile");
            assert_eq!(profile.describe("m"), Err(InferenceError::ServerError(503)));
            handle.join().expect("server thread must not panic");
        }
        // Connection refused → ConnectRefused (bind then drop → closed port).
        {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            let profile = EndpointProfile::new(
                format!("http://127.0.0.1:{port}"),
                ProtocolFamily::Native,
                caps(),
            )
            .expect("loopback profile");
            // This sandbox may time out instead of refusing (see the probe
            // test's note) — both are honest "could not reach" outcomes.
            assert!(matches!(
                profile.describe("m"),
                Err(InferenceError::ConnectRefused) | Err(InferenceError::Timeout)
            ));
        }
        // Malformed JSON body on a 200 → MalformedStream.
        {
            let server = MockSseServer::bind();
            let api_base = server.api_base();
            let handle = server.accept_and_run(|mut stream| {
                drain_request(&mut stream);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n{{ ["
                )
                .unwrap();
                let _ = stream.flush();
            });
            let profile = EndpointProfile::new(api_base, ProtocolFamily::Native, embeddable_caps())
                .expect("loopback profile");
            assert!(matches!(
                profile.embed("m", "hi"),
                Err(InferenceError::MalformedStream(_))
            ));
            handle.join().expect("server thread must not panic");
        }
    }

    #[test]
    fn embed_makes_exactly_one_request_no_retry() {
        // Every request gets a 500 — a retrying client would reconnect and
        // the counter would climb past 1.
        let server = CountingServer::start(|mut stream| {
            drain_request(&mut stream);
            write_status_only(&mut stream, 500);
        });
        let profile =
            EndpointProfile::new(server.api_base(), ProtocolFamily::Native, embeddable_caps())
                .expect("loopback profile");

        assert_eq!(
            profile.embed("m", "hi"),
            Err(InferenceError::ServerError(500))
        );
        // Give any (wrongly) spawned retry time to connect before counting.
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            server.connection_count(),
            1,
            "exactly one request per call — no internal retry"
        );
    }

    #[test]
    fn pull_is_capability_gated_off_yields_single_unsupported() {
        let profile = EndpointProfile::new("http://127.0.0.1:1", ProtocolFamily::Native, caps())
            .expect("loopback profile");
        let events: Vec<PullProgress> = profile.pull("m").collect();
        assert_eq!(
            events,
            vec![PullProgress::Error(InferenceError::Unsupported)]
        );
    }

    #[test]
    fn set_residency_is_capability_gated_off_reports_unsupported() {
        let profile = EndpointProfile::new("http://127.0.0.1:1", ProtocolFamily::Native, caps())
            .expect("loopback profile");
        assert_eq!(
            profile.set_residency("m", ResidencyHint::UnloadNow),
            Err(InferenceError::Unsupported)
        );
    }

    #[test]
    fn remote_profile_requires_a_matching_egress_grant() {
        // The loopback constructor refuses a non-loopback host outright.
        assert!(matches!(
            EndpointProfile::new(
                "http://10.0.0.5:8080",
                ProtocolFamily::OpenAiCompatible,
                caps()
            ),
            Err(ProfileError::NotLoopback(_))
        ));

        struct FixedCred(&'static str);
        impl CredentialResolver for FixedCred {
            fn resolve(&self, _endpoint: &str) -> Option<String> {
                Some(self.0.to_string())
            }
        }

        // A grant scoped to a DIFFERENT endpoint is rejected.
        let grant = EgressGrant::for_endpoint("http://10.0.0.5:8080");
        assert!(matches!(
            EndpointProfile::new_remote(
                "http://10.0.0.9:8080",
                ProtocolFamily::OpenAiCompatible,
                caps(),
                &grant,
                Arc::new(FixedCred("k")),
            ),
            Err(ProfileError::GrantEndpointMismatch { .. })
        ));

        // A grant scoped to the exact endpoint succeeds.
        assert!(
            EndpointProfile::new_remote(
                "http://10.0.0.5:8080",
                ProtocolFamily::OpenAiCompatible,
                caps(),
                &grant,
                Arc::new(FixedCred("k")),
            )
            .is_ok()
        );
    }

    #[test]
    fn remote_credential_resolved_per_call_and_attached_never_cached() {
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let seen_auth = Arc::new(std::sync::Mutex::new(String::new()));
        let seen_auth_t = seen_auth.clone();
        let handle = server.accept_and_run(move |mut stream| {
            let headers = read_request_headers(&mut stream);
            *seen_auth_t.lock().unwrap() = headers;
            write_json_response(
                &mut stream,
                200,
                &serde_json::json!({"data":[{"embedding":[1.0]}]}),
            );
        });

        // Count how often the resolver is consulted — proves per-call
        // resolution (not caching in the profile).
        struct CountingCred(Arc<std::sync::atomic::AtomicUsize>);
        impl CredentialResolver for CountingCred {
            fn resolve(&self, _endpoint: &str) -> Option<String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Some("secret-token".to_string())
            }
        }
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let grant = EgressGrant::for_endpoint(&api_base);
        let profile = EndpointProfile::new_remote(
            &api_base,
            ProtocolFamily::OpenAiCompatible,
            embeddable_caps(),
            &grant,
            Arc::new(CountingCred(calls.clone())),
        )
        .expect("remote profile with matching grant");

        let _ = profile.embed("m", "hi").expect("embedding");
        handle.join().expect("server thread must not panic");

        assert!(
            seen_auth
                .lock()
                .unwrap()
                .contains("Authorization: Bearer secret-token"),
            "the resolved credential must be attached as a bearer token"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "credential resolved fresh for this one call (not cached, not skipped)"
        );
    }

    #[test]
    fn inference_backend_trait_object_streams_through_the_real_path() {
        let server = MockSseServer::bind();
        let api_base = server.api_base();
        let handle = server.accept_and_run(|mut stream| {
            drain_request(&mut stream);
            write_sse_preamble(&mut stream);
            write_sse_token(&mut stream, "hi");
            write_sse_done(&mut stream);
        });

        let profile = EndpointProfile::new(api_base, ProtocolFamily::OpenAiCompatible, caps())
            .expect("loopback profile");
        // Exercise via the trait object — proves the InferenceBackend
        // impl wires to the same real transport as the inherent method.
        let backend: &dyn InferenceBackend = &profile;
        let events: Vec<StreamEvent> = backend
            .generate_stream(req(), CancelHandle::new())
            .collect();

        handle.join().expect("server thread must not panic");
        assert_eq!(
            events,
            vec![StreamEvent::Token("hi".to_string()), StreamEvent::Done]
        );
    }
}
