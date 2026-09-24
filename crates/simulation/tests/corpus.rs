//! Validates the root-level `simulations/` corpus: every scenario file
//! parses, the `short` tier is non-empty and every scenario in it declares
//! at least one perturbation, and no file leaks a reference to the
//! project's own planning/specification layer — a scenario ships with the
//! product and must run in a checkout where that layer is entirely absent
//! (the companion-artifact discipline; the
//! reference-containment rule that discipline rests on).

use std::path::{Path, PathBuf};

use cronus_simulation::scenario::{self, Tier};

fn simulations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../simulations")
}

/// Every `.md` file under `dir`, recursively, sorted for a stable order.
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

#[test]
fn the_corpus_exists_and_the_short_tier_is_perturbed() {
    let dir = simulations_dir();
    assert!(
        dir.is_dir(),
        "simulations/ must exist at the repository root: {dir:?}"
    );

    let files = markdown_files(&dir);
    assert!(!files.is_empty(), "the corpus must not be empty");

    let mut short_count = 0usize;
    for path in &files {
        if path.file_name().and_then(|n| n.to_str()) == Some("README.md") {
            continue; // documentation, not a scenario
        }
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("{}: failed to read: {err}", path.display()));
        let parsed = scenario::parse(&text)
            .unwrap_or_else(|err| panic!("{}: failed to parse: {err}", path.display()));

        if parsed.tier == Tier::Short {
            short_count += 1;
            assert!(
                !parsed.is_smoke(),
                "{}: a `short`-tier scenario must declare at least one perturbation",
                path.display()
            );
        }
    }

    assert!(
        short_count > 0,
        "the `short` tier must hold at least one scenario"
    );
}

#[test]
fn no_scenario_file_leaks_a_planning_layer_reference() {
    let dir = simulations_dir();
    for path in markdown_files(&dir) {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: failed to read: {err}", path.display()));

        assert!(
            !contains_task_id(&text),
            "{}: references a task id",
            path.display()
        );
        assert!(
            !contains_phase_designator(&text),
            "{}: references a phase designator",
            path.display()
        );
        assert!(
            !text.contains(".design/"),
            "{}: references a `.design/` path",
            path.display()
        );
        for system_file in ["PLAN.md", "TASKS.md", "INDEX.md", "RULES.md"] {
            assert!(
                !text.contains(system_file),
                "{}: references the SDD system file `{system_file}`",
                path.display()
            );
        }
    }
}

/// Matches the shape `T-\d+[A-Z]\d+(\.\d+)?` (bare or bracketed) — the
/// pattern the project's own containment rule names, generalized to the
/// two-digit-and-beyond phase numbers the real generator produces.
fn contains_task_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'T' && bytes[i + 1] == b'-' {
            let mut j = i + 2;
            let digits_start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > digits_start && j < bytes.len() && bytes[j].is_ascii_uppercase() {
                let letter_end = j + 1;
                let mut k = letter_end;
                let second_digits_start = k;
                while k < bytes.len() && bytes[k].is_ascii_digit() {
                    k += 1;
                }
                if k > second_digits_start {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Matches `[Pp]hase[-\s]\d+`.
fn contains_phase_designator(text: &str) -> bool {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let needle = b"phase";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            if j < bytes.len() && (bytes[j] == b'-' || bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
                if j < bytes.len() && bytes[j].is_ascii_digit() {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod self_tests {
    // The two hand-rolled matchers above guard every scenario file; prove
    // they actually fire before trusting them to guard anything.
    use super::{contains_phase_designator, contains_task_id};

    #[test]
    fn task_id_matcher_fires_on_bare_and_bracketed_forms() {
        assert!(contains_task_id("see T-1A01 for context"));
        assert!(contains_task_id("[T-22A01] fixed this"));
        assert!(contains_task_id("split id T-1A01.1 landed"));
        assert!(!contains_task_id("no task ids in this ordinary sentence"));
        assert!(!contains_task_id("T-shirt sizes vary"));
    }

    #[test]
    fn phase_designator_matcher_fires_on_prose_and_file_forms() {
        assert!(contains_phase_designator("closed in Phase 20"));
        assert!(contains_phase_designator("see phase-22 for detail"));
        assert!(!contains_phase_designator("the moon has phases"));
        assert!(!contains_phase_designator("a phase of matter"));
    }
}
