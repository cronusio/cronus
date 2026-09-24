//! Guards the SDD reference-containment boundary (product source never cites
//! the design layer): a specification file name — the `l1-*.md` / `l2-*.md` naming convention the
//! design layer uses — must never appear in product source, comments
//! included. A simulation-qa pass found 25 such references scattered across
//! six crates (rationale citations that named their source file instead of
//! just stating the rationale); this test keeps them from creeping back in.
//!
//! Scans every `.rs` file under the workspace's `crates/` tree structurally
//! (byte classification, not a literal pattern string) so the check itself
//! never needs to embed the shape it is forbidding.

use std::fs;
use std::path::{Path, PathBuf};

/// Finds a `l1-...md` / `l2-...md` style reference in a line, if present.
/// Matches `l` + (`1` | `2`) + `-` + one or more lowercase-letter/digit/`-`
/// characters + literal `.md` — the exact shape the design layer's spec
/// files are named with.
fn spec_file_reference(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'l' && (bytes[i + 1] == b'1' || bytes[i + 1] == b'2') {
            let start = i;
            let mut j = i + 2;
            if bytes.get(j) == Some(&b'-') {
                j += 1;
                let name_start = j;
                while j < bytes.len()
                    && (bytes[j].is_ascii_lowercase()
                        || bytes[j] == b'-'
                        || bytes[j].is_ascii_digit())
                {
                    j += 1;
                }
                if j > name_start && line.as_bytes()[j..].starts_with(b".md") {
                    return Some(&line[start..j + 3]);
                }
            }
        }
        i += 1;
    }
    None
}

/// Recursively collects every `.rs` file under `dir`.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Defensive: an isolated per-crate build could leave a stray
            // `target/` under `crates/`; never scan build output.
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_product_source_file_names_a_specification_file() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli sits two levels under the workspace root")
        .to_path_buf();
    let crates_dir = workspace_root.join("crates");

    let mut files = Vec::new();
    rust_files(&crates_dir, &mut files);
    assert!(
        files.len() > 100,
        "the workspace tree walk found suspiciously few .rs files ({}) under {} — \
         the path resolution is probably wrong rather than the tree being small",
        files.len(),
        crates_dir.display()
    );

    let mut offenders = Vec::new();
    for path in &files {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for (line_no, line) in content.lines().enumerate() {
            if let Some(reference) = spec_file_reference(line) {
                offenders.push(format!(
                    "{}:{}: names {reference}",
                    path.display(),
                    line_no + 1
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "product source must never name a specification file (reference containment) — \
         restate the rationale in plain language instead:\n{}",
        offenders.join("\n")
    );
}

// ── Design-process artifacts ─────────────────────────────────────────────────
//
// A comment that says *why* code is the way it is must stand on its own. An
// audit finding number, a task id or a phase number points at a record that
// exists only while the design layer does, and it goes stale as those records
// are renumbered or archived. Same containment boundary as above, three more
// shapes: audit-finding ids (`F-` plus two digits), task ids (`T-`, digits, a
// capital letter, digits) and phase designators (the word `Phase`, or a
// `phase-` slug, followed by a number).
//
// The conformance ledger's own single-digit finding ids are product data, not
// audit references, so they are deliberately outside the first shape.

/// Whether `bytes[i]` starts a new word (nothing alphanumeric right before it).
fn starts_word(bytes: &[u8], i: usize) -> bool {
    i == 0 || !bytes[i - 1].is_ascii_alphanumeric()
}

fn digits_at(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count()
}

/// Finds an audit-finding id, task id or phase designator in `line`.
fn process_artifact_reference(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        // F-<two digits>, not followed by a third digit or a letter.
        if bytes[i] == b'F' && starts_word(bytes, i) && bytes.get(i + 1) == Some(&b'-') {
            let n = digits_at(bytes, i + 2);
            let end = i + 2 + n;
            if n == 2 && !bytes.get(end).is_some_and(|b| b.is_ascii_alphanumeric()) {
                return Some(&line[i..end]);
            }
        }
        // T-<digits><capital letter><digits>.
        if bytes[i] == b'T' && starts_word(bytes, i) && bytes.get(i + 1) == Some(&b'-') {
            let first = digits_at(bytes, i + 2);
            let letter = i + 2 + first;
            if first > 0 && bytes.get(letter).is_some_and(|b| b.is_ascii_uppercase()) {
                let second = digits_at(bytes, letter + 1);
                if second > 0 {
                    return Some(&line[i..letter + 1 + second]);
                }
            }
        }
        // `Phase <number>` or `phase-<number>`.
        const PHASE_WORD_LEN: usize = 6; // both `Phase ` and `phase-`
        let rest = &bytes[i..];
        if (rest.starts_with(b"Phase ") || rest.starts_with(b"phase-"))
            && starts_word(bytes, i)
            && bytes
                .get(i + PHASE_WORD_LEN)
                .is_some_and(|b| b.is_ascii_digit())
        {
            let end = i + PHASE_WORD_LEN + digits_at(bytes, i + PHASE_WORD_LEN);
            return Some(&line[i..end]);
        }
    }
    None
}

#[test]
fn no_product_source_cites_an_audit_finding_task_or_phase() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli sits two levels under the workspace root")
        .to_path_buf();
    let crates_dir = workspace_root.join("crates");

    let mut files = Vec::new();
    rust_files(&crates_dir, &mut files);
    assert!(
        files.len() > 100,
        "the workspace tree walk found suspiciously few .rs files ({}) under {} — \
         the path resolution is probably wrong rather than the tree being small",
        files.len(),
        crates_dir.display()
    );

    // The simulation crate's own corpus tests feed these very shapes to the
    // detector that guards scenario text; they are inputs there, not citations.
    let exempt = Path::new("simulation").join("tests").join("corpus.rs");

    let mut offenders = Vec::new();
    for path in &files {
        if path.ends_with(&exempt) {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for (line_no, line) in content.lines().enumerate() {
            if let Some(reference) = process_artifact_reference(line) {
                offenders.push(format!(
                    "{}:{}: cites {reference}",
                    path.display(),
                    line_no + 1
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "product source must not cite an audit finding, task or phase (reference containment) — \
         state the rationale in plain language instead:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_process_artifact_detector_recognises_each_shape_and_only_those() {
    // Built from parts so this file never contains the shapes it forbids.
    let finding = format!("{}-{}", "F", "14");
    let task = format!("{}-{}{}{}", "T", "22", "A", "01");
    let phase = format!("{} {}", "Phase", "24");
    let slug = format!("{}-{}", "phase", "9");
    for hit in [&finding, &task, &phase, &slug] {
        let line = format!("// see {hit} for the reason");
        assert_eq!(
            process_artifact_reference(&line),
            Some(hit.as_str()),
            "must flag {hit}"
        );
    }
    for clean in [
        "// F-1 is the conformance ledger's first finding",
        "// AF-14 is not an audit id (part of a longer word)",
        "// F-143 has a third digit",
        "// the T-shirt (T-1) is not a task id",
        "// two-phase commit and multi-phase rollout",
        "// Phases of the moon",
    ] {
        assert_eq!(
            process_artifact_reference(clean),
            None,
            "must not flag: {clean}"
        );
    }
}

// ── Invariant ids and section marks ──────────────────────────────────────────
//
// The same boundary again, for the most common way a comment reaches back into
// the design layer: naming the invariant it implements (a capitalised prefix,
// a hyphen and a small number) or the section of a specification that says so
// (a section sign and a number). Both point at records that get renumbered,
// merged or archived, and neither means anything to a reader of the code. State
// the rule in words instead.
//
// Only comment text and string literals are inspected, so ordinary code such as
// `MAX-1` is left alone, and a short list of everyday technical names that share
// the shape (`UTF-8`, `SHA-256`) is allowed. Numbers with a leading zero are the
// tool-security rule ids, which are product data.

/// Hyphenated technical names that look like an id but are not one.
const ORDINARY_PREFIXES: &[&str] = &[
    "UTF", "SHA", "AES", "FNV", "ISO", "RFC", "HTTP", "TLS", "CVE", "MD", "RSA", "GCM", "TCP",
    "UDP",
];

/// For each byte of `line`: whether it sits inside a `//` comment or a string
/// literal. A block-comment continuation line (`* text`) is comment throughout.
fn text_mask(line: &str) -> Vec<bool> {
    const BACKSLASH: u8 = 0x5c;
    let bytes = line.as_bytes();
    let trimmed = line.trim_start();
    if trimmed.starts_with("* ") || trimmed.starts_with("/*") || trimmed == "*" || trimmed == "*/" {
        return vec![true; bytes.len()];
    }
    let mut mask = vec![false; bytes.len()];
    let mut in_string = false;
    let mut i = 0;
    while i < bytes.len() {
        if in_string {
            mask[i] = true;
            if bytes[i] == BACKSLASH {
                if i + 1 < bytes.len() {
                    mask[i + 1] = true;
                }
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                in_string = false;
            }
        } else if bytes[i] == b'"' {
            in_string = true;
            mask[i] = true;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            for slot in mask.iter_mut().skip(i) {
                *slot = true;
            }
            break;
        }
        i += 1;
    }
    mask
}

/// Finds an invariant id or a section mark in the comment text or string
/// literals of `line`.
fn design_layer_citation(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mask = text_mask(line);
    if !mask.iter().any(|&m| m) {
        return None;
    }

    // A section sign followed by a digit.
    if let Some(at) = line.find('\u{a7}') {
        let after = at + '\u{a7}'.len_utf8();
        if mask[at] && bytes.get(after).is_some_and(|b| b.is_ascii_digit()) {
            return Some(&line[at..after + 1]);
        }
    }

    // PREFIX-<1..3 digits, no leading zero>, where PREFIX is 2..=6 capitals.
    let mut i = 0;
    while i < bytes.len() {
        let boundary = i == 0
            || !(bytes[i - 1].is_ascii_alphanumeric()
                || bytes[i - 1] == b'_'
                || bytes[i - 1] == b'-');
        if bytes[i].is_ascii_uppercase() && boundary && mask[i] {
            let letters = bytes[i..]
                .iter()
                .take_while(|b| b.is_ascii_uppercase())
                .count();
            let dash = i + letters;
            if (2..=6).contains(&letters) && bytes.get(dash) == Some(&b'-') {
                let digits = digits_at(bytes, dash + 1);
                let end = dash + 1 + digits;
                let leading_zero = bytes.get(dash + 1) == Some(&b'0');
                let closed = !bytes
                    .get(end)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
                if (1..=3).contains(&digits) && !leading_zero && closed {
                    let prefix = &line[i..dash];
                    if !ORDINARY_PREFIXES.contains(&prefix) {
                        return Some(&line[i..end]);
                    }
                }
            }
            i += letters;
        } else {
            i += 1;
        }
    }
    None
}

/// Recursively collects source files (`.rs`, `.ts`, `.tsx`) under `dir`,
/// skipping build output and dependency folders.
fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skip = path
                .file_name()
                .is_some_and(|n| n == "target" || n == "node_modules" || n == "dist");
            if !skip {
                source_files(&path, out);
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext == "rs" || ext == "ts" || ext == "tsx")
        {
            out.push(path);
        }
    }
}

#[test]
fn no_product_source_cites_an_invariant_id_or_a_section() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli sits two levels under the workspace root")
        .to_path_buf();

    let mut files = Vec::new();
    for tree in ["crates", "packages", "apps"] {
        source_files(&workspace_root.join(tree), &mut files);
    }
    assert!(
        files.len() > 100,
        "the workspace tree walk found suspiciously few source files ({}) — \
         the path resolution is probably wrong rather than the tree being small",
        files.len()
    );

    // This file feeds the detector fixtures built from parts; the simulation
    // crate's corpus tests feed the process-artifact shapes to their own.
    let exempt = [
        Path::new("cli").join("tests").join("sdd_containment.rs"),
        Path::new("simulation").join("tests").join("corpus.rs"),
    ];

    let mut offenders = Vec::new();
    for path in &files {
        if exempt.iter().any(|e| path.ends_with(e)) {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for (line_no, line) in content.lines().enumerate() {
            if let Some(citation) = design_layer_citation(line) {
                offenders.push(format!(
                    "{}:{}: cites {citation}",
                    path.display(),
                    line_no + 1
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "product source must not cite an invariant id or a specification section \
         (reference containment) — state the rule in plain language instead:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_citation_detector_flags_ids_and_section_marks_only_in_text() {
    // Built from parts so this file never contains the shapes it forbids.
    let id = format!("{}-{}", "AB", "12");
    let section = format!("{}{}", '\u{a7}', "4");
    for hit in [&id, &section] {
        let comment = format!("// implements {hit} here");
        assert_eq!(
            design_layer_citation(&comment),
            Some(hit.as_str()),
            "must flag {hit} in a comment"
        );
        let string = format!("assert!(ok, \"{hit}: must hold\");");
        assert_eq!(
            design_layer_citation(&string),
            Some(hit.as_str()),
            "must flag {hit} in a string"
        );
    }
    for clean in [
        "let last = MAX-1;",
        "// the UTF-8 and SHA-256 names are ordinary",
        "// tool rule PT-001 is product data",
        "// RUSTSEC-2024-0429 has a long number",
        "// a lone F-1 and an X-2 have one-letter prefixes",
        "// LONGPREFIX-1 has too many capitals",
        "let flag = ab-12; // lowercase in code is not an id",
    ] {
        assert_eq!(design_layer_citation(clean), None, "must not flag: {clean}");
    }
}
