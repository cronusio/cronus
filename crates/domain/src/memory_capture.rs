//! The write-time content transforms that complete the
//! capture path an earlier stage deferred. The gate/dedup/cross-ref step needs
//! transactional multi-table writes and lives in the store tier
//! (`cronus-store-local::memory::capture`) — this module is the zero-I/O
//! half: pure content shaping, no schema, no DB access, composing with a
//! generator seam that has no implementor bound this phase (matching the
//! `answer` extractive degrade and the community detection: a documented
//! seam, never a fabricated model behavior).

// ── Raw vs inferred capture mode ─────────────────────────────────────

/// A per-write mode flag. `Inferred` is the default; `Raw` is the
/// local-first / audit-exact / no-generator escape hatch and MUST function
/// with no model bound — it never touches the generator seam at all, not
/// even for normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptureMode {
    #[default]
    Inferred,
    Raw,
}

/// The one model-dependent seam this module composes with (temporal
/// normalization, inferred extraction). No implementor is wired this
/// phase — every caller here exercises [`NoGenerator`], the degrade path
/// both invariants must prove.
pub trait ContentGenerator {
    /// Rewrite relative temporal expressions in `content` against
    /// `observation_instant`. `None` (no generator bound) means the caller
    /// falls back to storing verbatim — never a fabricated date.
    fn normalize_temporal(&self, content: &str, observation_instant: u64) -> Option<String>;
    /// `inferred` mode: extract salient facts. `None` (no generator
    /// bound) means the caller falls back to the input verbatim.
    fn extract_salient(&self, content: &str) -> Option<String>;
}

/// The always-absent generator — the concrete degrade path every capture
/// exercises until a real generator is wired.
pub struct NoGenerator;

impl ContentGenerator for NoGenerator {
    fn normalize_temporal(&self, _content: &str, _observation_instant: u64) -> Option<String> {
        None
    }
    fn extract_salient(&self, _content: &str) -> Option<String> {
        None
    }
}

/// Temporal normalization and inferred extraction composed: shape `content` for capture under `mode`. `Raw`
/// never calls the generator at all (the requirement's own text: raw "MUST function
/// with no model bound," realized here as never *asking* rather than asking
/// and falling back). `Inferred` normalizes first, then extracts; either
/// step degrades to its input verbatim when the generator returns `None`.
pub fn prepare_capture_body(
    generator: &dyn ContentGenerator,
    content: &str,
    mode: CaptureMode,
    observation_instant: u64,
) -> String {
    match mode {
        CaptureMode::Raw => content.to_string(),
        CaptureMode::Inferred => {
            let normalized = generator
                .normalize_temporal(content, observation_instant)
                .unwrap_or_else(|| content.to_string());
            generator.extract_salient(&normalized).unwrap_or(normalized)
        }
    }
}

// ── Caller capture directives ────────────────────────────────────────

/// Optional caller-scoped steering over what capture emphasizes. Absent
/// (`Default`) is baseline — [`apply_directives`] returns `content`
/// unchanged.
#[derive(Debug, Clone, Default)]
pub struct CaptureDirectives {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub custom_instruction: Option<String>,
}

/// A small closed keyword set naming content this module treats as
/// safety-relevant — deliberately deterministic (no model), the same
/// keyword-pattern-matching shape `autonomy::classify_command` already uses
/// for command-risk classification, applied here to captured content.
const SAFETY_KEYWORDS: &[&str] = &[
    "danger",
    "warning",
    "critical",
    "vulnerability",
    "unsafe",
    "hazard",
];

fn is_safety_relevant(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    SAFETY_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

fn contains_any(sentence: &str, terms: &[String]) -> bool {
    let lower = sentence.to_lowercase();
    terms.iter().any(|t| lower.contains(&t.to_lowercase()))
}

/// The result of applying directives: the steered content plus a plain
/// description of what happened — "recorded as capture provenance."
/// This function persists nothing itself (domain tier, zero I/O); the
/// caller decides where `provenance` goes.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveOutcome {
    pub content: String,
    pub provenance: String,
}

/// Steer emphasis via `include`/`exclude`/`custom_instruction`
/// without ever lowering the honesty floor or suppressing a
/// safety-relevant fact — both negative invariants are enforced here, not
/// left to the caller. `exclude` drops a matching sentence unless it is
/// also safety-relevant, in which case it is retained regardless.
/// `include` steers emphasis by moving matching sentences first — a
/// deterministic proxy for "emphasis" with no model bound, not a
/// truncation or priority-drop mechanism.
pub fn apply_directives(content: &str, directives: &CaptureDirectives) -> DirectiveOutcome {
    if directives.include.is_empty()
        && directives.exclude.is_empty()
        && directives.custom_instruction.is_none()
    {
        return DirectiveOutcome {
            content: content.to_string(),
            provenance: "no directives applied (baseline)".to_string(),
        };
    }

    let sentences: Vec<&str> = content
        .split_terminator('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let mut kept: Vec<&str> = Vec::new();
    let mut safety_retained = 0usize;
    for sentence in sentences {
        let excluded = contains_any(sentence, &directives.exclude);
        if excluded && is_safety_relevant(sentence) {
            safety_retained += 1;
            kept.push(sentence);
        } else if excluded {
            continue;
        } else {
            kept.push(sentence);
        }
    }

    if !directives.include.is_empty() {
        let (matched, rest): (Vec<&str>, Vec<&str>) = kept
            .into_iter()
            .partition(|s| contains_any(s, &directives.include));
        kept = matched.into_iter().chain(rest).collect();
    }

    let shaped = if kept.is_empty() {
        String::new()
    } else {
        format!("{}.", kept.join(". "))
    };

    let mut provenance = format!(
        "directives applied: include={:?} exclude={:?} custom={:?}",
        directives.include, directives.exclude, directives.custom_instruction
    );
    if safety_retained > 0 {
        provenance.push_str(&format!(
            "; {safety_retained} safety-relevant sentence(s) retained despite an exclude directive"
        ));
    }

    DirectiveOutcome {
        content: shaped,
        provenance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Raw vs inferred ───────────────────────────────────────────

    #[test]
    fn raw_mode_never_touches_the_generator_and_is_immediately_usable_with_none_bound() {
        let body = prepare_capture_body(&NoGenerator, "meet next Tuesday", CaptureMode::Raw, 1000);
        assert_eq!(body, "meet next Tuesday");
    }

    #[test]
    fn inferred_mode_with_no_generator_degrades_to_verbatim_never_fabricating() {
        let body = prepare_capture_body(
            &NoGenerator,
            "meet next Tuesday",
            CaptureMode::Inferred,
            1000,
        );
        assert_eq!(
            body, "meet next Tuesday",
            "no generator bound must never fabricate a normalized date or extracted summary"
        );
    }

    #[test]
    fn inferred_is_the_default_mode() {
        assert_eq!(CaptureMode::default(), CaptureMode::Inferred);
    }

    // ── Temporal normalization composes through the same seam ────

    struct StubGenerator;
    impl ContentGenerator for StubGenerator {
        fn normalize_temporal(&self, content: &str, _observation_instant: u64) -> Option<String> {
            Some(content.replace("next Tuesday", "2026-07-14"))
        }
        fn extract_salient(&self, content: &str) -> Option<String> {
            Some(content.to_string())
        }
    }

    #[test]
    fn a_bound_generator_normalizes_relative_dates_in_inferred_mode() {
        let body = prepare_capture_body(
            &StubGenerator,
            "meet next Tuesday",
            CaptureMode::Inferred,
            1000,
        );
        assert_eq!(body, "meet 2026-07-14");
    }

    #[test]
    fn a_bound_generator_is_never_consulted_in_raw_mode() {
        // StubGenerator would rewrite the date; raw must not call it at all.
        let body =
            prepare_capture_body(&StubGenerator, "meet next Tuesday", CaptureMode::Raw, 1000);
        assert_eq!(body, "meet next Tuesday");
    }

    // ── Capture directives ────────────────────────────────────────

    #[test]
    fn absent_directives_is_baseline_unchanged_content() {
        let out = apply_directives("fact one. fact two.", &CaptureDirectives::default());
        assert_eq!(out.content, "fact one. fact two.");
        assert_eq!(out.provenance, "no directives applied (baseline)");
    }

    #[test]
    fn exclude_drops_a_matching_ordinary_sentence() {
        let directives = CaptureDirectives {
            exclude: vec!["irrelevant".to_string()],
            ..Default::default()
        };
        let out = apply_directives("useful fact. an irrelevant aside.", &directives);
        assert_eq!(out.content, "useful fact.");
    }

    #[test]
    fn exclude_never_suppresses_a_safety_relevant_sentence() {
        let directives = CaptureDirectives {
            exclude: vec!["deploy".to_string()],
            ..Default::default()
        };
        let out = apply_directives(
            "useful fact. warning: deploy without the flag is unsafe.",
            &directives,
        );
        assert!(
            out.content
                .contains("warning: deploy without the flag is unsafe"),
            "a safety-relevant sentence must survive an exclude directive naming it"
        );
        assert!(out.provenance.contains("safety-relevant"));
    }

    #[test]
    fn include_moves_matching_sentences_first_without_dropping_anything() {
        let directives = CaptureDirectives {
            include: vec!["priority".to_string()],
            ..Default::default()
        };
        let out = apply_directives("background note. the priority item.", &directives);
        assert_eq!(out.content, "the priority item. background note.");
    }

    #[test]
    fn directives_never_touch_confidence_the_mi6_honesty_floor_is_a_separate_field() {
        // Compile-time + structural proof: apply_directives has no
        // confidence parameter and returns no confidence value — a
        // directive cannot express "raise my confidence," by construction.
        let directives = CaptureDirectives {
            include: vec!["anything".to_string()],
            ..Default::default()
        };
        let out = apply_directives("a low-confidence guess.", &directives);
        assert!(out.content.contains("a low-confidence guess"));
    }
}
