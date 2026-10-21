//! The fourth obligation of installation verbs: one outcome, two renderings.
//!
//! The command line answers an installation verb with an exit signal and
//! printed text; the terminal UI answers with an inline block. How each shows
//! and asks is legitimately per surface (the do-not-unify record); what the
//! answer *means* is not. A fixture here names a verb, the state of the
//! installation it runs against, the signal every rendering must give —
//! succeeded, unavailable, or rejected — and the facts every rendering must
//! carry. A surface reduces its own rendering to a [`Shown`] and the harness
//! holds it to the fixture, so two surfaces held to one fixture agree with
//! each other without either having to know how the other prints.
//!
//! The wording of an empty answer is deliberately not a fact: "No results."
//! and "(none)" are two correct renderings of one outcome. The distinction
//! that matters — empty versus unavailable — is the signal.

use crate::report::{ConformanceReport, RenderingProblem};

/// What a rendering tells its reader about how the verb went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The verb answered; an empty answer is still this.
    Success,
    /// The verb ran and could not answer (a store that cannot be read, a
    /// workspace that does not exist yet). Never shown as an empty success.
    Unavailable,
    /// An argument failed to bind; the verb did not run.
    Rejected,
}

/// The installation a fixture runs against. The surface under test prepares
/// it, in its own way, before showing the verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationState {
    /// A fresh state tier: nothing has been initialized.
    Fresh,
    /// A workspace has been initialized.
    Initialized,
}

/// One installation verb run against one state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationFixture {
    /// Stable across runs; identifies the fixture in a report.
    pub name: &'static str,
    pub state: InstallationState,
    /// The verb as the words that follow the program or the slash, group
    /// first: `["workspace", "list"]`.
    pub words: &'static [&'static str],
    /// The signal every rendering must give.
    pub signal: Signal,
    /// Text every rendering must contain (compared without regard to case).
    pub facts: &'static [&'static str],
}

/// A surface's rendering of one fixture, reduced to what the corpus compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub signal: Signal,
    /// Everything the rendering shows the reader.
    pub text: String,
}

/// A surface that can show an installation fixture. The implementation runs
/// the verb through the surface's own real path — the program for the command
/// line, the composed registry for the terminal UI — never a restatement.
pub trait InstallationRendering {
    fn show(&self, fixture: &InstallationFixture) -> Shown;
}

/// The shared installation fixtures: the read-only verbs, each against a
/// fresh state tier and an initialized one. Written from the divergence: the
/// two states are where an unavailable answer and an empty one are most
/// easily confused, and the listing verbs are where two renderings most
/// easily disagree about whether anything was there.
pub fn installation_fixtures() -> Vec<InstallationFixture> {
    use InstallationState::{Fresh, Initialized};
    use Signal::{Success, Unavailable};
    let fixture = |name, state, words, signal, facts| InstallationFixture {
        name,
        state,
        words,
        signal,
        facts,
    };
    vec![
        fixture(
            "status-without-a-workspace",
            Fresh,
            &["status"],
            Unavailable,
            &["No workspace initialized"],
        ),
        fixture(
            "doctor-without-a-workspace",
            Fresh,
            &["doctor"],
            Unavailable,
            &["No workspace initialized"],
        ),
        fixture(
            "workspace-list-with-none",
            Fresh,
            &["workspace", "list"],
            Success,
            &[],
        ),
        fixture(
            "backup-list-with-none",
            Fresh,
            &["backup", "list"],
            Success,
            &[],
        ),
        fixture("ext-list-with-none", Fresh, &["ext", "list"], Success, &[]),
        fixture(
            "registry-list",
            Fresh,
            &["registry", "list"],
            Success,
            &["plan", "refactor"],
        ),
        fixture(
            "activation-status-when-never-enabled",
            Fresh,
            &["activation", "status"],
            Success,
            &["inactive"],
        ),
        fixture(
            "archetype-list-names-the-blocked-ones",
            Fresh,
            &["archetype", "list"],
            Success,
            &["software-engineering", "advertising-agency"],
        ),
        fixture(
            "dev-status-when-absent",
            Fresh,
            &["dev", "status"],
            Success,
            &["absent"],
        ),
        fixture(
            "status-after-init",
            Initialized,
            &["status"],
            Success,
            &["ready"],
        ),
        fixture(
            "doctor-after-init",
            Initialized,
            &["doctor"],
            Success,
            &["all checks passed"],
        ),
        fixture(
            "backup-list-after-init",
            Initialized,
            &["backup", "list"],
            Success,
            &[],
        ),
    ]
}

/// Hold `rendering` to every fixture: the signal it gives must be the
/// fixture's, and the text it shows must carry the fixture's facts. Reports
/// every divergence, not the first.
pub fn check_installation_renderings(
    rendering: &dyn InstallationRendering,
    fixtures: &[InstallationFixture],
) -> Vec<ConformanceReport> {
    let mut reports = Vec::new();
    for fixture in fixtures {
        let shown = rendering.show(fixture);
        if shown.signal != fixture.signal {
            reports.push(ConformanceReport::Rendering {
                fixture: fixture.name,
                problem: RenderingProblem::Signal {
                    expected: fixture.signal,
                    actual: shown.signal,
                },
            });
        }
        let text = shown.text.to_lowercase();
        for fact in fixture.facts {
            if !text.contains(&fact.to_lowercase()) {
                reports.push(ConformanceReport::Rendering {
                    fixture: fixture.name,
                    problem: RenderingProblem::MissingFact(fact),
                });
            }
        }
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rendering that answers each fixture with exactly what it demands.
    struct Faithful;

    impl InstallationRendering for Faithful {
        fn show(&self, fixture: &InstallationFixture) -> Shown {
            Shown {
                signal: fixture.signal,
                text: fixture.facts.join(" "),
            }
        }
    }

    /// A rendering that shows every failure as a success.
    struct Quiet;

    impl InstallationRendering for Quiet {
        fn show(&self, fixture: &InstallationFixture) -> Shown {
            Shown {
                signal: Signal::Success,
                text: fixture.facts.join(" "),
            }
        }
    }

    #[test]
    fn a_rendering_that_gives_each_fixtures_signal_and_facts_conforms() {
        let reports = check_installation_renderings(&Faithful, &installation_fixtures());
        assert_eq!(reports, Vec::new());
    }

    #[test]
    fn an_unavailable_answer_shown_as_a_success_is_reported_with_both_signals() {
        let reports = check_installation_renderings(&Quiet, &installation_fixtures());
        let signals: Vec<_> = reports
            .iter()
            .filter_map(|report| match report {
                ConformanceReport::Rendering {
                    fixture,
                    problem: RenderingProblem::Signal { expected, actual },
                } => Some((*fixture, *expected, *actual)),
                _ => None,
            })
            .collect();
        assert_eq!(
            signals,
            vec![
                (
                    "status-without-a-workspace",
                    Signal::Unavailable,
                    Signal::Success
                ),
                (
                    "doctor-without-a-workspace",
                    Signal::Unavailable,
                    Signal::Success
                ),
            ]
        );
    }

    #[test]
    fn a_fact_the_rendering_leaves_out_is_reported_by_name_whatever_its_case() {
        struct Terse;
        impl InstallationRendering for Terse {
            fn show(&self, fixture: &InstallationFixture) -> Shown {
                Shown {
                    signal: fixture.signal,
                    text: if fixture.name == "registry-list" {
                        "PLAN only".to_string()
                    } else {
                        fixture.facts.join(" ")
                    },
                }
            }
        }
        let reports = check_installation_renderings(&Terse, &installation_fixtures());
        assert_eq!(
            reports,
            vec![ConformanceReport::Rendering {
                fixture: "registry-list",
                problem: RenderingProblem::MissingFact("refactor"),
            }]
        );
    }

    #[test]
    fn the_fixtures_have_distinct_stable_names() {
        let fixtures = installation_fixtures();
        let mut names: Vec<_> = fixtures.iter().map(|f| f.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), fixtures.len());
    }
}
