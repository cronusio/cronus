# QA remediation progress — branch fix/cli-simulation-qa-findings

Plan: all tranches in order. Host BSOD-unstable -> incremental, persist after each item.
Build: PowerShell, `cargo <cmd> -p cronus-cli -j 2` (rusqlite bundled needs C toolchain).

## Tranche 0 — stop the bleeding

- [x] R1  board id sanitization -> KanbanError::InvalidCardId; validate in add_card+get_card (domain/kanban/mod.rs). 3 unit tests. domain 492+9 green, core check+kanban_board 15 green.
- [x] R2  append_event now create_dir_all(events_dir) first (root cause: events/ never created by add_card path). unit test move_card_does_not_partially_write. domain green.

## Tranche 1 — conformance

- [x] R3  subcommand_required+arg_required_else_help on every semantic group (generated.rs) + every nested/sub-nested installation group (installation.rs). cli_smoke: a_group_with_no_verb_is_a_usage_failure. cli 48 lib + 32 smoke green, clippy clean.
- [x] R4  status::resolve_root walks cwd ancestors for app.json, falls back to OS state tier (was: only ever checked OS tier, which CLI init never populates). pure resolve_root_from + test. cli green. NOTE: couples with R15 (if init moves to .cronus/, update resolve_root_from marker).
- [x] R5  D4 RESOLVED: fix the backend. memory verbs now open ONE persistent MemoryStore::open(`<state>/memory/memory.db`) (was open_in_memory per-dispatch -> nothing persisted, fake id). Also fixed FTS injection (F-CLI-14, new): search_fts/search_ranked passed raw input to `MATCH` -> a hyphen errored "no such column"; now fts_match_expr tokenises + quotes each token. Rewrote the SP-10 test whose doc-comment declared the ephemeral behaviour intentional. New: crates/core/tests/memory_dispatch.rs (round-trip), store-local fts_search_tolerates_query_metacharacters. domain+store-local+core+cli all green.
- [x] R6  ctx threaded into archetype_cmd::{list,info,create}; each emits JSON when --format json. backup list empty arm -> [].
- [x] R7  shared crate::output::json_escape (proper: control chars,

 ). Applied to backup create/list, archetype set/list/info/create, workspace create/switch/delete (WorkspaceId->to_string), ext activate/deactivate, registry create/disable/enable. init deduped to shared helper. Stale "Known residual" JSON comments removed. New: output json_escape unit test + cli_smoke installation_verbs_emit_valid_json.

- [x] R8  DECISION: add serde to domain. serde+serde_json added to cronus-domain; #[derive(Serialize,Deserialize)] on AgentMode/Ruleset/AgentDefinition. New AgentRegistry::{persist_path,load,load_from,save,save_to} -> `<state>/agents/registry.json` (overlay: non-native + disabled + changed model_ref). CLI registry list/show use load(); create/disable/enable load->mutate->save (exit 1 + msg on save failure). 3 domain unit tests. domain 495, core agent tests + clippy green.
- [x] R9  workflow scaffold: `--out` wins; a positional ending in .nodus is used verbatim (no more foo.nodus.nodus); workflow identity = file stem so scaffold->validate round-trips. cli_smoke workflow_scaffold_writes_the_named_path_and_it_validates.
- [x] R10  completion verb: new Installation invocable `completion <shell>` (bash/zsh/fish/powershell/elvish) via clap_complete; emitted from the composed command tree in main.rs (joins --help/--version as a full-surface request so it is not routed through the pre-composition grammar). LH-6 pre-composition-artifact layer noted as follow-up. clap_complete added to workspace + cli deps. smoke test per shell + bad-shell.

## Tranche 2 — hardening & cost

- [x] R11 new cronus_domain::io_message::describe (ErrorKind -> English phrase + os code, never OS locale). Wired: workflow validate/run/transpile "cannot read", loop_group persist + read_report/read_spec (NotFound -> "run 'X' not found"). Helper reusable for future sites. unit test asserts ASCII/English.
- [x] R12  BinderKind::EnumText(&'static [&'static str]) added to contract. arg_for -> PossibleValuesParser (--help lists values, exit 2 on bad); invocation_from_matches + tui/dispatch read it as Text; dispatch.rs matches_kind enforces membership (direct callers cannot bypass). board move <state> now EnumText(CardState::NAMES). smoke test updated.
- [x] R13 added OutcomeValue::Float(f64) to contract; render_json emits bare number (finite-guarded), render_text + tui render arms added, redact catch-all covers it. budget show/set + nodus_value_to_outcome now use Float. render test extended (bare float + record-with-float, both parse as f64).
- [x] R14  exec_workspace::create now validates ws_id AND card_id as safe single path segments (ExecError::InvalidId) — the slug `<ws_id>-<card_id>-<ts>` becomes a directory name under the exec root, so `../` in either was an arbitrary-directory-create. unit test.

## Tranche 3 — debt & polish

- [x] R15  init now writes the state skeleton into <dir>/.cronus/ instead of scattering app.json/AGENTS.md/employees/... across the target dir top level. resolve_workspace_root_from checks .cronus/app.json first (returns the .cronus dir), then bare app.json (OS state tier shape), then fallback -- so status/doctor still work. smoke test: skeleton under .cronus/, project files untouched, status finds it.
- [x] R16  paths::display_clean strips the Windows verbatim-path prefix; wired into init + workflow scaffold; unit test.
- [x] R17  generated.rs semantic_group_about: curated one-liners for all 13 semantic groups.
- [x] R18  workspace list empty -> "No results."; also escaped w.id in that JSON.
- [x] R19  generated.rs binder_help(invocable_id, binder_name) -> Option<&'static str> (hand-kept map, mirrors semantic_group_about / group_about) wired via arg_for_with_help into every semantic + installation verb arg. No Binder contract change (side-table keeps it CLI-local). Also: status shows the .cronus parent dir name, not literal ".cronus" (R15 follow-on). smoke test.
- [x] R20  main.rs merges+sorts installation+semantic groups before adding subcommands; smoke test.
- [x] R21  root cause = doctor checked OS state tier not where init writes; shared resolve_workspace_root now used by status AND doctor.
- [x] R22  roles::hire without custom name -> `<preset>-<n>` lowest free n (was wall-clock ms).

## Log

- T1 further: R6+R7 in commands.rs (archetype_cmd, backup_cmd, workspace, ext, registry, init) + output.rs (new json_escape). clippy+test cli green (49 lib + 33 smoke).
- R8 hit a wall: registry has no persistence at all. Checkpointing.
- T1 further: R5 touches crates/store-local/src/memory/store.rs (fts_match_expr), crates/core/src/invocable_bootstrap/memory.rs (open_memory_store), crates/core/tests/{invocable_invariants.rs edited, memory_dispatch.rs new}, crates/store-local/tests/memory_store.rs. FULL GATES GREEN: clippy(domain,store-local,core,cli) clean; cargo test all crates 0 failed (core: ~35 binaries all ok). Still uncommitted.
- CHECKPOINT after R1-R5 (all 4 HIGH findings + D4 resolution + F-CLI-14 bonus). 17 items remain (R6-R22, MEDIUM/LOW).
- T1 partial: R3 (generated.rs, installation.rs), R4 (commands.rs status). Per-crate gates green. Still uncommitted.
- T0 done: R1+R2 in crates/domain/src/kanban/mod.rs. Gates: fmt+clippy+test(-p cronus-domain) clean; cargo check -p cronus-core + --test kanban_board clean. Not committed (awaiting user go on commits).
