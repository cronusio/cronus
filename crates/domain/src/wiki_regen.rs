//! Project-wiki regeneration pipeline.
//!
//! Office-owned, curator-driven: on a significant office change, map the change
//! to the affected page kinds, gather each page's ground truth, generate
//! client-language content, apply the grounding/honesty guards, and write the
//! whole set transactionally through the `WikiCache` seam. The client has no
//! path in here.
//!
//! Domain-logic-first via two seams: [`GroundTruth`] (no model — the sources a
//! page cites) is always present; [`PageGenerator`] (model-backed prose) is
//! optional, so a run with no generator degrades to a **grounded stub** rather
//! than fabricating. The guards — internal-detail filter and
//! citation guard — are pure pipeline steps, so the honesty properties
//! are testable without a model. This module owns the orchestration, the
//! guards, and the all-or-nothing write guarantee.

use std::sync::atomic::{AtomicU64, Ordering};

use cronus_contract::{
    WikiCache, WikiChangelogEntry, WikiCitation, WikiPage, WikiPageKind, now_secs,
};

/// A significant office change that triggers incremental regeneration.
/// Never per-agent-turn — only office events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeChange {
    BoardItemDone,
    DecisionRecorded,
    DeliverableProduced,
    MilestoneReached,
}

/// Map a change to the page kinds it affects — the "only affected pages"
/// discipline (never a full rebuild on every event). Every change touches the
/// `Overview` and `Changelog`; the rest is change-specific.
pub fn map_event_to_kinds(change: OfficeChange) -> Vec<WikiPageKind> {
    match change {
        OfficeChange::BoardItemDone => vec![
            WikiPageKind::Overview,
            WikiPageKind::Area,
            WikiPageKind::Changelog,
        ],
        OfficeChange::DecisionRecorded => vec![
            WikiPageKind::Overview,
            WikiPageKind::Decisions,
            WikiPageKind::Changelog,
        ],
        OfficeChange::DeliverableProduced => vec![
            WikiPageKind::Overview,
            WikiPageKind::Area,
            WikiPageKind::Changelog,
        ],
        OfficeChange::MilestoneReached => vec![WikiPageKind::Overview, WikiPageKind::Changelog],
    }
}

/// One candidate section of generated content: its text, the sources it cites,
/// and whether it carries internal engineering / SDD detail that must never
/// reach a client page. The generator flags internal detail; the guards
/// drop it and any uncited section before assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSection {
    pub text: String,
    pub citations: Vec<WikiCitation>,
    pub internal_detail: bool,
}

impl GeneratedSection {
    /// A normal, client-facing cited section.
    pub fn cited(text: impl Into<String>, citations: Vec<WikiCitation>) -> Self {
        GeneratedSection {
            text: text.into(),
            citations,
            internal_detail: false,
        }
    }
}

/// A generator's candidate content for one page, before guards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedContent {
    pub title: String,
    pub sections: Vec<GeneratedSection>,
}

/// Ground-truth gather (no model): the sources a page kind cites (citation basis).
/// Always available — reading records needs no generator. An empty result
/// means there is nothing grounded to write for this kind (the page is
/// skipped, never fabricated).
pub trait GroundTruth {
    fn sources(&self, office_id: &str, kind: WikiPageKind) -> Result<Vec<WikiCitation>, String>;
}

/// Client-language generation (model-backed) over the gathered sources.
/// Optional in the pipeline: absent = the no-generator degrade (a grounded
/// stub). `Err` aborts the whole regeneration before any write.
pub trait PageGenerator {
    fn generate(
        &self,
        office_id: &str,
        kind: WikiPageKind,
        sources: &[WikiCitation],
    ) -> Result<GeneratedContent, String>;
}

/// What a regeneration wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegenReport {
    /// Ids of the pages regenerated (in affected-kind order).
    pub regenerated: Vec<String>,
}

/// Internal-detail filter: drop every section carrying engineering / SDD
/// detail so it can never reach a client `wiki_page` row.
pub fn filter_internal_detail(sections: Vec<GeneratedSection>) -> Vec<GeneratedSection> {
    sections
        .into_iter()
        .filter(|s| !s.internal_detail)
        .collect()
}

/// Citation guard: drop every section that resolves to no source. An
/// uncited claim is never persisted.
pub fn citation_guard(sections: Vec<GeneratedSection>) -> Vec<GeneratedSection> {
    sections
        .into_iter()
        .filter(|s| !s.citations.is_empty())
        .collect()
}

/// Process-unique changelog-id suffix (the contract `MemoryId` id-generation
/// idiom, not per-instance state): a batch never collides its own changelog
/// primary keys even for two regenerations within the same second.
static CHANGELOG_SEQ: AtomicU64 = AtomicU64::new(0);

/// Regenerate the pages affected by `change` for one office, writing the whole
/// set transactionally. For each affected kind: gather sources; if none,
/// skip; else generate + guard (or, with no generator, a grounded stub). A
/// generation failure aborts before any write; the store write is
/// all-or-nothing.
pub fn regenerate(
    change: OfficeChange,
    office_id: &str,
    ground_truth: &dyn GroundTruth,
    generator: Option<&dyn PageGenerator>,
    cache: &dyn WikiCache,
) -> Result<RegenReport, String> {
    let (pages, changelog) = build_pages(
        &map_event_to_kinds(change),
        office_id,
        ground_truth,
        generator,
    )?;

    if !pages.is_empty() {
        cache.apply_regeneration(&pages, &changelog)?;
    }

    Ok(RegenReport {
        regenerated: pages.into_iter().map(|p| p.id).collect(),
    })
}

/// Rebuild an office's entire wiki from ground truth — the operational
/// proof the store is a rebuildable projection cache. Regenerates **every**
/// page kind (not just the ones an event touches), then hands the whole set to
/// the store's transactional drop-and-reinsert. Because every page is derived
/// from current ground truth, a dropped or corrupt `wiki.db` is reconstructed
/// into an *equivalent* wiki — same structure, same sources, same attributed
/// facts — losing nothing authoritative (the prose need not be byte-identical;
/// generation is model-based). A generation failure aborts before the store is
/// touched, so a failed rebuild leaves the prior rows intact.
pub fn rebuild(
    office_id: &str,
    ground_truth: &dyn GroundTruth,
    generator: Option<&dyn PageGenerator>,
    cache: &dyn WikiCache,
) -> Result<RegenReport, String> {
    let (pages, changelog) = build_pages(&WikiPageKind::all(), office_id, ground_truth, generator)?;

    // Always called — even with no grounded pages — so a rebuild also clears an
    // office whose ground truth no longer grounds anything.
    cache.rebuild_office(office_id, &pages, &changelog)?;

    Ok(RegenReport {
        regenerated: pages.into_iter().map(|p| p.id).collect(),
    })
}

/// Derive the pages + changelog for a set of kinds without touching the store
/// (the shared core of `regenerate` and `rebuild`). For each kind: gather
/// sources; if none, skip (never fabricate); else generate + guard, or a
/// grounded stub with no generator. A generation failure returns `Err` before
/// any page is built, so callers can write the result as one atomic unit.
fn build_pages(
    kinds: &[WikiPageKind],
    office_id: &str,
    ground_truth: &dyn GroundTruth,
    generator: Option<&dyn PageGenerator>,
) -> Result<(Vec<WikiPage>, Vec<WikiChangelogEntry>), String> {
    let at = now_secs();
    let mut pages: Vec<WikiPage> = Vec::new();
    let mut changelog: Vec<WikiChangelogEntry> = Vec::new();

    for &kind in kinds {
        let sources = ground_truth.sources(office_id, kind)?;
        if sources.is_empty() {
            // Nothing grounded to write for this kind — skip, never fabricate.
            continue;
        }

        let (title, body, citations, summary) = match generator {
            Some(g) => {
                // `?`: a generation failure returns before any page is built.
                let content = g.generate(office_id, kind, &sources)?;
                // Guards: strip internal detail then drop uncited
                // sections. What survives is client-facing and attributed.
                let kept = citation_guard(filter_internal_detail(content.sections));
                if kept.is_empty() {
                    // Everything was internal or uncited — fall back to a
                    // grounded stub rather than persisting an empty/uncited page.
                    (
                        content.title,
                        grounded_stub(kind, &sources),
                        sources.clone(),
                        format!("regenerated {} (stub — no citable content)", kind.as_str()),
                    )
                } else {
                    (
                        content.title,
                        assemble_body(&kept),
                        union_citations(&kept),
                        format!("regenerated {}", kind.as_str()),
                    )
                }
            }
            None => (
                default_title(kind),
                grounded_stub(kind, &sources),
                sources.clone(),
                format!("stub for {} (no generator)", kind.as_str()),
            ),
        };

        let page = WikiPage {
            id: format!("{office_id}:{}", kind.as_str()),
            office_id: office_id.to_string(),
            parent_id: None,
            ord: 0,
            kind,
            title,
            body,
            citations,
            source_fingerprint: fingerprint_of(&sources),
            generated_at: at,
            stale: false,
        };

        let seq = CHANGELOG_SEQ.fetch_add(1, Ordering::Relaxed);
        changelog.push(WikiChangelogEntry {
            id: format!("cl:{at}:{seq}"),
            office_id: office_id.to_string(),
            page_id: Some(page.id.clone()),
            change: summary,
            at,
        });
        pages.push(page);
    }

    Ok((pages, changelog))
}

fn assemble_body(sections: &[GeneratedSection]) -> String {
    sections
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn union_citations(sections: &[GeneratedSection]) -> Vec<WikiCitation> {
    let mut out: Vec<WikiCitation> = Vec::new();
    for s in sections {
        for c in &s.citations {
            if !out.contains(c) {
                out.push(c.clone());
            }
        }
    }
    out
}

/// The no-generator / no-citable-content body. States only grounded, true
/// facts — that the page is pending and how many sources back it — and
/// fabricates nothing.
fn grounded_stub(kind: WikiPageKind, sources: &[WikiCitation]) -> String {
    format!(
        "This {} page is not yet written in full. It is grounded in {} recorded source(s); \
         a detailed summary will appear once generation runs.",
        kind.as_str(),
        sources.len()
    )
}

fn default_title(kind: WikiPageKind) -> String {
    match kind {
        WikiPageKind::Overview => "Overview",
        WikiPageKind::Area => "Area",
        WikiPageKind::Decisions => "Decisions",
        WikiPageKind::Howto => "How-to",
        WikiPageKind::Glossary => "Glossary",
        WikiPageKind::Changelog => "Changelog",
    }
    .to_string()
}

/// A page's `source_fingerprint`: a hash of the exact sources it was
/// generated from, so a later freshness check can detect drift.
fn fingerprint_of(sources: &[WikiCitation]) -> String {
    let mut hasher = blake3::Hasher::new();
    for c in sources {
        hasher.update(c.source_kind.as_bytes());
        hasher.update(b":");
        hasher.update(c.source_id.as_bytes());
        hasher.update(b";");
    }
    hasher.finalize().to_hex().to_string()
}

/// Freshness sweep: for every non-stale page, recompute the current
/// fingerprint of its sources and, if it differs from the stored one, mark the
/// page stale — its sources moved without a regeneration catching up. Returns
/// the ids newly marked stale. This never regenerates or mutates content; it
/// only flips the honest "may be out of date" marker so the UI never presents
/// a drifted page silently as current.
pub fn check_freshness(
    office_id: &str,
    ground_truth: &dyn GroundTruth,
    cache: &dyn WikiCache,
) -> Result<Vec<String>, String> {
    let mut drifted: Vec<String> = Vec::new();
    for page in cache.pages_for_office(office_id)? {
        if page.stale {
            continue; // already flagged — nothing to do
        }
        let current = fingerprint_of(&ground_truth.sources(office_id, page.kind)?);
        if current != page.source_fingerprint {
            drifted.push(page.id);
        }
    }
    if !drifted.is_empty() {
        cache.mark_stale(&drifted)?;
    }
    Ok(drifted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A cache that records every applied batch (and never fails).
    #[derive(Default)]
    struct RecordingCache {
        applied: RefCell<Vec<Vec<WikiPage>>>,
        /// Pages the store already holds (for `pages_for_office` / freshness).
        seeded: RefCell<Vec<WikiPage>>,
        /// Ids passed to `mark_stale`, in order.
        marked: RefCell<Vec<String>>,
    }

    impl WikiCache for RecordingCache {
        fn get_page(&self, id: &str) -> Result<Option<WikiPage>, String> {
            Ok(self.seeded.borrow().iter().find(|p| p.id == id).cloned())
        }
        fn apply_regeneration(
            &self,
            pages: &[WikiPage],
            _changelog: &[WikiChangelogEntry],
        ) -> Result<(), String> {
            self.applied.borrow_mut().push(pages.to_vec());
            Ok(())
        }
        fn rebuild_office(
            &self,
            office_id: &str,
            pages: &[WikiPage],
            _changelog: &[WikiChangelogEntry],
        ) -> Result<(), String> {
            // Drop this office's rows, then install the rebuilt set — the
            // in-memory mirror of the store's transactional drop-and-reinsert.
            self.seeded
                .borrow_mut()
                .retain(|p| p.office_id != office_id);
            self.seeded.borrow_mut().extend_from_slice(pages);
            self.applied.borrow_mut().push(pages.to_vec());
            Ok(())
        }
        fn pages_for_office(&self, office_id: &str) -> Result<Vec<WikiPage>, String> {
            Ok(self
                .seeded
                .borrow()
                .iter()
                .filter(|p| p.office_id == office_id)
                .cloned()
                .collect())
        }
        fn mark_stale(&self, page_ids: &[String]) -> Result<(), String> {
            self.marked.borrow_mut().extend_from_slice(page_ids);
            for page in self.seeded.borrow_mut().iter_mut() {
                if page_ids.contains(&page.id) {
                    page.stale = true;
                }
            }
            Ok(())
        }
        fn changelog(
            &self,
            _office_id: &str,
            _limit: usize,
        ) -> Result<Vec<WikiChangelogEntry>, String> {
            Ok(vec![])
        }
    }

    impl RecordingCache {
        fn last_batch(&self) -> Vec<WikiPage> {
            self.applied.borrow().last().cloned().unwrap_or_default()
        }
        fn page(&self, kind: WikiPageKind) -> Option<WikiPage> {
            self.last_batch().into_iter().find(|p| p.kind == kind)
        }
    }

    /// Ground truth: one source per kind (so nothing is skipped), unless a kind
    /// is in `empty` (→ no sources → skipped).
    struct FixedGroundTruth {
        empty: Vec<WikiPageKind>,
    }
    impl GroundTruth for FixedGroundTruth {
        fn sources(
            &self,
            _office_id: &str,
            kind: WikiPageKind,
        ) -> Result<Vec<WikiCitation>, String> {
            if self.empty.contains(&kind) {
                Ok(vec![])
            } else {
                Ok(vec![WikiCitation::new(
                    "decision",
                    format!("d-{}", kind.as_str()),
                )])
            }
        }
    }

    fn ground() -> FixedGroundTruth {
        FixedGroundTruth { empty: vec![] }
    }

    /// A generator whose per-kind output is scripted by a closure.
    struct ScriptedGenerator<F: Fn(WikiPageKind) -> Result<GeneratedContent, String>>(F);
    impl<F: Fn(WikiPageKind) -> Result<GeneratedContent, String>> PageGenerator
        for ScriptedGenerator<F>
    {
        fn generate(
            &self,
            _office_id: &str,
            kind: WikiPageKind,
            _sources: &[WikiCitation],
        ) -> Result<GeneratedContent, String> {
            (self.0)(kind)
        }
    }

    #[test]
    fn event_maps_to_only_the_affected_kinds() {
        let kinds = map_event_to_kinds(OfficeChange::DecisionRecorded);
        assert_eq!(
            kinds,
            vec![
                WikiPageKind::Overview,
                WikiPageKind::Decisions,
                WikiPageKind::Changelog
            ]
        );
        assert!(!kinds.contains(&WikiPageKind::Glossary));
        assert!(!kinds.contains(&WikiPageKind::Howto));
    }

    #[test]
    fn regenerates_only_the_affected_pages_transactionally() {
        let cache = RecordingCache::default();
        let generator = ScriptedGenerator(|kind| {
            Ok(GeneratedContent {
                title: kind.as_str().to_string(),
                sections: vec![GeneratedSection::cited(
                    format!("body for {}", kind.as_str()),
                    vec![WikiCitation::new("decision", "d1")],
                )],
            })
        });

        let report = regenerate(
            OfficeChange::DecisionRecorded,
            "office-1",
            &ground(),
            Some(&generator),
            &cache,
        )
        .expect("regeneration succeeds");

        assert_eq!(
            report.regenerated,
            vec![
                "office-1:overview",
                "office-1:decisions",
                "office-1:changelog"
            ]
        );
        assert_eq!(cache.applied.borrow().len(), 1, "one transactional batch");
    }

    #[test]
    fn an_uncited_section_is_dropped_and_never_persisted() {
        let cache = RecordingCache::default();
        let generator = ScriptedGenerator(|kind| {
            Ok(GeneratedContent {
                title: kind.as_str().to_string(),
                sections: vec![
                    GeneratedSection::cited(
                        "CITED FACT",
                        vec![WikiCitation::new("decision", "d1")],
                    ),
                    // No citations → must be dropped.
                    GeneratedSection {
                        text: "UNCITED CLAIM".to_string(),
                        citations: vec![],
                        internal_detail: false,
                    },
                ],
            })
        });

        regenerate(
            OfficeChange::MilestoneReached,
            "office-1",
            &ground(),
            Some(&generator),
            &cache,
        )
        .expect("succeeds");

        let overview = cache.page(WikiPageKind::Overview).expect("overview page");
        assert!(overview.body.contains("CITED FACT"));
        assert!(
            !overview.body.contains("UNCITED CLAIM"),
            "an uncited claim must never be persisted"
        );
    }

    #[test]
    fn an_internal_detail_section_never_reaches_a_row() {
        let cache = RecordingCache::default();
        let generator = ScriptedGenerator(|kind| {
            Ok(GeneratedContent {
                title: kind.as_str().to_string(),
                sections: vec![
                    GeneratedSection::cited(
                        "CLIENT FACT",
                        vec![WikiCitation::new("board_item", "c1")],
                    ),
                    // Internal engineering detail — must be filtered.
                    GeneratedSection {
                        text: "SDD internal: crate topology guard".to_string(),
                        citations: vec![WikiCitation::new("decision", "d9")],
                        internal_detail: true,
                    },
                ],
            })
        });

        regenerate(
            OfficeChange::MilestoneReached,
            "office-1",
            &ground(),
            Some(&generator),
            &cache,
        )
        .expect("succeeds");

        let overview = cache.page(WikiPageKind::Overview).expect("overview page");
        assert!(overview.body.contains("CLIENT FACT"));
        assert!(
            !overview.body.contains("SDD internal"),
            "internal engineering detail must never reach a wiki row"
        );
    }

    #[test]
    fn no_generator_mode_stores_a_grounded_stub_and_never_fabricates() {
        let cache = RecordingCache::default();

        regenerate(
            OfficeChange::MilestoneReached,
            "office-1",
            &ground(),
            None, // no generator bound
            &cache,
        )
        .expect("succeeds");

        let overview = cache.page(WikiPageKind::Overview).expect("overview page");
        // The stub is exactly the grounded template — no invented prose — and
        // it carries the gathered sources so it is attributable.
        assert_eq!(
            overview.body,
            grounded_stub(
                WikiPageKind::Overview,
                &[WikiCitation::new("decision", "d-overview")]
            )
        );
        assert_eq!(overview.citations.len(), 1);
        assert!(!overview.source_fingerprint.is_empty());
    }

    #[test]
    fn all_sections_dropped_falls_back_to_a_grounded_stub() {
        let cache = RecordingCache::default();
        let generator = ScriptedGenerator(|kind| {
            Ok(GeneratedContent {
                title: kind.as_str().to_string(),
                // Only an uncited section → dropped → empty → stub.
                sections: vec![GeneratedSection {
                    text: "UNCITED".to_string(),
                    citations: vec![],
                    internal_detail: false,
                }],
            })
        });

        regenerate(
            OfficeChange::MilestoneReached,
            "office-1",
            &ground(),
            Some(&generator),
            &cache,
        )
        .expect("succeeds");

        let overview = cache.page(WikiPageKind::Overview).expect("overview page");
        assert!(!overview.body.contains("UNCITED"));
        assert!(overview.body.contains("grounded in 1 recorded source"));
    }

    #[test]
    fn a_kind_with_no_ground_truth_is_skipped() {
        let cache = RecordingCache::default();
        let gt = FixedGroundTruth {
            empty: vec![WikiPageKind::Decisions],
        };
        let generator = ScriptedGenerator(|kind| {
            Ok(GeneratedContent {
                title: kind.as_str().to_string(),
                sections: vec![GeneratedSection::cited(
                    "x",
                    vec![WikiCitation::new("decision", "d1")],
                )],
            })
        });

        let report = regenerate(
            OfficeChange::DecisionRecorded,
            "office-1",
            &gt,
            Some(&generator),
            &cache,
        )
        .expect("succeeds");

        assert_eq!(
            report.regenerated,
            vec!["office-1:overview", "office-1:changelog"],
            "the ungrounded decisions page is skipped, not fabricated"
        );
    }

    #[test]
    fn a_generation_failure_aborts_before_any_write() {
        let cache = RecordingCache::default();
        let generator = ScriptedGenerator(|kind| {
            if kind == WikiPageKind::Decisions {
                Err("generation failed".to_string())
            } else {
                Ok(GeneratedContent {
                    title: kind.as_str().to_string(),
                    sections: vec![GeneratedSection::cited(
                        "x",
                        vec![WikiCitation::new("decision", "d1")],
                    )],
                })
            }
        });

        let result = regenerate(
            OfficeChange::DecisionRecorded,
            "office-1",
            &ground(),
            Some(&generator),
            &cache,
        );

        assert!(result.is_err());
        assert!(
            cache.applied.borrow().is_empty(),
            "nothing written — prior rows stay intact"
        );
    }

    #[test]
    fn a_source_moving_without_regeneration_flips_the_page_stale() {
        // Seed one page whose stored fingerprint was computed from source `d1`.
        let cache = RecordingCache::default();
        let mut page = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "O",
            "body",
        );
        page.source_fingerprint = fingerprint_of(&[WikiCitation::new("decision", "d1")]);
        cache.seeded.borrow_mut().push(page);

        // Ground truth now yields a DIFFERENT source (`d-overview`) → the
        // current fingerprint differs → the page has drifted.
        let drifted = check_freshness("office-1", &ground(), &cache).expect("sweep");
        assert_eq!(drifted, vec!["office-1:overview"]);
        assert_eq!(cache.marked.borrow().as_slice(), ["office-1:overview"]);
        assert!(
            cache.get_page("office-1:overview").unwrap().unwrap().stale,
            "the drifted page is now marked stale"
        );

        // Idempotent: a second sweep marks nothing new (already stale).
        assert!(
            check_freshness("office-1", &ground(), &cache)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_page_whose_sources_are_unchanged_stays_fresh() {
        let cache = RecordingCache::default();
        let mut page = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "O",
            "body",
        );
        // Stored fingerprint matches exactly what ground() will report for
        // the Overview kind (`d-overview`), so there is no drift.
        page.source_fingerprint = fingerprint_of(&[WikiCitation::new("decision", "d-overview")]);
        cache.seeded.borrow_mut().push(page);

        assert!(
            check_freshness("office-1", &ground(), &cache)
                .unwrap()
                .is_empty()
        );
        assert!(!cache.get_page("office-1:overview").unwrap().unwrap().stale);
    }

    #[test]
    fn rebuild_regenerates_every_page_kind_from_ground_truth() {
        // Unlike an event (which touches only affected kinds), a rebuild
        // re-derives all six kinds — the whole projection from ground truth.
        let cache = RecordingCache::default();
        let generator = ScriptedGenerator(|kind| {
            Ok(GeneratedContent {
                title: kind.as_str().to_string(),
                sections: vec![GeneratedSection::cited(
                    format!("body for {}", kind.as_str()),
                    vec![WikiCitation::new("decision", "d1")],
                )],
            })
        });

        let report =
            rebuild("office-1", &ground(), Some(&generator), &cache).expect("rebuild succeeds");

        let mut kinds: Vec<String> = report.regenerated.clone();
        kinds.sort();
        assert_eq!(
            kinds,
            vec![
                "office-1:area",
                "office-1:changelog",
                "office-1:decisions",
                "office-1:glossary",
                "office-1:howto",
                "office-1:overview",
            ],
            "every page kind is re-derived"
        );
        assert_eq!(cache.applied.borrow().len(), 1, "one rebuild batch");
    }

    #[test]
    fn rebuild_replaces_the_prior_office_rows() {
        // A stale prior page for the office is dropped and re-derived — the
        // rebuilt row carries current ground truth, not the old body.
        let cache = RecordingCache::default();
        let stale = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "Stale",
            "OUTDATED BODY",
        );
        cache.seeded.borrow_mut().push(stale);

        rebuild("office-1", &ground(), None, &cache).expect("rebuild");

        let pages = cache.pages_for_office("office-1").unwrap();
        let overview = pages
            .iter()
            .find(|p| p.id == "office-1:overview")
            .expect("overview present");
        assert!(
            !overview.body.contains("OUTDATED BODY"),
            "the prior stale body was replaced by a re-derived one"
        );
        // Every rebuilt page is grounded (non-empty citations), proving the set
        // is a projection of ground truth, not carried over.
        assert!(pages.iter().all(|p| !p.citations.is_empty()));
    }

    #[test]
    fn rebuild_aborts_before_touching_the_store_on_generation_failure() {
        // A generation failure must leave the prior projection intact — the
        // office is never cleared on a rebuild that can't complete.
        let cache = RecordingCache::default();
        let prior = WikiPage::new(
            "office-1:overview",
            "office-1",
            WikiPageKind::Overview,
            "Prior",
            "still here",
        );
        cache.seeded.borrow_mut().push(prior);

        let generator = ScriptedGenerator(|kind| {
            if kind == WikiPageKind::Glossary {
                Err("generation failed".to_string())
            } else {
                Ok(GeneratedContent {
                    title: kind.as_str().to_string(),
                    sections: vec![GeneratedSection::cited(
                        "x",
                        vec![WikiCitation::new("decision", "d1")],
                    )],
                })
            }
        });

        let result = rebuild("office-1", &ground(), Some(&generator), &cache);
        assert!(result.is_err());
        assert!(
            cache.applied.borrow().is_empty(),
            "no rebuild batch was applied"
        );
        assert_eq!(
            cache.get_page("office-1:overview").unwrap().unwrap().body,
            "still here",
            "the prior projection is intact — the office was never cleared"
        );
    }
}
