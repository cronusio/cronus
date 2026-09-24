//! Self-healing service: a data-driven catalog of checks, each
//! producing typed findings that are either safely auto-repaired or escalated
//! for human review — never both. Diagnosis itself is read-only;
//! `repair` layers safe fixes on top of a `check` pass and never touches an
//! escalated finding. Every check and repair is written to the
//! returned audit trail.
//!
//! Checks operate over injected [`DoctorInputs`] signals rather than the
//! concrete `kanban`/`session`/`store` types directly — this keeps the check
//! engine itself independently testable against seeded broken state; a
//! caller projects real subsystem state into these signals (see the module
//! Notes in the task record for the current binding boundary).

use std::collections::BTreeMap;
use std::time::Duration;

/// Conservative "clearly abandoned" threshold (an open question resolved here): a
/// stuck card only qualifies for safe repair once it has run this long *and*
/// carries independent completion evidence — ambiguous cases always escalate.
const STUCK_CARD_THRESHOLD: Duration = Duration::from_secs(24 * 60 * 60);
const DANGLING_SESSION_THRESHOLD: Duration = Duration::from_secs(2 * 60 * 60);

/// The six check categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckCategory {
    StoreIndex,
    StuckCards,
    DanglingSessions,
    ConfigValidity,
    DiskPressure,
    CrashRecovery,
}

/// Whether a finding can be safely auto-repaired or must be escalated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Deterministic and safe; reversible where possible.
    SafeRepair,
    /// Ambiguous or destructive — reported, never applied.
    Escalate,
}

/// One diagnosed problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub category: CheckCategory,
    /// Stable identity of the affected item (card id, session id, config key, …).
    pub id: String,
    pub description: String,
    pub disposition: Disposition,
}

/// One logged check/repair action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub action: &'static str,
    pub finding_id: String,
    pub detail: String,
}

/// The full diagnostic report from one run.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub repaired: Vec<Finding>,
    pub escalated: Vec<Finding>,
    pub audit_log: Vec<AuditEntry>,
    /// `(extension id, informational lines)` from registered extensions.
    pub extension_notes: Vec<(String, Vec<String>)>,
}

// --- Injected signals (decoupled from concrete subsystem types) ---

#[derive(Debug, Clone)]
pub struct CardSignal {
    pub id: String,
    pub running_for: Duration,
    /// Independent evidence (e.g. a completion marker) that the card is
    /// actually done but was never transitioned off `running`.
    pub looks_abandoned: bool,
}

#[derive(Debug, Clone)]
pub struct SessionSignal {
    pub id: String,
    pub idle_for: Duration,
    /// A live process/lock is still held for this session.
    pub looks_active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigSignal {
    /// Keys absent from config but restorable from a known default.
    pub missing_defaults: Vec<String>,
    /// Keys present but contradicting each other or the environment.
    pub conflicting_keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiskSignal {
    pub free_bytes: u64,
    pub low_watermark_bytes: u64,
    pub reclaimable_cache_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct StoreSignal {
    pub index_matches_source: bool,
    pub source_corrupt: bool,
}

#[derive(Debug, Clone)]
pub struct CrashSignal {
    /// The last shutdown had no clean-exit marker.
    pub unclean_shutdown: bool,
    /// Durable state is internally consistent and resumable as-is.
    pub durable_state_consistent: bool,
}

/// The seeded state a doctor run diagnoses against. Every field is optional
/// or empty by default — an absent signal category is simply not checked.
#[derive(Debug, Clone, Default)]
pub struct DoctorInputs {
    pub cards: Vec<CardSignal>,
    pub sessions: Vec<SessionSignal>,
    pub config: ConfigSignal,
    pub disk: Option<DiskSignal>,
    pub store: Option<StoreSignal>,
    pub crash: Option<CrashSignal>,
}

fn check_store(store: &Option<StoreSignal>) -> Vec<Finding> {
    let Some(store) = store else {
        return Vec::new();
    };
    if store.source_corrupt {
        vec![Finding {
            category: CheckCategory::StoreIndex,
            id: "store".into(),
            description: "source data is corrupt".into(),
            disposition: Disposition::Escalate,
        }]
    } else if !store.index_matches_source {
        vec![Finding {
            category: CheckCategory::StoreIndex,
            id: "store".into(),
            description: "index diverges from source text".into(),
            disposition: Disposition::SafeRepair,
        }]
    } else {
        Vec::new()
    }
}

fn check_cards(cards: &[CardSignal]) -> Vec<Finding> {
    cards
        .iter()
        .filter(|card| card.running_for >= STUCK_CARD_THRESHOLD)
        .map(|card| Finding {
            category: CheckCategory::StuckCards,
            id: card.id.clone(),
            description: format!("card running for {:?}", card.running_for),
            disposition: if card.looks_abandoned {
                Disposition::SafeRepair
            } else {
                Disposition::Escalate
            },
        })
        .collect()
}

fn check_sessions(sessions: &[SessionSignal]) -> Vec<Finding> {
    sessions
        .iter()
        .filter(|session| session.idle_for >= DANGLING_SESSION_THRESHOLD)
        .map(|session| Finding {
            category: CheckCategory::DanglingSessions,
            id: session.id.clone(),
            description: format!("session idle for {:?}", session.idle_for),
            disposition: if session.looks_active {
                Disposition::Escalate
            } else {
                Disposition::SafeRepair
            },
        })
        .collect()
}

fn check_config(config: &ConfigSignal) -> Vec<Finding> {
    let mut findings = Vec::new();
    for key in &config.missing_defaults {
        findings.push(Finding {
            category: CheckCategory::ConfigValidity,
            id: key.clone(),
            description: "missing default value".into(),
            disposition: Disposition::SafeRepair,
        });
    }
    for key in &config.conflicting_keys {
        findings.push(Finding {
            category: CheckCategory::ConfigValidity,
            id: key.clone(),
            description: "conflicting user configuration".into(),
            disposition: Disposition::Escalate,
        });
    }
    findings
}

fn check_disk(disk: &Option<DiskSignal>) -> Vec<Finding> {
    let Some(disk) = disk else {
        return Vec::new();
    };
    if disk.free_bytes >= disk.low_watermark_bytes {
        return Vec::new();
    }
    if disk.reclaimable_cache_bytes > 0 {
        vec![Finding {
            category: CheckCategory::DiskPressure,
            id: "disk".into(),
            description: "low disk; cache is reclaimable".into(),
            disposition: Disposition::SafeRepair,
        }]
    } else {
        vec![Finding {
            category: CheckCategory::DiskPressure,
            id: "disk".into(),
            description: "low disk; no cache to reclaim, needs user action".into(),
            disposition: Disposition::Escalate,
        }]
    }
}

fn check_crash(crash: &Option<CrashSignal>) -> Vec<Finding> {
    let Some(crash) = crash else {
        return Vec::new();
    };
    if !crash.unclean_shutdown {
        return Vec::new();
    }
    if crash.durable_state_consistent {
        vec![Finding {
            category: CheckCategory::CrashRecovery,
            id: "crash".into(),
            description: "unclean shutdown; durable state is consistent, resuming".into(),
            disposition: Disposition::SafeRepair,
        }]
    } else {
        vec![Finding {
            category: CheckCategory::CrashRecovery,
            id: "crash".into(),
            description: "unclean shutdown with divergent partial writes".into(),
            disposition: Disposition::Escalate,
        }]
    }
}

/// Run every check category read-only (`inputs` is borrowed, never
/// mutated) and classify every finding, without applying any repair.
pub fn check(inputs: &DoctorInputs) -> Report {
    let mut findings = Vec::new();
    findings.extend(check_store(&inputs.store));
    findings.extend(check_cards(&inputs.cards));
    findings.extend(check_sessions(&inputs.sessions));
    findings.extend(check_config(&inputs.config));
    findings.extend(check_disk(&inputs.disk));
    findings.extend(check_crash(&inputs.crash));

    let audit_log = findings
        .iter()
        .map(|finding| AuditEntry {
            action: "check",
            finding_id: finding.id.clone(),
            detail: finding.description.clone(),
        })
        .collect();
    let escalated = findings
        .iter()
        .filter(|finding| finding.disposition == Disposition::Escalate)
        .cloned()
        .collect();

    Report {
        findings,
        repaired: Vec::new(),
        escalated,
        audit_log,
        extension_notes: Vec::new(),
    }
}

/// Run `check`, then apply every safe-repair finding and log the action.
/// Escalated findings are carried through untouched — a
/// risky finding is never applied, only reported.
pub fn repair(inputs: &DoctorInputs) -> Report {
    let mut report = check(inputs);
    let safe: Vec<Finding> = report
        .findings
        .iter()
        .filter(|finding| finding.disposition == Disposition::SafeRepair)
        .cloned()
        .collect();
    for finding in &safe {
        report.audit_log.push(AuditEntry {
            action: "repair",
            finding_id: finding.id.clone(),
            detail: format!("auto-repaired: {}", finding.description),
        });
    }
    report.repaired = safe;
    report
}

// --- Extensibility (programmatic registration path) ---

/// `(inputs) -> informational lines`; an empty vec means nothing to report.
pub type DoctorCheckFn = fn(&DoctorInputs) -> Vec<String>;

/// Third-party programmatic doctor contributions, run in isolation. Entries
/// are keyed by a namespaced id (e.g. `"myplugin.cron"`); a `BTreeMap` keeps
/// execution alphabetical by id (ordering) with no separate sort step.
/// Package entry-point discovery (the second registration path) is a
/// build/plugin-loading concern owned by the extension registry, not this
/// module — this registry covers same-process programmatic registration.
#[derive(Default)]
pub struct ExtensionRegistry {
    entries: BTreeMap<String, DoctorCheckFn>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, id: impl Into<String>, check: DoctorCheckFn) {
        self.entries.insert(id.into(), check);
    }

    /// Run every registered extension in alphabetical-by-id order. A
    /// panicking extension is caught, logged as a note, and skipped — it
    /// never aborts the rest of the suite.
    pub fn run_all(&self, inputs: &DoctorInputs) -> Vec<(String, Vec<String>)> {
        let mut notes = Vec::with_capacity(self.entries.len());
        for (id, check) in &self.entries {
            match std::panic::catch_unwind(|| check(inputs)) {
                Ok(lines) => notes.push((id.clone(), lines)),
                Err(_) => notes.push((
                    id.clone(),
                    vec![format!("extension '{id}' panicked and was skipped")],
                )),
            }
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stuck_and_abandoned() -> DoctorInputs {
        DoctorInputs {
            cards: vec![CardSignal {
                id: "card-1".into(),
                running_for: STUCK_CARD_THRESHOLD + Duration::from_secs(1),
                looks_abandoned: true,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn store_index_drift_is_safe_repair_but_corruption_escalates() {
        let drifted = DoctorInputs {
            store: Some(StoreSignal {
                index_matches_source: false,
                source_corrupt: false,
            }),
            ..Default::default()
        };
        let report = check(&drifted);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].disposition, Disposition::SafeRepair);

        let corrupt = DoctorInputs {
            store: Some(StoreSignal {
                index_matches_source: false,
                source_corrupt: true,
            }),
            ..Default::default()
        };
        let report = check(&corrupt);
        assert_eq!(report.findings[0].disposition, Disposition::Escalate);
    }

    #[test]
    fn a_stuck_card_with_abandonment_evidence_is_safe_repair() {
        let report = check(&stuck_and_abandoned());
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].category, CheckCategory::StuckCards);
        assert_eq!(report.findings[0].disposition, Disposition::SafeRepair);
    }

    #[test]
    fn an_ambiguous_stuck_card_escalates_conservatively() {
        let inputs = DoctorInputs {
            cards: vec![CardSignal {
                id: "card-2".into(),
                running_for: STUCK_CARD_THRESHOLD + Duration::from_secs(1),
                looks_abandoned: false,
            }],
            ..Default::default()
        };
        let report = check(&inputs);
        assert_eq!(report.findings[0].disposition, Disposition::Escalate);
    }

    #[test]
    fn a_freshly_running_card_is_not_flagged() {
        let inputs = DoctorInputs {
            cards: vec![CardSignal {
                id: "card-3".into(),
                running_for: Duration::from_secs(60),
                looks_abandoned: true,
            }],
            ..Default::default()
        };
        assert!(check(&inputs).findings.is_empty());
    }

    #[test]
    fn dangling_sessions_prune_safely_unless_they_look_active() {
        let dangling = DoctorInputs {
            sessions: vec![SessionSignal {
                id: "sess-1".into(),
                idle_for: DANGLING_SESSION_THRESHOLD + Duration::from_secs(1),
                looks_active: false,
            }],
            ..Default::default()
        };
        assert_eq!(
            check(&dangling).findings[0].disposition,
            Disposition::SafeRepair
        );

        let active_looking = DoctorInputs {
            sessions: vec![SessionSignal {
                id: "sess-2".into(),
                idle_for: DANGLING_SESSION_THRESHOLD + Duration::from_secs(1),
                looks_active: true,
            }],
            ..Default::default()
        };
        assert_eq!(
            check(&active_looking).findings[0].disposition,
            Disposition::Escalate
        );
    }

    #[test]
    fn config_checks_split_missing_defaults_from_conflicts() {
        let inputs = DoctorInputs {
            config: ConfigSignal {
                missing_defaults: vec!["theme".into()],
                conflicting_keys: vec!["port".into()],
            },
            ..Default::default()
        };
        let report = check(&inputs);
        assert_eq!(report.findings.len(), 2);
        let theme = report.findings.iter().find(|f| f.id == "theme").unwrap();
        assert_eq!(theme.disposition, Disposition::SafeRepair);
        let port = report.findings.iter().find(|f| f.id == "port").unwrap();
        assert_eq!(port.disposition, Disposition::Escalate);
    }

    #[test]
    fn disk_pressure_reclaims_cache_or_escalates_when_nothing_to_reclaim() {
        let reclaimable = DoctorInputs {
            disk: Some(DiskSignal {
                free_bytes: 100,
                low_watermark_bytes: 1000,
                reclaimable_cache_bytes: 500,
            }),
            ..Default::default()
        };
        assert_eq!(
            check(&reclaimable).findings[0].disposition,
            Disposition::SafeRepair
        );

        let nothing_to_reclaim = DoctorInputs {
            disk: Some(DiskSignal {
                free_bytes: 100,
                low_watermark_bytes: 1000,
                reclaimable_cache_bytes: 0,
            }),
            ..Default::default()
        };
        assert_eq!(
            check(&nothing_to_reclaim).findings[0].disposition,
            Disposition::Escalate
        );

        let healthy = DoctorInputs {
            disk: Some(DiskSignal {
                free_bytes: 5000,
                low_watermark_bytes: 1000,
                reclaimable_cache_bytes: 0,
            }),
            ..Default::default()
        };
        assert!(check(&healthy).findings.is_empty());
    }

    #[test]
    fn crash_recovery_resumes_consistent_state_but_escalates_divergent_writes() {
        let consistent = DoctorInputs {
            crash: Some(CrashSignal {
                unclean_shutdown: true,
                durable_state_consistent: true,
            }),
            ..Default::default()
        };
        assert_eq!(
            check(&consistent).findings[0].disposition,
            Disposition::SafeRepair
        );

        let divergent = DoctorInputs {
            crash: Some(CrashSignal {
                unclean_shutdown: true,
                durable_state_consistent: false,
            }),
            ..Default::default()
        };
        assert_eq!(
            check(&divergent).findings[0].disposition,
            Disposition::Escalate
        );

        let clean_exit = DoctorInputs {
            crash: Some(CrashSignal {
                unclean_shutdown: false,
                durable_state_consistent: false,
            }),
            ..Default::default()
        };
        assert!(check(&clean_exit).findings.is_empty());
    }

    #[test]
    fn check_never_mutates_its_inputs() {
        let inputs = stuck_and_abandoned();
        let snapshot = inputs.clone();
        let _ = check(&inputs);
        assert_eq!(inputs.cards.len(), snapshot.cards.len());
        assert_eq!(inputs.cards[0].id, snapshot.cards[0].id);
    }

    #[test]
    fn repair_applies_and_logs_only_safe_findings_never_escalated_ones() {
        let inputs = DoctorInputs {
            cards: vec![
                CardSignal {
                    id: "safe-card".into(),
                    running_for: STUCK_CARD_THRESHOLD + Duration::from_secs(1),
                    looks_abandoned: true,
                },
                CardSignal {
                    id: "risky-card".into(),
                    running_for: STUCK_CARD_THRESHOLD + Duration::from_secs(1),
                    looks_abandoned: false,
                },
            ],
            ..Default::default()
        };
        let report = repair(&inputs);

        assert_eq!(report.repaired.len(), 1);
        assert_eq!(report.repaired[0].id, "safe-card");
        assert_eq!(report.escalated.len(), 1);
        assert_eq!(report.escalated[0].id, "risky-card");
        assert!(
            report
                .repaired
                .iter()
                .all(|f| !report.escalated.contains(f)),
            "a finding is never both repaired and escalated"
        );

        let repair_entries: Vec<_> = report
            .audit_log
            .iter()
            .filter(|entry| entry.action == "repair")
            .collect();
        assert_eq!(repair_entries.len(), 1);
        assert_eq!(repair_entries[0].finding_id, "safe-card");
    }

    #[test]
    fn check_alone_never_repairs_anything() {
        let report = check(&stuck_and_abandoned());
        assert!(report.repaired.is_empty());
        assert!(report.audit_log.iter().all(|entry| entry.action == "check"));
    }

    #[test]
    fn extensions_run_in_alphabetical_order_and_isolate_panics() {
        fn ok_check(_inputs: &DoctorInputs) -> Vec<String> {
            vec!["all good".to_string()]
        }
        fn panicking_check(_inputs: &DoctorInputs) -> Vec<String> {
            panic!("boom");
        }
        fn empty_check(_inputs: &DoctorInputs) -> Vec<String> {
            Vec::new()
        }

        let mut registry = ExtensionRegistry::new();
        registry.register("zeta.plugin", ok_check);
        registry.register("alpha.plugin", panicking_check);
        registry.register("mid.plugin", empty_check);

        // Suppress the panic hook's stderr noise for this expected panic.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let notes = registry.run_all(&DoctorInputs::default());
        std::panic::set_hook(previous_hook);

        assert_eq!(
            notes.len(),
            3,
            "the panicking extension is skipped, not omitted"
        );
        assert_eq!(notes[0].0, "alpha.plugin", "alphabetical by id");
        assert!(notes[0].1[0].contains("panicked"));
        assert_eq!(notes[1].0, "mid.plugin");
        assert_eq!(notes[2].0, "zeta.plugin");
        assert_eq!(notes[2].1, vec!["all good".to_string()]);
    }
}
