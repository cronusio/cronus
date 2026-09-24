//! Objective persistence across in-session reduction. Two runtime
//! shapes share one durable-slot principle:
//! discrete iterations already reconstruct fresh from plan/status each time;
//! a continuous-session loop that compacts *in place* instead
//! re-projects the standing objective + progress cursor into every turn
//! from a durable `ObjectiveSlot`, so mid-session compaction can never drop
//! the north-star. Composes the shipped CC-9 protected-region mechanism
//! (`context_mgmt::trim_cascade`) rather than reinventing eviction.

use cronus_contract::ContextEntry;

use crate::loop_runner::spec::ObjectiveSlot;

/// The role tag identifying the objective entry among a turn's context, so
/// `re_project_objective` can find it without depending on its rendered text.
const OBJECTIVE_ROLE: &str = "objective_slot";

/// Render an `ObjectiveSlot` as a protected context entry (CC-9): a turn
/// that carries this entry can never have it removed by `trim_cascade`,
/// regardless of how aggressively the rest of the transcript is trimmed.
pub fn objective_context_entry(slot: &ObjectiveSlot) -> ContextEntry {
    ContextEntry::new(
        OBJECTIVE_ROLE,
        format!("Objective: {}\nProgress: {}", slot.objective, slot.progress),
        0,
    )
    .protect()
}

/// Update the durable progress cursor. Callers MUST do this before running
/// any lossy reduction (CC-10) — the slot, not the compacted transcript, is
/// what the next turn resumes from.
pub fn update_progress(slot: &mut ObjectiveSlot, progress: impl Into<String>) {
    slot.progress = progress.into();
}

/// Re-materialize the objective entry into `entries` if it is missing — a
/// freshly reconstructed turn that has not yet included it. A turn that
/// already carries the entry is left untouched (never duplicated). This is
/// the fallback the L1 spec's "re-projected if evicted" clause names, on top
/// of `protect()`'s structural guarantee that `trim_cascade` itself never
/// removes it once present.
pub fn re_project_objective(entries: &mut Vec<ContextEntry>, slot: &ObjectiveSlot) {
    let already_present = entries.iter().any(|entry| entry.role == OBJECTIVE_ROLE);
    if !already_present {
        entries.push(objective_context_entry(slot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_mgmt::trim_cascade;
    use cronus_contract::TrimPriority;

    fn chatter(n: u32) -> ContextEntry {
        ContextEntry::new("user", format!("turn {n} chatter"), 100)
            .with_priority(TrimPriority::NonProtectedUser)
    }

    fn slot(progress: &str) -> ObjectiveSlot {
        ObjectiveSlot {
            objective: "keep the release green".to_string(),
            progress: progress.to_string(),
        }
    }

    // --- CC-9: the objective survives aggressive trimming -------------------

    #[test]
    fn the_objective_entry_survives_trimming_that_evicts_everything_else() {
        let s = slot("3 of 5 checks passing");
        let mut entries = vec![
            objective_context_entry(&s),
            chatter(1),
            chatter(2),
            chatter(3),
        ];
        trim_cascade(&mut entries, 0); // force maximum eviction
        assert_eq!(entries.len(), 1);
        assert!(entries[0].protected);
        assert!(entries[0].body.contains("3 of 5 checks passing"));
    }

    // --- resumability: the durable slot, not stale rendered text, wins -----

    #[test]
    fn re_projecting_after_compaction_reflects_the_current_progress_not_the_old_one() {
        let mut s = slot("3 of 5 checks passing");
        let mut first_turn: Vec<ContextEntry> = vec![chatter(1)]; // objective not yet included
        re_project_objective(&mut first_turn, &s);
        assert!(
            first_turn
                .iter()
                .any(|e| e.role == "objective_slot" && e.body.contains("3 of 5"))
        );

        // Progress advances before the next lossy reduction runs (CC-10 timing).
        update_progress(&mut s, "5 of 5 checks passing — release green");

        // Simulate a compaction event that dropped the entire prior turn.
        let mut next_turn: Vec<ContextEntry> = Vec::new();
        re_project_objective(&mut next_turn, &s);
        let objective_entry = next_turn
            .iter()
            .find(|e| e.role == "objective_slot")
            .expect("objective entry re-materialized after eviction");
        assert!(objective_entry.body.contains("5 of 5 checks passing"));
        assert!(!objective_entry.body.contains("3 of 5"));
    }

    #[test]
    fn re_projecting_is_a_no_op_when_the_objective_entry_is_already_present() {
        let s = slot("y");
        let mut entries = vec![objective_context_entry(&s)];
        re_project_objective(&mut entries, &s);
        assert_eq!(entries.len(), 1, "the entry is never duplicated");
    }

    // --- an absent ObjectiveSlot means there is nothing to re-project ------
    //
    // Covered at the type level by spec::tests::
    // a_loop_spec_with_no_objective_slot_is_a_discrete_iteration_loop
    // `LoopSpec.objective_slot: Option<ObjectiveSlot>` being
    // `None` means a caller simply never calls `objective_context_entry` /
    // `re_project_objective` — fresh-context-per-iteration governs
    // instead. Not re-tested here to avoid duplicating that assertion.
}
