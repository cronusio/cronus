//! The catalog complement: what nothing in the scenario corpus simulates.
//!
//! Coverage is claimed against the
//! product's declared action catalog, and the complement — never a
//! percentage — is the report's headline number. A percentage compresses
//! "60% of a hundred actions" and "60% of six" into the same-looking
//! figure; the complement names exactly what is missing, which is the only
//! form of this report anyone can act on.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::scenario::{self, Scenario};

/// The result of intersecting a catalog against a corpus's `covers`
/// declarations.
#[derive(Debug, Clone)]
pub struct CoverageReport {
    pub total_catalog: usize,
    pub covered: Vec<String>,
    pub uncovered: Vec<String>,
}

impl CoverageReport {
    /// `catalog` is every action identity the product declares; `covers`
    /// is the union of every scenario's `covers` list. Neither argument
    /// needs to be sorted or deduplicated — this does both.
    pub fn compute<'a>(catalog: &[String], covers: impl IntoIterator<Item = &'a str>) -> Self {
        let covered_set: HashSet<&str> = covers.into_iter().collect();

        let mut covered = Vec::new();
        let mut uncovered = Vec::new();
        let mut seen = HashSet::new();
        for entry in catalog {
            if !seen.insert(entry.as_str()) {
                continue; // the catalog itself had a duplicate; count once
            }
            if covered_set.contains(entry.as_str()) {
                covered.push(entry.clone());
            } else {
                uncovered.push(entry.clone());
            }
        }
        covered.sort();
        uncovered.sort();

        CoverageReport {
            total_catalog: seen.len(),
            covered,
            uncovered,
        }
    }
}

impl fmt::Display for CoverageReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "catalog: {} action(s)", self.total_catalog)?;
        writeln!(f, "covered: {} action(s)", self.covered.len())?;
        writeln!(f, "uncovered: {} action(s)", self.uncovered.len())?;
        for entry in &self.uncovered {
            writeln!(f, "  - {entry}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum CoverageError {
    Io(std::io::Error),
    CompletionFailed(String),
    Scenario { path: PathBuf, message: String },
}

impl fmt::Display for CoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoverageError::Io(err) => write!(f, "coverage I/O error: {err}"),
            CoverageError::CompletionFailed(msg) => {
                write!(f, "failed to read the product's own catalog: {msg}")
            }
            CoverageError::Scenario { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for CoverageError {}

/// Derive the product's action catalog from its own shell-completion
/// script — a **best-effort structural approximation**, not the exact
/// `Invocable::is_projected()`-filtered set: completion output has no
/// locus or stability annotation, so an installation-only or
/// not-yet-shipped entry cannot be distinguished from an ordinary one here.
/// Chosen over linking a live registry because that would pull the domain
/// tier into a crate whose entire design keeps that tier at arm's length
/// — and chosen over scraping `--help` text
/// because the completion script already exists as a complete, structured
/// enumeration; `--help` text does not.
pub fn catalog_from_completion(cronus_binary: &Path) -> Result<Vec<String>, CoverageError> {
    let output = Command::new(cronus_binary)
        .args(["completion", "bash"])
        .output()
        .map_err(CoverageError::Io)?;
    if !output.status.success() {
        return Err(CoverageError::CompletionFailed(format!(
            "`completion bash` exited {:?}",
            output.status.code()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_completion_actions(&text))
}

/// Extract every `cronus__subcmd__a__subcmd__b…` identifier and rewrite it
/// as the dotted action path `a.b` — the same identity convention
/// scenario `covers` lists already use.
fn parse_completion_actions(text: &str) -> Vec<String> {
    let mut ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        let Some(rest) = token.strip_prefix("cronus__subcmd__") else {
            continue;
        };
        let path: Vec<&str> = rest.split("__subcmd__").collect();
        if path.iter().any(|segment| segment.is_empty()) {
            continue;
        }
        ids.insert(path.join("."));
    }
    ids.into_iter().collect()
}

/// The root-level `simulations/` directory, resolved relative to this
/// crate's own manifest — the same resolution `tests/corpus.rs` uses.
pub fn default_simulations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../simulations")
}

/// Parse every scenario file under `dir` (recursively, skipping
/// `README.md`). A file that fails to parse is a hard error — a coverage
/// report computed by silently skipping what it could not read would
/// undercount without saying so, which is the same silent-gap failure
/// the coverage claim exists to prevent.
pub fn load_corpus(dir: &Path) -> Result<Vec<Scenario>, CoverageError> {
    let mut scenarios = Vec::new();
    for path in markdown_files(dir) {
        if path.file_name().and_then(|n| n.to_str()) == Some("README.md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(CoverageError::Io)?;
        let parsed = scenario::parse(&text).map_err(|err| CoverageError::Scenario {
            path: path.clone(),
            message: err.to_string(),
        })?;
        scenarios.push(parsed);
    }
    Ok(scenarios)
}

fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(markdown_files(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_complement_names_exactly_what_is_not_covered() {
        let catalog = vec![
            "board.add".to_string(),
            "board.list".to_string(),
            "board.move".to_string(),
        ];
        let covers = vec!["board.add"];

        let report = CoverageReport::compute(&catalog, covers);

        assert_eq!(report.total_catalog, 3);
        assert_eq!(report.covered, vec!["board.add".to_string()]);
        assert_eq!(
            report.uncovered,
            vec!["board.list".to_string(), "board.move".to_string()]
        );
    }

    #[test]
    fn the_report_names_a_count_and_entries_and_never_a_percentage() {
        let catalog = vec![
            "board.add".to_string(),
            "board.list".to_string(),
            "board.move".to_string(),
        ];
        let covers = vec!["board.add"];
        let report = CoverageReport::compute(&catalog, covers);

        let text = report.to_string();
        assert!(
            text.contains("2 action(s)"),
            "must carry the uncovered count: {text}"
        );
        assert!(
            text.contains("board.list"),
            "must name each uncovered entry: {text}"
        );
        assert!(
            text.contains("board.move"),
            "must name each uncovered entry: {text}"
        );
        assert!(
            !text.contains('%'),
            "must never report a percentage: {text}"
        );
    }

    #[test]
    fn parses_nested_completion_identifiers_into_dotted_action_paths() {
        // Matches the real shape (`cronus completion bash`): a quoted
        // assignment and a bare case-label, both without any leading
        // underscore before `cronus`.
        let text = r#"
            cmd="cronus__subcmd__board__subcmd__add"
            cronus__subcmd__knowledge__subcmd__collection__subcmd__create)
            cmd="cronus"
        "#;
        let ids = parse_completion_actions(text);
        assert!(ids.contains(&"board.add".to_string()));
        assert!(ids.contains(&"knowledge.collection.create".to_string()));
        assert!(!ids.iter().any(|id| id.is_empty()));
    }
}
