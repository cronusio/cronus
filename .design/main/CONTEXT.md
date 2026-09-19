# Project Context

**Generated:** 2026-09-19

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
├── .artifacts/
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
├── .qwen/
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
├── installer/
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

- T-30C01: `scenario::parse`/`validate` — refuses at parse time an empty or duplicate-id obligation set, no-evidence obligation, activity-shaped statement, indistinct pass signal, and an absence claim with no `positive_control`; a missing `bound` or an unknown top-level key is refused structurally by `serde`
- T-30C02: the root-level `simulations/` corpus — `README.md` plus the first `short`-tier scenario, `first-run-three-cards.md` (renamed from the plan's illustrative "three-boards" once the real CLI — one board, many cards — was checked); `tests/corpus.rs` guards the corpus against SDD-reference leaks with its own hand-rolled, self-tested matchers
- T-30D01: `run_state::RunState` — cross-process state as one `run-state.json` per world, since `world`/`run`/`note`/`verdict`/`finish` are separate subprocess invocations by design; `record_run` refuses once the scenario's bound is exhausted; `findings::RunOutcome` (pass/fail/incomplete/contaminated/void) computed from obligation verdicts alone, contamination checked first; the `cronus-sim` wrapper binary and its `pin` verb closing the discover→pin→guard loop into `tests/replays/`
- T-30D02: `coverage::CoverageReport` — the catalog complement (never a percentage) against the product's own shell-completion script, disclosed as a best-effort structural approximation rather than the exact locus/stability-filtered set no callable seam yet exposes; wired as `cronus-sim coverage`
- T-30T01: full-workspace validation — `cargo check`/`clippy -D warnings`/`fmt --check`/`test` all green in one pass (87 test-result blocks, 0 failed, no `-j 2` retry needed); USM-1…USM-12 mapped to their discharging code/tests, with USM-4/USM-6 named as partially discharged and USM-10 as a process discipline this phase's code does not mechanically check; structural and reference containment both clean on inspection
- Verify: `cargo test --workspace` green — 87 `test result: ok` blocks, 0 failed (37 of them new, in `cronus-simulation`); `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean

## Phase 31 — Discovery Classification (2026-09-13)

- T-31A01: a closed five-name `DiscoveryClass` in `findings.rs` — `defect`/`friction`/`inefficiency`/`optimization-opportunity`/`improvement-idea`, reused verbatim from the improvement-loop's own IMP-1 taxonomy rather than inventing a sixth vocabulary; `parse`/`name`/`all_names_joined`, with a unit test asserting the parsed spelling and the serde form never drift apart
- T-31A02: `RunState.notes` widened from `Vec<String>` to `Vec<Discovery>` (text + optional class + optional remedy, both new fields `#[serde(default)]`); `add_note(text, class, remedy)`; `FinishReport.notes` retyped to carry the same value straight through with no conversion
- T-31B01: `cronus-sim note` accepts `--class <name>` / `--remedy <text>` anywhere in its trailing arguments; an unrecognized class is a usage error (exit 5) naming all five valid classes, built from the library's own `all_names_joined()` rather than the binary re-listing them
- T-31B02: `finish`'s report renders a discovery's class and a remedy explicitly labelled `proposed remedy (not applied)`; an unclassified, remedy-free discovery renders exactly as it did before this phase
- T-31T01: full-cycle validation — a real `world new → run → note --class → verdict → finish` proves classification survives end to end with the run's outcome unaffected (USM-3); `cargo test -p cronus-simulation` green (36 lib + 14 e2e + 1 replay, 9 new); `cargo clippy -p cronus-simulation --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean; production-path scan found zero new `unwrap`/`panic!`/`.expect()` outside test code
- Verify: `cargo test -j 2 -p cronus-simulation` green across all three test targets; `cargo clippy -p cronus-simulation --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean (PowerShell, per the project's native-build discipline)
