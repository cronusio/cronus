//! Office deliberation — orchestrator-initiated multi-worker debate.
//!
//! A round collects independent arguments (gathered in parallel by the
//! orchestration wave; here they arrive pre-generated so no participant reads
//! another before synthesis), the orchestrator synthesizes a final decision
//! with attribution (no vote), and the round is appended immutably to the
//! deliberation log. Over-budget arguments are truncated with a visible
//! marker. Participant selection maximizes specialty diversity.
//!
//! The parallel argument generation and the inbox-backed log store are seams; the
//! algebra of the round is implemented and tested here.

/// One participant's independent argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Argument {
    pub role: String,
    pub model: String,
    pub position_summary: String,
    pub key_points: Vec<String>,
    /// 0–100 self-declared confidence.
    pub confidence: u8,
    /// Whether this argument was cut short by the budget.
    pub truncated: bool,
}

/// An immutable record of one closed deliberation round.
#[derive(Debug, Clone)]
pub struct RoundEntry {
    pub round_id: String,
    pub question: String,
    pub arguments: Vec<Argument>,
    pub decision: String,
    pub reasoning: String,
    pub token_budget: u32,
    /// Whether any argument was truncated.
    pub truncated: bool,
}

/// The append-only deliberation log. Exposes append + read; no update or
/// delete path exists, so a past decision cannot be retroactively edited.
#[derive(Debug, Default)]
pub struct DeliberationLog {
    entries: Vec<RoundEntry>,
}

impl DeliberationLog {
    pub fn new() -> Self {
        DeliberationLog::default()
    }

    fn append(&mut self, entry: RoundEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[RoundEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Select up to `n` participants maximizing distinct specialty coverage.
/// Preserves input order among first-seen specialties; a repeated specialty is
/// skipped until distinct ones are exhausted.
pub fn select_participants(candidates: &[(String, String)], n: usize) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut chosen: Vec<(String, String)> = Vec::new();
    // First pass: distinct specialties.
    for (role, specialty) in candidates {
        if chosen.len() == n {
            break;
        }
        if seen.insert(specialty.clone()) {
            chosen.push((role.clone(), specialty.clone()));
        }
    }
    // Second pass: fill remaining slots with repeats if needed.
    if chosen.len() < n {
        for (role, specialty) in candidates {
            if chosen.len() == n {
                break;
            }
            if !chosen.iter().any(|(r, _)| r == role) {
                chosen.push((role.clone(), specialty.clone()));
            }
        }
    }
    chosen
}

/// Truncate an argument to a per-argument key-point budget, marking it.
pub fn apply_budget(mut arg: Argument, max_points: usize) -> Argument {
    if arg.key_points.len() > max_points {
        arg.key_points.truncate(max_points);
        arg.truncated = true;
    }
    arg
}

/// Run a deliberation round: the orchestrator synthesizes the pre-generated
/// independent arguments into a decision (no vote), and the round is appended
/// immutably to the log. Returns the decision string.
pub fn run_round(
    log: &mut DeliberationLog,
    round_id: &str,
    question: &str,
    arguments: Vec<Argument>,
    token_budget: u32,
    decision: &str,
    reasoning: &str,
) -> String {
    let truncated = arguments.iter().any(|a| a.truncated);
    log.append(RoundEntry {
        round_id: round_id.to_string(),
        question: question.to_string(),
        arguments,
        decision: decision.to_string(),
        reasoning: reasoning.to_string(),
        token_budget,
        truncated,
    });
    decision.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(role: &str, points: &[&str]) -> Argument {
        Argument {
            role: role.to_string(),
            model: "model-x".to_string(),
            position_summary: format!("{role}'s position"),
            key_points: points.iter().map(|s| s.to_string()).collect(),
            confidence: 70,
            truncated: false,
        }
    }

    #[test]
    fn participant_selection_maximizes_specialty_diversity() {
        // Distinct specialties are preferred over repeats.
        let candidates = vec![
            ("alice".into(), "backend".into()),
            ("bob".into(), "backend".into()),
            ("carol".into(), "security".into()),
            ("dan".into(), "frontend".into()),
        ];
        let chosen = select_participants(&candidates, 3);
        let specialties: Vec<&str> = chosen.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(specialties, vec!["backend", "security", "frontend"]);
    }

    #[test]
    fn budget_truncates_argument_with_marker() {
        // An over-budget argument is truncated and marked, not dropped.
        let a = arg("alice", &["p1", "p2", "p3", "p4"]);
        let bounded = apply_budget(a, 2);
        assert_eq!(bounded.key_points.len(), 2);
        assert!(bounded.truncated);
    }

    #[test]
    fn round_appends_immutably_with_orchestrator_decision() {
        // The orchestrator's decision is recorded; log is append-only.
        let mut log = DeliberationLog::new();
        let args = vec![arg("alice", &["use sqlite"]), arg("bob", &["use postgres"])];
        let decision = run_round(
            &mut log,
            "r1",
            "which datastore?",
            args,
            1000,
            "sqlite — local-first fits the constraint",
            "alice's local-first point outweighed bob's scale point",
        );
        assert_eq!(decision, "sqlite — local-first fits the constraint");
        assert_eq!(log.len(), 1);
        let entry = &log.entries()[0];
        assert_eq!(entry.arguments.len(), 2);
        assert!(!entry.truncated);
        // A second round appends; the first entry is unchanged.
        run_round(&mut log, "r2", "q2", vec![], 500, "d2", "r2");
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[0].round_id, "r1");
    }

    #[test]
    fn round_records_truncation_flag() {
        let mut log = DeliberationLog::new();
        let truncated_arg = apply_budget(arg("alice", &["p1", "p2", "p3"]), 1);
        run_round(&mut log, "r1", "q", vec![truncated_arg], 100, "d", "r");
        assert!(log.entries()[0].truncated);
    }
}
