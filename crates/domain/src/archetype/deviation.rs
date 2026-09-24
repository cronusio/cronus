//! Deviation recording: the three counters that make an
//! archetype falsifiable, and the load-bearing discipline that the candidate
//! **pool is read by the recorder, never by the hire gate**.
//!
//! Inverting that order — consulting the pool *before* the gate — would
//! reconstruct the fixed org chart the concept rejects. Here the recorder's
//! only method that touches the pool, `record_hire`, returns `()`: it cannot
//! refuse a hire, only classify one after the gate has already allowed it.

/// One office's deviation counters, attributed to whichever archetype was
/// active while the office ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OfficeDeviations {
    pub hired_outside_pool: u32,
    pub seeded_never_worked: u32,
    pub shape_never_grown: u32,
}

impl OfficeDeviations {
    /// Classify a hire the gate has **already allowed**. The pool is
    /// consulted only to record whether the hire fell outside the archetype's
    /// anticipated pool — this method has no return value, so it *cannot*
    /// refuse the hire, and the pool identifier reaches it only here, never
    /// the gate. A hire outside the pool is recorded and proceeds.
    pub fn record_hire(&mut self, pool: &[String], hired_role: &str) {
        if !pool.iter().any(|r| r == hired_role) {
            self.hired_outside_pool += 1;
        }
    }

    /// A seeded role was released, or the office closed, having received no
    /// delegated task (the seed seated someone first-contact work did not need).
    pub fn record_seeded_never_worked(&mut self) {
        self.seeded_never_worked += 1;
    }

    /// The office closed with no department layer introduced (the shape planned
    /// a structure the work never required).
    pub fn record_shape_never_grown(&mut self) {
        self.shape_never_grown += 1;
    }
}

/// An archetype's validation status. The distinction between "no
/// deviations recorded" and "no offices observed" is the distinction between
/// a validated prior and a guess — collapsing them would let an unexamined
/// default accumulate authority it never earned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationStatus {
    /// No office has ever been observed under this archetype — a guess, not a
    /// validated prior.
    Unvalidated,
    /// At least one office ran under this archetype; the aggregate counters.
    Validated {
        offices_observed: u32,
        hired_outside_pool: u32,
        seeded_never_worked: u32,
        shape_never_grown: u32,
    },
}

/// The per-archetype aggregate: signals sum across every office that adopted
/// the archetype, keyed by the archetype identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchetypeDeviations {
    offices_observed: u32,
    hired_outside_pool: u32,
    seeded_never_worked: u32,
    shape_never_grown: u32,
}

impl ArchetypeDeviations {
    /// Fold one observed office's counters into the archetype's aggregate.
    /// Observing an office — even one with zero deviations — is what moves the
    /// archetype from `Unvalidated` to `Validated`.
    pub fn absorb(&mut self, office: &OfficeDeviations) {
        self.offices_observed += 1;
        self.hired_outside_pool += office.hired_outside_pool;
        self.seeded_never_worked += office.seeded_never_worked;
        self.shape_never_grown += office.shape_never_grown;
    }

    /// Report the validation status — `Unvalidated` until at least one
    /// office has been observed, never "correct" by default.
    pub fn status(&self) -> ValidationStatus {
        if self.offices_observed == 0 {
            ValidationStatus::Unvalidated
        } else {
            ValidationStatus::Validated {
                offices_observed: self.offices_observed,
                hired_outside_pool: self.hired_outside_pool,
                seeded_never_worked: self.seeded_never_worked,
                shape_never_grown: self.shape_never_grown,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn se_pool() -> Vec<String> {
        ["architect", "backend-engineer", "code-reviewer"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    // --- A hire outside the pool succeeds and is recorded, never refused

    #[test]
    fn a_hire_outside_the_pool_is_recorded_and_never_refused() {
        let mut dev = OfficeDeviations::default();
        // `record_hire` returns `()` — there is no channel through which it
        // could refuse. A hire outside the pool proceeds and is classified.
        dev.record_hire(&se_pool(), "marketing"); // not in the pool
        assert_eq!(dev.hired_outside_pool, 1);
    }

    #[test]
    fn a_hire_inside_the_pool_records_no_deviation() {
        let mut dev = OfficeDeviations::default();
        dev.record_hire(&se_pool(), "architect"); // in the pool
        assert_eq!(dev.hired_outside_pool, 0);
    }

    // --- Unvalidated until an office is observed -----------------------

    #[test]
    fn an_archetype_with_no_observed_offices_reports_unvalidated_not_correct() {
        let agg = ArchetypeDeviations::default();
        assert_eq!(agg.status(), ValidationStatus::Unvalidated);
    }

    #[test]
    fn observing_a_clean_office_makes_the_archetype_validated_with_zero_deviations() {
        let mut agg = ArchetypeDeviations::default();
        agg.absorb(&OfficeDeviations::default()); // observed, no deviations
        assert_eq!(
            agg.status(),
            ValidationStatus::Validated {
                offices_observed: 1,
                hired_outside_pool: 0,
                seeded_never_worked: 0,
                shape_never_grown: 0,
            }
        );
    }

    // --- Counters aggregate per archetype identity across offices -----

    #[test]
    fn counters_aggregate_across_every_office_that_adopted_the_archetype() {
        let mut office_a = OfficeDeviations::default();
        office_a.record_hire(&se_pool(), "marketing");
        office_a.record_shape_never_grown();

        let mut office_b = OfficeDeviations::default();
        office_b.record_hire(&se_pool(), "finance");
        office_b.record_seeded_never_worked();

        let mut agg = ArchetypeDeviations::default();
        agg.absorb(&office_a);
        agg.absorb(&office_b);

        assert_eq!(
            agg.status(),
            ValidationStatus::Validated {
                offices_observed: 2,
                hired_outside_pool: 2,
                seeded_never_worked: 1,
                shape_never_grown: 1,
            }
        );
    }
}
