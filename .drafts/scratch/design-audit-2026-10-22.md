# Design-Layer Audit — 2026-10-22

A simulation-QA pass over the specification layer: every spec, the plan, the task ledger and
the state files of both workspaces, checked against each other and against the code they claim
to describe. Findings are proposals; nothing in `.design/` or in product code was edited.

## 1. Operator note (verbatim)

> .design/ примечание: я хочу что ты тщательно проанализировал все спеки, планы сделанные и не сделанные, на предмет несостыковок, нелогичности, изъянов, если что формируй опросы для уточнения

## 2. Scope and environment

| Item | Value |
| --- | --- |
| Target | `.design/` of this repository (workspaces `main` and `nodus`), cross-checked against `crates/`, `apps/`, `packages/`, `scripts/`, `.github/`, and the agent instruction files |
| Product class | Specification corpus (primary); Rust CLI/TUI + Tauri/React shell (secondary, read-only evidence) |
| Corpus size | 340 specs (main 317: 232 L1 + 85 L2; nodus 23), 7.7 MB; main `INDEX.md` 649 KB, `PLAN.md` 390 KB, `TASKS.md` 141 KB |
| Versions read | main INDEX 1.0.235 · PLAN 2.82.0 · TASKS 1.66.0; nodus INDEX 1.0.106 · PLAN 1.54.0 · TASKS 2.47.0; RULES 1.7.1; engine 2.1.144 (= snapshot, no drift) |
| Reference inputs | Specs and plans: present. Design reference: not applicable (spec layer). Existing tests: present (not run). Fixtures: none needed |
| Tools | Python/Node audit scripts (scratchpad), engine `check-prerequisites` and `build-spec-graph`, `rg`/`Select-String`, `git log -S`, the built `target/debug/cronus.exe --help` (run in a temp directory) |
| Load testing | Not applicable to a spec corpus; not authorized for the product |

## 3. What was checked (coverage matrix)

| Check family | main | nodus | Result |
| --- | --- | --- | --- |
| Header fields, layer prefix, required sections (RULES §5) | 317/317 | 23/23 | 1 miss (nodus `l2-nodus-testing` has no Motivation) |
| INDEX ↔ file parity (status, version, layer, presence) | 317/317 | 23/23 | Clean |
| `Implements:` targets valid, no L2→L2, no cycles | 85 L2 | 13 L2 | Clean |
| Union coverage: every L1 invariant cited by ≥1 implementing L2 | all L1 with L2 | all | Clean (0 uncited) |
| Requirement-ID namespaces (collisions, undefined refs) | full | full | **13 colliding prefixes** (F-06) |
| Document History shape, dates, version order | full | full | Hygiene issues (appendix) |
| Code fences vs RULES §4/§5 | full | full | 25 `rust`, 18 `sql`, + bash/json/yaml (F-17) |
| Relative links and canonical paths | full | full | 1 example link renders broken |
| `<!-- TBD -->` markers in Stable specs | 62 specs / 69 TBDs | 1 | F-17, F-19 |
| `Assumption (forecast)` premises awaiting the user | 10 | 0 | F-03, F-20 |
| PLAN coverage of every spec, checkbox states | full | full | Clean coverage; semantics issue F-04 |
| Pending/Partial compliance rows vs open phases | 32 specs | 7 specs | **8+ rows unplanned** (F-08) |
| Spec-cited code paths exist | full | full | 11 stale citations (outside Document History and future-tense lines) |
| Open phase files: named paths still exist | 33, 42 | 34 | Clean (only to-be-created files missing) |
| Concept-only marker (C28) | 232 L1 | 4 L1 | F-16 |
| STATE / TASKS / PLAN / CONTEXT consistency | yes | yes | F-05, appendix |
| Semantic probes by theme | defaults & egress, mobile/hub, source-of-truth, archival/retention, routing, office representations, CLI surface | sampled | F-03, F-09, F-10, F-11 |
| SDD containment in product files | full scan | — | F-22 |

## 4. Findings by severity

Severity scale: **High** — breaks a gate, a stated user promise, or misreports project state;
**Medium** — a contradiction or gap that will mislead the next workflow or let a defect through;
**Low** — hygiene. Improvements observed are listed separately and are not defects.

| ID | Sev | Class | Finding |
| --- | --- | --- | --- |
| F-01 | High | Drift / gate | The domain boundary guard is red on `master` since 2026-09-10 **and, because it is the first step of the only CI job, `cargo build`/`clippy`/`test` have not run in CI on any push or PR since**; the CI also runs only on manifest changes, so ordinary code pushes get no CI at all. Root cause: QA-remediation decision R8 (commit `66653c7`) added `serde`/`serde_json` to `cronus-domain`, but neither `l2-crate-topology` §4.3 nor the guard's allowlist was amended |
| F-02 | Medium (security) | Missing feature | `knowledge add-url` fetches any `http://host:port` with no SSRF guard (loopback, link-local `169.254.169.254`, private ranges all reachable), no egress consent and an unbounded `read_to_end`. `l2-security` §4.4 makes link-local blocking mandatory. Phase 35 names `EgressGate` and the model transport as the SSRF guard's consumers — **not this fetcher**, so the gap survives Phase 35 as planned |
| F-03 | High | Contradiction | `l1-program-lifecycle` (forecast) and `l2-program-lifecycle` set `update.check = true` by default — a daily HTTPS GET to the release host. That contradicts DN-1 ("no network egress for its own operation"), DN-4 ("the default never phones home") and HSO-4 ("nothing consent-bearing — … egress … — is on by default") |
| F-04 | High | Plan honesty | PLAN marks `[x]` on L2 specs whose capability has no user surface (13 specs named in Phase 37: mission, goal, research, trigger, user, model, route, office, migrate, report, serve…) and on `l2-security`, whose SSRF guard, trust registry, integrity shields, startup token and credential modes were never built (Phase 35 says so). STATE reports "Overall 94%" |
| F-05 | Medium | Engine bug | `update-state.js:824` counts only linked `\| [Phase N](…)` rows: unlinked Planned/Held/Done rows and `10a`/`10b` are excluded. Real progress is 34 of 47 rows (main) and 33 of 38 (nodus), not 31/33 and 33/34. Engine bug report in §10 |
| F-06 | Medium | Traceability | Thirteen requirement-ID prefixes are defined by 2–3 different specs each (28 specs). 227 citations in other specs name such an ID without saying which spec they mean; the open task files cite `DL-n` 30 times (phase 42) and `ES-n` 9 times (phase 33) |
| F-07 | Medium | Stale status | Realization tables not updated after Done phases: `l2-simulation-suite` §3 still says USM-13 "Pending — no realization yet" though Phase 31 shipped `DiscoveryClass` and `note --class/--remedy`; `l2-surface-conformance` §4.7 still lists "structured output hand-built, unescaped" though Phase 32 shipped one `json_escape`. PLAN's Backlog note repeats the USM-13 claim. Main run phases carry no spec-reconciliation track (nodus phases do) |
| F-08 | Medium | Unplanned debt | Pending invariant rows owned by no open phase and no Backlog entry: MR-15, MR-16 (`l2-model-runtime`), RTG-9, RTG-10 (`l2-model-router`), DL-6, DL-7 (`l2-deliberation`), AP-16 (`l2-automation-pipeline`), ERR-9 (`l2-github-issue`), plus Partial roadmap rows in `l2-codegraph` (7), `l2-deep-research` (2) and `l2-notes` (1). *Correction made during remediation:* the first scan also counted a row in `l2-application-shell`, one in `l1-task-graph-model`, one in `l1-automation-pipeline` and a "partial" in `l2-crate-topology` and six L1s; on reading them they are the word "pending" or "partial" in ordinary table cells, not compliance status, and were dropped |
| F-09 | Medium | Contradiction | `l1-multi-device-sync` (Overview, Motivation, SY-2, §5) and `l1-architecture` §5 treat "an always-on phone acting as a personal server" as a supported hub. INV-4 makes constrained hosts spokes, and the project's own earlier decision was that phones are thin clients, never always-on servers |
| F-10 | Medium | Spec describes intent as shipped | `l2-agent-session` compliance rows RTG-2 and RTG-7 say every model call in a turn is a router decision with fallback. The only shipped turn, `session say`, bypasses the router: hard-wired loopback endpoint (`CRONUS_CHAT_ENDPOINT`), `--model` required, no router reference in `invocable_bootstrap/session.rs` |
| F-11 | Medium | Drift | `packages/ui/src/surfaces/office-view.tsx` still ships two built-in representations (`"graph" \| "floor"`). `l1-office-visualization` 2.0.0 OVZ-9 and `l2-office-view` 1.0.2 forbid any spatial view in core. The spec was rewritten to the target design on 2026-09-24 but no task removes the floor mode; PLAN still describes the spec as "graph + spatial projection" |
| F-12 | Medium | Constitution (C30) | Phase 38 shipped user-observable behaviour (chat turn, streaming, stop; Sessions, Kanban and Chat screens; engine seat and relaunch; Install/Recompose from the TUI) with one new usage scenario in total and no QLY-8 debt record for the rest |
| F-13 | Medium | Cross-workspace gap | The Cronus-side binding of the nodus command seam (`l2-workflow-runtime` §4.2, designed 2026-09-24) has no main phase. nodus Phases 34–36 build the seam; nothing in main consumes it, so `workflow run` keeps the stub model |
| F-14 | Medium | Hidden dependency | Phase 34 replaces the keyword classifier `autonomy::classify_command`. `memory_intelligence::recall_for_reuse` (MI-13's authority gate) is built on it; neither `l2-memory-intelligence` nor that function appears in Phase 34's scope |
| F-15 | Medium | Plan risk | T-42B01's Verify requires `cargo check/clippy --manifest-path apps/desktop/tauri/Cargo.toml`. The desktop crate is outside the Cargo workspace and outside CI, and STATE (2026-10-08) records uncompiled edits there; the task may block on pre-existing breakage |
| F-16 | Medium | C28 inconsistency | 104 L1s carry `concept-only`; another 30 Stable L1s have no L2, no marker and are not mentioned by any L2 (list in A.3), so PLAN shows them `[x]` with nothing marking them unrealized. `l1-state-authority` is marked `concept-only` although `l2-filesystem-layout` declares `Implements:` on it (C28.4 auto-revert missed) |
| F-17 | Medium | Constitution contradictions | (a) C6 says the agent *asks* which Stable specs to pull; C9/C25/C27 forbid asking outside the gate list. (b) §2 makes Stable mean "no open questions"; 62 Stable specs carry 69 TBD markers, which C13.3 itself prescribes. (c) §5 forbids implementation code; 30 specs carry 25 `rust`, 18 `sql` and bash/json/yaml blocks. (d) §6 forbids duplication; INDEX descriptions (median 1.7 KB per row) and PLAN lines restate spec content, which produced ~70 stale invariant ranges |
| F-18 | Low | Constitution scope | RULES §7 carries engine-development conventions that cannot apply here: C11 names `magic.simulate` (no such workflow), C17 names `docs/distribution.md` (absent), C1/C14/C20 govern editing `.magic/`, which `rules/magic.md` forbids in this project |
| F-19 | Low | Stale TBD | TBDs the build already settled: shadcn/ui vs DaisyUI (`l2-app-ui`; neither is used), a Go core spike (`l2-core-library`), moon vs Nx (`l2-technology-stack`; neither adopted), consolidated vs strict XDG (`l2-filesystem-layout`), an empty TBD in `l2-archetype-catalog` |
| F-20 | Low | Decision pending | Ten `Assumption (forecast)` premises await confirmation (A.5); one of them is F-03 |
| F-21 | Low | Doc contradiction | `CLAUDE.md`, `AGENTS.md`, `CODEX.md` list `completion` among the verbs the TUI offers and, two lines later, among the verbs it leaves out; `l2-tui` 1.6.0 excludes it |
| F-22 | Low | SDD containment | `scripts/check-domain-boundary.mjs:67` cites task ID `T-17D01`. Borderline: a `.design/` mention in a test comment at `crates/cli/src/installation.rs:808` |
| F-23 | High | Spec describes intent as shipped | *Found during remediation.* `l2-crate-topology` §2/§4.3 and the PLAN call the domain tier I/O-free, but twenty-one of its 109 source files do filesystem or process I/O on a production path (`kanban`, `roles`, `backup`, `checkpoint`, `scheduler`, `state`, `store`, the session writer and journal, `extensions`, `secrets`, `quality` …). The boundary guard reads manifests only, so it cannot see `std::fs`. Disclosed in the spec (1.3.0); whether to make the tier I/O-free or to restate the rule is left as a marked TBD and asked of the person |

### Improvements observed (not defects)

- Phase 32's single JSON escaper and Phase 31's discovery classes are better than their specs say (F-07 is the spec lagging the code, not the reverse).
- The PLAN Risks section already records the red gate, the Phase 33 drift, the Phase 38 bypass and the domain-logic-first gap; several findings above extend those records rather than discover them.
- Union invariant coverage is complete, INDEX parity is clean and no spec is orphaned — the registry mechanics hold.

## 5. Three-way diff (plan ↔ spec ↔ build)

| Class | Items |
| --- | --- |
| Missing feature (spec requires, build lacks) | SSRF guard and its knowledge-fetcher consumer (F-02); Phase 37 command groups (F-04); router-mediated chat turn (F-10); main-side workflow binding (F-13) |
| Regression | None confirmed |
| Improvement (build ahead of spec) | USM-13 classification; one JSON escaper (F-07) |
| Drift (build diverges, no rationale recorded in spec) | `serde`/`serde_json` in the domain tier (F-01); office floor mode (F-11) |
| Spec ahead of build (planned) | Phases 33–37, 39–46 (main); 34–38 (nodus) |
| Design gap | Not applicable to this pass |

## 6. Presentation fidelity (CLI surface, observed)

- `cronus --help` describes the product as "workflow automation toolkit"; the specs define an AI office (`l1-office-model`).
- The `session` group's summary is "session operations" (lower case, generic); every other group has a descriptive summary.
- The help states that every error is English prose on stderr with no JSON shape. Correct today; it will need to change with Phase 45 (diagnostic codes, localized help).
- `README.md` is a single heading.

## 7. Load results

Not run. A specification corpus has no load surface, and no authorization statement was given for product load testing.

## 8. Not covered

- No line-by-line semantic reading of all 7.7 MB. Contradictions were searched corpus-wide **by theme** (defaults and egress, phone/hub, source-of-truth claims, archival/retention, routing, office representations, CLI surface); contradictions outside those themes may remain.
- No build, test or clippy run (host memory ceiling; native builds avoided). The desktop crate was not compiled (F-15 is therefore a risk, not a verified failure).
- The product was exercised only through `cronus --help`; no verb was run.
- Archived phase files (main 1–32, nodus 1–33) were not re-verified task by task against code.
- nodus spec semantics were sampled, not read in full.
- The ten forecast assumptions were listed, not validated.

## 9. Fix specifications

Each material finding, six fields. Proposed fixes follow the project's own route: spec changes via `/magic.spec`, plan changes via `/magic.task`, code via `/magic.run`.

### F-01 — Red boundary gate hides build and test

- **Problem:** the CI job stops at a known-red guard, so build, clippy and test have not run in CI since 2026-09-10.
- **Evidence:** `node scripts/check-domain-boundary.mjs` → exit 1, "forbidden normal dependencies: serde, serde_json"; `.github/workflows/deps-gate.yml` runs the guard (step 57) and its self-test (step 63, which also asserts a clean tree) before build (71), clippy (74), test (77); `paths:` limits triggers to manifests; `git log -S serde -- crates/domain/Cargo.toml` → `66653c7` (2026-09-10); `.drafts/scratch/qa-remediation-progress.md` R8 "DECISION: add serde to domain".
- **Impact:** every Dependabot PR since has merged on an unverified build; a new boundary offender would hide behind the known one.
- **Proposed fix:** amend `l2-crate-topology` §4.3 to admit the crates decided in R8 (and decide `tracing`, Q1); update the guard's allowlist in the same change; move the guard into its own CI job so a red guard never skips build and test; add a code-path trigger (or a push trigger without `paths`) so code changes get CI.
- **Alternatives:** move serialization out of the domain tier (reverses R8; larger refactor); keep the baseline-as-accepted approach of T-42T02 (institutionalizes a red gate).
- **Risk:** low; spec + script + workflow edits. Verify: guard exits 0; a CI run shows build/clippy/test executing.

### F-02 — Unguarded URL ingestion

- **Problem:** `HttpUrlFetcher` connects to any host with no SSRF guard, no egress consent, no size cap.
- **Evidence:** `crates/core/src/knowledge_bootstrap.rs:47-118` (`TcpStream::connect((host, port))`, `read_to_end`); registered as `Locus::Semantic` in `invocable_bootstrap/knowledge.rs:154-186`; PLAN Phase 35 consumer list.
- **Impact:** today reachable only by a person typing the URL; becomes an SSRF and memory-exhaustion path as soon as an agent, automation or extension can invoke `knowledge.add-url`.
- **Proposed fix:** add the knowledge fetcher to Phase 35's consumer list; until then, an interim pure check (refuse loopback, link-local, private, unspecified, mapped forms after resolution) and a body cap.
- **Alternatives:** mark the invocable human-only until Phase 35 (narrower, keeps the defect for humans).
- **Risk:** low; a refusal path with tests against a local server on a refused address.

### F-03 — Update checks on by default vs local-first

- **Problem:** `update.check = true` contradicts DN-1, DN-4 and HSO-4.
- **Evidence:** `l1-program-lifecycle.md:37` (forecast), `l2-program-lifecycle.md:166`, `l1-deployment-neutrality.md` DN-1/DN-4, `l1-honest-solicitation.md` HSO-4.
- **Impact:** the product would contact a vendor host on first run, against three invariants and the project's local-first stance.
- **Proposed fix:** decided by the user (Q2); then amend the losing side before Phase 43 is decomposed.
- **Alternatives:** see Q2.
- **Risk:** spec-only now; Phase 43 code later.

### F-04 / F-05 — Completion signals overstate

- **Problem:** `[x]` means "domain logic exists" for L2 specs whose surface or whole sections were never built; the engine's Overall counter also drops 13 rows.
- **Evidence:** PLAN Phase 37 ledger (lines 887–903), Phase 35 (line 859), checkbox lines for the 13 specs; `update-state.js:824-828`.
- **Impact:** a later planning pass or a reader treats unbuilt capability as done; progress reads 94% where the plan has 13 phases open.
- **Proposed fix:** a distinct PLAN marker for "domain-only, no surface" (Q4), applied to the specs Phase 37 and Phase 35 name; engine fix reported upstream (§10), not edited here.
- **Alternatives:** reopen the checkboxes to `[ ]` (contradicts the plan's "Done phases are immutable" policy).
- **Risk:** plan-text only.

### F-06 — Colliding requirement-ID prefixes

- **Problem:** 13 prefixes are each owned by 2–3 specs.
- **Evidence:** A.1; 227 unqualified citations; phase-42 (DL ×30), phase-33 (ES ×9).
- **Impact:** a citation such as `DL-5` or `IR-9` resolves to two different requirements; graph and coverage tools cannot tell them apart.
- **Proposed fix:** Q6. Default: give the later-authored spec of each pair a new prefix in one `/magic.spec` pass (e.g. `DLG` for diagnostic log, `EXS` for execution sandbox), rewrite citations, and add a uniqueness check to the review gate.
- **Alternatives:** qualify every citation as `spec:ID` and keep prefixes.
- **Risk:** wide text change across specs and two open task files; mechanical, verifiable by re-running the collision scan.

### F-07 — Stale realization rows after Done phases

- **Problem:** two spec tables still report as missing what shipped.
- **Evidence:** `l2-simulation-suite.md:113`; `crates/simulation/src/findings.rs:71`; `l2-surface-conformance.md` §4.7; `crates/cli/src/output.rs:31`; TASKS Phase 32 note.
- **Impact:** the next phase re-plans finished work or trusts a stale table.
- **Proposed fix:** update both rows to as-built; add a "spec reconciliation" task to every main run phase, as nodus already does.
- **Alternatives:** none worth keeping.
- **Risk:** spec-only.

### F-08 — Pending rows without an owner

- **Problem:** eight-plus Pending invariants have no phase and no Backlog entry.
- **Evidence:** `l2-model-runtime.md:92-93`, `l2-model-router.md:44-45`, `l2-deliberation.md:40-41`, `l2-automation-pipeline.md:52`, `l2-github-issue.md:40`.
- **Impact:** they will be lost; the plan's own rule is that a Pending row lives in a phase or the Backlog.
- **Proposed fix:** a `/magic.task main` sync that places each into an existing planned phase or a new "Pending deltas" phase.
- **Alternatives:** record them in Phase 37's ledger.
- **Risk:** plan-only.

### F-09 — Phone as hub

- **Problem:** two L1s allow a phone to be an always-on hub.
- **Evidence:** `l1-multi-device-sync.md:9,25,48,126`; `l1-architecture.md:142`; INV-4.
- **Impact:** any future sync L2 would be designed around a host class the project has rejected.
- **Proposed fix:** Q3; default: amend both specs so phones are spokes and the convergence point is a hub or a user-controlled relay.
- **Alternatives:** keep phone-as-hub "where the OS permits".
- **Risk:** spec-only; no code exists.

### F-10 — Chat turn bypasses the router

- **Problem:** compliance rows claim router-mediated turns; the shipped turn has no router.
- **Evidence:** `l2-agent-session.md:44-45,919`; `crates/core/src/invocable_bootstrap/session.rs:160-165`.
- **Impact:** a later phase assumes routing, fallback and privacy routing apply to chat; they do not.
- **Proposed fix:** mark RTG-2/RTG-7 Partial with the `session say` exception named, and plan routing the turn through the router when the model setting arrives.
- **Alternatives:** route now (larger, belongs with Phase 37).
- **Risk:** spec-only now.

### F-11 — Office floor mode in core UI

- **Problem:** the shipped UI renders a spatial floor the spec forbids in core.
- **Evidence:** `packages/ui/src/surfaces/office-view.tsx:2,40`; `l2-office-view.md:29,45,127`; PLAN line for `l2-office-view`.
- **Impact:** spec and product disagree on a visible surface.
- **Proposed fix:** a removal task when the GUI track reopens (the GUI hold applies), plus the PLAN description fix now.
- **Alternatives:** amend OVZ-9 back to allow a built-in floor.
- **Risk:** small UI removal; covered by existing UI tests.

### F-12 — C30 scenarios missing for Phase 38

- **Problem:** user-observable behaviour shipped without its usage scenario or a recorded debt.
- **Evidence:** `simulations/` contents (one scenario dated after 2026-09-21); C30; no QLY-8 mention in `l2-tui`, `l2-agent-session`, `l2-engine-endpoint`.
- **Impact:** the discovery instrument does not cover the newest surfaces.
- **Proposed fix:** author scenarios for the chat turn, the three TUI screens and engine relaunch, or record each as QLY-8 debt.
- **Alternatives:** none (C30 requires one or the other).
- **Risk:** low.

### F-13 — Workflow binding unplanned in main

- **Problem:** the designed binding has no phase.
- **Evidence:** `l2-workflow-runtime` §4.2; nodus PLAN Backlog line 601; main PLAN line 432; no main open phase names it.
- **Impact:** nodus builds a seam nobody uses.
- **Proposed fix:** a main phase "Workflow command binding", ordered after nodus Phase 34 (and 35/36 where the binding needs them).
- **Risk:** plan-only.

### F-14 — Phase 34 hidden consumer

- **Problem:** `recall_for_reuse` depends on the classifier Phase 34 replaces.
- **Evidence:** `crates/domain/src/memory_intelligence.rs:294,440`.
- **Impact:** Phase 34 either breaks MI-13's gate or keeps the keyword classifier alive for it.
- **Proposed fix:** add `l2-memory-intelligence` MI-13 to Phase 34's scope table.
- **Risk:** plan-only.

### F-15 — Desktop precondition for T-42B01

- **Problem:** a task Verify depends on a crate nobody has compiled since 2026-10-08.
- **Evidence:** root `Cargo.toml` members; `phase-42.md:79,81`; STATE 2026-10-08.
- **Impact:** Phase 42 stalls on unrelated breakage.
- **Proposed fix:** a first step in T-42B01 that compiles the desktop crate as-is and records the result, splitting the desktop half out if it fails.
- **Risk:** none.

### F-16 — Concept-only marker drift

- **Problem:** 30 unrealized L1s are unmarked; one realized L1 is marked.
- **Evidence:** A.3; `l2-filesystem-layout.md` `**Implements:**`; PLAN line 397.
- **Impact:** C28's health signal under-counts the concept/code gap.
- **Proposed fix:** mark the 30 (or plan them), drop the marker from `l1-state-authority`.
- **Risk:** plan-only.

### F-17 / F-18 — Constitution contradictions and foreign conventions

- **Problem:** RULES contradicts itself (C6 vs C9/C27; §2 vs C13.3; §5 vs 30 specs; §6 vs INDEX/PLAN) and carries five conventions that cannot apply here.
- **Evidence:** RULES lines 77–80, 19–20, 39, 46, 124–126, 162–164; fence counts in §3.
- **Impact:** every workflow reads a constitution it cannot obey literally.
- **Proposed fix:** Q8 — one `/magic.rule` amendment.
- **Risk:** a constitutional amendment (E4); the user approved all four parts (§11, Q8).

## 10. Engine bug report

> [!CAUTION]
> **MAGIC-SPEC ENGINE BUG REPORT**
>
> **Symptom:** STATE.md "Overall" progress undercounts phases. Only `| [Phase N](...)` rows are counted, so unlinked rows (Planned, Held, or Done without a workbook) and letter-suffixed phases (`10a`, `10b`) vanish from both numerator and denominator. main reads 31/33 (94%) for a registry of 47 rows with 34 Done; nodus reads 33/34 (97%) for 38 rows with 33 Done.
> **Engine Version:** 2.1.144
> **Environment Context:** finalize → update-state, workspaces main and nodus, Windows 11, Node 24.12.0
> **File/Line:** `.magic/scripts/update-state.js:824-828` (`computeProgress`)
> **Reproduction / Context:**
>
> - Command executed: `node .magic/scripts/executor.js finalize --workflow=task --workspace=main`
> - State / Action during failure: TASKS.md registry with unlinked `| Phase 34 | … | \`Planned\` |` rows
>
> **Hypothesis / Fix:** match `^\| \[?Phase [0-9]+[a-z]?\]?` rows and decide Done from the status cell, not from any `Done` word in the row.

Recorded via `record-diagnostic` (`STATE_OVERALL_UNDERCOUNTS_PHASES`), together with `DOMAIN_BOUNDARY_SPEC_NOT_SYNCED` for F-01.

## 11. Decisions recorded (survey, 2026-10-22)

| Q | Question | Answer | Effect on the plan |
| --- | --- | --- | --- |
| Q1 | What may the domain tier depend on? | `serde`, `serde_json` and the `tracing` facade | R-01 is a spec update, no longer a decision; T-42B03 unblocks once it lands |
| Q2 | Update checks by default? | Off; consent asked once at installation, unticked | R-04 amends `l1-program-lifecycle` (forecast replaced) and `l2-program-lifecycle` (`update.check` default `false` + installer question); DN-1/DN-4/HSO-4 stand |
| Q3 | Can a phone be a hub? | No — a spoke in core; a personal-server mode on a phone may arrive later as an optional plugin, in this repository or another | R-09 amends `l1-multi-device-sync` (SY-2, Overview, §5) and `l1-architecture` §5; the phone-as-server path is named as an extension seam, never a core default |
| Q4 | What does `[x]` mean for an L2? | A — a distinct `domain-only` marker | R-05 introduces the marker and applies it to the specs Phases 35 and 37 name; Done phases stay Done |
| Q5 | Build order after Phase 42? | Author the agent-runtime specification now; build it after Phases 33–34 | New R-24 |
| Q6 | Colliding ID prefixes? | Rename the later-authored spec's prefix | R-16 with the mapping below; no product-code mentions exist, so the change is design-layer only |
| Q7 | Concept growth? | `[DR]` mark the 30 unrealized L1s `concept-only`, no authoring freeze — the reversible option; C28 then reports the gap honestly. (Override: `/magic.task main "freeze new L1"`) | R-17 |
| Q8 | RULES amendments? | All four: C6 aligned with C27; §2 allows a non-blocking TBD in Stable; §5 enforced (code → pseudo-code in 30 specs); engine-development conventions C1/C11/C14/C17/C20 removed | R-18 |
| Q9 | Domain-tier I/O (asked after the first remediation pass)? | Make the tier I/O-free | R-25: decision in `l2-crate-topology`, I/O ratchet in the guard, Phase 50 |
| Q10 | Eight forecast premises (asked as three questions)? | Confirm: local usage recording on by default; remote = own hub + project VCS remote; client-involvement, epistemic-status, onboarding, prompt-templates as forecast | R-20 |

**R-16 prefix mapping** (the earliest-authored spec keeps its prefix; ties broken by INDEX row order):

| Prefix | Keeps | Renamed to |
| --- | --- | --- |
| AC | `l1-automation-canvas` (2026-06-24) | `l1-anomaly-consensus` → `ANC` |
| CA | `l1-context-attachment` (2026-07-02) | `l1-change-attribution` → `CHA` |
| CC | `l1-context-compression` (2026-06-26) | `l1-conversational-control` → `CVC` |
| CE | `l1-code-execution` (2026-06-26) | `l1-competitive-execution` → `CPE` |
| CR | `l1-crash-recovery` (2026-07-02) | `l1-cost-rating` → `CRT` |
| DL | `l1-deliberation` (2026-06-24) | `l1-diagnostic-log` → `DLG` |
| ES | `l1-evaluation-suites` (2026-06-25) | `l1-execution-sandbox` → `EXS` |
| GS | `l1-generative-surface` (2026-06-26) | `l1-generation-shaping` → `GSH` |
| IR | `l1-intent-resolution` (2026-06-26) | `l1-iterative-refinement` → `ITR` |
| OC | `l1-office-control` (2026-06-24, earlier INDEX row) | `l1-output-contracts` → `OCT`; `l1-outcome-confidence` → `OCF` |
| PM | `l1-perspective-model` (2026-06-27) | `l1-process-monitor` → `PMN` |
| SR | `l1-system-readout` (2026-07-07) | `l1-session-reinforcement` → `SRF`; `l1-staged-rollout` → `SRO` |
| LP | both (cross-workspace) | main → nodus citations written as `nodus LP-n` |

All fourteen new prefixes are unused anywhere in `.design/` and in product code.

## 12. Remediation plan

Every finding has an entry. Proposing is not applying.

| ID | Title | Closes | Type | Sev | Touches | Effort | Risk | Depends | Verified by |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| R-01 | Amend `l2-crate-topology` §4.3 and the guard allowlist: `serde`, `serde_json`, `tracing` | F-01 | spec-update + fix | High | spec, `scripts/check-domain-boundary.mjs` | small | low | Q1 ✔ | guard and self-test exit 0 |
| R-02 | Split the guard into its own CI job; add code-change triggers | F-01 | fix | High | `.github/workflows/deps-gate.yml` | small | low | — | CI run shows build/test steps |
| R-03 | Name the knowledge fetcher as an SSRF consumer in Phase 35; interim address check + body cap | F-02 | security | Medium | PLAN; `knowledge_bootstrap.rs` | small | low | — | refusal test on loopback/link-local |
| R-04 | `update.check` off by default, one unticked consent at installation | F-03 | spec-update | High | `l1-`/`l2-program-lifecycle` | small | low | Q2 ✔ | spec review; DN-1/DN-4/HSO-4 untouched |
| R-05 | Introduce the `domain-only` PLAN marker; apply to the specs Phases 35 and 37 name | F-04 | spec-update | High | PLAN, TASKS legend | small | low | Q4 ✔ | PLAN diff |
| R-06 | Report the Overall-counter bug upstream (done here: §10) | F-05 | debt | Medium | engine (read-only) | trivial | none | — | upstream fix |
| R-07 | Reconcile USM-13 and the escaper residual; add a reconciliation task to main run phases | F-07 | spec-update | Medium | 2 specs, PLAN | small | low | — | re-run stale-row scan |
| R-08 | Place the unplanned Pending rows | F-08 | spec-update | Medium | PLAN/TASKS | small | low | — | Pending-coverage scan clean |
| R-09 | Phones are spokes in core; phone-as-server named as an optional plugin seam | F-09 | spec-update | Medium | 2 L1 | small | low | Q3 ✔ | spec review |
| R-10 | Mark RTG-2/RTG-7 Partial in `l2-agent-session` | F-10 | spec-update | Medium | 1 spec | trivial | none | — | spec diff |
| R-11 | Fix PLAN description now; removal task for floor mode on GUI reopen | F-11 | fix | Medium | PLAN; `office-view.tsx` later | small | low | GUI hold | UI tests |
| R-12 | Scenarios or QLY-8 debt records for Phase 38 behaviour | F-12 | debt | Medium | `simulations/`, specs | medium | low | — | `cronus-sim` accepts scenarios |
| R-13 | Open a main phase for the workflow command binding | F-13 | spec-update | Medium | PLAN | small | low | nodus 34 | PLAN diff |
| R-14 | Add MI-13 to Phase 34 scope | F-14 | spec-update | Medium | PLAN | trivial | none | — | PLAN diff |
| R-15 | Desktop compile pre-check in T-42B01 | F-15 | fix | Medium | phase-42.md | trivial | none | — | recorded result |
| R-16 | Rename the later-authored prefixes per §11; add a prefix-uniqueness check to spec review | F-06 | debt | Medium | 16 specs renamed, citers, phase-33/42 task files | large | medium (wide text edit, no code) | Q6 ✔ | collision scan = 0 |
| R-17 | Mark the 30 unrealized L1s `concept-only`; drop the marker from `l1-state-authority` | F-16 | spec-update | Medium | PLAN | small | low | Q7 `[DR]` | C28 scan |
| R-18 | RULES amendment: C6 → C27 posture; §2 non-blocking TBD allowed; §5 enforced (30 specs to pseudo-code); remove C1/C11/C14/C17/C20 | F-17, F-18 | spec-update | Medium | RULES.md, 30 specs | medium | low | Q8 ✔ | `/magic.rule` review; fence scan shows no `rust`/`sql` |
| R-19 | Close stale TBDs as-built | F-19 | spec-update | Low | 5 specs | small | none | — | TBD list |
| R-20 | Confirm or override the forecast assumptions | F-20 | decision-needed | Low | 9 specs | small | none | user | spec diffs |
| R-21 | Fix the `completion` contradiction in the agent files | F-21 | fix | Low | 3 files | trivial | none | — | text diff |
| R-22 | Remove `T-17D01` from the guard script comment | F-22 | fix | Low | 1 file | trivial | none | R-01 | containment scan |
| R-23 | Hygiene batch (appendix A.6) | appendix | debt | Low | INDEX, PLAN, specs | medium | none | — | audit scripts clean |
| R-24 | Author the agent-runtime specification (tool-call loop, session runner, router-mediated turns — the composition Phase 37 waits on); schedule its build after Phases 33–34 | F-04, F-10 | spec-update | High | new L2 + PLAN | large | medium | Q5 ✔, R-10 | spec review; PLAN orders the build after 34 |

- **Tranche 0 — stop the bleeding:** R-01, R-02, R-03, R-04. Buys a CI that actually builds and tests, closes the one shipped security gap, and settles the update default before Phase 43.
- **Tranche 1 — conformance and direction:** R-05, R-07, R-08, R-09, R-10, R-11 (PLAN half), R-13, R-14, R-15, R-21, R-22, R-24. Buys a plan and spec set that say what is true, and a designed path to a working agent.
- **Tranche 2 — governance and traceability:** R-12, R-16, R-17, R-18. Buys unambiguous citations and a constitution the workflows can follow.
- **Tranche 3 — debt and polish:** R-19, R-20, R-23.
- **Deferred:** R-06 (upstream engine fix), R-11 code half (GUI hold).

**Deliberately not proposed:**

- Reopening Done phases — the plan's immutability policy is sound; amendment deltas carry the corrections.
- Renaming nodus `LP-n` — inside the nodus workspace the prefix is unambiguous; only main→nodus citations need qualifying (part of R-16).
- Rewriting INDEX descriptions wholesale — R-23 trims only rows that contradict their spec.
- Fixing the engine counter in `.magic/` — read-only in this project; reported instead.

## 13. Remediation log (applied 2026-10-22, on the person's "everything")

Nothing was committed. Everything below is in the working tree.

| ID | Result | Evidence |
| --- | --- | --- |
| R-01 | **Done** | `l2-crate-topology` 1.3.1 (§2, §4.2, §4.3); guard allowlist and self-test amended; `node scripts/check-domain-boundary.mjs` and `--self-test` exit 0; T-42B03 unblocked |
| R-02 | **Done, not run in CI** | `deps-gate.yml`: guard is its own job, `cargo` job no longer depends on it, code directories trigger the gate. The workflow was read, not executed (no CI run from here) |
| R-03 | **Done** | interim address guard + response cap in `HttpUrlFetcher`; 19 unit tests in `knowledge_bootstrap` green; clippy clean; `l2-knowledge-store` §5 note 7, `l2-security` §4.4, Phase 35 consumer list. A consequence the person should know: loopback and private addresses now have **no off switch** for `knowledge add-url` until Phase 35's operator setting exists |
| R-04 | **Done (spec)** | `l1-` and `l2-program-lifecycle` 1.2.0; new §4.1.1; Phase 43 text. No code existed to change |
| R-05 | **Done** | RULES C31; `[/]` on ten L2 specs (`domain-only`; `l2-multi-user-auth` with a partial note) and `l2-security` (partial); PLAN legend |
| R-06 | **Reported** | engine bug report §10, recorded as a diagnostic; `.magic/` untouched |
| R-07 | **Done** | USM-13 row, `l2-surface-conformance` §4.7 status table; T-42E01 and T-33G01 added |
| R-08 | **Done** | Phase 47 |
| R-09 | **Done** | `l1-multi-device-sync` 1.1.1, `l1-architecture` 1.4.1 |
| R-10 | **Done** | `l2-agent-session` 1.5.1 (RTG-2/RTG-7 Partial; §3 preamble) |
| R-11 | **PLAN half done; code half held** | PLAN line corrected; floor-mode removal recorded in Phase 47, waits on the GUI hold |
| R-12 | **Done in part** | two scenarios authored and accepted by `cronus-sim` (parse-time validator) and by the corpus guard test; the engine seat and `Recompose` gate recorded as QLY-8 debt in `l2-engine-endpoint` §5 note 6 — a refused change needs a genuinely running turn the harness cannot yet provide |
| R-13 | **Done** | Phase 48 (`Held`) |
| R-14 | **Done** | MI-13 row in Phase 34 |
| R-15 | **Done** | T-42B01 pre-check; contingency to split the desktop half |
| R-16 | **Done** | fourteen renames (table in §11); 56 specs bumped; INDEX, PLAN, TASKS, STATE and open task files updated; 0 colliding prefixes inside a workspace remain. **Not done:** cross-workspace citations still read `NL-n`/`LP-n` unqualified in a few main specs (the new §1 rule asks for `nodus LP-n`) |
| R-17 | **Done** | thirty L1s marked `concept-only`; `l1-state-authority` loses the marker. Forty-one unmarked L1s that some L2 mentions without `Implements:` remain untracked |
| R-18 | **Done** | RULES 1.8.0; 47 executable code blocks in 18 specs rewritten as pseudo-code (data-format fences kept); `C23` still names `/magic.simulate` |
| R-19 | **Done for the stale ones** | five TBDs closed or given a trigger. **Not done:** the other ~60 TBDs do not yet all name an owner or trigger, which the amended §2 asks of a TBD in a Stable spec |
| R-20 | **Done for seven of eight** | survey at the end of the session: the person confirmed local-only usage recording on by default, the two "remote" scopes, and four form forecasts (client-involvement, epistemic-status, onboarding, prompt-templates); each is now a *Confirmed* decision in its spec. `l1-scripted-surface-contract`'s forecast (a diagnostic with a stable code makes the code the contract) was not put to the person and is still a forecast |
| R-25 | **Done** *(new, from the survey)* | F-23: the person chose to make the domain tier I/O-free. `l2-crate-topology` 1.3.2 records the decision; the boundary guard gained a shrink-only I/O ratchet (`scripts/domain-io-baseline.json`, 21 files; self-test covers both drift directions); Phase 50 plans the migration module by module |
| R-21 | **Done** | `AGENTS.md`, and its two gitignored copies `CLAUDE.md`, `CODEX.md` |
| R-22 | **Done** | |
| R-23 | **Done in part** | 44 INDEX rows repaired (first attempt mis-split them; caught by markdownlint and redone), 66 stale invariant ranges refreshed in INDEX and PLAN, a Motivation added, stale paths fixed. **Not done:** history dates (62 cells), 21 mixed-order histories, 37 date regressions, the phase-journal gaps, the `RETROSPECTIVE` and `CONTEXT` regeneration, the nodus Backlog items kept in HTML comments |
| R-24 | **Done as RFC** | `l2-agent-runtime` 1.0.0; Backlog entry; Phase 49 `Held` |

**Verification run:** engine `check-prerequisites --verify-headers` ok for both workspaces (no version or status drift); `markdownlint-cli2` clean over all 104 touched Markdown files; `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean; the boundary guard and its self-test green; `finalize --workflow=spec` run for both workspaces (project version 0.1.627 → 0.1.629; root `CHANGELOG.md` entry checked for leaked references: none).

**Not verified:** the CI workflow was not executed; the new spec's design was not reviewed by anyone but its author; the prefix renames were classified by a script, with sixty-two ambiguous lines decided by hand — a spot check of about seventy renamed citations found none wrong, but it is a sample.

## Appendix A — Detail lists

### A.1 Colliding prefixes

| Prefix | Specs | Unqualified citations elsewhere |
| --- | --- | --- |
| AC | `l1-anomaly-consensus`, `l1-automation-canvas` | 30 |
| CA | `l1-change-attribution`, `l1-context-attachment` | 1 |
| CE | `l1-code-execution`, `l1-competitive-execution` | 9 |
| CC | `l1-context-compression`, `l1-conversational-control` | 22 |
| CR | `l1-cost-rating`, `l1-crash-recovery` | 13 |
| DL | `l1-deliberation`, `l1-diagnostic-log` | 50 |
| ES | `l1-evaluation-suites`, `l1-execution-sandbox` | 21 |
| GS | `l1-generation-shaping`, `l1-generative-surface` | 10 |
| IR | `l1-intent-resolution`, `l1-iterative-refinement` | 17 |
| LP | main `l1-lookahead-planning`, nodus `l1-nodus-portability` | (intra-nodus use is unambiguous) |
| OC | `l1-office-control`, `l1-outcome-confidence`, `l1-output-contracts` | 49 |
| PM | `l1-perspective-model`, `l1-process-monitor` | 1 |
| SR | `l1-session-reinforcement`, `l1-staged-rollout`, `l1-system-readout` | 4 |

### A.2 Unplanned Pending rows

MR-15, MR-16 · RTG-9, RTG-10 · DL-6, DL-7 (deliberation) · AP-16 · ERR-9. Partial rows without a phase (roadmap): `l2-codegraph` CI-5, CI-8, CI-9, CI-13, CI-16, CI-18, CI-19; `l2-deep-research` DR-4, DR-7; `l2-notes` NOT-2. The earlier version of this list also named a row each in `l2-application-shell`, `l1-task-graph-model` and `l1-automation-pipeline` and a "partial" in `l2-crate-topology` and six L1s; those were word matches in ordinary table cells and are withdrawn.

### A.3 Stable L1s with no L2, no marker and no L2 mention

`l1-agent-coevaluation`, `l1-agent-federation`, `l1-cache-stable-context`, `l1-change-merge`, `l1-context-attachment`, `l1-employee-availability`, `l1-evaluations`, `l1-facilitation`, `l1-folders`, `l1-generation-budget`, `l1-harness-composition`, `l1-harness-optimization`, `l1-inference-cache`, `l1-model-benchmarking`, `l1-operational-health`, `l1-practice-analytics`, `l1-progressive-disclosure`, `l1-project-priority`, `l1-project-support`, `l1-provenance-taint`, `l1-report-prompting`, `l1-requirement-checklists`, `l1-search`, `l1-simulation`, `l1-specialty-exemplars`, `l1-system-readout`, `l1-tool-call-transport`, `l1-usage-allowance`, `l1-user-model`, `l1-work-import`. A further 42 unmarked L1s are mentioned by some L2 without an `Implements:` link (partial realization, untracked).

### A.4 Stale TBDs

`l2-app-ui` (shadcn/ui vs DaisyUI), `l2-core-library` (Go core spike), `l2-technology-stack` (moon vs Nx), `l2-filesystem-layout` (XDG), `l2-archetype-catalog` (empty marker), `l2-kanban-board` (SQLite board), `l1-storage-model` (one database or several).

### A.5 Forecast assumptions awaiting confirmation

`l1-program-lifecycle` (update checks on — F-03) · `l1-usage-analytics` (local recording on; semantic actions, not clicks) · `l1-client-involvement` (four levels) · `l1-remote-attachment` (own hub only) · `l1-artifact-reach` (remote = VCS remote) · `l1-onboarding` (two arrivals) · `l1-epistemic-status` (compact form) · `l1-prompt-templates` (three callers) · `l1-scripted-surface-contract` (codes are the contract).

### A.6 Hygiene

- 44 INDEX rows carry the description inside the link cell (`[file](…) ; v…`).
- ~70 stale invariant ranges in INDEX/PLAN descriptions (e.g. nodus INDEX `NL-1…10` for NL-30, main PLAN `CB-1…3` for CB-11).
- Document History: six table shapes; six specs without a Date column; ~25 rows dated `—`; 21 histories in mixed order; 37 date regressions; non-numeric versions (`≤1.0.5`, `1.0.0 (Stable)`, `1.0.0 (RFC)`).
- `nodus/l2-nodus-testing`: no Motivation; compliance heading says NT-1…NT-11 while NT-12 exists.
- 42 unlabeled code fences; 1029 `text` fences where §4 asks `plaintext` for trees.
- `l2-memory-store.md:717` template example renders as a broken link.
- `l2-execution-workspace.md:303` "fixed at 7 days: configurable via …".
- `l2-knowledge-store.md:407` cites nonexistent `crates/embeddings`, `crates/memory-store`; `l2-application-shell.md:495` cites `packages/ui/src/bridge.ts` (now `shared/bridge.ts`).
- PLAN Overview is a 38 KB single line of revision history.
- Main phase journal (`.design/main/CHANGELOG.md`) skips Phases 10–11, 16–22, 24–29, 32, 38; RETROSPECTIVE snapshots stop at Phase 30; CONTEXT "Recent Changes" stops at Phase 31 and lists only Node.js and Rust.
- nodus STATE "Last Session Ended: 2026-06-24".
- T-42B03 is `[ ]` in the checklist and `Blocked [!]` in its block.
- Phase 37's ledger lists `session` as missing; the group now exists.
- Eight later L2 specs are tracked in PLAN tables rather than checkbox lines (C10).
- nodus Backlog keeps live open items inside HTML comments (lexical scoping, dotted `§runtime` value, v0.7 parity clusters, the 4-vs-8 extension-role registry).
- L1 boilerplate "L2 specs cannot reach RFC until all invariants here are addressed" contradicts multi-L2 realization (42 parent/child pairs are partial by design).
