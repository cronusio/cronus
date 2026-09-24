//! Quality pipeline: per-language gate runner with result recording.
//!
//! Language is auto-detected from project markers (Cargo.toml, package.json,
//! pyproject.toml, go.mod). Each gate maps to the ecosystem's de-facto standard
//! tool. Gate results feed into the kanban board's done-transition guard.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
    Unknown,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
            Language::Python => "python",
            Language::Go => "go",
            Language::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    Tests,
    Lint,
    TypeFormat,
    Benchmarks,
    Security,
    Review,
    Refactor,
}

impl GateKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GateKind::Tests => "tests",
            GateKind::Lint => "lint",
            GateKind::TypeFormat => "type_format",
            GateKind::Benchmarks => "benchmarks",
            GateKind::Security => "security",
            GateKind::Review => "review",
            GateKind::Refactor => "refactor",
        }
    }

    /// Returns true when this gate is always required.
    pub fn is_always_on(&self) -> bool {
        matches!(
            self,
            GateKind::Tests | GateKind::Lint | GateKind::TypeFormat
        )
    }

    /// Returns true when this gate is conditional (performance/security tags).
    pub fn is_conditional(&self) -> bool {
        matches!(self, GateKind::Benchmarks | GateKind::Security)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateStatus {
    Pass,
    Fail,
    Warn,
    Skipped,
}

impl GateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GateStatus::Pass => "pass",
            GateStatus::Fail => "fail",
            GateStatus::Warn => "warn",
            GateStatus::Skipped => "skipped",
        }
    }
}

/// A tag on a card indicating performance or security relevance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardTag {
    Performance,
    Security,
}

impl CardTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            CardTag::Performance => "performance",
            CardTag::Security => "security",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateResult {
    pub gate: GateKind,
    pub status: GateStatus,
    pub output: String,
    pub duration_ms: u64,
}

// ── Language detection ────────────────────────────────────────────────────────

/// Detect the primary language of a project from its marker files.
pub fn detect_language(project_root: &Path) -> Language {
    if project_root.join("Cargo.toml").exists() {
        Language::Rust
    } else if project_root.join("package.json").exists() {
        Language::TypeScript
    } else if project_root.join("pyproject.toml").exists() || project_root.join("setup.py").exists()
    {
        Language::Python
    } else if project_root.join("go.mod").exists() {
        Language::Go
    } else {
        Language::Unknown
    }
}

// ── Gate runner ───────────────────────────────────────────────────────────────

/// Run a single quality gate for a project.
pub fn run_gate(gate: GateKind, project_root: &Path, tags: &[CardTag]) -> GateResult {
    // Skip conditional gates when the relevant tag is absent
    if gate == GateKind::Benchmarks && !tags.contains(&CardTag::Performance) {
        return GateResult {
            gate,
            status: GateStatus::Skipped,
            output: "skipped: card not tagged as performance-relevant".to_string(),
            duration_ms: 0,
        };
    }
    if gate == GateKind::Security && !tags.contains(&CardTag::Security) {
        return GateResult {
            gate,
            status: GateStatus::Skipped,
            output: "skipped: card not tagged as security-sensitive".to_string(),
            duration_ms: 0,
        };
    }

    let lang = detect_language(project_root);
    let (program, args) = match (lang, gate) {
        (Language::Rust, GateKind::Tests) => ("cargo", vec!["test"]),
        (Language::Rust, GateKind::Lint) => ("cargo", vec!["clippy", "--", "-D", "warnings"]),
        (Language::Rust, GateKind::TypeFormat) => ("cargo", vec!["fmt", "--check"]),
        (Language::Rust, GateKind::Benchmarks) => ("cargo", vec!["bench"]),
        (Language::Rust, GateKind::Security) => ("cargo", vec!["audit"]),
        (Language::TypeScript, GateKind::Tests) => ("pnpm", vec!["test"]),
        (Language::TypeScript, GateKind::Lint) => ("biome", vec!["check"]),
        (Language::TypeScript, GateKind::TypeFormat) => ("tsc", vec!["--noEmit"]),
        (Language::Python, GateKind::Tests) => ("pytest", vec![]),
        (Language::Python, GateKind::Lint) => ("ruff", vec!["check"]),
        (Language::Python, GateKind::TypeFormat) => ("mypy", vec!["."]),
        (Language::Go, GateKind::Tests) => ("go", vec!["test", "./..."]),
        (Language::Go, GateKind::Lint) => ("golangci-lint", vec!["run"]),
        (Language::Go, GateKind::TypeFormat) => ("gofmt", vec!["-l", "."]),
        _ => {
            return GateResult {
                gate,
                status: GateStatus::Skipped,
                output: format!("no tool configured for language {:?}", lang),
                duration_ms: 0,
            };
        }
    };

    let start = Instant::now();
    let output = Command::new(program)
        .args(&args)
        .current_dir(project_root)
        .output();

    let duration_ms = start.elapsed().as_millis() as u64;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{stdout}{stderr}");
            if out.status.success() {
                GateResult {
                    gate,
                    status: GateStatus::Pass,
                    output: combined,
                    duration_ms,
                }
            } else {
                GateResult {
                    gate,
                    status: GateStatus::Fail,
                    output: combined,
                    duration_ms,
                }
            }
        }
        Err(e) => GateResult {
            gate,
            status: GateStatus::Fail,
            output: format!("failed to spawn {program}: {e}"),
            duration_ms,
        },
    }
}

// ── Done transition guard seam ────────────────────────────────────────────────

/// Result of the done-transition gate check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoneGateStatus {
    Pass,
    Fail,
}

/// Check whether all required gates have passed for a card.
///
/// The real gate result store is not yet wired to the kanban board;
/// this seam accepts explicit results so callers control the outcome in tests.
pub fn check_done_gate(gate_results: &[GateResult]) -> DoneGateStatus {
    let required_kinds = [GateKind::Tests, GateKind::Lint, GateKind::TypeFormat];
    for kind in &required_kinds {
        let result = gate_results.iter().find(|r| r.gate == *kind);
        match result {
            None => return DoneGateStatus::Fail,
            Some(r) if r.status == GateStatus::Fail => return DoneGateStatus::Fail,
            _ => {}
        }
    }
    DoneGateStatus::Pass
}

// ── Gate result store ─────────────────────────────────────────────────────────

/// In-memory gate result store (backed by SQLite in production wiring).
#[derive(Debug, Default)]
pub struct GateResultStore {
    entries: Vec<(String, GateResult)>,
}

impl GateResultStore {
    pub fn new() -> Self {
        GateResultStore::default()
    }

    pub fn record(&mut self, card_id: &str, result: GateResult) {
        self.entries.push((card_id.to_string(), result));
    }

    pub fn results_for(&self, card_id: &str) -> Vec<&GateResult> {
        self.entries
            .iter()
            .filter(|(id, _)| id == card_id)
            .map(|(_, r)| r)
            .collect()
    }
}
