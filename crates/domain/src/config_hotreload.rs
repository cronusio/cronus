//! Configuration hot-reload: diff two config snapshots
//! into changed key-paths, classify each path through a prefix-ordered rule
//! table into a restart/hot/none reload plan, invalidate the skills snapshot
//! when applicable, and recover a failing file watcher through bounded
//! backoff before degrading to polling.
//!
//! Config *values* never appear here — a snapshot diff yields key-paths
//! only, so secret fields are structurally excluded from every log-worthy
//! artifact this module produces, with no separate redaction step
//! needed.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// A flattened config document: key-path → stringified value. Values are
/// used only to detect *whether* a path changed; they never leave this type.
pub type ConfigSnapshot = BTreeMap<String, String>;

/// Diff two snapshots into the flat set of key-paths that differ (added,
/// removed, or changed value) — never the values themselves.
pub fn diff_config(old: &ConfigSnapshot, new: &ConfigSnapshot) -> Vec<String> {
    let mut changed: Vec<String> = old
        .iter()
        .filter_map(|(path, old_value)| match new.get(path) {
            Some(new_value) if new_value == old_value => None,
            _ => Some(path.clone()),
        })
        .collect();
    for path in new.keys() {
        if !old.contains_key(path) {
            changed.push(path.clone());
        }
    }
    changed.sort();
    changed.dedup();
    changed
}

/// How a matched config path should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadKind {
    Restart,
    Hot,
    None,
}

/// A subsystem action dispatched for a `Hot` reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadAction {
    ReloadHooks,
    RestartCron,
    RestartHeartbeat,
    RestartHealthMonitor,
    ReloadPlugins,
    DisposeMcpRuntimes,
    RestartChannel(String),
}

/// One entry of the prefix-ordered rule table. The first entry whose
/// `prefix` matches a changed path wins.
#[derive(Debug, Clone)]
pub struct ReloadRule {
    pub prefix: &'static str,
    pub kind: ReloadKind,
    pub actions: Vec<ReloadAction>,
}

fn rule(prefix: &'static str, kind: ReloadKind, actions: Vec<ReloadAction>) -> ReloadRule {
    ReloadRule {
        prefix,
        kind,
        actions,
    }
}

/// Whether `path` is matched by `prefix`: an exact match, or a match at a
/// `.`-delimited boundary (so `"hooks"` matches `"hooks.gmail"` but not
/// `"hooksmith"`).
fn prefix_matches(prefix: &str, path: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}."))
}

/// The built-in rule table, in priority order. More specific prefixes
/// (`hooks.gmail`) are listed before their broader parents (`hooks`) so
/// first-match-wins resolves correctly; custom rules may be prepended by a
/// caller to take precedence over these.
pub fn builtin_rules() -> Vec<ReloadRule> {
    use ReloadAction::*;
    use ReloadKind::*;
    vec![
        rule("daemon.reload", None, vec![]),
        rule("daemon.remote", None, vec![]),
        rule("diagnostics.stuckSessionWarnMs", None, vec![]),
        rule("diagnostics.stuckSessionAbortMs", None, vec![]),
        rule("diagnostics.memoryPressureSnapshot", Hot, vec![]),
        rule("hooks.gmail", Hot, vec![RestartHeartbeat]),
        rule("hooks", Hot, vec![ReloadHooks]),
        rule("agents.defaults.heartbeat", Hot, vec![RestartHeartbeat]),
        rule("agents.defaults.models", Hot, vec![RestartHeartbeat]),
        rule("agents.defaults.model", Hot, vec![RestartHeartbeat]),
        rule("models.pricing", Restart, vec![]),
        rule("models", Hot, vec![]),
        rule("skills", Hot, vec![]), // skills snapshot invalidation is separate.3
        rule("mcp", Hot, vec![DisposeMcpRuntimes]),
        rule("plugins", Hot, vec![ReloadPlugins]),
        rule("cron", Hot, vec![RestartCron]),
        rule("scheduler", Hot, vec![RestartCron]),
    ]
}

/// Match `channels.<id>` paths specifically, since the channel id is part of
/// the emitted action; checked after the static table finds no match.
fn channel_rule_for(path: &str) -> Option<(ReloadKind, Vec<ReloadAction>)> {
    let id = path.strip_prefix("channels.")?.split('.').next()?;
    Some((
        ReloadKind::Hot,
        vec![ReloadAction::RestartChannel(id.to_string())],
    ))
}

/// The reload planner's output.
#[derive(Debug, Clone, Default)]
pub struct ConfigReloadPlan {
    pub changed_paths: Vec<String>,
    pub restart_daemon: bool,
    pub restart_reasons: Vec<String>,
    pub hot_reasons: Vec<String>,
    pub actions: Vec<ReloadAction>,
    pub noop_paths: Vec<String>,
}

/// Build a reload plan by matching every changed path against `rules`
/// (first-match-wins), falling back to `restart` for anything unmatched —
/// the safe default for unknown config changes.
pub fn build_plan(changed_paths: &[String], rules: &[ReloadRule]) -> ConfigReloadPlan {
    let mut plan = ConfigReloadPlan {
        changed_paths: changed_paths.to_vec(),
        ..Default::default()
    };
    for path in changed_paths {
        let matched = rules
            .iter()
            .find(|rule| prefix_matches(rule.prefix, path))
            .map(|rule| (rule.kind, rule.actions.clone()))
            .or_else(|| channel_rule_for(path));

        match matched {
            Some((ReloadKind::Restart, _)) => {
                plan.restart_daemon = true;
                plan.restart_reasons.push(path.clone());
            }
            Some((ReloadKind::Hot, actions)) => {
                plan.hot_reasons.push(path.clone());
                for action in actions {
                    if !plan.actions.contains(&action) {
                        plan.actions.push(action);
                    }
                }
            }
            Some((ReloadKind::None, _)) => plan.noop_paths.push(path.clone()),
            None => {
                // No prefix matched at all: unknown path, safe default = restart.
                plan.restart_daemon = true;
                plan.restart_reasons.push(path.clone());
            }
        }
    }
    plan
}

/// A plan requiring no subsystem notification at all.
pub fn is_noop(plan: &ConfigReloadPlan) -> bool {
    !plan.restart_daemon && plan.hot_reasons.is_empty() && plan.actions.is_empty()
}

/// Prefixes whose change invalidates the skills snapshot.
const SKILLS_INVALIDATION_PREFIXES: &[&str] = &["skills"];

/// Whether any changed path falls under a skills-invalidating prefix.
pub fn should_invalidate_skills_snapshot(changed_paths: &[String]) -> bool {
    changed_paths.iter().any(|path| {
        SKILLS_INVALIDATION_PREFIXES
            .iter()
            .any(|prefix| prefix_matches(prefix, path))
    })
}

/// Monotonic counter sessions compare against their cached tool-set snapshot
/// version; a relaxed ordering is sufficient — this is a "did it change at
/// all" signal, not a synchronization point.
#[derive(Debug, Default)]
pub struct SkillsSnapshotVersion(AtomicU64);

impl SkillsSnapshotVersion {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn bump(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

// --- File watcher lifecycle ---

pub const WATCHER_RECREATE_MAX_RETRIES: u8 = 3;
pub const WATCHER_RECREATE_BACKOFF_MS: [u64; 3] = [500, 2_000, 5_000];
pub const WATCHER_POLL_INTERVAL_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherStatus {
    /// Native OS watcher running.
    Active,
    /// OS watcher failed; falling back to interval polling.
    Polling,
    /// All retries exhausted; reloads are no longer detected.
    Disabled,
}

/// What the caller should do next after a recovery-loop transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStep {
    /// Wait this many milliseconds, then attempt to recreate the watcher.
    WaitThenRecreate(u64),
    /// Switch to interval polling at this cadence.
    SwitchToPolling(u64),
    /// Poll failed too; give up until an operator restarts the daemon.
    GiveUp,
    /// Nothing to do — already healthy.
    Continue,
}

/// The bounded backoff → polling → disabled state machine, driven by
/// caller-reported outcomes rather than owning any real OS watcher — this
/// keeps the recovery *logic* testable without spawning inotify/FSEvents.
#[derive(Debug)]
pub struct WatcherRecovery {
    status: WatcherStatus,
    attempt: u8,
}

impl Default for WatcherRecovery {
    fn default() -> Self {
        Self {
            status: WatcherStatus::Active,
            attempt: 0,
        }
    }
}

impl WatcherRecovery {
    pub fn new() -> Self {
        Self::default()
    }

    /// Force polling mode from the start (`CONFIG_WATCHER_POLL=1`).
    pub fn force_polling() -> Self {
        Self {
            status: WatcherStatus::Polling,
            attempt: 0,
        }
    }

    pub fn status(&self) -> WatcherStatus {
        self.status
    }

    /// The native watcher reported an error: log-worthy (caller's job), and
    /// begin the bounded-retry recovery loop.
    pub fn on_watcher_error(&mut self) -> RecoveryStep {
        self.attempt = 0;
        self.retry_step()
    }

    fn retry_step(&self) -> RecoveryStep {
        RecoveryStep::WaitThenRecreate(WATCHER_RECREATE_BACKOFF_MS[self.attempt as usize])
    }

    /// The caller attempted to recreate the watcher after the backoff wait.
    pub fn on_recreate_result(&mut self, succeeded: bool) -> RecoveryStep {
        if succeeded {
            self.status = WatcherStatus::Active;
            self.attempt = 0;
            return RecoveryStep::Continue;
        }
        if self.attempt + 1 < WATCHER_RECREATE_MAX_RETRIES {
            self.attempt += 1;
            return self.retry_step();
        }
        self.status = WatcherStatus::Polling;
        RecoveryStep::SwitchToPolling(WATCHER_POLL_INTERVAL_MS)
    }

    /// Polling mode itself failed (e.g. the config path became unreadable).
    pub fn on_polling_failure(&mut self) -> RecoveryStep {
        self.status = WatcherStatus::Disabled;
        RecoveryStep::GiveUp
    }
}

/// The reload status exposed through health/diagnostics. `last_plan`
/// carries only key-paths and action names — never values.
#[derive(Debug, Clone, Default)]
pub struct HotReloadStatus {
    pub watcher_status: Option<WatcherStatus>,
    pub last_reload_at: Option<Instant>,
    pub last_plan_changed_paths: Vec<String>,
    pub last_plan_actions: Vec<ReloadAction>,
    pub last_plan_restart: bool,
}

impl HotReloadStatus {
    pub fn record(&mut self, plan: &ConfigReloadPlan, at: Instant) {
        self.last_reload_at = Some(at);
        self.last_plan_changed_paths = plan.changed_paths.clone();
        self.last_plan_actions = plan.actions.clone();
        self.last_plan_restart = plan.restart_daemon;
    }
}

/// Convenience: run the full reload sequence for one detected change,
/// returning the plan and whether it was a no-op. Skills invalidation is
/// applied as a side effect on `skills_version` before the caller dispatches
/// hot actions, matching the documented ordering.
pub fn plan_reload(
    old: &ConfigSnapshot,
    new: &ConfigSnapshot,
    rules: &[ReloadRule],
    skills_version: &SkillsSnapshotVersion,
) -> (ConfigReloadPlan, bool) {
    let changed_paths = diff_config(old, new);
    let plan = build_plan(&changed_paths, rules);
    let noop = is_noop(&plan);
    if !noop && should_invalidate_skills_snapshot(&plan.changed_paths) {
        skills_version.bump();
    }
    (plan, noop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(pairs: &[(&str, &str)]) -> ConfigSnapshot {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn diff_detects_added_removed_and_changed_paths() {
        let old = snapshot(&[("a", "1"), ("b", "2"), ("c", "3")]);
        let new = snapshot(&[("a", "1"), ("b", "changed"), ("d", "4")]);
        assert_eq!(diff_config(&old, &new), vec!["b", "c", "d"]);
    }

    #[test]
    fn identical_snapshots_diff_to_nothing() {
        let snap = snapshot(&[("a", "1")]);
        assert!(diff_config(&snap, &snap).is_empty());
    }

    #[test]
    fn a_changed_path_maps_to_exactly_its_subsystem_action() {
        let rules = builtin_rules();
        let plan = build_plan(&["hooks.gmail".to_string()], &rules);
        assert!(!plan.restart_daemon);
        assert_eq!(plan.actions, vec![ReloadAction::RestartHeartbeat]);
    }

    #[test]
    fn a_more_specific_prefix_wins_over_its_broader_parent() {
        let rules = builtin_rules();
        // "hooks.gmail" must resolve via its specific rule (RestartHeartbeat),
        // not the broader "hooks" rule (ReloadHooks) — first-match-wins over
        // a correctly-ordered table.
        let plan = build_plan(&["hooks.gmail".to_string()], &rules);
        assert!(!plan.actions.contains(&ReloadAction::ReloadHooks));

        let plan = build_plan(&["hooks.slack".to_string()], &rules);
        assert_eq!(plan.actions, vec![ReloadAction::ReloadHooks]);
    }

    #[test]
    fn model_pricing_restarts_but_other_model_paths_hot_reload() {
        let rules = builtin_rules();
        let plan = build_plan(&["models.pricing.gpt".to_string()], &rules);
        assert!(plan.restart_daemon);

        let plan = build_plan(&["models.defaultProvider".to_string()], &rules);
        assert!(!plan.restart_daemon);
    }

    #[test]
    fn channel_paths_restart_the_named_channel() {
        let rules = builtin_rules();
        let plan = build_plan(&["channels.slack-1.token".to_string()], &rules);
        assert_eq!(
            plan.actions,
            vec![ReloadAction::RestartChannel("slack-1".to_string())]
        );
    }

    #[test]
    fn unmatched_paths_default_to_restart_the_safe_fallback() {
        let rules = builtin_rules();
        let plan = build_plan(&["totally.unknown.path".to_string()], &rules);
        assert!(plan.restart_daemon);
        assert_eq!(
            plan.restart_reasons,
            vec!["totally.unknown.path".to_string()]
        );
    }

    #[test]
    fn none_kind_paths_are_tracked_as_noop_and_produce_no_plan_action() {
        let rules = builtin_rules();
        let plan = build_plan(&["daemon.reload".to_string()], &rules);
        assert!(is_noop(&plan));
        assert_eq!(plan.noop_paths, vec!["daemon.reload".to_string()]);
    }

    #[test]
    fn an_empty_change_set_is_a_noop_plan() {
        let plan = build_plan(&[], &builtin_rules());
        assert!(is_noop(&plan));
    }

    #[test]
    fn skills_prefix_change_invalidates_the_snapshot_version() {
        let version = SkillsSnapshotVersion::new();
        assert_eq!(version.current(), 0);
        assert!(should_invalidate_skills_snapshot(&[
            "skills.myPack.enabled".to_string()
        ]));
        assert!(should_invalidate_skills_snapshot(&["skills".to_string()]));
        assert!(!should_invalidate_skills_snapshot(&[
            "skillset.other".to_string()
        ]));

        let old = snapshot(&[("skills.myPack.enabled", "false")]);
        let new = snapshot(&[("skills.myPack.enabled", "true")]);
        let (_, noop) = plan_reload(&old, &new, &builtin_rules(), &version);
        assert!(!noop);
        assert_eq!(
            version.current(),
            1,
            "skills change bumps the version exactly once"
        );
    }

    #[test]
    fn non_skills_changes_never_bump_the_skills_version() {
        let version = SkillsSnapshotVersion::new();
        let old = snapshot(&[("cron.enabled", "false")]);
        let new = snapshot(&[("cron.enabled", "true")]);
        plan_reload(&old, &new, &builtin_rules(), &version);
        assert_eq!(version.current(), 0);
    }

    #[test]
    fn watcher_recovery_backs_off_then_recreates_successfully() {
        let mut recovery = WatcherRecovery::new();
        assert_eq!(recovery.status(), WatcherStatus::Active);

        let step = recovery.on_watcher_error();
        assert_eq!(
            step,
            RecoveryStep::WaitThenRecreate(WATCHER_RECREATE_BACKOFF_MS[0])
        );

        let step = recovery.on_recreate_result(true);
        assert_eq!(step, RecoveryStep::Continue);
        assert_eq!(recovery.status(), WatcherStatus::Active);
    }

    #[test]
    fn watcher_recovery_degrades_to_polling_after_exhausting_retries_never_a_busy_loop() {
        let mut recovery = WatcherRecovery::new();
        recovery.on_watcher_error();

        // Two failed retries stay in the backoff loop with increasing delays.
        let step = recovery.on_recreate_result(false);
        assert_eq!(
            step,
            RecoveryStep::WaitThenRecreate(WATCHER_RECREATE_BACKOFF_MS[1])
        );
        let step = recovery.on_recreate_result(false);
        assert_eq!(
            step,
            RecoveryStep::WaitThenRecreate(WATCHER_RECREATE_BACKOFF_MS[2])
        );

        // The third failure exhausts MAX_RETRIES and degrades to polling —
        // never an unbounded retry loop.
        let step = recovery.on_recreate_result(false);
        assert_eq!(
            step,
            RecoveryStep::SwitchToPolling(WATCHER_POLL_INTERVAL_MS)
        );
        assert_eq!(recovery.status(), WatcherStatus::Polling);
    }

    #[test]
    fn polling_failure_disables_reloads_rather_than_dying_silently() {
        let mut recovery = WatcherRecovery::new();
        recovery.on_watcher_error();
        for _ in 0..WATCHER_RECREATE_MAX_RETRIES {
            recovery.on_recreate_result(false);
        }
        assert_eq!(recovery.status(), WatcherStatus::Polling);

        let step = recovery.on_polling_failure();
        assert_eq!(step, RecoveryStep::GiveUp);
        assert_eq!(
            recovery.status(),
            WatcherStatus::Disabled,
            "disabled is reported, not a silent stop"
        );
    }

    #[test]
    fn a_recovered_watcher_resets_the_retry_counter_for_the_next_failure() {
        let mut recovery = WatcherRecovery::new();
        recovery.on_watcher_error();
        recovery.on_recreate_result(false); // attempt 1 failed
        recovery.on_recreate_result(true); // recovered
        assert_eq!(recovery.status(), WatcherStatus::Active);

        // A fresh error starts the backoff sequence over, not mid-sequence.
        let step = recovery.on_watcher_error();
        assert_eq!(
            step,
            RecoveryStep::WaitThenRecreate(WATCHER_RECREATE_BACKOFF_MS[0])
        );
    }

    #[test]
    fn force_polling_starts_in_polling_status() {
        let recovery = WatcherRecovery::force_polling();
        assert_eq!(recovery.status(), WatcherStatus::Polling);
    }

    #[test]
    fn hot_reload_status_records_key_paths_and_actions_never_values() {
        let mut status = HotReloadStatus::default();
        let old = snapshot(&[("cron.enabled", "false")]);
        let new = snapshot(&[("cron.enabled", "true")]);
        let plan = build_plan(&diff_config(&old, &new), &builtin_rules());
        status.record(&plan, Instant::now());

        assert_eq!(
            status.last_plan_changed_paths,
            vec!["cron.enabled".to_string()]
        );
        assert!(
            status
                .last_plan_actions
                .contains(&ReloadAction::RestartCron)
        );
        // The recorded status carries paths/actions only — no value ever
        // appears anywhere in ConfigReloadPlan or HotReloadStatus by
        // construction.
    }
}
