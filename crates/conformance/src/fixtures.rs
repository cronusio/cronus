//! The shared fixture data every surface's test drives through the harness.
//! Written **from the divergence, not the feature**: each fixture
//! exists because it is a shape where two correct-looking implementations
//! diverge, not because it exercises a capability. A fixture drawn from the
//! happy path proves nothing, since the happy path is where implementations
//! already agree.
//!
//! Registering a fixture into a real registry and dispatcher is the calling
//! surface's own work — this crate names contract-tier data only (it
//! depends on `cronus-contract` alone), never a live `InvocableRegistry` or
//! `Dispatcher`. A surface's test constructs matching descriptors and
//! handlers using [`Corpus::canonical`] and the fixtures below as its source
//! of truth for what to register and what to expect.

use cronus_contract::{
    ArgValue, ArgValues, Binder, BinderKind, Dispatched, Invocable, InvocableId, Locus, Outcome,
    OutcomeValue, Rejection, RejectionMode, Stability,
};

/// The identity every canonical fixture registers under. Not `"core"`
/// (a reserved identity — colliding with it here would shadow a real
/// surface's actual core invocables the moment a test registers both into
/// the same registry) and not a name a real extension would plausibly pick.
pub const FIXTURE_IDENTITY: &str = "conformance";

/// The exact secret value the secret-bearing and boundary-crossing fixtures
/// embed in their raw (pre-redaction) handler output. A registering
/// surface's test must configure its dispatcher's secret list to include
/// this value before running those two fixtures — omitting it is not a
/// harness bug, it is the currently-true, deliberately-carried residual
///  that every surface's redaction is fed an empty secret list.
/// Until a surface's real secret store is wired into its dispatcher, these
/// two fixtures are **expected to fail**, and that failure is the finding,
/// not a defect in the corpus.
pub const SECRET_VALUE: &str = "sk-LIVE-conformance-secret";

fn id(tail: &str) -> InvocableId {
    InvocableId::new(format!("{FIXTURE_IDENTITY}:{tail}"))
        .expect("fixture tail is a well-formed identity by construction")
}

fn no_binders_descriptor(tail: &str, name: &'static str, summary: &'static str) -> Invocable {
    Invocable {
        id: id(tail),
        name,
        summary,
        group: "conformance",
        locus: Locus::Semantic,
        binders: Vec::new(),
        stability: Stability::Shipped,
        journal_raw_input: true,
    }
}

/// One outcome-family fixture: an invocation, and the `Dispatched` every
/// registering surface's real dispatch path must produce for it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeFixture {
    /// Stable across runs — identifies this fixture in a report, never
    /// derived from its position in the corpus.
    pub name: &'static str,
    pub id: InvocableId,
    pub args: ArgValues,
    pub expected: Dispatched,
}

/// The full shared corpus: the canonical shipped set every surface-set
/// comparison measures against, and the outcome fixtures every surface's
/// dispatch path is driven through.
#[derive(Debug, Clone)]
pub struct Corpus {
    /// The canonical, `Shipped` invocable set — what a conforming surface's
    /// `exposed()` should equal, once the caller's own declared exclusions
    /// are subtracted. A `Stability::Retired` descriptor is never a
    /// member of this set by construction: retirement and shipped status
    /// are the same field's two mutually exclusive values, so a retired
    /// fixture belongs to [`retired_pair`], not here.
    pub canonical: Vec<Invocable>,
    pub outcome_fixtures: Vec<OutcomeFixture>,
}

/// The shared corpus. Callers register their own declared exclusions
/// separately — this data is what a **conforming** surface would expose in
/// full, before any surface-specific carve-out is subtracted.
pub fn corpus() -> Corpus {
    Corpus {
        canonical: vec![
            no_binders_descriptor(
                "empty-result",
                "Empty result",
                "Returns a genuinely empty result.",
            ),
            no_binders_descriptor(
                "single-element",
                "Single element",
                "Returns a one-item list.",
            ),
            no_binders_descriptor(
                "zero-count-list",
                "Zero-count list",
                "Returns a zero-item list — distinct from an empty result.",
            ),
            Invocable {
                binders: vec![Binder {
                    name: "value",
                    kind: BinderKind::Text,
                    optional: false,
                }],
                ..no_binders_descriptor(
                    "oversized-input",
                    "Oversized input",
                    "Echoes back a large text value unchanged.",
                )
            },
            Invocable {
                binders: vec![Binder {
                    name: "id",
                    kind: BinderKind::Text,
                    optional: false,
                }],
                ..no_binders_descriptor(
                    "absent-required",
                    "Absent required argument",
                    "Rejects when its one required argument is not supplied.",
                )
            },
            no_binders_descriptor(
                "unavailable-resource",
                "Unavailable resource",
                "Always reports its resource as unavailable — never a success-shaped empty result.",
            ),
            no_binders_descriptor(
                "secret-bearing",
                "Secret-bearing output",
                "Returns text containing a known secret, for the redaction boundary.",
            ),
            no_binders_descriptor(
                "boundary-crossing",
                "Boundary-crossing output",
                "Returns a known secret nested inside a record and a list.",
            ),
        ],
        outcome_fixtures: vec![
            OutcomeFixture {
                name: "empty-result",
                id: id("empty-result"),
                args: ArgValues::new(),
                expected: Dispatched::Ran(Outcome::Value(OutcomeValue::Empty)),
            },
            OutcomeFixture {
                name: "single-element",
                id: id("single-element"),
                args: ArgValues::new(),
                expected: Dispatched::Ran(Outcome::Value(OutcomeValue::List(vec![
                    OutcomeValue::Text("only".to_string()),
                ]))),
            },
            OutcomeFixture {
                // A zero-item List and a genuinely Empty result look the
                // same to a casual reader and must not look the same to
                // dispatch: collapsing either into the other is exactly the
                // kind of quiet re-derivation that is forbidden.
                name: "zero-count-list",
                id: id("zero-count-list"),
                args: ArgValues::new(),
                expected: Dispatched::Ran(Outcome::Value(OutcomeValue::List(Vec::new()))),
            },
            OutcomeFixture {
                name: "oversized-input",
                id: id("oversized-input"),
                args: {
                    let mut args = ArgValues::new();
                    args.insert("value", ArgValue::Text(oversized_text()));
                    args
                },
                expected: Dispatched::Ran(Outcome::Value(OutcomeValue::Text(oversized_text()))),
            },
            OutcomeFixture {
                name: "absent-required",
                id: id("absent-required"),
                args: ArgValues::new(),
                expected: Dispatched::Ran(Outcome::Rejected(Rejection {
                    binder: "id",
                    mode: RejectionMode::Absent,
                    detail: "no value supplied for a required binder".to_string(),
                })),
            },
            OutcomeFixture {
                // The residual this fixture guards: a store failure reported
                // as an empty, success-shaped result is indistinguishable
                // from "there was nothing to return" to any caller that
                // only checks for success. `Unavailable` must never collapse
                // into `Value(Empty)`.
                name: "unavailable-resource",
                id: id("unavailable-resource"),
                args: ArgValues::new(),
                expected: Dispatched::Ran(Outcome::Unavailable {
                    reason: "conformance fixture: resource unavailable".to_string(),
                }),
            },
            OutcomeFixture {
                name: "secret-bearing",
                id: id("secret-bearing"),
                args: ArgValues::new(),
                expected: Dispatched::Ran(Outcome::Value(OutcomeValue::Text(
                    "token=*** ready".to_string(),
                ))),
            },
            OutcomeFixture {
                name: "boundary-crossing",
                id: id("boundary-crossing"),
                args: ArgValues::new(),
                expected: Dispatched::Ran(Outcome::Value(OutcomeValue::Record(vec![(
                    "tags".to_string(),
                    OutcomeValue::List(vec![OutcomeValue::Text("token=***".to_string())]),
                )]))),
            },
        ],
    }
}

/// Ten thousand characters — large enough that a fixed-size buffer, a
/// truncating renderer, or a lossy encoding step would visibly clip it,
/// without costing real time to construct or compare.
fn oversized_text() -> String {
    "x".repeat(10_000)
}

/// A retired invocable and the replacement it names (the declared-
/// retirement rule): still registered and still resolvable — never a silent
/// unknown-command failure — but off the canonical shipped set, since
/// `Stability` is `Shipped` xor `Retired`, never both. Not part of
/// [`Corpus::canonical`] for that reason; exported separately for a
/// surface's own retirement-aware test to register and drive.
pub fn retired_pair() -> (Invocable, Invocable) {
    let replacement = no_binders_descriptor(
        "new-verb",
        "New verb",
        "The replacement a retired verb names.",
    );
    let retired = Invocable {
        stability: Stability::Retired {
            superseded_by: replacement.id.clone(),
        },
        ..no_binders_descriptor(
            "legacy-verb",
            "Legacy verb",
            "Retired; superseded by new-verb.",
        )
    };
    (retired, replacement)
}

/// Two invocables under two different identities sharing one bare tail
/// (the declared *contribute* collision rule and the identity
/// namespace) — a standard, shared pair so multiple surfaces test the
/// identical shadowing scenario rather than each inventing its own. Which
/// one wins the bare form depends on registration order (the earlier
/// registrant wins when neither is core), which this crate does not
/// control; the pairing itself, and asserting the shadow/reclaim mechanism,
/// is domain-tier behavior already proven directly against
/// `InvocableRegistry` — this data exists so a surface's own catalog
/// rendering can be checked against a shared scenario, not to re-prove the
/// registry mechanism here.
pub fn shadow_pair() -> (Invocable, Invocable) {
    let first = Invocable {
        id: InvocableId::new("conformance-a:shared-tail").expect("well-formed fixture identity"),
        ..no_binders_descriptor("shared-tail", "Shared tail (a)", "First registrant.")
    };
    let second = Invocable {
        id: InvocableId::new("conformance-b:shared-tail").expect("well-formed fixture identity"),
        ..no_binders_descriptor("shared-tail", "Shared tail (b)", "Second registrant.")
    };
    (first, second)
}
