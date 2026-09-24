//! Sandbox network egress policy: named, per-binary
//! allowlisted policy entries on top of a deny-by-default baseline, with
//! opt-in tiers/presets and typed, auditable access-failure classification.
//! This is the binary-scoped least-privilege layer on top of the
//! flat [`crate::egress::EgressGate`] — a binary not listed in any policy
//! entry is denied every endpoint, regardless of what the gate itself would
//! otherwise allow.

use std::collections::{BTreeMap, BTreeSet};

/// Transport protocol for an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Rest,
    WebSocket,
}

/// TLS handling for an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    Terminate,
    Passthrough,
    /// No TLS — restricted to localhost/trusted internal networks by policy.
    Skip,
}

/// Whether violations of an endpoint's rules block the call or only log it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    Enforce,
    Audit,
}

/// A single allowed remote endpoint. `host` may be a wildcard `"*.example.com"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub enforcement: Enforcement,
    pub tls: TlsMode,
}

impl Endpoint {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            protocol: Protocol::Rest,
            enforcement: Enforcement::Enforce,
            tls: TlsMode::Terminate,
        }
    }

    /// Whether this endpoint's host pattern matches a target host. A
    /// `"*.suffix"` pattern matches the apex `suffix` and any subdomain.
    pub fn matches_host(&self, host: &str) -> bool {
        match self.host.strip_prefix("*.") {
            Some(suffix) => host == suffix || host.ends_with(&format!(".{suffix}")),
            None => self.host == host,
        }
    }
}

/// A named network policy entry: allowed endpoints plus the canonical
/// absolute binary paths authorized to use them.
#[derive(Debug, Clone, Default)]
pub struct NetworkPolicyEntry {
    pub name: String,
    pub endpoints: Vec<Endpoint>,
    pub binaries: BTreeSet<String>,
}

impl NetworkPolicyEntry {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            endpoints: Vec::new(),
            binaries: BTreeSet::new(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: Endpoint) -> Self {
        self.endpoints.push(endpoint);
        self
    }

    pub fn with_binary(mut self, binary: impl Into<String>) -> Self {
        self.binaries.insert(binary.into());
        self
    }
}

/// Why an access request was denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDenied {
    /// The calling binary is not listed in any policy entry — denied
    /// regardless of target (deny-by-default).
    BinaryNotAllowlisted,
    /// The binary has entries, but none of its reachable endpoints match.
    EndpointNotAllowed,
}

/// The sandbox network policy: named entries over a deny-by-default baseline.
#[derive(Debug, Clone, Default)]
pub struct SandboxPolicy {
    pub version: u32,
    entries: BTreeMap<String, NetworkPolicyEntry>,
}

impl SandboxPolicy {
    pub fn new() -> Self {
        Self {
            version: 1,
            entries: BTreeMap::new(),
        }
    }

    pub fn add_entry(&mut self, entry: NetworkPolicyEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    /// The union of endpoints reachable by `binary` across every entry it
    /// appears in (`effective_egress`).
    pub fn effective_egress(&self, binary: &str) -> Vec<&Endpoint> {
        self.entries
            .values()
            .filter(|entry| entry.binaries.contains(binary))
            .flat_map(|entry| entry.endpoints.iter())
            .collect()
    }

    /// Deny-by-default check: a binary absent from every entry is denied
    /// outright; otherwise the target host/port must match one of its
    /// reachable endpoints.
    pub fn check_access(&self, binary: &str, host: &str, port: u16) -> Result<(), AccessDenied> {
        let reachable = self.effective_egress(binary);
        if reachable.is_empty() {
            return Err(AccessDenied::BinaryNotAllowlisted);
        }
        if reachable
            .iter()
            .any(|endpoint| endpoint.matches_host(host) && endpoint.port == port)
        {
            Ok(())
        } else {
            Err(AccessDenied::EndpointNotAllowed)
        }
    }
}

/// The filesystem half of the sandbox schema —
/// only `read_write` matters for the write barrier: a location absent from every entry has
/// no write path for agent-run code, regardless of what `read_only` or
/// `include_workdir` (not modeled here — orthogonal to write access) exposes.
/// Deny-by-default: absence means denied, never merely "not confirmed".
#[derive(Debug, Clone, Default)]
pub struct FilesystemPolicy {
    read_write: BTreeSet<String>,
}

impl FilesystemPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_read_write(mut self, path: impl Into<String>) -> Self {
        self.read_write.insert(path.into());
        self
    }

    /// Whether `path` (or a path under it) is mounted read-write by any
    /// entry.
    pub fn is_read_write(&self, path: &str) -> bool {
        self.read_write.iter().any(|mount| {
            path == mount
                || path.starts_with(&format!("{mount}/"))
                || path.starts_with(&format!("{mount}\\"))
        })
    }
}

/// OS activation-registration locations that are filesystem paths
/// (the second structural barrier): none of
/// these may ever appear in a `FilesystemPolicy`'s `read_write` set, or
/// agent-run code would gain a write path to make the engine persistent and
/// unattended.
///
/// The Windows registry `Run` key and the Windows Task Scheduler store are
/// OS registration surfaces too, but are not filesystem paths — no
/// `read_write` model can represent them. They are covered by a different
/// mechanism (the OS's own registry/Task-Scheduler ACLs, which a sandboxed
/// process does not hold by default), named here only for documentation
/// completeness — never asserted against `FilesystemPolicy`, which has no
/// vocabulary for them.
pub const FILESYSTEM_REGISTRATION_LOCATIONS: &[&str] = &[
    "~/Library/LaunchAgents",
    "~/.config/systemd/user",
    "/etc/systemd/system",
    "~/.config/autostart",
];

/// Non-filesystem OS registration surfaces, named for documentation
/// completeness only — see [`FILESYSTEM_REGISTRATION_LOCATIONS`].
pub const NON_FILESYSTEM_REGISTRATION_SURFACES: &[&str] = &[
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
    "Windows Task Scheduler",
];

/// An access tier: a named combination of presets over the restricted baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyTier {
    Restricted,
    Balanced,
    Open,
}

/// The built-in preset names a tier includes. `restricted` is the
/// baseline (inference + gateway only) and carries no additional presets;
/// `open` is never the default — appropriate only for a fully-trusted,
/// user-reviewed sandbox.
pub fn tier_presets(tier: PolicyTier) -> &'static [&'static str] {
    const BALANCED: &[&str] = &[
        "npm-registry",
        "pypi",
        "huggingface",
        "brave-search",
        "weather",
    ];
    const OPEN: &[&str] = &[
        "npm-registry",
        "pypi",
        "huggingface",
        "brave-search",
        "weather",
        "public-reference",
        "slack",
        "discord",
        "telegram",
        "jira",
        "email",
    ];
    match tier {
        PolicyTier::Restricted => &[],
        PolicyTier::Balanced => BALANCED,
        PolicyTier::Open => OPEN,
    }
}

/// Where a preset came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetSource {
    Builtin,
    Custom,
}

/// How a preset's registry entry and the live policy agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetVerification {
    /// Verified against both the registry and the active policy.
    Verified,
    /// Known in the registry but not yet applied to the policy.
    RegistryOnly,
    /// Applied in the policy but not found in the registry (policy ahead).
    GatewayOnly,
    /// Registry unreachable; verification was skipped.
    GatewayUnavailable,
}

/// A discoverable, opt-in policy add-on. Specific hosts are deliberately
/// **not** enumerated here (`redacted_host_count` only) — the privacy-facing
/// object never lists real hostnames; matching a target host against a
/// preset is an internal registry lookup (see [`preset_for_host`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyPreset {
    pub name: String,
    pub description: String,
    pub allowed_host_categories: Vec<String>,
    pub redacted_host_count: u32,
    pub source: PresetSource,
    pub verification: PresetVerification,
}

/// Internal host → preset-name lookup backing access-failure classification.
/// Kept separate from [`PolicyPreset`] so the user-facing preset object never
/// carries real hostnames (privacy note).
fn preset_for_host(host: &str) -> Option<&'static str> {
    const REGISTRY: &[(&str, &str)] = &[
        ("registry.npmjs.org", "npm-registry"),
        ("pypi.org", "pypi"),
        ("huggingface.co", "huggingface"),
        ("api.search.brave.com", "brave-search"),
        ("api.weather.gov", "weather"),
        ("slack.com", "slack"),
        ("discord.com", "discord"),
        ("api.telegram.org", "telegram"),
        ("atlassian.net", "jira"),
    ];
    REGISTRY
        .iter()
        .find(|(known_host, _)| host == *known_host || host.ends_with(&format!(".{known_host}")))
        .map(|(_, preset)| *preset)
}

/// Who owns enforcement of a policy capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryOwner {
    Host,
    Agent,
    External,
}

/// A capability boundary surfaced to the agent for transparency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportBoundary {
    pub capability: String,
    pub owner: BoundaryOwner,
    pub note: Option<String>,
}

/// Read-only snapshot of the running sandbox's policy state. The host
/// process is the sole writer of the policy file; the agent inspects this
/// snapshot and requests changes only through `approval_path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyContext {
    pub sandbox_name: String,
    pub tier: PolicyTier,
    pub active_presets: Vec<PolicyPreset>,
    pub known_unapplied_presets: Vec<PolicyPreset>,
    pub approval_path: String,
    pub support_boundaries: Vec<SupportBoundary>,
}

/// A signal from the OS/network layer strongly indicating a policy block
/// (stands in for the raw OS error codes named).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsErrorSignal {
    DnsResolutionFailed,
    NetworkUnreachable,
    HostUnreachable,
    ConnectionRefused,
    TimedOut,
}

/// Classification confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Low,
}

/// Why an outbound call was blocked or failed, classified for the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessFailureKind {
    BlockedByPolicy,
    MissingApproval,
    Unsupported,
    Unknown,
}

/// The full classification record, written to the audit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessFailureClassification {
    pub kind: AccessFailureKind,
    pub reason: String,
    pub next_step: String,
    pub matched_preset: Option<String>,
    pub confidence: Confidence,
}

/// Classify an access failure (first match wins):
/// 1. an OS-level network signal → `blocked-by-policy` (high confidence);
/// 2. a 401/403 response → `missing-approval` (high confidence);
/// 3. the target host matches a known-but-unapplied preset → `missing-approval` (high confidence);
/// 4. otherwise → `unknown` (low confidence) — this may not be policy-related.
pub fn classify_access_failure(
    os_signal: Option<OsErrorSignal>,
    http_status: Option<u16>,
    target_host: &str,
    known_unapplied: &[PolicyPreset],
) -> AccessFailureClassification {
    if os_signal.is_some() {
        return AccessFailureClassification {
            kind: AccessFailureKind::BlockedByPolicy,
            reason: "a network-level signal indicates the sandbox blocked this connection".into(),
            next_step: "review the active sandbox policy or request an expansion".into(),
            matched_preset: None,
            confidence: Confidence::High,
        };
    }
    if matches!(http_status, Some(401) | Some(403)) {
        let matched = preset_for_host(target_host)
            .filter(|preset| known_unapplied.iter().any(|p| p.name == *preset))
            .map(str::to_string);
        return AccessFailureClassification {
            kind: AccessFailureKind::MissingApproval,
            reason: format!(
                "endpoint responded with {}",
                http_status.unwrap_or_default()
            ),
            next_step: "request the preset that grants this endpoint via the approval path".into(),
            matched_preset: matched,
            confidence: Confidence::High,
        };
    }
    if let Some(preset) = preset_for_host(target_host)
        && let Some(known) = known_unapplied.iter().find(|p| p.name == preset)
    {
        return AccessFailureClassification {
            kind: AccessFailureKind::MissingApproval,
            reason: format!(
                "{target_host} is covered by the known but unapplied `{preset}` preset"
            ),
            next_step: "request the preset via the approval path".into(),
            matched_preset: Some(known.name.clone()),
            confidence: Confidence::High,
        };
    }
    AccessFailureClassification {
        kind: AccessFailureKind::Unknown,
        reason: "failure does not match any known policy pattern".into(),
        next_step: "inspect the raw error; this may not be policy-related".into(),
        matched_preset: None,
        confidence: Confidence::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npm_entry() -> NetworkPolicyEntry {
        NetworkPolicyEntry::new("npm-registry")
            .with_endpoint(Endpoint::new("registry.npmjs.org", 443))
            .with_binary("/usr/bin/node")
    }

    #[test]
    fn binary_absent_from_every_entry_is_denied_regardless_of_target() {
        let mut policy = SandboxPolicy::new();
        policy.add_entry(npm_entry());
        assert_eq!(
            policy.check_access("/usr/bin/curl", "registry.npmjs.org", 443),
            Err(AccessDenied::BinaryNotAllowlisted)
        );
    }

    #[test]
    fn allowlisted_binary_with_a_matching_endpoint_is_permitted() {
        let mut policy = SandboxPolicy::new();
        policy.add_entry(npm_entry());
        assert_eq!(
            policy.check_access("/usr/bin/node", "registry.npmjs.org", 443),
            Ok(())
        );
    }

    #[test]
    fn allowlisted_binary_requesting_an_uncovered_endpoint_is_denied() {
        let mut policy = SandboxPolicy::new();
        policy.add_entry(npm_entry());
        assert_eq!(
            policy.check_access("/usr/bin/node", "evil.example.com", 443),
            Err(AccessDenied::EndpointNotAllowed)
        );
    }

    #[test]
    fn effective_egress_unions_endpoints_across_entries_for_the_same_binary() {
        let mut policy = SandboxPolicy::new();
        policy.add_entry(npm_entry());
        policy.add_entry(
            NetworkPolicyEntry::new("pypi")
                .with_endpoint(Endpoint::new("pypi.org", 443))
                .with_binary("/usr/bin/node"),
        );
        let reachable = policy.effective_egress("/usr/bin/node");
        assert_eq!(reachable.len(), 2);
        assert!(reachable.iter().any(|e| e.host == "registry.npmjs.org"));
        assert!(reachable.iter().any(|e| e.host == "pypi.org"));
    }

    #[test]
    fn wildcard_host_matches_subdomains_and_the_apex() {
        let endpoint = Endpoint::new("*.example.com", 443);
        assert!(endpoint.matches_host("api.example.com"));
        assert!(endpoint.matches_host("example.com"));
        assert!(!endpoint.matches_host("example.org"));
    }

    #[test]
    fn the_three_tiers_resolve_distinct_preset_sets() {
        let restricted: BTreeSet<_> = tier_presets(PolicyTier::Restricted).iter().collect();
        let balanced: BTreeSet<_> = tier_presets(PolicyTier::Balanced).iter().collect();
        let open: BTreeSet<_> = tier_presets(PolicyTier::Open).iter().collect();

        assert!(restricted.is_empty(), "restricted is the baseline only");
        assert!(!balanced.is_empty() && balanced.len() < open.len());
        assert!(
            balanced.is_subset(&open),
            "open extends balanced, never replaces it"
        );
        assert_ne!(restricted, balanced);
        assert_ne!(balanced, open);
    }

    #[test]
    fn open_tier_uniquely_includes_messaging_platforms() {
        assert!(!tier_presets(PolicyTier::Balanced).contains(&"slack"));
        assert!(tier_presets(PolicyTier::Open).contains(&"slack"));
    }

    fn npm_preset(verification: PresetVerification) -> PolicyPreset {
        PolicyPreset {
            name: "npm-registry".into(),
            description: "Node package registry access".into(),
            allowed_host_categories: vec!["package-registry".into()],
            redacted_host_count: 1,
            source: PresetSource::Builtin,
            verification,
        }
    }

    #[test]
    fn os_signal_classifies_as_blocked_by_policy_with_high_confidence() {
        let result = classify_access_failure(
            Some(OsErrorSignal::ConnectionRefused),
            None,
            "registry.npmjs.org",
            &[],
        );
        assert_eq!(result.kind, AccessFailureKind::BlockedByPolicy);
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn http_403_with_a_matching_unapplied_preset_identifies_it() {
        let known_unapplied = [npm_preset(PresetVerification::RegistryOnly)];
        let result =
            classify_access_failure(None, Some(403), "registry.npmjs.org", &known_unapplied);
        assert_eq!(result.kind, AccessFailureKind::MissingApproval);
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.matched_preset.as_deref(), Some("npm-registry"));
    }

    #[test]
    fn known_unapplied_preset_matches_without_an_http_status() {
        let known_unapplied = [npm_preset(PresetVerification::RegistryOnly)];
        let result = classify_access_failure(None, None, "registry.npmjs.org", &known_unapplied);
        assert_eq!(result.kind, AccessFailureKind::MissingApproval);
        assert_eq!(result.matched_preset.as_deref(), Some("npm-registry"));
    }

    #[test]
    fn unmatched_failure_classifies_as_unknown_with_low_confidence() {
        let result = classify_access_failure(None, None, "totally-unknown.example", &[]);
        assert_eq!(result.kind, AccessFailureKind::Unknown);
        assert_eq!(result.confidence, Confidence::Low);
        assert!(result.matched_preset.is_none());
    }

    #[test]
    fn no_activation_registration_location_is_read_write_in_the_baseline() {
        // The deny-by-default baseline (no entries at all) must leave
        // every activation registration location without a write path.
        let baseline = FilesystemPolicy::new();
        for location in FILESYSTEM_REGISTRATION_LOCATIONS {
            assert!(
                !baseline.is_read_write(location),
                "{location} must not be write-accessible in the deny-by-default baseline"
            );
        }
    }

    #[test]
    fn a_registration_location_would_be_caught_if_misconfigured_as_read_write() {
        // Self-test (mirrors check-domain-boundary.mjs --self-test): prove
        // the mechanism itself would flag a violation, not just that today's
        // baseline happens to be clean.
        let misconfigured = FilesystemPolicy::new().with_read_write("~/.config/systemd/user");
        assert!(misconfigured.is_read_write("~/.config/systemd/user"));
        assert!(misconfigured.is_read_write("~/.config/systemd/user/cronus.service"));
        assert!(!misconfigured.is_read_write("~/.config/systemd/system"));
    }

    #[test]
    fn read_write_matches_the_mount_and_paths_under_it_only() {
        let policy = FilesystemPolicy::new().with_read_write("/home/user/workspace");
        assert!(policy.is_read_write("/home/user/workspace"));
        assert!(policy.is_read_write("/home/user/workspace/file.txt"));
        assert!(
            !policy.is_read_write("/home/user/workspace-other"),
            "a sibling path with the mount as a prefix must not match"
        );
    }

    #[test]
    fn policy_context_is_a_plain_read_only_snapshot() {
        let ctx = PolicyContext {
            sandbox_name: "session-42".into(),
            tier: PolicyTier::Balanced,
            active_presets: vec![npm_preset(PresetVerification::Verified)],
            known_unapplied_presets: vec![],
            approval_path: "cronus sandbox request-preset".into(),
            support_boundaries: vec![SupportBoundary {
                capability: "network-policy".into(),
                owner: BoundaryOwner::Host,
                note: None,
            }],
        };
        assert_eq!(ctx.tier, PolicyTier::Balanced);
        assert_eq!(ctx.active_presets.len(), 1);
    }
}
