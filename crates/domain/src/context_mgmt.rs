//! Context management — adaptive token budget, 8-step trim cascade,
//! LLM-compaction seam, and tool-output truncation.
//!
//! `TrimPriority`, `ContextEntry`, and the `Compactor` trait moved to
//! `cronus-contract`; the trim cascade and the
//! no-op default stay here, in the domain tier.

pub use cronus_contract::{Compactor, ContextEntry, TrimPriority};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Characters per tool result before truncation.
pub const TOOL_RESULT_MAX_CHARS: usize = 2_000;

/// Lines before truncation for file-read tool output.
pub const DEFAULT_MAX_LINES: usize = 2_000;

/// Bytes before truncation for any tool output.
pub const DEFAULT_MAX_BYTES: usize = 50_000;

/// Maximum characters per grep match line.
pub const GREP_MAX_LINE_LENGTH: usize = 500;

/// Tokens reserved for the model's own generation.
pub const CONTEXT_RESERVE_TOKENS: u64 = 16_384;

// ── Adaptive budget ───────────────────────────────────────────────────────────

/// Compute usable context tokens from the model's declared window.
///
/// `usable = (context_window × 0.85).min(200_000)`
pub fn adaptive_budget(context_window: u64) -> u64 {
    let raw = (context_window as f64 * 0.85) as u64;
    raw.min(200_000)
}

/// Returns true when the context should be compacted.
///
/// Fires when `context_tokens > context_window - CONTEXT_RESERVE_TOKENS`.
pub fn should_compact(context_tokens: u64, context_window: u64) -> bool {
    context_tokens > context_window.saturating_sub(CONTEXT_RESERVE_TOKENS)
}

// ── 8-step trim cascade ───────────────────────────────────────────────────────

/// Remove entries in priority order until `target_tokens` is reached.
///
/// Protected entries are never removed (invariant: step 8 never executes).
pub fn trim_cascade(entries: &mut Vec<ContextEntry>, target_tokens: u64) {
    let priorities = [
        TrimPriority::OrphanedToolResult,
        TrimPriority::ToolUsePair,
        TrimPriority::NonProtectedThinking,
        TrimPriority::NonProtectedAssistant,
        TrimPriority::NonProtectedUser,
        TrimPriority::CompactionMarker,
        TrimPriority::ModelChangeMarker,
    ];

    for priority in &priorities {
        let total: u64 = entries.iter().map(|e| e.token_count).sum();
        if total <= target_tokens {
            return;
        }

        entries.retain(|e| {
            if e.protected {
                return true;
            }
            e.priority != *priority
        });
    }
}

/// Current total token count of the context.
pub fn total_tokens(entries: &[ContextEntry]) -> u64 {
    entries.iter().map(|e| e.token_count).sum()
}

// ── Compactor implementations ─────────────────────────────────────────────────

/// No-op compactor — returns a fixed placeholder string (stub).
pub struct NoOpCompactor;

impl Compactor for NoOpCompactor {
    fn compact(&self, _: &[ContextEntry], _: u64) -> Result<String, String> {
        Ok("[context compacted]".to_owned())
    }
}

// ── Tool output truncation ────────────────────────────────────────────────────

/// Truncate from the head — keeps the FIRST `max_bytes` bytes.
/// Used for file-read tool results.
pub fn truncate_head(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    // Snap to a valid UTF-8 boundary
    let boundary = (0..=max_bytes)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    format!("{}\n[truncated]", &s[..boundary])
}

/// Truncate from the tail — keeps the LAST `max_bytes` bytes.
/// Used for bash command output.
pub fn truncate_tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    let start = s.len() - max_bytes;
    // Snap to a valid UTF-8 boundary
    let boundary = (start..=s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(start);
    format!("[truncated]\n{}", &s[boundary..])
}

/// Truncate a single tool result to TOOL_RESULT_MAX_CHARS.
pub fn truncate_tool_result(s: &str) -> String {
    truncate_head(s, TOOL_RESULT_MAX_CHARS)
}

// ── CompactionDetails ─────────────────────────────────────────────────────────

/// Accumulated file tracking across compaction passes.
#[derive(Debug, Default, Clone)]
pub struct CompactionDetails {
    pub read_files: Vec<std::path::PathBuf>,
    pub modified_files: Vec<std::path::PathBuf>,
}

impl CompactionDetails {
    pub fn record_read(&mut self, path: impl Into<std::path::PathBuf>) {
        self.read_files.push(path.into());
    }

    pub fn record_modified(&mut self, path: impl Into<std::path::PathBuf>) {
        self.modified_files.push(path.into());
    }

    /// Render as XML tags for injection into a compaction summary prompt.
    pub fn to_xml(&self) -> String {
        let reads: String = self
            .read_files
            .iter()
            .map(|p| format!("  <file>{}</file>\n", p.display()))
            .collect();
        let mods: String = self
            .modified_files
            .iter()
            .map(|p| format!("  <file>{}</file>\n", p.display()))
            .collect();
        format!("<read-files>\n{reads}</read-files>\n<modified-files>\n{mods}</modified-files>")
    }
}
