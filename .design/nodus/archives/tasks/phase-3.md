---
phase: 3
name: "Standalone Extraction"
status: Done
subsystem: "crates/nodus"
requires:
  - "Phase 2 — Library Hardening"
provides: []
key_files:
  created:
    - "crates/nodus/.github/workflows/ci.yml"
    - "crates/nodus/EXTRACTION.md"
  modified:
    - "crates/nodus/Cargo.toml"
    - "crates/nodus/src/lib.rs"
    - ".design/nodus/specifications/l2-nodus-runtime.md"
    - ".design/nodus/INDEX.md"
patterns_established: []
duration_minutes: ~
---

# Phase 3 Tasks — Standalone Extraction

**Phase:** 3
**Status:** Done
**Strategic Goal:** Prepare `crates/nodus` for publication as an independent library. Sync the spec with Phase 2 implementation state, harden the Cargo manifest for crates.io, document the entire public API, and produce the extraction artifacts (CI workflow, human extraction procedure).

## Atomic Checklist

### Track A — Spec Sync

- [x] [T-3A01] Sync `l2-nodus-runtime.md` with Phase 2 implementation state; bumped spec to v1.0.2

### Track B — Cargo Manifest Hardening

- [x] [T-3B01] Replace workspace-delegated fields in `crates/nodus/Cargo.toml` with explicit values
- [x] [T-3B02] Add crates.io publication metadata to `crates/nodus/Cargo.toml`; updated `README.md` for standalone audience

### Track C — Public API Documentation

- [x] [T-3C01] Updated `crates/nodus/src/lib.rs` `//!` doc — standalone quick-start example, lifecycle table, accurate description; removed Cronus-internal references
- [x] [T-3C02] Fixed broken intra-doc link in `workflows.rs`; removed SDD task-ID references from `executor.rs` and `workflows.rs`; zero `cargo doc` warnings

### Track D — Extraction Artifacts

- [x] [T-3D01] Written `crates/nodus/.github/workflows/ci.yml` — check + test + clippy + fmt + doc steps
- [x] [T-3D02] Written `crates/nodus/EXTRACTION.md` — 7-step human extraction procedure (create repo, copy, commit, tag, publish, update Cronus, archive)

### Track T — Validation

- [x] [T-3T01] `cargo test -p nodus` — 143 passed (91 unit + 17 invariant + 34 parity + 1 doctest); 0 failed
- [x] [T-3T02] `cargo doc --no-deps -p nodus` — 0 warnings; `cargo clippy -p nodus -- -D warnings` — 0 lints

## Detailed Tracking

### [T-3A01] Sync l2-nodus-runtime.md with Phase 2 implementation state

**Track:** A — Spec Sync
**File:** `.design/nodus/specifications/l2-nodus-runtime.md`

Update the spec to match the Phase 2 implementation:

1. **§4.6 Vocabulary Schema**: change `BUILTIN_SCHEMA_VERSION = "0.4.5"` → `"0.4.6"`. Update command count from 50 → 51 (added `RUN`). Add `RUN` to the command table as a meta-command row: `RUN | meta | @macro_name | — | Expand a named @macro: body in-place`.
2. **§3 or new §4.x Invariant Table**: add `RUNTIME_OWNED_VARIABLES` constant (9 variables: `$in`, `$error`, `$meta`, `$ctx`, `$user`, `$session`, `$flags`, `$memory`, `$kb_results`) — the subset of RESERVED_VARIABLES that the runtime owns exclusively and cannot be reassigned.
3. **§5 Invariant Compliance Table**: update NL-8 row — validator check exists (E013); update NL-10 row — validator check exists (E014). Set both cells to `✅ Enforced (E013)`/`✅ Enforced (E014)`.
4. Bump spec version from `1.0.1` → `1.0.2` in the file header and add a Document History entry: `| v1.0.2 | 2026-06-24 | Sync with implementation: BUILTIN_SCHEMA_VERSION v0.4.6, 51 commands (RUN added), RUNTIME_OWNED_VARIABLES, E013/E014 enforced |`.
5. Update `INDEX.md` entry for l2-nodus-runtime.md: version `1.0.1` → `1.0.2`; update description to mention v0.4.6 and 51 commands.

**Verify:** File header shows `Version: 1.0.2`; §4.6 says `BUILTIN_SCHEMA_VERSION = "0.4.6"` and 51 commands; E013/E014 rows are `✅` in the invariant compliance table; INDEX.md entry updated.

### [T-3B01] Replace workspace-delegated Cargo.toml fields

**Track:** B — Cargo Manifest Hardening
**File:** `crates/nodus/Cargo.toml`

Replace all `.workspace = true` declarations with explicit values:

```toml
[package]
name    = "nodus"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
repository = "https://github.com/cronusio/cronus"
```

Remove lines of the form `version.workspace = true`, `edition.workspace = true`, etc. The replacement values come from the workspace root `Cargo.toml` (verified in T-2C01).

**Verify:** `cargo check -p nodus` exits 0; `grep "workspace = true" crates/nodus/Cargo.toml` returns no output.

### [T-3B02] Add crates.io publication metadata

**Track:** B — Cargo Manifest Hardening
**File:** `crates/nodus/Cargo.toml`

Add the following fields to the `[package]` section:

```toml
description = "Declarative workflow DSL and Rust runtime for AI-augmented automation pipelines"
homepage    = "https://github.com/cronusio/cronus"
documentation = "https://docs.rs/nodus"
keywords    = ["workflow", "dsl", "automation", "ai", "declarative"]
categories  = ["parser-implementations", "development-tools"]
readme      = "README.md"
```

Also add a `[package.metadata.docs.rs]` section:

```toml
[package.metadata.docs.rs]
all-features = true
```

If `crates/nodus/README.md` does not exist, create a minimal one: crate name, one-line description, link to the repository, and a minimal usage example (the public API scaffold → validate → run chain).

**Verify:** `cargo package -p nodus --list` exits 0 and includes `README.md`; `cargo metadata -p nodus` shows the description field.

### [T-3C01] Add crate-level documentation to lib.rs

**Track:** C — Public API Documentation
**File:** `crates/nodus/src/lib.rs`

Add a `//!` doc block at the top of `lib.rs` covering:

1. **Purpose**: one paragraph describing nodus as a declarative workflow DSL runtime.
2. **Quick start**: a `# Usage` section with a minimal Rust example — call `workflows::run(source, filename, None)` on an inline `.nodus` string, check `result.status`.
3. **Workflow lifecycle**: `scaffold → validate → run → transpile` chain (one-sentence each).
4. **Feature flags**: note that the crate has no optional features currently (`default = []`).

The doc block should follow Rust `//!` conventions (not `///`) and be placed before all `pub mod` re-exports.

**Verify:** `cargo doc --no-deps -p nodus` generates `nodus/index.html`; the crate-level page has a non-empty description.

### [T-3C02] Add item-level doc comments to public API

**Track:** C — Public API Documentation
**Files:** `crates/nodus/src/workflows.rs`, `crates/nodus/src/validator.rs`, `crates/nodus/src/executor.rs`, `crates/nodus/src/transpiler.rs`

For every `pub` item (function, struct, enum, trait, const) visible through the crate's public surface:

- Add a `///` doc comment describing what the item does, its parameters, and return value (or field meaning for structs/enums).
- For `pub` enums with variants, document each variant.
- For `pub` traits (e.g., `ModelProvider`), document the trait contract and each method.
- Do not duplicate the type signature — explain the behaviour.

Priority order (highest public surface impact):

1. `workflows::run`, `workflows::validate`, `workflows::scaffold`, `workflows::transpile`
2. `executor::Status` and its variants
3. `validator::Diagnostic`, `validator::Severity`
4. `executor::ModelProvider` trait and `StubProvider`
5. `transpiler::Transpiler` pub methods

**Verify:** `cargo doc --no-deps -p nodus 2>&1 | grep -c "warning\[missing_docs\]"` → 0. Zero `missing_docs` warnings.

### [T-3D01] Write standalone CI workflow

**Track:** D — Extraction Artifacts
**File:** `crates/nodus/.github/workflows/ci.yml`
*(This file is for the standalone repo — placed here for review; will be moved to `.github/workflows/ci.yml` at repo root after extraction.)*

Write a GitHub Actions workflow that runs on `push` and `pull_request` against `main`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo test --all-targets
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo fmt --all -- --check
      - run: cargo doc --no-deps 2>&1 | grep -v "^$" | (! grep warning)
```

The workflow must not reference any workspace-level paths (`Cargo.toml` at monorepo root, `crates/` prefix) — it assumes the standalone repo root IS the crate root.

**Verify:** `cat crates/nodus/.github/workflows/ci.yml` outputs valid YAML; `python3 -c "import yaml; yaml.safe_load(open('crates/nodus/.github/workflows/ci.yml'))"` exits 0.

### [T-3D02] Write extraction procedure document

**Track:** D — Extraction Artifacts
**File:** `crates/nodus/EXTRACTION.md`

Write a step-by-step human procedure for extracting the crate. The document must cover:

1. **Pre-checks**: verify Phase 3 validation tasks (T-3T01, T-3T02) are green.
2. **Create repository**: `gh repo create cronusio/nodus --public --clone`
3. **Copy contents**: copy `crates/nodus/` into the new repo root (excluding `.github/workflows/ci.yml` which goes to `.github/workflows/`).
4. **Initial commit**: `git add . && git commit -m "feat: initial standalone release (extracted from cronusio/cronus)"`.
5. **Tag release**: `git tag -a v0.1.0 -m "nodus v0.1.0" && git push origin main --tags`.
6. **Publish**: `cargo publish`.
7. **Update Cronus**: replace `path = "../nodus"` (or equivalent) in Cronus `Cargo.toml` with `nodus = "0.1.0"`; run `cargo update && cargo test`.
8. **Archive**: mark Phase 3 Done in `.design/nodus/TASKS.md`; run `/magic.run nodus` for final closure.

Do not use absolute paths; reference file names relative to the repository roots.

**Verify:** `cat crates/nodus/EXTRACTION.md` has all 8 numbered steps; the document mentions `v0.1.0` and `cargo publish`.

### [T-3T01] Run full test suite after manifest changes

**Track:** T — Validation
**Command:** `cargo test -p nodus`

Run after T-3B01 and T-3B02 are complete. The workspace-delegated field replacement in Cargo.toml must not break the build. All 142 tests from Phase 2 must still pass.

**Verify:** `cargo test -p nodus` exits 0; output line `N passed; 0 failed` where N ≥ 142.

### [T-3T02] Run doc and lint validation

**Track:** T — Validation
**Commands:** `cargo doc --no-deps -p nodus` and `cargo clippy -p nodus -- -D warnings`

Run after T-3C01 and T-3C02 are complete.

1. `cargo doc --no-deps -p nodus` exits 0 with no `warning` lines.
2. `cargo clippy -p nodus -- -D warnings` exits 0.

**Verify:** Both commands exit 0; `cargo doc --no-deps -p nodus 2>&1 | grep -c warning` = 0.

## Results

### T-3A01 l2-nodus-runtime.md sync

`l2-nodus-runtime.md` bumped to v1.0.2. Changes: `BUILTIN_SCHEMA_VERSION` → `"0.4.6"`, command count 50 → 51 (RUN meta-command added), `RUNTIME_OWNED_VARIABLES` constant documented (9 read-only reserved vars), NL-8 row updated to E013 enforced, NL-10 row updated to E014 enforced. `INDEX.md` updated to v1.0.3 with revised l2 entry.

### T-3B02 Cargo.toml metadata additions

Added to `[package]`: `description`, `homepage`, `documentation`, `keywords` (5 terms), `categories` (2 terms), `readme = "README.md"`. Added `[package.metadata.docs.rs]` with `all-features = true`. Rewrote `README.md` for standalone audience (removed monorepo-internal references, added lifecycle table, MIT license note).

### T-3C02 Doc coverage

`cargo doc --no-deps -p nodus` → 0 warnings. Fixed broken intra-doc link `[StubProvider]` → `[crate::executor::StubProvider]` in `workflows.rs`. Removed SDD task-ID references (`T-2F01`, `T-2T01`) and internal invariant labels (`WFL-8`, `WFL-9`) from `executor.rs` and `workflows.rs` doc comments. `lib.rs` `//!` doc rewritten: standalone quick-start doctest (passes in `cargo test`), lifecycle table, design note on `ModelProvider` extension point.
