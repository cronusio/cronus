//! Facade wiring for the loop runner: the adapter-touching
//! composition — a real clock, a real file-existence oracle, and on-disk
//! ledger persistence — that the no-I/O domain tier cannot hold.
//!
//! Full worktree isolation / budget-engine drawdown / model-router judge
//! lineage composition is disclosed future work: this facade proves the
//! CLI → facade → domain wiring end to end over the shipped `Deterministic`
//! oracle path — the same scope discipline the earlier real-integration
//! stages used (an earlier one landed the CLI over the built adapters first,
//! deferring TUI/desktop wiring with no parity violation since nothing
//! divergent shipped).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cronus_domain::budget::BudgetStatus;
use cronus_domain::loop_runner::{
    Ceiling, ExecutionBackend, ExecutionReport, LoopClass, LoopOutcome, LoopSpec, Mutation,
    MutationManifest, Oracle, TurnResult, WorkspaceKind,
};
/// A real `ExecutionBackend` whose deterministic oracle checks whether
/// `target_path` exists. No mutations, no workspace write-back — a
/// "trivial" unit proves the CLI/facade/domain wiring is real without
/// requiring the heavier subsystem composition a production workload would
/// need.
pub struct FileExistsBackend {
    pub target_path: PathBuf,
}

impl ExecutionBackend for FileExistsBackend {
    fn current_time(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn budget_status(&self) -> Option<BudgetStatus> {
        None
    }

    fn run_turn(&mut self, _plan: &str, _status: &str) -> (Vec<Mutation>, TurnResult) {
        let done = self.target_path.exists();
        let feedback = if done {
            None
        } else {
            Some(format!("{} does not exist yet", self.target_path.display()))
        };
        (
            Vec::new(),
            TurnResult {
                raw_done: done,
                actor_lineage: "cli".to_string(),
                feedback,
            },
        )
    }

    fn rollback(&mut self, feedback: Option<&str>) -> String {
        feedback.unwrap_or("").to_string()
    }

    fn finalize(&mut self) {}
}

/// A ready-to-run `LoopSpec` for a `FileExistsBackend` unit: `Execution`
/// class, no mutable artifacts (a trivial unit writes nothing), a
/// `Deterministic` oracle naming the check, and the given ceiling.
pub fn file_exists_spec(max_iterations: u32) -> LoopSpec {
    LoopSpec {
        class: LoopClass::Execution,
        manifest: MutationManifest::same_prompt(),
        oracle: Oracle::Deterministic {
            validator: "file-exists".to_string(),
        },
        ceiling: Ceiling {
            max_iterations,
            budget_ref: None,
            deadline: None,
            patience: max_iterations,
        },
        workspace_kind: WorkspaceKind::Worktree,
        objective_slot: None,
    }
}

/// Where completed runs persist their ledger (`<run-id>.log`) — not a
/// scratch temp directory, so `log`/`show` remain readable across separate
/// CLI invocations. A run operates on a specific project's unit file —
/// resolves against the current workspace, same as every other
/// project-scoped semantic verb.
pub fn loop_log_dir() -> PathBuf {
    cronus_domain::paths::resolve_workspace_root().join("loops")
}

/// Render a completed run's ledger + outcome as plain text (one line per
/// entry, terminated by an OUTCOME line) — the format `log` reads back.
pub fn render_report(report: &ExecutionReport) -> String {
    let mut lines: Vec<String> = report
        .ledger
        .iter()
        .map(|entry| {
            format!(
                "{}\t{:?}\t{}\t{}",
                entry.iteration, entry.artifact, entry.summary, entry.why
            )
        })
        .collect();
    lines.push(match &report.outcome {
        LoopOutcome::Done(verdict) => format!(
            "OUTCOME\tDone\treduced_confidence={}\t{}",
            verdict.reduced_confidence,
            verdict.feedback.clone().unwrap_or_default()
        ),
        LoopOutcome::Stopped(reason) => format!("OUTCOME\tStopped\t{reason}"),
    });
    lines.join("\n")
}

/// Persist a completed run's ledger under `loop_log_dir()/<run_id>.log`.
pub fn write_report(run_id: &str, report: &ExecutionReport) -> std::io::Result<PathBuf> {
    let dir = loop_log_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{run_id}.log"));
    std::fs::write(&path, render_report(report))?;
    Ok(path)
}

/// Read a persisted run's ledger back as plain text.
pub fn read_report(run_id: &str) -> std::io::Result<String> {
    std::fs::read_to_string(loop_log_dir().join(format!("{run_id}.log")))
}

/// Render a `LoopSpec`'s manifest/oracle/ceiling as plain text — the format
/// `show` reads back.
pub fn render_spec(spec: &LoopSpec) -> String {
    format!(
        "class\t{:?}\ntier\t{}\nmutable\t{:?}\noracle\t{:?}\nmax_iterations\t{}\npatience\t{}",
        spec.class,
        spec.manifest.tier,
        spec.manifest.mutable,
        spec.oracle,
        spec.ceiling.max_iterations,
        spec.ceiling.patience,
    )
}

/// Persist the `LoopSpec` a run used under `loop_log_dir()/<run_id>.spec`.
pub fn write_spec(run_id: &str, spec: &LoopSpec) -> std::io::Result<PathBuf> {
    let dir = loop_log_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{run_id}.spec"));
    std::fs::write(&path, render_spec(spec))?;
    Ok(path)
}

/// Read a persisted run's spec summary back as plain text.
pub fn read_spec(run_id: &str) -> std::io::Result<String> {
    std::fs::read_to_string(loop_log_dir().join(format!("{run_id}.spec")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cronus_domain::loop_runner::{LedgerEntry, MutableArtifact, Verdict, run_execution};

    #[test]
    fn a_file_exists_backend_reports_done_when_the_target_already_exists() {
        let dir = std::env::temp_dir().join(format!(
            "cronus-loop-bootstrap-test-{}-a",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("marker");
        std::fs::write(&target, b"present").unwrap();

        let spec = file_exists_spec(3);
        let mut backend = FileExistsBackend {
            target_path: target,
        };
        let report = run_execution(&spec, &mut backend, "");
        assert!(matches!(report.outcome, LoopOutcome::Done(_)));
        assert_eq!(report.iterations_run, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_exists_backend_stops_at_the_ceiling_when_the_target_never_appears() {
        let dir = std::env::temp_dir().join(format!(
            "cronus-loop-bootstrap-test-{}-b",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("never-created");

        let spec = file_exists_spec(2);
        let mut backend = FileExistsBackend {
            target_path: target,
        };
        let report = run_execution(&spec, &mut backend, "");
        assert!(matches!(report.outcome, LoopOutcome::Stopped(_)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_report_round_trips_through_its_rendered_text_form() {
        let report = ExecutionReport {
            outcome: LoopOutcome::Done(Verdict {
                done: true,
                reduced_confidence: false,
                feedback: None,
            }),
            ledger: vec![LedgerEntry {
                iteration: 0,
                artifact: MutableArtifact::Plan,
                summary: "narrowed the plan".to_string(),
                why: "applied within the declared manifest".to_string(),
            }],
            iterations_run: 1,
        };
        let run_id = format!("test-run-{}", std::process::id());
        let path = write_report(&run_id, &report).expect("write_report");
        let text = read_report(&run_id).expect("read_report");
        assert!(text.contains("narrowed the plan"));
        assert!(text.contains("OUTCOME\tDone"));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_spec_round_trips_through_its_rendered_text_form() {
        let spec = file_exists_spec(5);
        let run_id = format!("test-spec-{}", std::process::id());
        let path = write_spec(&run_id, &spec).expect("write_spec");
        let text = read_spec(&run_id).expect("read_spec");
        assert!(text.contains("max_iterations\t5"));
        assert!(text.contains("Execution"));
        std::fs::remove_file(path).ok();
    }
}
