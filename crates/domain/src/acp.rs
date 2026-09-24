//! Agent Client Protocol (ACP) server semantics.
//!
//! Session lifecycle store, the per-session monotonic event bus, capability
//! declaration + trust gating, pure protocol-projection adapters, the live-steering
//! queue, and the interrupt fence. The Streamable HTTP transport lives in the
//! agent-session daemon; this module owns the protocol semantics beneath it.
//!
//! In-memory here (SQLite-backed in production); the transport and the turn loop
//! that drives emission/steering polling are the documented seams.

use std::collections::{HashMap, VecDeque};

/// The effective trust level of a caller, resolved once at authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Authenticated local user / admin token — full access.
    Trusted,
    /// Authenticated external caller — declared subset, read-only budget.
    Restricted,
    /// Unauthenticated — only `capabilities` is served.
    Anonymous,
}

/// A streamed protocol event. Carries a per-session monotonic `seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpEvent {
    pub seq: u64,
    pub kind: EventKind,
}

/// The kind of a streamed event. Terminal kinds end a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Thinking,
    ToolCall,
    ClientToolRequest,
    TextDelta,
    /// Turn complete; carries the remaining budget.
    Done {
        remaining_budget: u64,
    },
    Error,
    /// Budget consumed; office entering hibernation.
    BudgetExhausted,
    /// Turn interrupted by client signal; session resumable.
    Interrupted {
        remaining_budget: u64,
    },
    /// Client-injected redirect; the turn continues.
    Steering,
    /// A not-yet-started planned action cancelled by steering.
    ActionSkipped {
        action: String,
    },
}

impl EventKind {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            EventKind::Done { .. }
                | EventKind::Error
                | EventKind::BudgetExhausted
                | EventKind::Interrupted { .. }
        )
    }
}

/// The machine-readable capability declaration served before any task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub office_id: String,
    pub version: String,
    pub trust_level: TrustLevel,
    pub budget_remaining: u64,
    pub tool_delegation: bool,
    pub streaming: bool,
}

/// Errors from ACP operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpError {
    /// No session with that id.
    UnknownSession,
    /// Anonymous callers may only query capabilities.
    AuthRequired,
    /// Tool delegation attempted without opt-in.
    DelegationNotOptedIn,
    /// The bounded steering queue is full; the steer is rejected visibly.
    SteerRejected,
}

impl std::fmt::Display for AcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AcpError::UnknownSession => "unknown session",
            AcpError::AuthRequired => "authentication required",
            AcpError::DelegationNotOptedIn => "tool delegation not opted in",
            AcpError::SteerRejected => "steering queue full",
        };
        f.write_str(s)
    }
}

impl std::error::Error for AcpError {}

/// The bound on a session's steering queue; overflow rejects visibly.
const STEER_QUEUE_MAX: usize = 16;

#[derive(Debug)]
struct SessionRuntime {
    office_id: String,
    trust: TrustLevel,
    budget_remaining: u64,
    tool_delegation_opt_in: bool,
    /// Monotonic sequence high-water mark.
    next_seq: u64,
    events: Vec<AcpEvent>,
    steer_queue: VecDeque<String>,
    /// Interrupt fence — set by `interrupt`, drained at the next safe boundary.
    interrupt_requested: bool,
}

/// The ACP server: session store + per-session event bus, capability/trust gate,
/// steering queue, and interrupt fence. Sole owner of session state.
#[derive(Debug, Default)]
pub struct AcpServer {
    version: String,
    sessions: HashMap<String, SessionRuntime>,
}

impl AcpServer {
    pub fn new(version: &str) -> Self {
        AcpServer {
            version: version.to_string(),
            sessions: HashMap::new(),
        }
    }

    /// Create a session. Idempotent: creating with an existing id
    /// returns without disturbing the existing session's state. Returns `true` if a
    /// new session was created, `false` if it already existed.
    pub fn create_session(
        &mut self,
        session_id: &str,
        office_id: &str,
        trust: TrustLevel,
        tool_delegation_opt_in: bool,
    ) -> bool {
        if self.sessions.contains_key(session_id) {
            return false;
        }
        self.sessions.insert(
            session_id.to_string(),
            SessionRuntime {
                office_id: office_id.to_string(),
                trust,
                budget_remaining: 0,
                tool_delegation_opt_in,
                next_seq: 0,
                events: Vec::new(),
                steer_queue: VecDeque::new(),
                interrupt_requested: false,
            },
        );
        true
    }

    pub fn set_budget(&mut self, session_id: &str, budget: u64) -> Result<(), AcpError> {
        let s = self.session_mut(session_id)?;
        s.budget_remaining = budget;
        Ok(())
    }

    fn session(&self, id: &str) -> Result<&SessionRuntime, AcpError> {
        self.sessions.get(id).ok_or(AcpError::UnknownSession)
    }

    fn session_mut(&mut self, id: &str) -> Result<&mut SessionRuntime, AcpError> {
        self.sessions.get_mut(id).ok_or(AcpError::UnknownSession)
    }

    /// Serve the capability declaration. Available to any trust level,
    /// including anonymous.
    pub fn capabilities(&self, session_id: &str) -> Result<Capabilities, AcpError> {
        let s = self.session(session_id)?;
        Ok(Capabilities {
            office_id: s.office_id.clone(),
            version: self.version.clone(),
            trust_level: s.trust,
            budget_remaining: s.budget_remaining,
            tool_delegation: s.tool_delegation_opt_in,
            streaming: true,
        })
    }

    /// Emit an event onto the session bus, assigning the next `seq`.
    /// Anonymous callers may not drive turns (trust gate).
    pub fn emit(&mut self, session_id: &str, kind: EventKind) -> Result<u64, AcpError> {
        // Resolve trust before borrowing mutably to satisfy the gate up front.
        if self.session(session_id)?.trust == TrustLevel::Anonymous {
            return Err(AcpError::AuthRequired);
        }
        let s = self.session_mut(session_id)?;
        let seq = s.next_seq;
        s.next_seq += 1;
        s.events.push(AcpEvent { seq, kind });
        Ok(seq)
    }

    /// Emit a delegated client-tool request. Fails closed unless the
    /// session opted into tool delegation at creation.
    pub fn request_client_tool(&mut self, session_id: &str) -> Result<u64, AcpError> {
        if !self.session(session_id)?.tool_delegation_opt_in {
            return Err(AcpError::DelegationNotOptedIn);
        }
        self.emit(session_id, EventKind::ClientToolRequest)
    }

    /// The ordered event log for a session (a transport replays from `since_seq`).
    pub fn events(&self, session_id: &str) -> Result<&[AcpEvent], AcpError> {
        Ok(&self.session(session_id)?.events)
    }

    /// Whether the ordered event log has a sequence gap — dropped events.
    /// A gap must be surfaced, never silently ignored.
    pub fn has_gap(&self, session_id: &str) -> Result<bool, AcpError> {
        let events = &self.session(session_id)?.events;
        Ok(events.iter().enumerate().any(|(i, e)| e.seq != i as u64))
    }

    /// Request an interrupt. The turn loop drains the fence at its next
    /// safe boundary, finishing the current atomic step, then emits a partial
    /// terminal. Modeled here as setting the fence.
    pub fn interrupt(&mut self, session_id: &str) -> Result<(), AcpError> {
        self.session_mut(session_id)?.interrupt_requested = true;
        Ok(())
    }

    pub fn interrupt_requested(&self, session_id: &str) -> Result<bool, AcpError> {
        Ok(self.session(session_id)?.interrupt_requested)
    }

    /// Enqueue a steering message. Session-scoped: the message resolves to
    /// exactly this session's queue and can never land in another. Bounded — an
    /// overflow rejects the steer visibly rather than dropping it silently.
    pub fn steer(&mut self, session_id: &str, message: &str) -> Result<(), AcpError> {
        let s = self.session_mut(session_id)?;
        if s.steer_queue.len() >= STEER_QUEUE_MAX {
            return Err(AcpError::SteerRejected);
        }
        s.steer_queue.push_back(message.to_string());
        Ok(())
    }

    /// Poll one steering message at a safe boundary (turn-loop poll point).
    pub fn poll_steer(&mut self, session_id: &str) -> Result<Option<String>, AcpError> {
        Ok(self.session_mut(session_id)?.steer_queue.pop_front())
    }

    /// Absorb a steering message mid-turn: cancel each not-yet-started
    /// planned action as an `ActionSkipped` event (never silently completed, never
    /// silently dropped), then emit `Steering`. The turn continues, redirected.
    pub fn apply_steer(
        &mut self,
        session_id: &str,
        pending_actions: &[String],
    ) -> Result<Vec<u64>, AcpError> {
        let mut seqs = Vec::new();
        for action in pending_actions {
            seqs.push(self.emit(
                session_id,
                EventKind::ActionSkipped {
                    action: action.clone(),
                },
            )?);
        }
        seqs.push(self.emit(session_id, EventKind::Steering)?);
        Ok(seqs)
    }
}

/// A pure, logic-free adapter over the one ordered event stream. It
/// re-frames each event into a foreign wire shape and adds/drops/reorders nothing.
pub trait ProjectionAdapter {
    fn translate(&self, event: &AcpEvent) -> String;
}

/// A peer-agent projection (illustrative pure adapter).
pub struct PeerAgentAdapter;
impl ProjectionAdapter for PeerAgentAdapter {
    fn translate(&self, event: &AcpEvent) -> String {
        format!("peer:{}:{:?}", event.seq, event.kind)
    }
}

/// A UI-event projection (illustrative pure adapter).
pub struct UiEventAdapter;
impl ProjectionAdapter for UiEventAdapter {
    fn translate(&self, event: &AcpEvent) -> String {
        format!("ui#{}", event.seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_with_session() -> AcpServer {
        let mut srv = AcpServer::new("0.1.0");
        srv.create_session("s1", "office-1", TrustLevel::Trusted, false);
        srv.set_budget("s1", 100).unwrap();
        srv
    }

    #[test]
    fn create_session_is_idempotent() {
        // A repeat create returns without disturbing existing state.
        let mut srv = server_with_session();
        srv.emit("s1", EventKind::TextDelta).unwrap();
        let created_again = srv.create_session("s1", "office-1", TrustLevel::Trusted, false);
        assert!(!created_again);
        // The earlier event survived — the session was not recreated.
        assert_eq!(srv.events("s1").unwrap().len(), 1);
    }

    #[test]
    fn capabilities_reflect_trust_and_budget() {
        let srv = server_with_session();
        let caps = srv.capabilities("s1").unwrap();
        assert_eq!(caps.trust_level, TrustLevel::Trusted);
        assert_eq!(caps.budget_remaining, 100);
        assert!(caps.streaming);
    }

    #[test]
    fn anonymous_may_only_query_capabilities() {
        // Trust gate: anonymous callers cannot drive turns.
        let mut srv = AcpServer::new("0.1.0");
        srv.create_session("anon", "office-1", TrustLevel::Anonymous, false);
        assert!(srv.capabilities("anon").is_ok());
        assert_eq!(
            srv.emit("anon", EventKind::TextDelta),
            Err(AcpError::AuthRequired)
        );
    }

    #[test]
    fn events_are_monotonic_and_gapless() {
        // Seq is assigned in emission order with no gaps.
        let mut srv = server_with_session();
        srv.emit("s1", EventKind::Thinking).unwrap();
        srv.emit("s1", EventKind::TextDelta).unwrap();
        let done = srv
            .emit(
                "s1",
                EventKind::Done {
                    remaining_budget: 90,
                },
            )
            .unwrap();
        assert_eq!(done, 2);
        let seqs: Vec<u64> = srv.events("s1").unwrap().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
        assert!(!srv.has_gap("s1").unwrap());
        assert!(srv.events("s1").unwrap().last().unwrap().kind.is_terminal());
    }

    #[test]
    fn tool_delegation_gated_on_opt_in() {
        // Fails closed without opt-in; succeeds when opted in.
        let mut srv = server_with_session(); // opt_in = false
        assert_eq!(
            srv.request_client_tool("s1"),
            Err(AcpError::DelegationNotOptedIn)
        );

        srv.create_session("s2", "office-1", TrustLevel::Trusted, true);
        assert!(srv.request_client_tool("s2").is_ok());
    }

    #[test]
    fn interrupt_sets_a_resumable_fence() {
        // Interrupt is honored; the session remains valid/resumable.
        let mut srv = server_with_session();
        assert!(!srv.interrupt_requested("s1").unwrap());
        srv.interrupt("s1").unwrap();
        assert!(srv.interrupt_requested("s1").unwrap());
        // Still a live session — capabilities still serve.
        assert!(srv.capabilities("s1").is_ok());
    }

    #[test]
    fn projections_observe_identical_ordered_stream() {
        // Two adapters over one session see the same ordered events.
        let mut srv = server_with_session();
        srv.emit("s1", EventKind::Thinking).unwrap();
        srv.emit("s1", EventKind::TextDelta).unwrap();
        let events = srv.events("s1").unwrap();
        let peer = PeerAgentAdapter;
        let ui = UiEventAdapter;
        let peer_seqs: Vec<String> = events.iter().map(|e| peer.translate(e)).collect();
        let ui_seqs: Vec<String> = events.iter().map(|e| ui.translate(e)).collect();
        // Same count, same order — adapters neither add, drop, nor reorder.
        assert_eq!(peer_seqs.len(), ui_seqs.len());
        assert_eq!(ui_seqs, vec!["ui#0", "ui#1"]);
    }

    #[test]
    fn steering_is_session_scoped_and_bounded() {
        // A steer resolves to exactly one session; overflow rejects visibly.
        let mut srv = server_with_session();
        srv.create_session("s2", "office-1", TrustLevel::Trusted, false);
        srv.steer("s1", "focus on tests").unwrap();
        // s2's queue is untouched — no cross-session leak.
        assert_eq!(srv.poll_steer("s2").unwrap(), None);
        assert_eq!(
            srv.poll_steer("s1").unwrap().as_deref(),
            Some("focus on tests")
        );

        for i in 0..STEER_QUEUE_MAX {
            srv.steer("s1", &format!("m{i}")).unwrap();
        }
        assert_eq!(srv.steer("s1", "overflow"), Err(AcpError::SteerRejected));
    }

    #[test]
    fn apply_steer_cancels_pending_actions_then_continues() {
        // Not-yet-started actions surface as ActionSkipped, then Steering.
        let mut srv = server_with_session();
        let pending = vec!["send_email".to_string(), "post_message".to_string()];
        srv.apply_steer("s1", &pending).unwrap();
        let kinds: Vec<&EventKind> = srv.events("s1").unwrap().iter().map(|e| &e.kind).collect();
        assert_eq!(
            kinds[0],
            &EventKind::ActionSkipped {
                action: "send_email".to_string()
            }
        );
        assert_eq!(
            kinds[1],
            &EventKind::ActionSkipped {
                action: "post_message".to_string()
            }
        );
        assert_eq!(kinds[2], &EventKind::Steering);
        // Steering is non-terminal — the turn continues.
        assert!(!kinds[2].is_terminal());
    }
}
