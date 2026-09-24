//! What a finished run actually means: the outcome computed from obligation
//! verdicts alone (a discovery can never fail a run), and the
//! attribution that keeps a harness or environment defect from being filed
//! as a product finding.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The five outcomes a `finish` can produce. Deliberately more than
/// pass/fail: a run that never finished deciding, or that turned out to be
/// measuring something other than the product, is a different kind of
/// result and must be reported as one rather than folded into either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// Every declared obligation was decided, and all passed.
    Pass,
    /// Every declared obligation was decided, and at least one failed.
    Fail,
    /// At least one declared obligation was never decided — the run's
    /// bound was exhausted, or it was interrupted, before the actor
    /// finished judging everything.
    Incomplete,
    /// The repository's own tracked-file dirtiness changed between world
    /// build and `finish` — something wrote outside the world. Verdicts
    /// are discarded regardless of what they said.
    Contaminated,
    /// The harness or its environment failed (a world could not be torn
    /// down, for example) — not the product. Reported `void`, and must
    /// never be filed as a product finding.
    Environment,
}

impl RunOutcome {
    /// The process exit code `cronus-sim finish` reports for this outcome.
    /// A stable, small, disjoint set: a caller scripting against this
    /// binary can branch on it without parsing the report text.
    pub fn exit_code(self) -> i32 {
        match self {
            RunOutcome::Pass => 0,
            RunOutcome::Fail => 1,
            RunOutcome::Incomplete => 2,
            RunOutcome::Contaminated => 3,
            RunOutcome::Environment => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RunOutcome::Pass => "pass",
            RunOutcome::Fail => "fail",
            RunOutcome::Incomplete => "incomplete",
            RunOutcome::Contaminated => "contaminated",
            RunOutcome::Environment => "void",
        }
    }
}

/// The improvement-loop's own five-name taxonomy —
/// reused verbatim rather than invented a second time:
/// only `Defect` has anywhere further to go — the existing pinning
/// path, driven by a human running `pin`, unchanged by this enum's
/// existence. The other four name something that failed no stated
/// contract, so they stop at the report for a human to weigh.
///
/// Closed by construction: [`DiscoveryClass::parse`] rejects anything
/// outside these five rather than coercing an unrecognized spelling to the
/// nearest guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoveryClass {
    Defect,
    Friction,
    Inefficiency,
    OptimizationOpportunity,
    ImprovementIdea,
}

impl DiscoveryClass {
    /// All five variants, in the order the improvement loop lists
    /// them. Used both to parse and to render the closed vocabulary a
    /// caller can choose from.
    pub const ALL: [DiscoveryClass; 5] = [
        DiscoveryClass::Defect,
        DiscoveryClass::Friction,
        DiscoveryClass::Inefficiency,
        DiscoveryClass::OptimizationOpportunity,
        DiscoveryClass::ImprovementIdea,
    ];

    /// The name this class parses from and renders as — identical to its
    /// serialized JSON form (asserted by a unit test below), so the CLI
    /// surface and the persisted record can never drift apart.
    pub fn name(self) -> &'static str {
        match self {
            DiscoveryClass::Defect => "defect",
            DiscoveryClass::Friction => "friction",
            DiscoveryClass::Inefficiency => "inefficiency",
            DiscoveryClass::OptimizationOpportunity => "optimization-opportunity",
            DiscoveryClass::ImprovementIdea => "improvement-idea",
        }
    }

    /// Parse a class name exactly as `name()` renders it. `None` for
    /// anything else — a class the taxonomy did not name is a usage error
    /// for the caller to correct, never a value silently substituted.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.name() == name)
    }

    /// The five valid names, comma-joined in declaration order — for a
    /// usage-error message that shows the closed vocabulary rather than
    /// leaving the caller to guess at it.
    pub fn all_names_joined() -> String {
        Self::ALL
            .iter()
            .map(|class| class.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for DiscoveryClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// One recorded observation from a run.
///
/// `class` and `remedy` are never consulted when computing an
/// outcome — a discovery is information, not an obligation, classified or
/// not. A `remedy` is a claim for a human to weigh, never an
/// act this record performs or implies was performed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Discovery {
    pub text: String,
    /// Optional: a discovery is not required to carry a class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<DiscoveryClass>,
    /// Optional: what the reporter judges would close the gap —
    /// a claim, never an applied change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

impl Discovery {
    /// An unclassified discovery carrying no proposed remedy — the shape
    /// every `note` produced before it gained two more fields to fill.
    pub fn plain(text: String) -> Self {
        Discovery {
            text,
            class: None,
            remedy: None,
        }
    }
}

/// The report `cronus-sim finish` prints and tears its world down after
/// producing.
#[derive(Debug, Clone)]
pub struct FinishReport {
    pub world_id: String,
    pub outcome: RunOutcome,
    /// Obligation ids left undecided — always empty unless
    /// `outcome == Incomplete`.
    pub undecided_obligations: Vec<String>,
    /// Discoveries recorded by `note`, carried through for the record.
    /// Never consulted to compute `outcome`.
    pub notes: Vec<Discovery>,
}

impl fmt::Display for FinishReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "world: {}", self.world_id)?;
        writeln!(f, "outcome: {}", self.outcome.label())?;
        if !self.undecided_obligations.is_empty() {
            writeln!(
                f,
                "undecided obligations: {}",
                self.undecided_obligations.join(", ")
            )?;
        }
        if !self.notes.is_empty() {
            writeln!(f, "notes:")?;
            for note in &self.notes {
                writeln!(f, "  - {}", note.text)?;
                if let Some(class) = note.class {
                    writeln!(f, "    class: {class}")?;
                }
                if let Some(remedy) = &note.remedy {
                    writeln!(f, "    proposed remedy (not applied): {remedy}")?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_five_class_names_parse_back_to_the_variant_that_named_them() {
        for class in DiscoveryClass::ALL {
            assert_eq!(DiscoveryClass::parse(class.name()), Some(class));
        }
    }

    #[test]
    fn an_unrecognized_class_name_is_rejected_rather_than_coerced() {
        assert_eq!(DiscoveryClass::parse("bug"), None);
        assert_eq!(DiscoveryClass::parse(""), None);
        assert_eq!(DiscoveryClass::parse("Defect"), None); // case-sensitive
    }

    #[test]
    fn every_class_survives_a_json_round_trip_under_its_own_name() {
        for class in DiscoveryClass::ALL {
            let json = serde_json::to_string(&class).expect("serialize must succeed");
            assert_eq!(json, format!("\"{}\"", class.name()));
            let back: DiscoveryClass =
                serde_json::from_str(&json).expect("deserialize must succeed");
            assert_eq!(back, class);
        }
    }

    #[test]
    fn all_names_joined_names_every_one_of_the_five_classes() {
        let joined = DiscoveryClass::all_names_joined();
        for class in DiscoveryClass::ALL {
            assert!(
                joined.contains(class.name()),
                "{joined} must mention {}",
                class.name()
            );
        }
    }

    #[test]
    fn a_plain_discovery_carries_no_class_or_remedy() {
        let d = Discovery::plain("something happened".to_string());
        assert_eq!(d.text, "something happened");
        assert_eq!(d.class, None);
        assert_eq!(d.remedy, None);
    }

    #[test]
    fn a_discovery_json_object_lacking_class_and_remedy_keys_still_deserializes() {
        let json = r#"{"text": "an old-shape discovery"}"#;
        let d: Discovery = serde_json::from_str(json).expect("must deserialize with defaults");
        assert_eq!(d.text, "an old-shape discovery");
        assert_eq!(d.class, None);
        assert_eq!(d.remedy, None);
    }

    #[test]
    fn a_fully_classified_discovery_round_trips_through_json_intact() {
        let d = Discovery {
            text: "the scaffold truncates hyphenated names".to_string(),
            class: Some(DiscoveryClass::Defect),
            remedy: Some("stop splitting on the first hyphen".to_string()),
        };
        let json = serde_json::to_string(&d).expect("serialize must succeed");
        let back: Discovery = serde_json::from_str(&json).expect("deserialize must succeed");
        assert_eq!(back, d);
    }
}
