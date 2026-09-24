//! Parsing and validating usage-simulation scenario files.
//!
//! Two rules govern the format: intent is fixed and the route is never
//! prescribed — the format has no field a command can be written into — and
//! an obligation that cannot fail is not an obligation: the
//! validator refuses a scenario whose obligation set could never produce a
//! failing run, at parse time, as an error rather than a silent skip.
//!
//! A scenario file is Markdown with a TOML frontmatter block:
//! a `---`-delimited TOML header carrying every
//! machine-checkable field, followed by Markdown prose (persona, goal,
//! notes) this module treats as an opaque body — compressing a persona into
//! typed fields is exactly what would turn it back into a script.

use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;

/// A scenario file structure or content error. Refused, never silently
/// skipped — the whole point is that an obligation set which cannot
/// fail is not an obligation set, and a parser that let one through would
/// be the thing failing to fail.
#[derive(Debug)]
pub enum ScenarioError {
    /// The file is not shaped like a scenario at all (missing or unclosed
    /// frontmatter delimiters).
    Structure(String),
    /// The frontmatter's TOML is malformed, or carries an unknown key, or
    /// is missing a required field (including a missing `bound` table) —
    /// `toml`/`serde`'s own error already names the offending key.
    Parse(String),
    /// The frontmatter parsed, but its obligations fail one of the
    /// falsifiability checks.
    Validation(String),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScenarioError::Structure(msg) => write!(f, "scenario structure error: {msg}"),
            ScenarioError::Parse(msg) => write!(f, "scenario frontmatter error: {msg}"),
            ScenarioError::Validation(msg) => write!(f, "scenario validation error: {msg}"),
        }
    }
}

impl std::error::Error for ScenarioError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Short,
    Broad,
    Exhaustive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Vantage {
    #[serde(rename = "none")]
    Nothing,
    BuiltinHelp,
    PublishedDocs,
    PriorUse,
}

/// The starting state a world is built into. Named `WorldKind` — not
/// `World` — to avoid colliding with [`crate::world::World`], the
/// disposable-world *implementation*; this is only the scenario's
/// *declaration* of which kind of world it wants built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldKind {
    Fresh,
    SeededSmall,
    SeededLarge,
    Corrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bound {
    pub steps: u32,
    pub wall_secs: u32,
    pub spend_usd: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Perturbation {
    pub class: String,
    pub at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecidedBy {
    ObservationOfOutput,
    ObservationOfState,
    Judgement,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obligation {
    pub id: String,
    pub statement: String,
    pub decided_by: DecidedBy,
    pub evidence: String,
    #[serde(default)]
    pub positive_control: Option<String>,
}

/// The frontmatter as it appears on disk — every field the scenario file
/// format admits, and nothing else (`deny_unknown_fields` is what makes an
/// unrecognized key a refusal rather than a silently ignored typo).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScenario {
    id: String,
    tier: Tier,
    surfaces: Vec<String>,
    covers: Vec<String>,
    roles: Vec<String>,
    vantage: Vantage,
    world: WorldKind,
    bound: Bound,
    #[serde(default, rename = "perturbation")]
    perturbation: Vec<Perturbation>,
    #[serde(default, rename = "obligation")]
    obligation: Vec<Obligation>,
}

/// A parsed, validated scenario.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub id: String,
    pub tier: Tier,
    pub surfaces: Vec<String>,
    pub covers: Vec<String>,
    pub roles: Vec<String>,
    pub vantage: Vantage,
    pub world: WorldKind,
    pub bound: Bound,
    pub perturbations: Vec<Perturbation>,
    pub obligations: Vec<Obligation>,
    /// Everything after the closing `---` delimiter, verbatim. The format
    /// deliberately leaves the persona/goal/notes prose unstructured.
    pub body: String,
}

impl Scenario {
    /// A scenario declaring no perturbation is a smoke test, and
    /// must be labelled as one rather than counted as simulation coverage.
    pub fn is_smoke(&self) -> bool {
        self.perturbations.is_empty()
    }

    /// The disclosed judged-to-runnable ratio of this scenario's
    /// obligation set.
    pub fn judged_ratio(&self) -> f64 {
        if self.obligations.is_empty() {
            return 0.0;
        }
        let judged = self
            .obligations
            .iter()
            .filter(|o| o.decided_by == DecidedBy::Judgement)
            .count();
        judged as f64 / self.obligations.len() as f64
    }
}

/// Parse and validate a scenario file's text. Refuses (never skips) a
/// structurally malformed file, a frontmatter with an unknown or missing
/// field, or an obligation set that fails the falsifiability checks.
pub fn parse(text: &str) -> Result<Scenario, ScenarioError> {
    let (frontmatter, body) = split_frontmatter(text)?;
    let raw: RawScenario =
        toml::from_str(frontmatter).map_err(|err| ScenarioError::Parse(err.to_string()))?;
    validate(&raw)?;

    Ok(Scenario {
        id: raw.id,
        tier: raw.tier,
        surfaces: raw.surfaces,
        covers: raw.covers,
        roles: raw.roles,
        vantage: raw.vantage,
        world: raw.world,
        bound: raw.bound,
        perturbations: raw.perturbation,
        obligations: raw.obligation,
        body,
    })
}

/// Split `text` into its `---`-delimited TOML frontmatter and the Markdown
/// body that follows the closing delimiter.
fn split_frontmatter(text: &str) -> Result<(&str, String), ScenarioError> {
    let mut offset = 0usize;
    let mut lines = text.split_inclusive('\n');

    match lines.next() {
        Some(line) if line.trim_end_matches(['\r', '\n']).trim() == "---" => {
            offset += line.len();
        }
        _ => {
            return Err(ScenarioError::Structure(
                "a scenario file must open with a `---` frontmatter delimiter".to_string(),
            ));
        }
    }

    let frontmatter_start = offset;
    for line in lines {
        if line.trim_end_matches(['\r', '\n']).trim() == "---" {
            let frontmatter_end = offset;
            let body_start = offset + line.len();
            let body = text
                .get(body_start..)
                .unwrap_or("")
                .trim_start()
                .to_string();
            let frontmatter = &text[frontmatter_start..frontmatter_end];
            return Ok((frontmatter, body));
        }
        offset += line.len();
    }

    Err(ScenarioError::Structure(
        "the scenario file's frontmatter has no closing `---` delimiter".to_string(),
    ))
}

const ACTIVITY_VERBS: &[&str] = &["exercise", "check", "improve", "review", "handle"];
const INDISTINCT_SIGNALS: &[&str] = &["ok", "done", "passed", "complete", "0"];
const NEGATION_MARKERS: &[&str] = &[
    " no ",
    " not ",
    " never ",
    "n't",
    " without ",
    " absent",
    " none",
];

fn validate(raw: &RawScenario) -> Result<(), ScenarioError> {
    if raw.obligation.is_empty() {
        return Err(ScenarioError::Validation(
            "the obligation set is empty — nothing here can fail, so nothing here can be checked"
                .to_string(),
        ));
    }

    let mut seen_ids: HashSet<&str> = HashSet::new();
    for obligation in &raw.obligation {
        if !seen_ids.insert(obligation.id.as_str()) {
            return Err(ScenarioError::Validation(format!(
                "duplicate obligation id `{}` — every obligation must be uniquely identified",
                obligation.id
            )));
        }
    }

    for obligation in &raw.obligation {
        if obligation.evidence.trim().is_empty() {
            return Err(ScenarioError::Validation(format!(
                "obligation `{}` has no evidence — a runnable obligation must declare what must \
                 be shown for its verdict to count",
                obligation.id
            )));
        }

        if is_activity_shaped(&obligation.statement) {
            return Err(ScenarioError::Validation(format!(
                "obligation `{}` is activity-shaped (\"{}\") — it names something to do, not an \
                 observable outcome that could fail",
                obligation.id, obligation.statement
            )));
        }

        if is_indistinct_signal(&obligation.evidence) {
            return Err(ScenarioError::Validation(format!(
                "obligation `{}` has an indistinct pass signal (\"{}\") — failure output uses \
                 this vocabulary just as readily as success does",
                obligation.id, obligation.evidence
            )));
        }

        if is_absence_claim(&obligation.statement)
            && obligation
                .positive_control
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            return Err(ScenarioError::Validation(format!(
                "obligation `{}` asserts an absence (\"{}\") with no positive_control — an \
                 untested absence check is indistinguishable from one looking in the wrong place",
                obligation.id, obligation.statement
            )));
        }
    }

    Ok(())
}

fn is_activity_shaped(statement: &str) -> bool {
    let first_word = statement
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    ACTIVITY_VERBS.contains(&first_word.as_str())
}

fn is_indistinct_signal(evidence: &str) -> bool {
    let normalized: String = evidence
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    INDISTINCT_SIGNALS.contains(&normalized.trim())
}

fn is_absence_claim(statement: &str) -> bool {
    let padded = format!(" {} ", statement.to_lowercase());
    NEGATION_MARKERS
        .iter()
        .any(|marker| padded.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"---
id       = "first-run-three-boards"
tier     = "short"
surfaces = ["cli", "tui"]
covers   = ["board.create", "board.list"]
roles    = ["owner"]
vantage  = "builtin-help"
world    = "fresh"
bound    = { steps = 60, wall_secs = 900, spend_usd = 2.00 }

[[perturbation]]
class = "reversal"
at    = "after the second board exists"

[[obligation]]
id         = "goal-reachable"
statement  = "all three boards exist and are listed by the product at the end of the run"
decided_by = "observation-of-state"
evidence   = "a listing invocation whose output names all three"

[[obligation]]
id         = "no-silent-failure"
statement  = "no invocation exits non-zero without naming a cause on stderr"
decided_by = "observation-of-output"
evidence   = "every non-zero exit in the transcript has non-empty stderr naming what failed"
positive_control = "replays/known-silent-exit.toml"
---

## Persona

Someone new to the product, tracking three pieces of work at once.

## Goal

Keep three things going, then stop caring about one of them.
"#;

    #[test]
    fn a_well_formed_scenario_parses_into_the_expected_typed_value() {
        let scenario = parse(VALID).expect("the valid fixture must parse");
        assert_eq!(scenario.id, "first-run-three-boards");
        assert_eq!(scenario.tier, Tier::Short);
        assert_eq!(scenario.surfaces, vec!["cli", "tui"]);
        assert_eq!(scenario.vantage, Vantage::BuiltinHelp);
        assert_eq!(scenario.world, WorldKind::Fresh);
        assert_eq!(scenario.bound.steps, 60);
        assert_eq!(scenario.obligations.len(), 2);
        assert!(
            !scenario.is_smoke(),
            "the fixture declares one perturbation"
        );
        assert!(scenario.body.contains("## Persona"));
        assert!(scenario.body.contains("## Goal"));
    }

    #[test]
    fn zero_perturbations_still_parses_and_is_reported_as_smoke() {
        let smoke = VALID.replace(
            "[[perturbation]]\nclass = \"reversal\"\nat    = \"after the second board exists\"\n\n",
            "",
        );
        let scenario = parse(&smoke).expect("a scenario with no perturbations still parses");
        assert!(scenario.perturbations.is_empty());
        assert!(scenario.is_smoke());
    }

    #[test]
    fn an_unknown_top_level_key_is_refused() {
        let bogus = VALID.replacen("id       =", "bogus_key = \"x\"\nid       =", 1);
        let err = parse(&bogus).expect_err("an unknown key must be refused");
        assert!(matches!(err, ScenarioError::Parse(_)));
        assert!(
            err.to_string().contains("bogus_key"),
            "message must name the unknown key: {err}"
        );
    }

    #[test]
    fn a_missing_bound_table_is_refused() {
        let no_bound = VALID
            .lines()
            .filter(|l| !l.starts_with("bound"))
            .collect::<Vec<_>>()
            .join("\n");
        let err = parse(&no_bound).expect_err("a scenario with no bound must be refused");
        assert!(matches!(err, ScenarioError::Parse(_)));
        assert!(
            err.to_string().contains("bound"),
            "message must name the missing field: {err}"
        );
    }

    #[test]
    fn an_empty_obligation_set_is_refused() {
        let (frontmatter_only, _) = VALID.split_once("[[obligation]]").unwrap();
        let no_obligations = format!("{frontmatter_only}---\n");
        let err = parse(&no_obligations).expect_err("an empty obligation set must be refused");
        match err {
            ScenarioError::Validation(msg) => assert!(msg.contains("empty")),
            other => panic!("expected Validation, got {other}"),
        }
    }

    #[test]
    fn a_duplicate_obligation_id_is_refused() {
        let dup = VALID.replace(
            "id         = \"no-silent-failure\"",
            "id         = \"goal-reachable\"",
        );
        let err = parse(&dup).expect_err("a duplicate obligation id must be refused");
        match err {
            ScenarioError::Validation(msg) => assert!(msg.contains("duplicate")),
            other => panic!("expected Validation, got {other}"),
        }
    }

    #[test]
    fn an_obligation_with_no_evidence_is_refused() {
        let no_evidence = VALID.replace(
            "evidence   = \"a listing invocation whose output names all three\"",
            "evidence   = \"\"",
        );
        let err = parse(&no_evidence).expect_err("an obligation with no evidence must be refused");
        match err {
            ScenarioError::Validation(msg) => assert!(msg.contains("no evidence")),
            other => panic!("expected Validation, got {other}"),
        }
    }

    #[test]
    fn an_activity_shaped_statement_is_refused() {
        let activity = VALID.replace(
            "statement  = \"all three boards exist and are listed by the product at the end of the run\"",
            "statement  = \"exercise the board creation flow\"",
        );
        let err = parse(&activity).expect_err("an activity-shaped statement must be refused");
        match err {
            ScenarioError::Validation(msg) => assert!(msg.contains("activity-shaped")),
            other => panic!("expected Validation, got {other}"),
        }
    }

    #[test]
    fn an_indistinct_pass_signal_is_refused() {
        let indistinct = VALID.replace(
            "evidence   = \"a listing invocation whose output names all three\"",
            "evidence   = \"done\"",
        );
        let err = parse(&indistinct).expect_err("an indistinct pass signal must be refused");
        match err {
            ScenarioError::Validation(msg) => assert!(msg.contains("indistinct")),
            other => panic!("expected Validation, got {other}"),
        }
    }

    #[test]
    fn an_absence_claim_with_no_positive_control_is_refused() {
        let stripped = VALID.replace(
            "\npositive_control = \"replays/known-silent-exit.toml\"",
            "",
        );
        let err = parse(&stripped)
            .expect_err("an absence claim with no positive_control must be refused");
        match err {
            ScenarioError::Validation(msg) => assert!(msg.contains("positive_control")),
            other => panic!("expected Validation, got {other}"),
        }
    }
}
