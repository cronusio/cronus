//! Self-improvement brief surface: five signal
//! tables — calibration buckets, a mistake log, should-have-asked gaps,
//! at-most-one-pending-per-project ask-backs, and reasoning templates — all
//! joined by one `build_brief` call against the current working context.
//! Each store is independently optional: a missing/unavailable store yields
//! an empty section rather than failing the whole brief ("a partial brief is
//! better than no brief").
//!
//! Scope note: this module covers only the first part of the design: calibration,
//! mistakes, should-have-asked, ask-backs, templates, and the brief join.
//! Later sections (advisor-executor handoff, plan backlog lifecycle,
//! retrospective/milestone formats, behavior gates, spec quality scoring,
//! decision velocity, the learnings journal, and skill-document training/
//! evolution) describe a separate, much larger planning/training subsystem
//! outside this task's scope and are not implemented here.

use std::collections::BTreeMap;

// --- Calibration buckets ---

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationBucket {
    pub task_type: String,
    pub project: String,
    pub declared_success: u32,
    pub verified_success: u32,
    pub declared_failure: u32,
    pub refuted_success: u32,
    pub last_updated: u64,
}

/// `overconfidence = max(0, 1 - verified_success / max(declared_success, 1))`.
pub fn overconfidence(bucket: &CalibrationBucket) -> f64 {
    let ratio = f64::from(bucket.verified_success) / f64::from(bucket.declared_success.max(1));
    (1.0 - ratio).max(0.0)
}

pub fn verified_ratio(bucket: &CalibrationBucket) -> f64 {
    f64::from(bucket.verified_success) / f64::from(bucket.declared_success.max(1))
}

pub const CALIBRATION_MIN_SAMPLE_FOR_WARN: u32 = 5;
pub const VERIFIED_RATIO_WARN_THRESHOLD: f64 = 0.50;

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationWarning {
    pub task_type: String,
    pub project: String,
    pub verified_ratio: f64,
}

/// Fires when `declared_success >= 5 AND verified_ratio < 0.50` (brief gate).
pub fn calibration_warning(bucket: &CalibrationBucket) -> Option<CalibrationWarning> {
    if bucket.declared_success < CALIBRATION_MIN_SAMPLE_FOR_WARN {
        return None;
    }
    let ratio = verified_ratio(bucket);
    if ratio < VERIFIED_RATIO_WARN_THRESHOLD {
        Some(CalibrationWarning {
            task_type: bucket.task_type.clone(),
            project: bucket.project.clone(),
            verified_ratio: ratio,
        })
    } else {
        None
    }
}

/// Keyed by `(task_type, project)`; updates are additive.
#[derive(Debug, Default)]
pub struct CalibrationStore {
    buckets: BTreeMap<(String, String), CalibrationBucket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    DeclaredSuccess,
    VerifiedSuccess,
    DeclaredFailure,
    RefutedSuccess,
}

impl CalibrationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, task_type: &str, project: &str, outcome: Outcome, at: u64) {
        let bucket = self
            .buckets
            .entry((task_type.to_string(), project.to_string()))
            .or_insert_with(|| CalibrationBucket {
                task_type: task_type.to_string(),
                project: project.to_string(),
                declared_success: 0,
                verified_success: 0,
                declared_failure: 0,
                refuted_success: 0,
                last_updated: at,
            });
        match outcome {
            Outcome::DeclaredSuccess => bucket.declared_success += 1,
            Outcome::VerifiedSuccess => bucket.verified_success += 1,
            Outcome::DeclaredFailure => bucket.declared_failure += 1,
            Outcome::RefutedSuccess => bucket.refuted_success += 1,
        }
        bucket.last_updated = at;
    }

    pub fn get(&self, task_type: &str, project: &str) -> Option<&CalibrationBucket> {
        self.buckets
            .get(&(task_type.to_string(), project.to_string()))
    }
}

// --- Mistake log ---

#[derive(Debug, Clone, PartialEq)]
pub struct Mistake {
    pub id: u64,
    pub project: String,
    pub category: String,
    pub episode_id: Option<String>,
    pub files: Vec<String>,
    pub description: String,
    pub correction: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CategoryCount {
    pub category: String,
    pub count: u32,
    pub last_seen: u64,
    /// Set only in cross-project mode, tagging a foreign-project row.
    pub source_project: Option<String>,
}

/// Append-only mistake log.
#[derive(Debug, Default)]
pub struct MistakeLog {
    rows: Vec<Mistake>,
    next_id: u64,
}

fn files_overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|f| b.contains(f))
}

impl MistakeLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, mut mistake: Mistake) -> u64 {
        self.next_id += 1;
        mistake.id = self.next_id;
        let id = mistake.id;
        self.rows.push(mistake);
        id
    }

    /// Top-N mistake categories for `project` whose `files` overlap `files`,
    /// ordered by count desc (brief query).
    pub fn top_categories_for_files(
        &self,
        project: &str,
        files: &[String],
        limit: usize,
    ) -> Vec<CategoryCount> {
        top_categories(
            self.rows.iter().filter(|m| m.project == project),
            files,
            limit,
            None,
        )
    }

    /// Cross-project variant: rows from other projects also count,
    /// tagged with their originating project.
    pub fn top_categories_cross_project(
        &self,
        current_project: &str,
        files: &[String],
        limit: usize,
    ) -> Vec<CategoryCount> {
        top_categories(self.rows.iter(), files, limit, Some(current_project))
    }
}

fn top_categories<'a>(
    rows: impl Iterator<Item = &'a Mistake>,
    files: &[String],
    limit: usize,
    tag_foreign_against: Option<&str>,
) -> Vec<CategoryCount> {
    let mut by_category: BTreeMap<(String, Option<String>), (u32, u64)> = BTreeMap::new();
    for mistake in rows.filter(|m| files_overlap(&m.files, files)) {
        let source_project = tag_foreign_against.and_then(|current| {
            if mistake.project != current {
                Some(mistake.project.clone())
            } else {
                None
            }
        });
        let entry = by_category
            .entry((mistake.category.clone(), source_project))
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 = entry.1.max(mistake.created_at);
    }
    let mut counts: Vec<CategoryCount> = by_category
        .into_iter()
        .map(
            |((category, source_project), (count, last_seen))| CategoryCount {
                category,
                count,
                last_seen,
                source_project,
            },
        )
        .collect();
    counts.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
    });
    counts.truncate(limit);
    counts
}

// --- Should-have-asked ---

#[derive(Debug, Clone, PartialEq)]
pub struct ShouldHaveAsked {
    pub id: u64,
    pub project: String,
    pub trigger: String,
    pub question: String,
    pub answer: String,
    pub episode_id: Option<String>,
    pub files: Vec<String>,
    pub created_at: u64,
    pub source_project: Option<String>,
}

#[derive(Debug, Default)]
pub struct ShouldHaveAskedLog {
    rows: Vec<ShouldHaveAsked>,
    next_id: u64,
}

impl ShouldHaveAskedLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, mut row: ShouldHaveAsked) -> u64 {
        self.next_id += 1;
        row.id = self.next_id;
        let id = row.id;
        self.rows.push(row);
        id
    }

    /// Distinct triggers for `project` overlapping `files`, most recent first
    /// (brief query).
    pub fn triggers_for_files(
        &self,
        project: &str,
        files: &[String],
        limit: usize,
    ) -> Vec<ShouldHaveAsked> {
        self.triggers_impl(files, limit, |row| row.project == project, None)
    }

    pub fn triggers_cross_project(
        &self,
        current_project: &str,
        files: &[String],
        limit: usize,
    ) -> Vec<ShouldHaveAsked> {
        self.triggers_impl(files, limit, |_| true, Some(current_project))
    }

    fn triggers_impl(
        &self,
        files: &[String],
        limit: usize,
        keep: impl Fn(&ShouldHaveAsked) -> bool,
        tag_foreign_against: Option<&str>,
    ) -> Vec<ShouldHaveAsked> {
        let mut candidates: Vec<&ShouldHaveAsked> = self
            .rows
            .iter()
            .filter(|row| keep(row) && files_overlap(&row.files, files))
            .collect();
        // Most recent first, so the DISTINCT-by-trigger pass below keeps each
        // trigger's newest occurrence rather than its first-inserted one.
        candidates.sort_by_key(|row| std::cmp::Reverse(row.created_at));

        let mut seen_triggers = std::collections::BTreeSet::new();
        let mut matched: Vec<ShouldHaveAsked> = candidates
            .into_iter()
            .filter(|row| seen_triggers.insert(row.trigger.clone())) // DISTINCT trigger, newest wins
            .map(|row| {
                let mut row = row.clone();
                if let Some(current) = tag_foreign_against
                    && row.project != current
                {
                    row.source_project = Some(row.project.clone());
                }
                row
            })
            .collect();
        matched.truncate(limit);
        matched
    }
}

// --- Ask-backs ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskBackStatus {
    Pending,
    Served,
    Dismissed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AskBack {
    pub id: u64,
    pub project: String,
    pub episode_id: String,
    pub question: String,
    pub status: AskBackStatus,
    pub model: String,
    pub created_at: u64,
    pub served_at: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PendingAskBackExists;

/// At-most-one-pending-per-project, enforced here the way the
/// reference partial UNIQUE INDEX enforces it at the database level: the
/// insert itself fails immediately when a pending row already exists for
/// the project.
#[derive(Debug, Default)]
pub struct AskBackStore {
    rows: Vec<AskBack>,
    next_id: u64,
}

impl AskBackStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_pending(
        &mut self,
        project: &str,
        episode_id: &str,
        question: &str,
        model: &str,
        at: u64,
    ) -> Result<u64, PendingAskBackExists> {
        if self.pending_for(project).is_some() {
            return Err(PendingAskBackExists);
        }
        self.next_id += 1;
        let id = self.next_id;
        self.rows.push(AskBack {
            id,
            project: project.to_string(),
            episode_id: episode_id.to_string(),
            question: question.to_string(),
            status: AskBackStatus::Pending,
            model: model.to_string(),
            created_at: at,
            served_at: None,
        });
        Ok(id)
    }

    pub fn pending_for(&self, project: &str) -> Option<&AskBack> {
        self.rows
            .iter()
            .find(|row| row.project == project && row.status == AskBackStatus::Pending)
    }

    pub fn mark_served(&mut self, id: u64, at: u64) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.status = AskBackStatus::Served;
            row.served_at = Some(at);
        }
    }

    pub fn mark_dismissed(&mut self, id: u64) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.status = AskBackStatus::Dismissed;
        }
    }
}

// --- Reasoning templates ---

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub id: String,
    pub task_type: String,
    pub domain: String,
    pub name: String,
    pub steps: Vec<String>,
    pub evidence_episodes: Vec<String>,
    pub success_rate: f64,
    pub times_used: u32,
    pub model: String,
    pub created_at: u64,
    pub last_used: Option<u64>,
}

/// One row per `(task_type, domain)`; re-extraction upserts in place,
/// accumulating evidence rather than duplicating.
#[derive(Debug, Default)]
pub struct TemplateStore {
    templates: BTreeMap<(String, String), Template>,
}

/// Caller-supplied fields for one extraction/re-extraction; `id`,
/// `times_used`, and `created_at` are store-managed bookkeeping, not inputs.
pub struct TemplateUpsert<'a> {
    pub task_type: &'a str,
    pub domain: &'a str,
    pub name: &'a str,
    pub steps: Vec<String>,
    pub episode_id: &'a str,
    pub success_rate: f64,
    pub model: &'a str,
}

impl TemplateStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert: a new episode + refreshed success rate merge into the
    /// existing row for the pair, or create it on first extraction.
    pub fn upsert(&mut self, extraction: TemplateUpsert<'_>, at: u64) {
        let key = (
            extraction.task_type.to_string(),
            extraction.domain.to_string(),
        );
        match self.templates.get_mut(&key) {
            Some(existing) => {
                if !existing
                    .evidence_episodes
                    .contains(&extraction.episode_id.to_string())
                {
                    existing
                        .evidence_episodes
                        .push(extraction.episode_id.to_string());
                }
                existing.name = extraction.name.to_string();
                existing.steps = extraction.steps;
                existing.success_rate = extraction.success_rate;
            }
            None => {
                self.templates.insert(
                    key,
                    Template {
                        id: format!("{}:{}", extraction.task_type, extraction.domain),
                        task_type: extraction.task_type.to_string(),
                        domain: extraction.domain.to_string(),
                        name: extraction.name.to_string(),
                        steps: extraction.steps,
                        evidence_episodes: vec![extraction.episode_id.to_string()],
                        success_rate: extraction.success_rate,
                        times_used: 0,
                        model: extraction.model.to_string(),
                        created_at: at,
                        last_used: None,
                    },
                );
            }
        }
    }

    pub fn get_by_pair(&self, task_type: &str, domain: &str) -> Option<&Template> {
        self.templates
            .get(&(task_type.to_string(), domain.to_string()))
    }

    pub fn record_use(&mut self, task_type: &str, domain: &str, at: u64) {
        if let Some(template) = self
            .templates
            .get_mut(&(task_type.to_string(), domain.to_string()))
        {
            template.times_used += 1;
            template.last_used = Some(at);
        }
    }
}

// --- Brief surface ---

pub const BRIEF_TOP_CATEGORIES: usize = 5;
pub const BRIEF_TOP_ASKS: usize = 5;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Brief {
    pub project: String,
    pub files: Vec<String>,
    pub task_type: Option<String>,
    pub domain: Option<String>,
    pub pending_ask_back: Option<AskBack>,
    pub template: Option<Template>,
    pub top_correction_categories: Vec<CategoryCount>,
    pub should_have_asked_triggers: Vec<ShouldHaveAsked>,
    pub calibration_warning: Option<CalibrationWarning>,
}

/// Join all five signals for `(project, files, task_type?, domain?)`. Every
/// store parameter is optional — an absent store (simulating an open
/// failure elsewhere) yields an empty section for that signal only, never a
/// failed brief ("a partial brief is better than no brief").
#[allow(clippy::too_many_arguments)]
pub fn build_brief(
    project: &str,
    files: &[String],
    task_type: Option<&str>,
    domain: Option<&str>,
    cross_project: bool,
    ask_backs: Option<&AskBackStore>,
    templates: Option<&TemplateStore>,
    mistakes: Option<&MistakeLog>,
    should_have_asked: Option<&ShouldHaveAskedLog>,
    calibration: Option<&CalibrationStore>,
) -> Brief {
    let pending_ask_back = ask_backs
        .and_then(|store| store.pending_for(project))
        .cloned();

    let template = match (task_type, domain, templates) {
        (Some(tt), Some(d), Some(store)) => store.get_by_pair(tt, d).cloned(),
        _ => None,
    };

    let top_correction_categories = mistakes
        .map(|log| {
            if cross_project {
                log.top_categories_cross_project(project, files, BRIEF_TOP_CATEGORIES)
            } else {
                log.top_categories_for_files(project, files, BRIEF_TOP_CATEGORIES)
            }
        })
        .unwrap_or_default();

    let should_have_asked_triggers = should_have_asked
        .map(|log| {
            if cross_project {
                log.triggers_cross_project(project, files, BRIEF_TOP_ASKS)
            } else {
                log.triggers_for_files(project, files, BRIEF_TOP_ASKS)
            }
        })
        .unwrap_or_default();

    // Calibration and template sections are always project-scoped — they do
    // not generalize across project boundaries even in cross-project mode.
    let calibration_warning = match (task_type, calibration) {
        (Some(tt), Some(store)) => store.get(tt, project).and_then(calibration_warning),
        _ => None,
    };

    Brief {
        project: project.to_string(),
        files: files.to_vec(),
        task_type: task_type.map(str::to_string),
        domain: domain.map(str::to_string),
        pending_ask_back,
        template,
        top_correction_categories,
        should_have_asked_triggers,
        calibration_warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- calibration ---

    #[test]
    fn overconfidence_and_verified_ratio_match_the_formula() {
        let bucket = CalibrationBucket {
            task_type: "refactor".into(),
            project: "p".into(),
            declared_success: 10,
            verified_success: 3,
            declared_failure: 0,
            refuted_success: 0,
            last_updated: 0,
        };
        assert!((verified_ratio(&bucket) - 0.3).abs() < 1e-9);
        assert!((overconfidence(&bucket) - 0.7).abs() < 1e-9);
    }

    #[test]
    fn overconfidence_never_goes_negative_when_verified_exceeds_declared() {
        let bucket = CalibrationBucket {
            task_type: "t".into(),
            project: "p".into(),
            declared_success: 2,
            verified_success: 5,
            declared_failure: 0,
            refuted_success: 0,
            last_updated: 0,
        };
        assert_eq!(overconfidence(&bucket), 0.0);
    }

    #[test]
    fn calibration_warning_fires_only_above_the_sample_floor_and_below_the_ratio_threshold() {
        let mut store = CalibrationStore::new();
        for _ in 0..4 {
            store.record("refactor", "p", Outcome::DeclaredSuccess, 1);
        }
        // Only 4 declared successes, 0 verified: below the min-sample floor (5).
        assert!(calibration_warning(store.get("refactor", "p").unwrap()).is_none());

        store.record("refactor", "p", Outcome::DeclaredSuccess, 2);
        // Now 5 declared, 0 verified -> ratio 0.0 < 0.50, sample >= 5: warns.
        let warning = calibration_warning(store.get("refactor", "p").unwrap()).unwrap();
        assert_eq!(warning.task_type, "refactor");
        assert!(warning.verified_ratio < VERIFIED_RATIO_WARN_THRESHOLD);
    }

    #[test]
    fn a_well_calibrated_bucket_never_warns() {
        let mut store = CalibrationStore::new();
        for _ in 0..10 {
            store.record("t", "p", Outcome::DeclaredSuccess, 1);
            store.record("t", "p", Outcome::VerifiedSuccess, 1);
        }
        assert!(calibration_warning(store.get("t", "p").unwrap()).is_none());
    }

    // --- mistakes ---

    fn mistake(project: &str, category: &str, files: &[&str], created_at: u64) -> Mistake {
        Mistake {
            id: 0,
            project: project.to_string(),
            category: category.to_string(),
            episode_id: None,
            files: files.iter().map(|f| f.to_string()).collect(),
            description: "desc".into(),
            correction: "fix".into(),
            created_at,
        }
    }

    #[test]
    fn top_categories_ranks_by_count_and_filters_by_project_and_files() {
        let mut log = MistakeLog::new();
        log.append(mistake("p", "lifetimes", &["src/a.rs"], 1));
        log.append(mistake("p", "lifetimes", &["src/a.rs"], 2));
        log.append(mistake("p", "auth_order", &["src/b.rs"], 3));
        log.append(mistake("other", "lifetimes", &["src/a.rs"], 4)); // different project

        let top = log.top_categories_for_files("p", &["src/a.rs".to_string()], 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].category, "lifetimes");
        assert_eq!(top[0].count, 2);
        assert!(top[0].source_project.is_none());
    }

    #[test]
    fn top_categories_respects_the_limit() {
        let mut log = MistakeLog::new();
        for i in 0..8 {
            log.append(mistake("p", &format!("cat{i}"), &["src/x.rs"], i as u64));
        }
        assert_eq!(
            log.top_categories_for_files("p", &["src/x.rs".to_string()], BRIEF_TOP_CATEGORIES)
                .len(),
            5
        );
    }

    #[test]
    fn cross_project_mode_tags_foreign_rows_with_their_source_project() {
        let mut log = MistakeLog::new();
        log.append(mistake("other-proj", "lifetimes", &["src/shared.rs"], 5));
        let cross = log.top_categories_cross_project("p", &["src/shared.rs".to_string()], 5);
        assert_eq!(cross.len(), 1);
        assert_eq!(cross[0].source_project.as_deref(), Some("other-proj"));
    }

    // --- should-have-asked ---

    #[test]
    fn triggers_for_files_returns_distinct_recency_ordered_triggers() {
        let mut log = ShouldHaveAskedLog::new();
        log.append(ShouldHaveAsked {
            id: 0,
            project: "p".into(),
            trigger: "edit_auth_middleware".into(),
            question: "q1".into(),
            answer: "a1".into(),
            episode_id: None,
            files: vec!["src/auth/middleware.rs".into()],
            created_at: 100,
            source_project: None,
        });
        log.append(ShouldHaveAsked {
            id: 0,
            project: "p".into(),
            trigger: "edit_auth_middleware".into(), // same trigger again
            question: "q2".into(),
            answer: "a2".into(),
            episode_id: None,
            files: vec!["src/auth/middleware.rs".into()],
            created_at: 200,
            source_project: None,
        });
        let found = log.triggers_for_files("p", &["src/auth/middleware.rs".to_string()], 5);
        assert_eq!(found.len(), 1, "DISTINCT trigger per SS4.3 brief query");
        assert_eq!(found[0].created_at, 200, "most recent occurrence wins");
    }

    // --- ask-backs ---

    #[test]
    fn at_most_one_pending_ask_back_per_project() {
        let mut store = AskBackStore::new();
        store
            .create_pending("p", "ep-1", "what did you mean?", "sonnet", 1)
            .unwrap();
        let second = store.create_pending("p", "ep-2", "another question?", "sonnet", 2);
        assert_eq!(second, Err(PendingAskBackExists));
    }

    #[test]
    fn a_served_ask_back_frees_the_project_for_a_new_pending_one() {
        let mut store = AskBackStore::new();
        let id = store
            .create_pending("p", "ep-1", "q1", "sonnet", 1)
            .unwrap();
        store.mark_served(id, 2);
        assert!(store.pending_for("p").is_none());
        assert!(store.create_pending("p", "ep-2", "q2", "sonnet", 3).is_ok());
    }

    #[test]
    fn a_dismissed_ask_back_also_frees_the_project() {
        let mut store = AskBackStore::new();
        let id = store
            .create_pending("p", "ep-1", "q1", "sonnet", 1)
            .unwrap();
        store.mark_dismissed(id);
        assert!(store.create_pending("p", "ep-2", "q2", "sonnet", 2).is_ok());
    }

    #[test]
    fn different_projects_each_get_their_own_pending_ask_back() {
        let mut store = AskBackStore::new();
        assert!(
            store
                .create_pending("p1", "ep-1", "q1", "sonnet", 1)
                .is_ok()
        );
        assert!(
            store
                .create_pending("p2", "ep-2", "q2", "sonnet", 2)
                .is_ok()
        );
    }

    // --- templates ---

    #[test]
    fn upsert_accumulates_evidence_without_duplicating_the_row() {
        let mut store = TemplateStore::new();
        store.upsert(
            TemplateUpsert {
                task_type: "refactor",
                domain: "rust",
                name: "Extract module",
                steps: vec!["step1".into()],
                episode_id: "ep-1",
                success_rate: 0.8,
                model: "sonnet",
            },
            1,
        );
        store.upsert(
            TemplateUpsert {
                task_type: "refactor",
                domain: "rust",
                name: "Extract module v2",
                steps: vec!["step1".into(), "step2".into()],
                episode_id: "ep-2",
                success_rate: 0.9,
                model: "sonnet",
            },
            2,
        );

        let template = store.get_by_pair("refactor", "rust").unwrap();
        assert_eq!(
            template.evidence_episodes,
            vec!["ep-1".to_string(), "ep-2".to_string()]
        );
        assert_eq!(template.success_rate, 0.9, "refreshed on re-extraction");
        assert_eq!(template.name, "Extract module v2");
    }

    #[test]
    fn get_by_pair_returns_none_for_an_unknown_pair() {
        let store = TemplateStore::new();
        assert!(store.get_by_pair("refactor", "rust").is_none());
    }

    // --- brief ---

    #[test]
    fn brief_omits_every_section_when_no_stores_are_supplied() {
        let brief = build_brief(
            "p",
            &["src/a.rs".to_string()],
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(brief.pending_ask_back.is_none());
        assert!(brief.template.is_none());
        assert!(brief.top_correction_categories.is_empty());
        assert!(brief.should_have_asked_triggers.is_empty());
        assert!(brief.calibration_warning.is_none());
    }

    #[test]
    fn brief_joins_every_available_signal() {
        let mut ask_backs = AskBackStore::new();
        ask_backs
            .create_pending("p", "ep-1", "clarify scope?", "sonnet", 1)
            .unwrap();

        let mut templates = TemplateStore::new();
        templates.upsert(
            TemplateUpsert {
                task_type: "refactor",
                domain: "rust",
                name: "Extract module",
                steps: vec!["a".into()],
                episode_id: "ep-1",
                success_rate: 0.9,
                model: "sonnet",
            },
            1,
        );

        let mut mistakes = MistakeLog::new();
        mistakes.append(mistake("p", "lifetimes", &["src/a.rs"], 1));

        let mut shas = ShouldHaveAskedLog::new();
        shas.append(ShouldHaveAsked {
            id: 0,
            project: "p".into(),
            trigger: "edit_a".into(),
            question: "q".into(),
            answer: "a".into(),
            episode_id: None,
            files: vec!["src/a.rs".into()],
            created_at: 1,
            source_project: None,
        });

        let mut calibration = CalibrationStore::new();
        for _ in 0..5 {
            calibration.record("refactor", "p", Outcome::DeclaredSuccess, 1);
        }

        let brief = build_brief(
            "p",
            &["src/a.rs".to_string()],
            Some("refactor"),
            Some("rust"),
            false,
            Some(&ask_backs),
            Some(&templates),
            Some(&mistakes),
            Some(&shas),
            Some(&calibration),
        );

        assert!(brief.pending_ask_back.is_some());
        assert!(brief.template.is_some());
        assert_eq!(brief.top_correction_categories.len(), 1);
        assert_eq!(brief.should_have_asked_triggers.len(), 1);
        assert!(
            brief.calibration_warning.is_some(),
            "0 verified / 5 declared warns"
        );
    }

    #[test]
    fn template_section_requires_both_task_type_and_domain() {
        let mut templates = TemplateStore::new();
        templates.upsert(
            TemplateUpsert {
                task_type: "refactor",
                domain: "rust",
                name: "name",
                steps: vec![],
                episode_id: "ep",
                success_rate: 0.9,
                model: "sonnet",
            },
            1,
        );
        let brief = build_brief(
            "p",
            &[],
            Some("refactor"),
            None, // domain absent
            false,
            None,
            Some(&templates),
            None,
            None,
            None,
        );
        assert!(
            brief.template.is_none(),
            "SS4.5: only surfaced when both task_type and domain are given"
        );
    }

    #[test]
    fn calibration_and_template_stay_project_scoped_even_in_cross_project_mode() {
        // Cross-project mode only affects mistakes/should-have-asked (SS4.6.1);
        // this is exercised structurally: calibration/template lookups never
        // take a cross_project parameter at all, so they cannot generalize.
        let mut calibration = CalibrationStore::new();
        for _ in 0..5 {
            calibration.record("t", "other-project", Outcome::DeclaredSuccess, 1);
        }
        let brief = build_brief(
            "p",
            &[],
            Some("t"),
            None,
            true,
            None,
            None,
            None,
            None,
            Some(&calibration),
        );
        assert!(
            brief.calibration_warning.is_none(),
            "warning for a different project must not leak in"
        );
    }
}
