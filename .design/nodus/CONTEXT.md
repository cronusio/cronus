# Project Context

**Generated:** 2026-09-24

## Active Technologies

- Node.js
- Rust

## Core Project Structure

```plaintext
.
├── .agents/
│   ├── rules/
│   ├── skills/
│   └── workflows/
├── .cargo/
│   └── config.toml
├── .claude/
├── .codex/
├── .design/
│   ├── .version
│   ├── INDEX.md
│   ├── RULES.md
│   ├── main/
│   ├── nodus/
│   └── workspace.json
├── .drafts/
│   ├── TODO.md
│   ├── UX-UI - разбор 3 кейсов.md
│   ├── UX-дизайн - 6 психологических принципов.md
│   ├── heartbeat.md
│   ├── project-names.md
│   ├── qa-remediation-progress.md
│   ├── references.md
│   ├── release/
│   ├── reverse-derivation-mechanism.md
│   ├── swarm.md
│   ├── technology-stack-research.md
│   └── ui-ux.md
├── .env.example
├── .fallowrc.jsonc
├── .gitattributes
├── .github/
│   ├── dependabot.yml
│   └── workflows/
├── .gitignore
├── .magic/
├── .markdownlint.json
├── AGENTS.md
├── CHANGELOG.md
├── CONTRIBUTING.md
├── Cargo.lock
├── Cargo.toml
├── LICENSE
├── README.md
├── SECURITY.md
├── apps/
│   └── desktop/
├── biome.json
├── crates/
│   ├── activation-os/
│   ├── auth-local/
│   ├── cli/
│   ├── codegraph/
│   ├── conformance/
│   ├── contract/
│   ├── core/
│   ├── domain/
│   ├── model-local/
│   ├── nodus/
│   ├── simulation/
│   ├── store-local/
│   └── tui/
├── docs/
│   ├── README.md
│   ├── building.md
│   ├── building.ru.md
│   ├── simulation.md
│   └── simulation.ru.md
├── package.json
├── packages/
│   └── ui/
├── pnpm-lock.yaml
├── pnpm-workspace.yaml
├── rust-toolchain.toml
├── scripts/
│   └── check-domain-boundary.mjs
└── simulations/
    ├── README.md
    ├── broad/
    └── short/
```

## Recent Changes

- T-31C01: `l2-nodus-dialog.md` 1.3.0 → 1.4.0 (§4.3's `workflow_digest` tags → `[IMPLEMENTED]`, new closing paragraph citing both tests) and `l2-nodus-portability.md` 1.9.0 → 1.10.0 (§3.2 LP-22 row (c) → Implemented, dropped from the "what is plannable" residue list; LP-22 overall stays Partially realized). `INDEX.md` version cells + top-level version synced.
- T-31T01: Added `resume_descriptor_workflow_digest_matches_digest_ast` (unit, `executor.rs`) and `resume_descriptor_digest_agrees_with_repro_recipe_digest` (integration, `tests/dialog.rs`, new `ManifestCapture` `AuditProvider`) — the latter proves `ResumeDescriptor.workflow_digest` agrees with the independently-built `ReproRecipe.workflow_digest` for the same paused run, since the two construction sites are never cross-checked in production code. `cargo test -p nodus` — 484 passed (was 482, +2), 0 failed; clippy `-D warnings` clean; fmt clean; no `.unwrap()`/`panic!()`/`unreachable!()`/`.expect(` outside `#[cfg(test)]`; `git diff --stat` on `Cargo.toml`/`Cargo.lock` empty (LP-1 preserved)

## Phase 32 — NE-12/HO-20 Digest Unification (l2-nodus-environment §4.4 · l2-nodus-observability §4.7) (2026-09-02)

- T-32A01: `digest_ast` (`executor.rs`) widened `fn` → `pub(crate) fn`; `EnvRunResult::candidate()` (`environment.rs`) now matches `Parser::parse(workflow_source)` — `Ok(ast)` hashes `digest_ast(&ast)`, `Err(_)` falls back to `digest_source(workflow_source)` exactly as before. Signature unchanged.
- T-32C01: `l2-nodus-environment.md` 1.2.0 → 1.3.0 (NE-12 row + §4.4 "Implemented [Phase 32]" paragraph) and `l2-nodus-portability.md` 1.11.0 → 1.11.1 (§3.2 cross-reference updated to the implemented version). `INDEX.md` version cells + top-level version synced.
- T-32T01: Added `candidate_digest_agrees_with_repro_recipe_digest` (integration, `tests/environment.rs`, new `ManifestCapture` `AuditProvider` mirroring Phase 31's `tests/dialog.rs` precedent) — proves `CandidateResult.workflow_digest` and `ReproRecipe.workflow_digest` agree for the same run — and `candidate_digest_falls_back_to_source_hash_when_unparseable` (unit, `environment.rs`), pinning the fallback value the pre-existing unparseable-input test never asserted on. `cargo test -p nodus` — 486 passed (was 484, +2), 0 failed; clippy `-D warnings` clean; fmt clean; no `.unwrap()`/`panic!()`/`unreachable!()`/`.expect(` outside `#[cfg(test)]`; `git diff --stat` on `Cargo.toml`/`Cargo.lock` empty (LP-1 preserved)

## Phase 33 — Exclusive-Binding Duplicate Detection (l1-nodus-language NL-27 · l2-nodus-runtime §3.1) (2026-09-05)

- T-33A01: `ConfigReason` gained `DuplicateField`; `check_config_values` (`validator.rs`) now scans `decl.fields` for repeated names in a pre-accept-loop pass (two `HashSet<&str>`, one violation per repeated name however many times it repeats) before the existing accept loop runs, so a `§config` name declared twice can no longer reach `AcceptedConfig` at all — closing a real confidentiality path: previously `get` resolved the first declaration, `is_secret` answered `.any()` over both, and `non_secret_fields()` (the set `run_with_config` merges into `$in.config`) could emit the non-secret declaration for a name `is_secret` simultaneously reported as secret. Return type unchanged.
- T-33B01: New `e020_no_duplicate_macro_names` validator rule, modelled directly on `e015_no_duplicate_test_names` (same `HashSet::insert` shape, same per-extra-occurrence firing), registered in `Validator::validate`'s error section — `E020` was the next available code. Module doc comment corrected (`33` → `34` rules, `E001–E017` → `E001–E020`; it already undercounted before this task).
- T-33C01: `l2-nodus-runtime.md` 1.5.0 → 1.5.1 — NL-27 row **Pending → Partially realized**, naming the two closed classes and preserving verbatim why `@err:` last-write-wins (no advisory channel in `Parser::parse`'s return type), the inverted host-vocabulary discard (`Schema::with_provider` has nowhere to report it), the two vacuous classes, and the entire stated-displacement half all stay open. `l2-nodus-registries.md` 1.1.0 → 1.1.1 — re-confirmed its own **Vacuous** verdict is unaffected (none of its three registries is host-extensible). `INDEX.md` v1.0.99 → v1.0.100, both rows synced atomically.
- T-33T01: `shape_check_duplicate_field` (unit) + `duplicate_secret_field_rejects_rather_than_leaking` (integration, `tests/config.rs` — parses a real `§config:` text with one field declared twice, once `secret` once not, and asserts on the run's rejection rather than any surviving two-entry shape, so the test survives a later accept-loop refactor) for Track A. `e020_fires_on_duplicate_macro_names` + `e020_absent_with_unique_macro_names` for Track B. `cargo test -p nodus` — 490 passed (was 486, +4), 0 failed; clippy `-D warnings` clean; fmt clean; no `.unwrap()`/`panic!()`/`unreachable!()`/`.expect(` outside `#[cfg(test)]`; `git diff --stat` on `Cargo.toml`/`Cargo.lock` empty (LP-1 preserved)
