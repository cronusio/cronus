//! Agent migration: a source-neutral manifest (schema
//! `agent-migration.v1`) that lets an agent receive memories, skills,
//! conversation threads, and archive documents from another agent. Archive
//! content is searchable-but-not-memory; memory candidates always go through
//! human review before saving; skills land inactive (`discovered`) until
//! explicitly granted; items flagged as credentials are always skipped. A
//! staged apply protocol (dry-run → backup → import archives → review
//! memories → import skills → skip secrets) means no unreviewed data ever
//! reaches durable memory, and no stage proceeds without the previous one
//! completing.

use std::collections::BTreeSet;
use std::path::Path;

use crate::backup::{self, BackupRef};

pub const SCHEMA_VERSION: &str = "agent-migration.v1";

/// The four item kinds a manifest carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Memory,
    Skill,
    ConversationThread,
    ArchiveDocument,
}

/// One manifest entry. `content` is kept as an opaque payload here — parsing
/// it into a concrete document/memory/skill record is the receiving
/// subsystem's job, not this module's; migration owns classification,
/// staging, and identity, not content interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationItem {
    pub id: String,
    pub kind: ItemKind,
    pub source: String,
    pub content: String,
    /// Set by the source adapter when it recognizes the item as credential
    /// material (e.g. an API key read from the source system's config).
    /// Credential items are always skipped regardless of kind.
    pub is_credential: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationWarning {
    pub item_id: Option<String>,
    pub message: String,
}

/// The manifest itself.
#[derive(Debug, Clone)]
pub struct AgentMigrationManifest {
    pub schema_version: String,
    pub source_name: String,
    pub source_kind: String,
    pub items: Vec<MigrationItem>,
    pub warnings: Vec<MigrationWarning>,
}

/// Failures raised validating or applying a manifest.
#[derive(Debug)]
pub enum ApplyError {
    /// The manifest's `schema_version` is not `agent-migration.v1` — rejected
    /// loudly rather than best-effort parsed.
    UnknownSchemaVersion(String),
    /// The pre-write backup (stage 2) failed; no later stage ran.
    BackupFailed(std::io::Error),
}

/// Validate a manifest before any stage runs.
pub fn validate(manifest: &AgentMigrationManifest) -> Result<(), ApplyError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(ApplyError::UnknownSchemaVersion(
            manifest.schema_version.clone(),
        ));
    }
    Ok(())
}

// --- Stage 1: dry-run summary (read-only) ---

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DryRunSummary {
    pub memory_count: u32,
    pub skill_count: u32,
    pub archive_count: u32,
    pub duplicate_ids: Vec<String>,
    pub credential_count: u32,
    pub warning_count: u32,
}

/// Summarize a manifest against `already_imported` ids without writing
/// anything ("never writes anything").
pub fn dry_run_summary(
    manifest: &AgentMigrationManifest,
    already_imported: &BTreeSet<String>,
) -> DryRunSummary {
    let mut summary = DryRunSummary {
        warning_count: manifest.warnings.len() as u32,
        ..Default::default()
    };
    for item in &manifest.items {
        if already_imported.contains(&item.id) {
            summary.duplicate_ids.push(item.id.clone());
            continue;
        }
        if item.is_credential {
            summary.credential_count += 1;
            continue;
        }
        match item.kind {
            ItemKind::Memory => summary.memory_count += 1,
            ItemKind::Skill => summary.skill_count += 1,
            ItemKind::ArchiveDocument | ItemKind::ConversationThread => summary.archive_count += 1,
        }
    }
    summary
}

// --- Two-layer split ---

#[derive(Debug, Clone, Default)]
pub struct SplitItems {
    /// `archive_document` + `conversation_thread` → document store.
    pub archive: Vec<MigrationItem>,
    /// `memory` → review queue, never auto-saved.
    pub memory_candidates: Vec<MigrationItem>,
    /// `skill` → extension registry, `discovered`/inactive.
    pub skills: Vec<MigrationItem>,
    pub secrets_skipped: Vec<MigrationItem>,
    /// Already present by id — never re-applied (identity-based merge).
    pub duplicates_skipped: Vec<MigrationItem>,
}

/// Classify every manifest item, honoring identity-based dedup and the
/// always-skip-credentials rule before any kind-based routing happens.
pub fn split_items(
    manifest: &AgentMigrationManifest,
    already_imported: &BTreeSet<String>,
) -> SplitItems {
    let mut split = SplitItems::default();
    for item in &manifest.items {
        if already_imported.contains(&item.id) {
            split.duplicates_skipped.push(item.clone());
            continue;
        }
        if item.is_credential {
            split.secrets_skipped.push(item.clone());
            continue;
        }
        match item.kind {
            ItemKind::ArchiveDocument | ItemKind::ConversationThread => {
                split.archive.push(item.clone())
            }
            ItemKind::Memory => split.memory_candidates.push(item.clone()),
            ItemKind::Skill => split.skills.push(item.clone()),
        }
    }
    split
}

// --- Staged apply ---

/// Which stage the run reached; also the terminal "everything ran" marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyStage {
    DryRunSummary,
    Backup,
    ImportArchives,
    ReviewMemories,
    ImportSkills,
    SkipSecrets,
}

/// A sink the apply protocol writes archive items into. The real document
/// store binds here later; this seam keeps the staged protocol itself
/// testable without it.
pub trait ArchiveSink {
    fn import_archive(&mut self, item: &MigrationItem);
}

/// A sink for memory candidates awaiting human review.
pub trait MemoryReviewQueue {
    fn enqueue_for_review(&mut self, item: &MigrationItem);
}

/// A sink for imported skills; returns `Err(reason)` on a name/category
/// conflict without aborting the rest of the batch.
pub trait SkillRegistrySink {
    fn import_discovered(&mut self, item: &MigrationItem) -> Result<(), String>;
}

/// One object servicing all three sink roles — real callers implement one
/// type; this supertrait lets `apply` take a single mutable reference
/// instead of three simultaneous borrows of the same value.
pub trait MigrationSinks: ArchiveSink + MemoryReviewQueue + SkillRegistrySink {}
impl<T: ArchiveSink + MemoryReviewQueue + SkillRegistrySink> MigrationSinks for T {}

#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub dry_run: DryRunSummary,
    pub backup: Option<BackupRef>,
    pub archives_imported: usize,
    pub memories_queued: usize,
    pub skills_imported: usize,
    pub skills_conflicted: Vec<String>,
    pub secrets_skipped: usize,
    pub duplicates_skipped: usize,
    pub stopped_at: Option<ApplyStage>,
}

/// Run the staged apply protocol. `dry_run = true` stops after stage 1 and
/// performs no writes at all — no backup, no sink calls. Otherwise every
/// stage runs in order; a failed backup (stage 2) aborts before any sink is
/// touched, so a failed apply never leaves partially-imported state.
pub fn apply(
    manifest: &AgentMigrationManifest,
    already_imported: &BTreeSet<String>,
    dry_run: bool,
    state_root: &Path,
    backups_dir: &Path,
    sinks: &mut dyn MigrationSinks,
) -> Result<ApplyReport, ApplyError> {
    validate(manifest)?;

    let mut report = ApplyReport {
        dry_run: dry_run_summary(manifest, already_imported),
        stopped_at: Some(ApplyStage::DryRunSummary),
        ..Default::default()
    };
    if dry_run {
        return Ok(report);
    }

    let backup_ref = backup::create(
        state_root,
        backups_dir,
        None,
        backup::BackupOptions::default(),
    )
    .map_err(ApplyError::BackupFailed)?;
    report.backup = Some(backup_ref);
    report.stopped_at = Some(ApplyStage::Backup);

    let split = split_items(manifest, already_imported);
    report.duplicates_skipped = split.duplicates_skipped.len();
    report.secrets_skipped = split.secrets_skipped.len();

    for item in &split.archive {
        sinks.import_archive(item);
        report.archives_imported += 1;
    }
    report.stopped_at = Some(ApplyStage::ImportArchives);

    for item in &split.memory_candidates {
        sinks.enqueue_for_review(item);
        report.memories_queued += 1;
    }
    report.stopped_at = Some(ApplyStage::ReviewMemories);

    for item in &split.skills {
        match sinks.import_discovered(item) {
            Ok(()) => report.skills_imported += 1,
            Err(reason) => report
                .skills_conflicted
                .push(format!("{}: {reason}", item.id)),
        }
    }
    report.stopped_at = Some(ApplyStage::ImportSkills);
    // Stage 6 (skip_secrets) already happened structurally in split_items —
    // credential items never reached any sink; the count is just reported.

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn item(id: &str, kind: ItemKind) -> MigrationItem {
        MigrationItem {
            id: id.to_string(),
            kind,
            source: "test-source".to_string(),
            content: "content".to_string(),
            is_credential: false,
        }
    }

    fn credential_item(id: &str) -> MigrationItem {
        MigrationItem {
            is_credential: true,
            ..item(id, ItemKind::Memory)
        }
    }

    fn manifest(items: Vec<MigrationItem>) -> AgentMigrationManifest {
        AgentMigrationManifest {
            schema_version: SCHEMA_VERSION.to_string(),
            source_name: "hermes".to_string(),
            source_kind: "hermes".to_string(),
            items,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn unknown_schema_version_is_rejected_loudly() {
        let mut m = manifest(vec![]);
        m.schema_version = "agent-migration.v2".to_string();
        match validate(&m) {
            Err(ApplyError::UnknownSchemaVersion(v)) => assert_eq!(v, "agent-migration.v2"),
            other => panic!("expected UnknownSchemaVersion, got {other:?}"),
        }
    }

    #[test]
    fn the_current_schema_version_validates() {
        assert!(validate(&manifest(vec![])).is_ok());
    }

    #[test]
    fn split_routes_each_kind_to_its_layer() {
        let m = manifest(vec![
            item("m1", ItemKind::Memory),
            item("s1", ItemKind::Skill),
            item("c1", ItemKind::ConversationThread),
            item("a1", ItemKind::ArchiveDocument),
        ]);
        let split = split_items(&m, &BTreeSet::new());
        assert_eq!(split.memory_candidates.len(), 1);
        assert_eq!(split.skills.len(), 1);
        assert_eq!(
            split.archive.len(),
            2,
            "conversation threads and archive docs share the archive layer"
        );
        assert!(split.secrets_skipped.is_empty());
    }

    #[test]
    fn credential_items_are_always_skipped_regardless_of_kind() {
        let m = manifest(vec![credential_item("cred-1")]);
        let split = split_items(&m, &BTreeSet::new());
        assert_eq!(split.secrets_skipped.len(), 1);
        assert!(
            split.memory_candidates.is_empty(),
            "never reaches the review queue"
        );
    }

    #[test]
    fn an_already_imported_id_is_skipped_never_reapplied() {
        let m = manifest(vec![item("dup-1", ItemKind::Memory)]);
        let mut seen = BTreeSet::new();
        seen.insert("dup-1".to_string());
        let split = split_items(&m, &seen);
        assert_eq!(split.duplicates_skipped.len(), 1);
        assert!(
            split.memory_candidates.is_empty(),
            "identity-based merge: never blind-clobbers"
        );
    }

    #[derive(Default)]
    struct RecordingSinks {
        archived: Vec<String>,
        reviewed: Vec<String>,
        skilled: Vec<String>,
    }
    impl ArchiveSink for RecordingSinks {
        fn import_archive(&mut self, item: &MigrationItem) {
            self.archived.push(item.id.clone());
        }
    }
    impl MemoryReviewQueue for RecordingSinks {
        fn enqueue_for_review(&mut self, item: &MigrationItem) {
            self.reviewed.push(item.id.clone());
        }
    }
    impl SkillRegistrySink for RecordingSinks {
        fn import_discovered(&mut self, item: &MigrationItem) -> Result<(), String> {
            if item.id == "conflict" {
                return Err("name already in use".to_string());
            }
            self.skilled.push(item.id.clone());
            Ok(())
        }
    }

    fn temp_dirs(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let state_root =
            std::env::temp_dir().join(format!("cronus-migrate-state-{tag}-{}", std::process::id()));
        let backups_dir = std::env::temp_dir().join(format!(
            "cronus-migrate-backups-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
        fs::create_dir_all(&state_root).unwrap();
        fs::write(state_root.join("config.json"), "{}").unwrap();
        (state_root, backups_dir)
    }

    #[test]
    fn dry_run_performs_no_writes_and_touches_no_sink() {
        let (state_root, backups_dir) = temp_dirs("dryrun");
        let m = manifest(vec![
            item("m1", ItemKind::Memory),
            item("s1", ItemKind::Skill),
        ]);
        let mut sinks = RecordingSinks::default();

        let report = apply(
            &m,
            &BTreeSet::new(),
            true,
            &state_root,
            &backups_dir,
            &mut sinks,
        )
        .unwrap();

        assert_eq!(report.stopped_at, Some(ApplyStage::DryRunSummary));
        assert!(report.backup.is_none());
        assert!(sinks.archived.is_empty() && sinks.reviewed.is_empty() && sinks.skilled.is_empty());
        assert!(!backups_dir.exists(), "dry run creates no backup directory");
        assert_eq!(report.dry_run.memory_count, 1);
        assert_eq!(report.dry_run.skill_count, 1);

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn a_real_apply_backs_up_before_writing_and_dispatches_every_layer() {
        let (state_root, backups_dir) = temp_dirs("apply");
        let m = manifest(vec![
            item("m1", ItemKind::Memory),
            item("s1", ItemKind::Skill),
            item("a1", ItemKind::ArchiveDocument),
            credential_item("cred-1"),
        ]);
        let mut sinks = RecordingSinks::default();

        let report = apply(
            &m,
            &BTreeSet::new(),
            false,
            &state_root,
            &backups_dir,
            &mut sinks,
        )
        .unwrap();

        assert_eq!(report.stopped_at, Some(ApplyStage::ImportSkills));
        let backup_ref = report.backup.expect("a pre-write backup was taken");
        assert!(
            backup_ref.path.join("config.json").exists(),
            "backup is a real, restorable snapshot"
        );

        assert_eq!(sinks.archived, vec!["a1".to_string()]);
        assert_eq!(
            sinks.reviewed,
            vec!["m1".to_string()],
            "memory candidates are queued, never auto-saved"
        );
        assert_eq!(sinks.skilled, vec!["s1".to_string()]);
        assert_eq!(report.secrets_skipped, 1);

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn a_skill_name_conflict_is_reported_without_aborting_the_batch() {
        let (state_root, backups_dir) = temp_dirs("conflict");
        let m = manifest(vec![
            item("conflict", ItemKind::Skill),
            item("s2", ItemKind::Skill),
        ]);
        let mut sinks = RecordingSinks::default();

        let report = apply(
            &m,
            &BTreeSet::new(),
            false,
            &state_root,
            &backups_dir,
            &mut sinks,
        )
        .unwrap();

        assert_eq!(report.skills_imported, 1);
        assert_eq!(report.skills_conflicted.len(), 1);
        assert!(sinks.skilled.contains(&"s2".to_string()));

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn duplicate_items_never_reach_any_sink_on_a_real_apply() {
        let (state_root, backups_dir) = temp_dirs("dedup");
        let m = manifest(vec![item("already-here", ItemKind::Memory)]);
        let mut seen = BTreeSet::new();
        seen.insert("already-here".to_string());
        let mut sinks = RecordingSinks::default();

        let report = apply(&m, &seen, false, &state_root, &backups_dir, &mut sinks).unwrap();

        assert!(sinks.reviewed.is_empty());
        assert_eq!(report.duplicates_skipped, 1);

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }
}
