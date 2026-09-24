//! The execution half: bind an invocation's arguments against its
//! invocable's declared binders, and only then run the attached handler —
//! under containment when the handler belongs to a contribution.
//!
//! Binding and execution are deliberately two separate steps with a hard
//! ordering: every declared binder is checked before the handler
//! runs at all, so a rejection is a structural fact ("the handler did not
//! run") rather than something the handler itself would have to notice and
//! report. This module's own [`bind`] can produce only two of the four
//! rejection modes — [`RejectionMode::Absent`] (nothing supplied for a
//! required binder) and [`RejectionMode::IllShaped`] (a supplied value's
//! shape does not match the binder's declared kind) — because the values it
//! checks ([`ArgValue`]) are already fully parsed, typed, in-memory data.
//! [`RejectionMode::Unreadable`] (a source could not be obtained) and
//! [`RejectionMode::Malformed`] (present but not parseable at all) describe
//! failures that happen *before* a value becomes an [`ArgValue`] — reading
//! an external source, parsing raw surface input — and that is I/O and
//! surface-specific work this crate's no-I/O tier must not perform;
//! it belongs to whatever surface constructs an [`ArgValues`] from raw
//! input, upstream of dispatch. Both modes remain fully representable and
//! located by [`Rejection`] itself; this module just is not where they
//! originate.
//!
//! A core invocable's handler runs directly — it is the trusted kernel, and
//! a core panic is a real defect that must propagate, not a contained
//! failure papered over. A contribution's handler runs on a bounded,
//! panic-caught path: Rust cannot safely preempt a running thread, so the
//! bound is enforced by handing the call to a fresh, unscoped thread and
//! giving up on waiting for it past the bound — the orphaned thread runs to
//! completion on its own and its (now-unwanted) result is discarded, but
//! the caller is never held past the bound, and no dispatch call ever
//! blocks indefinitely or ends in an unstructured panic.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use cronus_contract::{
    ArgValue, ArgValues, Dispatched, Invocable, InvocableId, Invocation, Outcome, Rejection,
    RejectionMode, Resolved,
};

use super::registry::{CORE_IDENTITY, InvocableRegistry};

/// The executable behavior attached to one invocable. Kept separate from
/// the invocable's descriptor (its catalog entry) so a surface can read the
/// catalog — for help, completion, or the desktop's `catalog()` — without
/// needing a handler to exist at all. `Arc` rather than `Box`: a
/// contribution's handler is cloned onto the bounded thread that runs it
/// (§ above), which needs a `'static`, cheaply-shareable handle rather than
/// a borrow of `self`.
pub type Handler = Arc<dyn Fn(&ArgValues) -> Outcome + Send + Sync>;

/// How long a contributed invocable's handler is given before dispatch
/// gives up on it. Core invocables are not bounded at all — see the
/// module doc.
pub const CONTRIBUTION_TIME_BOUND: Duration = Duration::from_secs(5);

/// Checks one supplied value against one binder's declared kind.
fn matches_kind(value: &ArgValue, kind: cronus_contract::BinderKind) -> bool {
    use cronus_contract::BinderKind;
    // A closed value set: the runtime shape is a string, and the string must
    // be one of the declared values — enforced here too, not only at the
    // command line, so a direct dispatch caller cannot bypass it.
    if let (ArgValue::Text(s), BinderKind::EnumText(allowed)) = (value, kind) {
        return allowed.contains(&s.as_str());
    }
    matches!(
        (value, kind),
        (ArgValue::Text(_), BinderKind::Text)
            | (ArgValue::Integer(_), BinderKind::Integer)
            | (ArgValue::Boolean(_), BinderKind::Boolean)
            | (ArgValue::Flag, BinderKind::Flag)
            // A `NamedText` binder's bound value is still a plain
            // `ArgValue::Text` — only its command-line binding style
            // differs from a positional `Text`, not its runtime shape.
            | (ArgValue::Text(_), BinderKind::NamedText)
            | (ArgValue::Float(_), BinderKind::Float)
            | (ArgValue::List(_), BinderKind::RepeatableNamedText)
    )
}

/// Bind every declared binder against `args`, in declared order, stopping at
/// the first rejection (a stable, learnable order — the same malformed
/// input always fails at the same binder). `None` means every binder is
/// satisfied and the body may run.
pub fn bind(descriptor: &Invocable, args: &ArgValues) -> Option<Rejection> {
    for binder in &descriptor.binders {
        match args.get(binder.name) {
            None if binder.optional => continue,
            None => {
                return Some(Rejection {
                    binder: binder.name,
                    mode: RejectionMode::Absent,
                    detail: "no value supplied for a required binder".to_string(),
                });
            }
            Some(value) if !matches_kind(value, binder.kind) => {
                return Some(Rejection {
                    binder: binder.name,
                    mode: RejectionMode::IllShaped,
                    detail: format!(
                        "supplied value does not match the declared {:?} shape",
                        binder.kind
                    ),
                });
            }
            Some(_) => continue,
        }
    }
    None
}

/// Run a contribution's handler under containment: a panic is caught
/// and never unwinds into the caller, and the caller is never held past
/// `bound`. Either failure resolves to [`Outcome::Unavailable`], its reason
/// naming `contributor` — a rejection attributed to the contribution, never
/// a core fault and never a hang.
fn run_contribution(
    handler: &Handler,
    args: &ArgValues,
    contributor: &str,
    bound: Duration,
) -> Outcome {
    let handler = Arc::clone(handler);
    let args = args.clone();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let result = panic::catch_unwind(AssertUnwindSafe(|| handler(&args)));
        // If the receiver already gave up (timed out), this send finds no
        // one listening — that is not itself an error to report anywhere;
        // the timeout branch below has already produced the Outcome.
        let _ = tx.send(result);
    });

    match rx.recv_timeout(bound) {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_panic_payload)) => Outcome::Unavailable {
            reason: format!("contribution '{contributor}' panicked while handling this invocation"),
        },
        Err(_timeout_or_disconnected) => Outcome::Unavailable {
            reason: format!("contribution '{contributor}' exceeded its time bound"),
        },
    }
}

/// The effect that reverses one [`Dispatcher::attach`] call:
/// disposing it removes exactly the handler it attached, and nothing else.
/// The invocable's descriptor (if any) is untouched — disposing a handler
/// leaves a normal, catalog-readable, unattached descriptor behind, the
/// same state as one that was never given a handler at all. Consumed by
/// value on disposal, so a handle can be spent only once.
#[derive(Debug)]
pub struct DispatchHandle {
    id: InvocableId,
}

impl DispatchHandle {
    /// Remove this exact handler from `dispatcher`.
    pub fn dispose(self, dispatcher: &mut Dispatcher) {
        dispatcher.handlers.remove(&self.id);
    }
}

/// One record of a resolved dispatch's lifecycle. The pair — never
/// a single combined record — is what makes an unsettled dispatch (one
/// whose handler never returned) visible: a lone `Enter` with no matching
/// `Settle` is exactly that state, which a record written only on
/// completion could never represent.
pub enum JournalRecord<'a> {
    /// Written before the handler runs.
    Enter {
        dispatch_id: &'a str,
        invocable: &'a InvocableId,
        /// `None` when the invocable declares its raw input unrecordable
        /// (`Invocable::journal_raw_input == false`) — a secret-bearing
        /// argument, or a payload an authoritative domain event already
        /// owns. Removes that argument class from the journal entirely
        /// rather than filtering it after the fact, which is what
        /// redaction alone cannot do for an argument that IS entirely a
        /// credential the secret store never learns.
        args: Option<&'a ArgValues>,
    },
    /// Written after the handler settles, paired with the `Enter` sharing
    /// the same `dispatch_id`.
    Settle {
        dispatch_id: &'a str,
        outcome: &'a Outcome,
    },
}

/// Where a resolved dispatch's lifecycle is recorded. A pure
/// interface — no I/O lives in this crate; a real durable journal is
/// supplied by whatever composes the `Dispatcher`, the same residual shape
/// as [`Dispatcher::set_secrets`]: the seam exists and is exercised here,
/// wiring a real sink into the facade is a separate, recorded obligation.
pub trait DispatchJournal {
    /// Write one record. `Err` on an `Enter` record fails the dispatch
    /// before the handler runs; the same failure on `Settle` is
    /// contained by the caller so the handler's own outcome stays the
    /// reported one.
    fn record(&mut self, record: JournalRecord<'_>) -> Result<(), String>;
}

/// A boxed, thread-safe [`DispatchJournal`] — `Dispatcher` stores it behind
/// a `Mutex` because `dispatch` takes `&self`, not `&mut self` (multiple
/// callers may dispatch concurrently through one shared `Dispatcher`).
pub type JournalSink = Box<dyn DispatchJournal + Send + Sync>;

/// A token unique to this process instance, salted by the process id and a
/// wall-clock timestamp taken at construction — the "unique across
/// process restarts" requirement. A per-process counter alone repeats after
/// a restart and would silently pair a new dispatch with an old one; this
/// token makes that collision require the same pid AND the same nanosecond
/// of construction, which is not a bound worth spending a dependency on.
fn mint_instance_token() -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_nanos())
        .unwrap_or(0);
    format!("{pid:x}-{nanos:x}")
}

/// Holds the executable behavior attached to each invocable, and runs
/// bind-before-invoke over a [`InvocableRegistry`]'s descriptors.
pub struct Dispatcher {
    handlers: HashMap<InvocableId, Handler>,
    contribution_bound: Duration,
    secrets: Vec<String>,
    journal: Option<Mutex<JournalSink>>,
    instance_token: String,
    dispatch_seq: AtomicU64,
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Dispatcher {
    pub fn new() -> Self {
        Dispatcher {
            handlers: HashMap::new(),
            contribution_bound: CONTRIBUTION_TIME_BOUND,
            secrets: Vec::new(),
            journal: None,
            instance_token: mint_instance_token(),
            dispatch_seq: AtomicU64::new(0),
        }
    }

    /// A dispatcher whose contribution time bound is `bound` rather than
    /// the production default — for tests that must observe timeout
    /// behavior without waiting out [`CONTRIBUTION_TIME_BOUND`].
    pub fn with_contribution_bound(bound: Duration) -> Self {
        Dispatcher {
            handlers: HashMap::new(),
            contribution_bound: bound,
            secrets: Vec::new(),
            journal: None,
            instance_token: mint_instance_token(),
            dispatch_seq: AtomicU64::new(0),
        }
    }

    /// Attach the executable behavior for an invocable. Independent of
    /// registry registration — the two may happen in either order, and a
    /// descriptor with no attached handler is a normal, catalog-readable
    /// state (dispatch reports it as unavailable, never panics). Returns
    /// the effect that reverses exactly this attachment.
    pub fn attach(&mut self, id: InvocableId, handler: Handler) -> DispatchHandle {
        self.handlers.insert(id.clone(), handler);
        DispatchHandle { id }
    }

    /// Replace the secret values every dispatched [`Outcome`] is masked
    /// against. Starts empty — populating it from the core's real
    /// secret store is a separate, recorded obligation this method makes
    /// possible but does not itself discharge.
    pub fn set_secrets(&mut self, secrets: Vec<String>) {
        self.secrets = secrets;
    }

    /// Install where this dispatcher records a resolved dispatch's
    /// lifecycle. Starts unconfigured — with no journal, dispatch
    /// proceeds exactly as before journaling existed, the same residual
    /// shape as [`Dispatcher::set_secrets`] starting with an empty list.
    pub fn set_journal(&mut self, journal: JournalSink) {
        self.journal = Some(Mutex::new(journal));
    }

    fn next_dispatch_id(&self) -> String {
        let seq = self.dispatch_seq.fetch_add(1, Ordering::Relaxed);
        format!("{}-{seq}", self.instance_token)
    }

    /// Write one journal record if a journal is configured. No journal
    /// configured is success — dispatch behaves exactly as if this method
    /// did not exist. A poisoned mutex is treated as a journal failure
    /// rather than propagated as a panic (no panics on this path).
    fn write_journal(&self, record: JournalRecord<'_>) -> Result<(), String> {
        let Some(journal) = &self.journal else {
            return Ok(());
        };
        match journal.lock() {
            Ok(mut sink) => sink.record(record),
            Err(_poisoned) => Err("dispatch journal mutex poisoned".to_string()),
        }
    }

    /// Resolve `invocation` against `registry` first — an id
    /// naming nothing the registry knows returns [`Dispatched::Unknown`]
    /// immediately, before any journal record is written and before any
    /// binder runs, because nothing happened that a journal entry or a
    /// binding rejection could describe. A resolved invocation is journaled
    /// as a paired entry/settlement record: a failure to write the
    /// entry fails the dispatch loudly, before the handler runs; the same
    /// failure on the settlement record is contained so the handler's own
    /// outcome — masked against the configured secrets at this single
    /// boundary point, regardless of which branch produced it — stays the
    /// reported one.
    pub fn dispatch(&self, registry: &InvocableRegistry, invocation: &Invocation) -> Dispatched {
        let descriptor = match registry.resolve(&invocation.id) {
            Resolved::Found(descriptor) => descriptor,
            Resolved::Unknown => return Dispatched::Unknown,
        };

        let dispatch_id = self.next_dispatch_id();
        let entry_args = descriptor.journal_raw_input.then_some(&invocation.args);
        if let Err(reason) = self.write_journal(JournalRecord::Enter {
            dispatch_id: &dispatch_id,
            invocable: &invocation.id,
            args: entry_args,
        }) {
            // A dispatch that runs unrecorded is the one case the
            // journal exists to prevent — refuse before the handler runs.
            return Dispatched::Ran(Outcome::Unavailable {
                reason: format!("dispatch journal refused the entry record: {reason}"),
            });
        }

        let outcome = self.run_resolved(descriptor, invocation);
        let secret_refs: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
        let outcome = crate::redact::redact_outcome(outcome, &secret_refs);

        // A settlement-record failure is contained: the handler's own
        // outcome stays the reported one regardless.
        let _ = self.write_journal(JournalRecord::Settle {
            dispatch_id: &dispatch_id,
            outcome: &outcome,
        });

        Dispatched::Ran(outcome)
    }

    /// Bind and run a resolved invocation's handler — the part of dispatch
    /// that happens only once resolution and the journal's entry record
    /// have both already succeeded.
    fn run_resolved(&self, descriptor: &Invocable, invocation: &Invocation) -> Outcome {
        if let Some(rejection) = bind(descriptor, &invocation.args) {
            return Outcome::Rejected(rejection);
        }

        let Some(handler) = self.handlers.get(&invocation.id) else {
            return Outcome::Unavailable {
                reason: format!("no handler attached: {}", invocation.id.as_str()),
            };
        };

        if invocation.id.qualifier() == CORE_IDENTITY {
            // The trusted kernel: runs directly, unbounded, uncaught. A
            // core panic is a real defect and must propagate as one.
            handler(&invocation.args)
        } else {
            run_contribution(
                handler,
                &invocation.args,
                invocation.id.qualifier(),
                self.contribution_bound,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use cronus_contract::{Binder, BinderKind, Locus, OutcomeValue, Stability, Surface};

    use super::super::registry::{CONTRIBUTE_GRANT, Registrant};
    use super::*;

    fn contributed_descriptor() -> Invocable {
        Invocable {
            id: InvocableId::new("myext:risky").expect("well-formed invocable id"),
            name: "Risky",
            summary: "A contributed invocable used to exercise containment.",
            group: "test",
            locus: Locus::Semantic,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        }
    }

    fn granted_registrant() -> Registrant {
        Registrant::extension("myext", "src-1").with_grant(CONTRIBUTE_GRANT)
    }

    fn card_add_descriptor() -> Invocable {
        Invocable {
            id: InvocableId::new("core:board.add").expect("well-formed invocable id"),
            name: "Add card",
            summary: "Add a card to the board",
            group: "board",
            locus: Locus::Semantic,
            binders: vec![
                Binder {
                    name: "id",
                    kind: BinderKind::Text,
                    optional: false,
                },
                Binder {
                    name: "task_ref",
                    kind: BinderKind::Text,
                    optional: true,
                },
            ],
            stability: Stability::Shipped,
            journal_raw_input: true,
        }
    }

    fn invocation_with(args: ArgValues) -> Invocation {
        Invocation {
            id: InvocableId::new("core:board.add").expect("well-formed invocable id"),
            args,
            caller: Surface::Cli,
        }
    }

    /// Unwrap a resolved dispatch's outcome, panicking with a clear message
    /// if resolution unexpectedly missed — every call site below expects
    /// the invocable to exist, so an `Unknown` here is the test's own setup
    /// being wrong, not a case it means to exercise.
    fn ran(dispatched: Dispatched) -> Outcome {
        match dispatched {
            Dispatched::Ran(outcome) => outcome,
            Dispatched::Unknown => panic!("expected a resolved dispatch, got Unknown"),
        }
    }

    #[test]
    fn a_binding_failure_leaves_no_side_effect() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();

        let handler_calls = Arc::new(AtomicUsize::new(0));
        let handler_calls_in_handler = Arc::clone(&handler_calls);
        let mut dispatcher = Dispatcher::new();
        dispatcher.attach(
            InvocableId::new("core:board.add").expect("well-formed invocable id"),
            Arc::new(move |_args: &ArgValues| {
                handler_calls_in_handler.fetch_add(1, Ordering::SeqCst);
                Outcome::Value(OutcomeValue::Empty)
            }),
        );

        // No "id" supplied — a required binder is absent.
        let outcome = ran(dispatcher.dispatch(&registry, &invocation_with(ArgValues::new())));

        assert!(matches!(outcome, Outcome::Rejected(_)));
        assert_eq!(
            handler_calls.load(Ordering::SeqCst),
            0,
            "the handler must not run when binding rejects"
        );
    }

    #[test]
    fn a_satisfied_optional_binder_absent_still_invokes_the_handler() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();

        let mut dispatcher = Dispatcher::new();
        dispatcher.attach(
            InvocableId::new("core:board.add").expect("well-formed invocable id"),
            Arc::new(|_args: &ArgValues| Outcome::Value(OutcomeValue::Text("added".to_string()))),
        );

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        // "task_ref" is optional and genuinely absent — not a rejection.
        let outcome = ran(dispatcher.dispatch(&registry, &invocation_with(args)));

        assert_eq!(
            outcome,
            Outcome::Value(OutcomeValue::Text("added".to_string()))
        );
    }

    #[test]
    fn a_present_but_wrong_shaped_value_rejects_as_ill_shaped_with_its_binder_location() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();
        let dispatcher = Dispatcher::new();

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Boolean(true)); // declared Text, supplied Boolean
        let outcome = ran(dispatcher.dispatch(&registry, &invocation_with(args)));

        match outcome {
            Outcome::Rejected(rejection) => {
                assert_eq!(rejection.binder, "id");
                assert_eq!(rejection.mode, RejectionMode::IllShaped);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    /// `NamedText` differs from `Text` only in how a CLI frontend positions
    /// it on the command line — its bound runtime value is the same
    /// `ArgValue::Text`, and `bind` must accept it exactly as it would a
    /// positional `Text` binder. Not caught by the compiler if this arm is
    /// ever dropped from `matches_kind` (it is a `matches!` over a tuple,
    /// not an exhaustive match on `BinderKind` alone) — this test is the
    /// only thing that would catch a silent regression here.
    #[test]
    fn a_named_text_binder_accepts_a_text_value_exactly_like_a_positional_one() {
        let descriptor = Invocable {
            binders: vec![Binder {
                name: "actor",
                kind: BinderKind::NamedText,
                optional: false,
            }],
            ..card_add_descriptor()
        };
        let mut args = ArgValues::new();
        args.insert("actor", ArgValue::Text("cli".to_string()));
        assert_eq!(
            bind(&descriptor, &args),
            None,
            "a NamedText binder must accept a Text-shaped value, not reject it as ill-shaped"
        );

        let mut wrong_shape = ArgValues::new();
        wrong_shape.insert("actor", ArgValue::Boolean(true));
        assert_eq!(
            bind(&descriptor, &wrong_shape).map(|r| r.mode),
            Some(RejectionMode::IllShaped),
            "a NamedText binder must still reject a genuinely wrong-shaped value"
        );
    }

    /// Same discipline as the `NamedText` test above, for the `Float` and
    /// `RepeatableNamedText` kinds: neither arm is compiler-enforced
    /// (`matches_kind` is a `matches!` over a tuple, not an exhaustive match
    /// on `BinderKind` alone), so a dropped arm fails silently — every value
    /// of that kind would reject as ill-shaped — unless a test proves it.
    #[test]
    fn a_float_binder_accepts_a_float_value_and_a_repeatable_binder_accepts_a_list() {
        let float_descriptor = Invocable {
            binders: vec![Binder {
                name: "limit",
                kind: BinderKind::Float,
                optional: false,
            }],
            ..card_add_descriptor()
        };
        let mut float_args = ArgValues::new();
        float_args.insert("limit", ArgValue::Float(12.5));
        assert_eq!(bind(&float_descriptor, &float_args), None);

        let mut float_wrong_shape = ArgValues::new();
        float_wrong_shape.insert("limit", ArgValue::Integer(12));
        assert_eq!(
            bind(&float_descriptor, &float_wrong_shape).map(|r| r.mode),
            Some(RejectionMode::IllShaped)
        );

        let list_descriptor = Invocable {
            binders: vec![Binder {
                name: "collection",
                kind: BinderKind::RepeatableNamedText,
                optional: false,
            }],
            ..card_add_descriptor()
        };
        let mut list_args = ArgValues::new();
        list_args.insert(
            "collection",
            ArgValue::List(vec!["a".to_string(), "b".to_string()]),
        );
        assert_eq!(bind(&list_descriptor, &list_args), None);

        let mut list_wrong_shape = ArgValues::new();
        list_wrong_shape.insert("collection", ArgValue::Text("a".to_string()));
        assert_eq!(
            bind(&list_descriptor, &list_wrong_shape).map(|r| r.mode),
            Some(RejectionMode::IllShaped)
        );
    }

    #[test]
    fn dispatch_against_an_unknown_invocable_yields_resolved_unknown_not_an_outcome() {
        // Nothing ran, nothing was rejected — this is a resolution
        // miss, not a failure-shaped `Outcome`. Folding this into
        // `Outcome::Unavailable` forces one wrong reading on the other two,
        // because the three surfaces answer `Unknown` three incompatible
        // ways (fall through / usage error / catalog refresh) and none of
        // those readings is "render an error".
        let registry = InvocableRegistry::new();
        let dispatcher = Dispatcher::new();
        let dispatched = dispatcher.dispatch(&registry, &invocation_with(ArgValues::new()));
        assert!(matches!(dispatched, Dispatched::Unknown));
    }

    #[test]
    fn a_registered_descriptor_with_no_attached_handler_is_unavailable_not_a_panic() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();
        let dispatcher = Dispatcher::new(); // nothing attached

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        let outcome = ran(dispatcher.dispatch(&registry, &invocation_with(args)));

        assert!(matches!(outcome, Outcome::Unavailable { .. }));
    }

    #[test]
    fn a_panicking_contribution_resolves_to_unavailable_attributed_to_it_and_does_not_unwind() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&granted_registrant(), contributed_descriptor())
            .unwrap();

        let mut dispatcher = Dispatcher::new();
        dispatcher.attach(
            InvocableId::new("myext:risky").expect("well-formed invocable id"),
            Arc::new(|_args: &ArgValues| panic!("boom")),
        );

        let invocation = Invocation {
            id: InvocableId::new("myext:risky").expect("well-formed invocable id"),
            args: ArgValues::new(),
            caller: Surface::Cli,
        };
        // If the panic unwound into this thread rather than being caught on
        // its own thread, this call itself would abort the test process —
        // reaching the assertion below is part of the proof.
        let outcome = ran(dispatcher.dispatch(&registry, &invocation));

        match outcome {
            Outcome::Unavailable { reason } => assert!(
                reason.contains("myext"),
                "reason must attribute the failure to the contribution: {reason}"
            ),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_contribution_exceeding_its_time_bound_resolves_to_unavailable_within_the_bound() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&granted_registrant(), contributed_descriptor())
            .unwrap();

        let bound = Duration::from_millis(50);
        let mut dispatcher = Dispatcher::with_contribution_bound(bound);
        dispatcher.attach(
            InvocableId::new("myext:risky").expect("well-formed invocable id"),
            Arc::new(|_args: &ArgValues| {
                std::thread::sleep(Duration::from_secs(60));
                Outcome::Value(OutcomeValue::Empty)
            }),
        );

        let invocation = Invocation {
            id: InvocableId::new("myext:risky").expect("well-formed invocable id"),
            args: ArgValues::new(),
            caller: Surface::Cli,
        };

        let started = std::time::Instant::now();
        let outcome = ran(dispatcher.dispatch(&registry, &invocation));
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(5),
            "dispatch must give up at the configured bound, not wait out the handler; took {elapsed:?}"
        );
        match outcome {
            Outcome::Unavailable { reason } => assert!(
                reason.contains("myext"),
                "reason must attribute the timeout to the contribution: {reason}"
            ),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_core_invocable_runs_unbounded_and_a_panic_there_is_not_contained() {
        // The trusted kernel is not subject to the contribution containment
        // path at all — proven by taking the code path that WOULD apply
        // containment (a real panic) and confirming it is the CALLER, not
        // this module, that decides whether to catch it. `catch_unwind`
        // here belongs to the test, standing in for "the core's own
        // surrounding code", not to dispatch.
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();
        let mut dispatcher = Dispatcher::new();
        dispatcher.attach(
            InvocableId::new("core:board.add").expect("well-formed invocable id"),
            Arc::new(|_args: &ArgValues| panic!("a genuine core defect")),
        );

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        let invocation = invocation_with(args);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatcher.dispatch(&registry, &invocation)
        }));
        assert!(
            result.is_err(),
            "a core panic must propagate, not resolve to a contained Outcome"
        );
    }

    #[test]
    fn disposing_a_registration_and_its_handler_removes_both_independently() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:board.add").expect("well-formed invocable id");

        let registration = registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();
        let dispatch_handle = dispatcher.attach(
            id.clone(),
            Arc::new(|_args: &ArgValues| Outcome::Value(OutcomeValue::Text("added".to_string()))),
        );

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        assert_eq!(
            ran(dispatcher.dispatch(&registry, &invocation_with(args.clone()))),
            Outcome::Value(OutcomeValue::Text("added".to_string()))
        );

        // Disposing only the handler: the descriptor still resolves and
        // binds, but nothing remains to run it.
        dispatch_handle.dispose(&mut dispatcher);
        assert!(registry.resolve(&id).is_found(), "the descriptor survives");
        assert!(
            matches!(
                ran(dispatcher.dispatch(&registry, &invocation_with(args.clone()))),
                Outcome::Unavailable { .. }
            ),
            "no handler remains attached"
        );

        // Disposing the registration itself: the descriptor is gone too, so
        // dispatch can no longer even resolve the invocable — this is now a
        // resolution miss, not an Outcome at all.
        registration.handle.dispose(&mut registry);
        assert!(registry.resolve(&id).is_unknown());
        assert!(matches!(
            dispatcher.dispatch(&registry, &invocation_with(args)),
            Dispatched::Unknown
        ));
    }

    #[derive(Debug, Clone, PartialEq)]
    enum RecordedEntry {
        Enter { dispatch_id: String, had_args: bool },
        Settle { dispatch_id: String },
    }

    /// A journal that records every call it receives, for tests to inspect
    /// after dispatch returns. Shares its log via `Arc` so the constructing
    /// test keeps a handle after the sink itself is moved into the
    /// `Dispatcher`.
    struct RecordingJournal(Arc<Mutex<Vec<RecordedEntry>>>);

    impl DispatchJournal for RecordingJournal {
        fn record(&mut self, record: JournalRecord<'_>) -> Result<(), String> {
            let entry = match record {
                JournalRecord::Enter {
                    dispatch_id, args, ..
                } => RecordedEntry::Enter {
                    dispatch_id: dispatch_id.to_string(),
                    had_args: args.is_some(),
                },
                JournalRecord::Settle { dispatch_id, .. } => RecordedEntry::Settle {
                    dispatch_id: dispatch_id.to_string(),
                },
            };
            self.0.lock().expect("test mutex poisoned").push(entry);
            Ok(())
        }
    }

    /// A journal whose entry record always fails — for proving the
    /// failure asymmetry: an entry failure must refuse the dispatch before
    /// the handler runs.
    struct FailingEntryJournal;

    impl DispatchJournal for FailingEntryJournal {
        fn record(&mut self, record: JournalRecord<'_>) -> Result<(), String> {
            match record {
                JournalRecord::Enter { .. } => Err("simulated entry failure".to_string()),
                JournalRecord::Settle { .. } => Ok(()),
            }
        }
    }

    #[test]
    fn dispatching_an_unknown_invocable_writes_no_journal_entry() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        dispatcher.set_journal(Box::new(RecordingJournal(Arc::clone(&log))));

        let dispatched = dispatcher.dispatch(&registry, &invocation_with(ArgValues::new()));

        assert!(matches!(dispatched, Dispatched::Unknown));
        assert!(
            log.lock().unwrap().is_empty(),
            "a resolution miss entered no handler and must journal nothing"
        );
    }

    #[test]
    fn a_resolved_dispatch_is_journaled_as_a_paired_entry_and_settlement() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();
        let mut dispatcher = Dispatcher::new();
        dispatcher.set_journal(Box::new(RecordingJournal(Arc::clone(&log))));
        dispatcher.attach(
            InvocableId::new("core:board.add").expect("well-formed invocable id"),
            Arc::new(|_args: &ArgValues| Outcome::Value(OutcomeValue::Text("added".to_string()))),
        );

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        let outcome = ran(dispatcher.dispatch(&registry, &invocation_with(args)));
        assert_eq!(
            outcome,
            Outcome::Value(OutcomeValue::Text("added".to_string()))
        );

        let recorded = log.lock().unwrap().clone();
        assert_eq!(recorded.len(), 2, "exactly an entry and a settlement");
        match (&recorded[0], &recorded[1]) {
            (
                RecordedEntry::Enter {
                    dispatch_id: enter_id,
                    had_args,
                },
                RecordedEntry::Settle {
                    dispatch_id: settle_id,
                },
            ) => {
                assert!(
                    *had_args,
                    "this invocable journals its raw input by default"
                );
                assert_eq!(enter_id, settle_id, "the pair shares one dispatch identity");
            }
            other => panic!("expected [Enter, Settle], got {other:?}"),
        }
    }

    #[test]
    fn an_invocable_may_decline_to_journal_its_raw_input() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut registry = InvocableRegistry::new();
        let mut secret_descriptor = card_add_descriptor();
        secret_descriptor.journal_raw_input = false;
        registry
            .register(&Registrant::core(), secret_descriptor)
            .unwrap();
        let mut dispatcher = Dispatcher::new();
        dispatcher.set_journal(Box::new(RecordingJournal(Arc::clone(&log))));
        dispatcher.attach(
            InvocableId::new("core:board.add").expect("well-formed invocable id"),
            Arc::new(|_args: &ArgValues| Outcome::Value(OutcomeValue::Empty)),
        );

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        ran(dispatcher.dispatch(&registry, &invocation_with(args)));

        match &log.lock().unwrap()[0] {
            RecordedEntry::Enter { had_args, .. } => {
                assert!(
                    !had_args,
                    "a suppressed invocable must not journal its args"
                )
            }
            other => panic!("expected Enter, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_entry_record_refuses_the_dispatch_before_the_handler_runs() {
        let mut registry = InvocableRegistry::new();
        registry
            .register(&Registrant::core(), card_add_descriptor())
            .unwrap();
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let handler_calls_in_handler = Arc::clone(&handler_calls);
        let mut dispatcher = Dispatcher::new();
        dispatcher.set_journal(Box::new(FailingEntryJournal));
        dispatcher.attach(
            InvocableId::new("core:board.add").expect("well-formed invocable id"),
            Arc::new(move |_args: &ArgValues| {
                handler_calls_in_handler.fetch_add(1, Ordering::SeqCst);
                Outcome::Value(OutcomeValue::Text("added".to_string()))
            }),
        );

        let mut args = ArgValues::new();
        args.insert("id", ArgValue::Text("card-1".to_string()));
        let outcome = ran(dispatcher.dispatch(&registry, &invocation_with(args)));

        assert!(
            matches!(outcome, Outcome::Unavailable { .. }),
            "an entry-record failure is a loud, visible outcome"
        );
        assert_eq!(
            handler_calls.load(Ordering::SeqCst),
            0,
            "a dispatch that could not be journaled must never run — that is the one case the journal exists to prevent"
        );
    }
}
