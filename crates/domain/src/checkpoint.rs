//! Session checkpoint — three-file hierarchy, section-budgeted reads,
//! fork-agent parity, and snapshot retention.
//!
//! `CheckpointPaths`, `CheckpointError`, and the `CheckpointWriter` trait
//! moved to `cronus-contract`; the file-backed
//! implementations stay here, in the domain tier.

use std::fs;
use std::path::{Path, PathBuf};

pub use cronus_contract::{CheckpointError, CheckpointPaths, CheckpointWriter};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum body bytes before auto-memory trigger fires.
pub const AUTO_MEMORY_THRESHOLD_BYTES: usize = 50_000;

/// Maximum checkpoint snapshots retained per project.
pub const MAX_SNAPSHOTS: usize = 50;

// ── CheckpointWriter implementations ─────────────────────────────────────────

/// File-based checkpoint writer.
pub struct FileCheckpointWriter;

impl CheckpointWriter for FileCheckpointWriter {
    fn write(&self, paths: &CheckpointPaths, body: &str) -> Result<(), CheckpointError> {
        write_atomic(&paths.context, body)
    }
}

/// No-op checkpoint writer (default stub).
pub struct NoOpCheckpointWriter;

impl CheckpointWriter for NoOpCheckpointWriter {
    fn write(&self, _: &CheckpointPaths, _: &str) -> Result<(), CheckpointError> {
        Ok(())
    }
}

// ── Atomic write ──────────────────────────────────────────────────────────────

/// Write `content` to `path` atomically (write to temp, rename).
pub fn write_atomic(path: &Path, content: &str) -> Result<(), CheckpointError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ── Section-budgeted read ─────────────────────────────────────────────────────

/// Read up to `max_bytes` from a checkpoint section.
pub fn read_section(path: &Path, max_bytes: usize) -> Result<String, CheckpointError> {
    let raw = fs::read_to_string(path)?;
    if raw.len() <= max_bytes {
        return Ok(raw);
    }
    let boundary = (0..=max_bytes)
        .rev()
        .find(|&i| raw.is_char_boundary(i))
        .unwrap_or(0);
    Ok(raw[..boundary].to_owned())
}

// ── System reminder injection ─────────────────────────────────────────────────

/// Build the session-start system reminder for an existing checkpoint.
pub fn build_resume_reminder(checkpoint_created_at: &str) -> String {
    format!("[Resuming from checkpoint: {checkpoint_created_at}]")
}

/// Returns true when the checkpoint body warrants auto-memory extraction.
pub fn needs_auto_memory(body_bytes: usize) -> bool {
    body_bytes > AUTO_MEMORY_THRESHOLD_BYTES
}

// ── Snapshot retention ────────────────────────────────────────────────────────

/// List snapshots under `snapshots_dir`, sorted oldest-first.
pub fn list_snapshots(snapshots_dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(snapshots_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();
    entries
}

/// Prune snapshots under `snapshots_dir` so at most `MAX_SNAPSHOTS` remain.
/// Removes the oldest entries.
pub fn prune_snapshots(snapshots_dir: &Path) -> Result<usize, CheckpointError> {
    let mut snaps = list_snapshots(snapshots_dir);
    let removed = if snaps.len() > MAX_SNAPSHOTS {
        let to_remove = snaps.len() - MAX_SNAPSHOTS;
        for path in snaps.drain(..to_remove) {
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        to_remove
    } else {
        0
    };
    Ok(removed)
}
