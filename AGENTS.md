# Agents Instructions, Guidelines & Engineering Standards

Cronus is a polyglot monorepo: a Rust workspace (`crates/`) for the engine and binaries, a JS/TS layer (`packages/`) for the UI, and a Tauri shell (`apps/`). These rules cover how agents write and verify code. Source files reference no design/spec artifacts — restate rationale in plain language.

## Rust (`crates/`)

### Implementation

- **Clean Code**: idiomatic Rust; small, well-named modules; dependencies point inward to `core`.
- **Dependency Policy**: prefer the standard library; add a third-party crate only when strictly necessary and justified.
- **Error Handling**: no `unwrap()` / `panic!()` on production paths — use `Result` / `Option`. Reserve panics for truly unrecoverable invariants.
- **Tests (mandatory)**: every feature or fix has tests. Unit tests in a `#[cfg(test)] mod tests` inside the source file; integration tests in that crate's `crates/<crate>/tests/`.
- **Benchmarks**: performance-critical logic gets benches in that crate's `crates/<crate>/benches/`.
- **Logging**: descriptive `tracing` (or `log`) events; critical errors logged with enough context for session-level debugging.
- **Secrets & data**: never read or log secrets in the clear; never write secrets to tracked files; keep user data on-device unless the user explicitly authorizes egress.

### Verification (per affected crate or workspace-wide)

- `cargo check` — compiles.
- `cargo clippy --all-targets -- -D warnings` — zero lints.
- `cargo test` — all tests pass.
- `cargo bench` — when performance-relevant.
- `cargo fmt --all` — consistent style.

## Frontend (`packages/` — TypeScript / React)

### Implementation

- **Presentation only**: the UI calls the core over the IPC bridge; no business logic in TypeScript.
- **Type-safe**: no `any` on public surfaces; externalize all user-facing strings (localization); honor the theme/design-token system.

### Verification (per affected package)

- `pnpm -C packages/<pkg> test` (vitest) — tests pass.
- lint + format (`biome`) — zero errors.
- `tsc --noEmit` — type-checks.
- `fallow audit --changed-since <base>` — no new dead code, duplication, circular dependencies, or architecture-boundary violations (structural gate; `--format json` in CI). The boundary rules enforce presentation-only UI with inward-pointing dependencies.

## Build Environment (Windows host)

- **Run native builds via PowerShell, not Git Bash.** `cargo` steps that invoke the C toolchain (e.g. `rusqlite` `bundled`, the Tauri crate's `windres` `.exe`-resource step) and Tauri CLI commands (`tauri info`/`dev`/`build`) must run in **PowerShell**. Git Bash's MSYS2 environment makes the mingw64 `cc1.exe` fail to load (exit 127), so `gcc`/`windres` fail silently there (`gcc -E` exits 1 with no output) even though the same `gcc.exe` works fine in PowerShell. Pure-`rustc` builds (no fresh C compilation) work in either shell — so a clean `cargo check` from Git Bash that suddenly fails at a C/resource step is an environment artifact, not a code defect.
- When capturing a native exe's output in PowerShell, do **not** redirect `2>&1` — PowerShell 5.1 wraps each stderr line as a `NativeCommandError`; stderr is already captured, so read the output as-is.

## Conventions

- **CLI grammar**: verb-first with flags, explicit verbs, command groups — `cronus <group> <verb> [--flags]`. The TUI mirrors every *semantic* group with a leading slash (`/board add …`) and also offers the installation-half verbs (`status`, `doctor`, `backup`, `restore`, `workspace`, `dev`, `archetype`, `registry`, `ext`, `activation`, `init`, `completion`) under the same names and flags (`/doctor`). The CLI stays their guaranteed path — an installation verb must stay answerable even when the composition the TUI itself depends on fails to come up — so the TUI never offers one the CLI lacks; it leaves out `tui` (nothing to bring up inside it), `completion` (the script is generated from the CLI's own grammar) and `version` (a flag, not a verb), and answers a request for one with the reason. Each installation verb declares its effect on a running composition (inspect / install / recompose); the TUI runs the inspect ones and, until it can collect consent and tell whether work is in flight, answers the rest with the `cronus …` command that carries them out. A recompose verb is refused while work it would alter is in flight. The library method is the source of truth and the CLI/TUI are thin bindings.
- **Definition of done**: a change is done only when its required quality gates pass — tests + lint + type/format always; benchmarks for performance-relevant changes; security review for sensitive ones.
- **One source of truth**: domain logic lives in the core; frontends and shells hold none.

## Completion Protocol (Mandatory Checklist)

Before declaring a task complete, verify:

- [ ] Required quality gates are green for every touched crate/package (tests + lint + type/format).
- [ ] No `unwrap()` / `panic!()` on production paths; no secrets in tracked files or logs.
- [ ] New or changed behavior is covered by tests (unit in-module; integration per-crate).
- [ ] Technical content (code, comments, docs) in English; conversational replies in Russian.
- [ ] No design/spec-layer references leaked into source files.
