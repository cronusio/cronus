//! Context router — most-specific-first routing across memory, rules,
//! and active session context.
//!
//! The pivot of the crate-topology migration: this module depends on
//! the `MemorySearch` seam, never on a concrete persistence type. Before this
//! change it held a `&MemoryStore` (SQLite-backed) and called one method on
//! it — the single edge that chained the entire domain tier to `rusqlite`.

use std::path::{Path, PathBuf};

use cronus_contract::{MemoryEntry, MemorySearch};

// ── RuleEntry ─────────────────────────────────────────────────────────────────

/// A rules file loaded from the workspace precedence chain.
#[derive(Debug, Clone)]
pub struct RuleEntry {
    pub path: PathBuf,
    pub body: String,
    pub precedence: RulePrecedence,
}

/// Precedence levels for rules — higher wins on conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RulePrecedence {
    Global,
    Project,
    Workspace,
}

// ── SessionContext seam ───────────────────────────────────────────────────────

/// Minimal session context type; real fields are wired in later.
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub session_id: String,
    pub entries: Vec<String>,
}

/// Lifecycle action applied to a session in the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    Continue,
    Retire,
}

// ── ContextBundle ─────────────────────────────────────────────────────────────

/// Assembled context ready for injection into a model request.
#[derive(Debug, Default)]
pub struct ContextBundle {
    pub memories: Vec<MemoryEntry>,
    pub rules: Vec<RuleEntry>,
    pub session: Option<SessionContext>,
}

impl ContextBundle {
    pub fn is_empty(&self) -> bool {
        self.memories.is_empty() && self.rules.is_empty() && self.session.is_none()
    }
}

// ── ContextRouter ─────────────────────────────────────────────────────────────

pub struct ContextRouter<'a> {
    memory_store: &'a dyn MemorySearch,
    workspace_dir: PathBuf,
    project_dir: PathBuf,
}

impl<'a> ContextRouter<'a> {
    pub fn new(
        memory_store: &'a dyn MemorySearch,
        workspace_dir: impl Into<PathBuf>,
        project_dir: impl Into<PathBuf>,
    ) -> Self {
        ContextRouter {
            memory_store,
            workspace_dir: workspace_dir.into(),
            project_dir: project_dir.into(),
        }
    }

    /// Assemble a `ContextBundle` for the given query.
    ///
    /// Memory entries come from FTS search (trust-filtered by the store).
    /// Rules are loaded most-specific-first (workspace > project > global).
    /// Session context is threaded through externally via `with_session`.
    pub fn assemble(&self, query: &str, limit: usize) -> Result<ContextBundle, String> {
        let memories = self.memory_store.search_fts(query, limit)?;
        let rules = self.load_rules();
        Ok(ContextBundle {
            memories,
            rules,
            session: None,
        })
    }

    /// Attach a session context and apply a lifecycle action.
    ///
    /// - `Continue`: appends the bundle to an existing session.
    /// - `Retire`: finalises the session (context is cleared).
    pub fn with_session(
        &self,
        mut bundle: ContextBundle,
        ctx: SessionContext,
        action: SessionAction,
    ) -> ContextBundle {
        match action {
            SessionAction::Continue => {
                bundle.session = Some(ctx);
            }
            SessionAction::Retire => {
                bundle.session = None;
            }
        }
        bundle
    }

    // ── private helpers ───────────────────────────────────────────────────────

    /// Load rules files in most-specific-first order.
    fn load_rules(&self) -> Vec<RuleEntry> {
        let candidates = [
            (
                self.workspace_dir.join("RULES.md"),
                RulePrecedence::Workspace,
            ),
            (
                self.project_dir.join("PROJECT_RULES.md"),
                RulePrecedence::Project,
            ),
            (self.project_dir.join("RULES.md"), RulePrecedence::Global),
        ];

        let mut entries: Vec<RuleEntry> = candidates
            .into_iter()
            .filter_map(|(path, prec)| load_rule_file(&path, prec))
            .collect();

        // Highest precedence first
        entries.sort_by_key(|e| std::cmp::Reverse(e.precedence));
        entries
    }
}

fn load_rule_file(path: &Path, precedence: RulePrecedence) -> Option<RuleEntry> {
    let body = std::fs::read_to_string(path).ok()?;
    Some(RuleEntry {
        path: path.to_owned(),
        body,
        precedence,
    })
}
