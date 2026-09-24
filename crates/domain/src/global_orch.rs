//! Global orchestration — the building-level coordinator (the home workspace
//! manager), above individual offices.
//!
//! One coordinator per building. It maintains a read-only aggregate view of
//! all offices fed by their state events, routes cross-office messages over
//! the ACP relay bypassing paused/hibernating offices, annotates new-
//! component cards with the phase's mandatory cross-cutting concerns, and
//! escalates conflicts. It never cancels or re-delegates an office's active
//! work — this module exposes no such path.
//!
//! The ACP relay transport and the event-bus subscription are seams; the aggregate,
//! routing decision, phase-concern annotation, and escalation algebra are here.

use crate::office_control::OfficeState;
use std::collections::HashMap;

/// A read-only per-office summary in the building aggregate view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeSummary {
    pub office_id: String,
    pub state: OfficeState,
    pub active_cards: u32,
    pub blocked_cards: u32,
    pub budget_today_cents: u64,
    pub active_sessions: u32,
}

/// A mandatory cross-cutting concern in the phase-awareness catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Concern {
    Localization,
    Observability,
    Security,
    Accessibility,
    ErrorHandling,
    BudgetSafety,
}

/// How the coordinator resolves an escalation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Escalation {
    /// Resolved directly within the coordinator's authority.
    ResolveDirect,
    /// A cross-office deliberation round among affected offices' orchestrators.
    CrossOfficeDeliberation(Vec<String>),
    /// Escalated to the user (HITL).
    Hitl,
}

/// The building-level coordinator. Sole coordinator; read-only toward
/// offices — it forwards routing/escalation as messages, never mutates an
/// office's state and offers no cancel/re-delegate path.
#[derive(Debug, Default)]
pub struct BuildingView {
    offices: HashMap<String, OfficeSummary>,
}

impl BuildingView {
    pub fn new() -> Self {
        BuildingView::default()
    }

    /// Ingest an office summary from the event bus. Read-only projection —
    /// the office remains the sole writer of its own state.
    pub fn observe(&mut self, summary: OfficeSummary) {
        self.offices.insert(summary.office_id.clone(), summary);
    }

    pub fn summary(&self, office_id: &str) -> Option<&OfficeSummary> {
        self.offices.get(office_id)
    }

    pub fn office_count(&self) -> usize {
        self.offices.len()
    }

    /// Total active cards across the building (aggregate view).
    pub fn total_active_cards(&self) -> u32 {
        self.offices.values().map(|o| o.active_cards).sum()
    }

    /// Route a message to the first reachable candidate office. A
    /// Paused/Hibernating/Offline/Error office is bypassed. Order = caller priority.
    pub fn route(&self, candidates: &[String]) -> Option<String> {
        candidates
            .iter()
            .find(|id| self.offices.get(*id).is_some_and(|o| is_reachable(o.state)))
            .cloned()
    }
}

/// Whether an office in `state` can receive routed work.
pub fn is_reachable(state: OfficeState) -> bool {
    matches!(state, OfficeState::Active | OfficeState::Idle)
}

/// The phase-awareness check: the mandatory concerns for the current phase
/// that are also relevant to the component become non-optional acceptance criteria
/// on the new card. Returns them in a stable order.
pub fn check_phase_concerns(mandatory: &[Concern], relevant: &[Concern]) -> Vec<Concern> {
    let relevant_set: std::collections::HashSet<Concern> = relevant.iter().copied().collect();
    mandatory
        .iter()
        .copied()
        .filter(|c| relevant_set.contains(c))
        .collect()
}

/// Decide an escalation path. A conflict spanning multiple offices with an
/// objective resolution resolves directly; a design conflict requests a cross-office
/// deliberation; an ambiguous or high-impact one escalates to the human.
pub fn decide_escalation(
    affected_offices: &[String],
    needs_human: bool,
    resolvable_directly: bool,
) -> Escalation {
    if needs_human {
        Escalation::Hitl
    } else if resolvable_directly {
        Escalation::ResolveDirect
    } else {
        Escalation::CrossOfficeDeliberation(affected_offices.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn office(id: &str, state: OfficeState, active: u32) -> OfficeSummary {
        OfficeSummary {
            office_id: id.to_string(),
            state,
            active_cards: active,
            blocked_cards: 0,
            budget_today_cents: 0,
            active_sessions: 1,
        }
    }

    #[test]
    fn aggregate_view_is_read_only_projection() {
        // The view aggregates observed office summaries.
        let mut bv = BuildingView::new();
        bv.observe(office("a", OfficeState::Active, 3));
        bv.observe(office("b", OfficeState::Idle, 2));
        assert_eq!(bv.office_count(), 2);
        assert_eq!(bv.total_active_cards(), 5);
        assert_eq!(bv.summary("a").unwrap().state, OfficeState::Active);
    }

    #[test]
    fn routing_bypasses_unreachable_offices() {
        // paused/hibernating offices are skipped; order is caller priority.
        let mut bv = BuildingView::new();
        bv.observe(office("a", OfficeState::Hibernating, 0));
        bv.observe(office("b", OfficeState::Paused, 0));
        bv.observe(office("c", OfficeState::Active, 1));
        let target = bv.route(&["a".into(), "b".into(), "c".into()]);
        assert_eq!(target.as_deref(), Some("c"));
        // No reachable candidate -> None.
        assert_eq!(bv.route(&["a".into(), "b".into()]), None);
    }

    #[test]
    fn reachability_matches_running_states() {
        assert!(is_reachable(OfficeState::Active));
        assert!(is_reachable(OfficeState::Idle));
        assert!(!is_reachable(OfficeState::Paused));
        assert!(!is_reachable(OfficeState::Hibernating));
        assert!(!is_reachable(OfficeState::Error));
        assert!(!is_reachable(OfficeState::Offline));
    }

    #[test]
    fn phase_concerns_are_mandatory_intersection() {
        // Only concerns both mandatory for the phase AND relevant apply.
        let mandatory = [
            Concern::Localization,
            Concern::Observability,
            Concern::Security,
        ];
        let relevant = [Concern::Security, Concern::Accessibility];
        let applied = check_phase_concerns(&mandatory, &relevant);
        assert_eq!(applied, vec![Concern::Security]);
    }

    #[test]
    fn escalation_paths_resolve_by_inputs() {
        assert_eq!(
            decide_escalation(&["a".into()], true, false),
            Escalation::Hitl
        );
        assert_eq!(
            decide_escalation(&["a".into()], false, true),
            Escalation::ResolveDirect
        );
        assert_eq!(
            decide_escalation(&["a".into(), "b".into()], false, false),
            Escalation::CrossOfficeDeliberation(vec!["a".into(), "b".into()])
        );
    }
}
