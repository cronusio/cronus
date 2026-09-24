//! Outbound egress gate: default-deny. A target may leave the device
//! only if explicitly allowed; denied attempts are recorded for audit.

use std::collections::HashSet;

/// Returned when an egress attempt is not on the allowlist.
#[derive(Debug, PartialEq, Eq)]
pub struct Denied;

/// A default-deny gate for outbound sends (telemetry, model cloud calls, issue
/// reporting all pass through this).
#[derive(Debug, Default)]
pub struct EgressGate {
    allowed: HashSet<String>,
    denied_audit: Vec<String>,
}

impl EgressGate {
    /// A fresh gate that denies everything.
    pub fn new() -> Self {
        Self::default()
    }

    /// Authorize a target (e.g. a host or named channel).
    pub fn allow(&mut self, target: impl Into<String>) {
        self.allowed.insert(target.into());
    }

    /// Check an outbound target. Unauthorized targets are denied and audited.
    pub fn check(&mut self, target: &str) -> Result<(), Denied> {
        if self.allowed.contains(target) {
            Ok(())
        } else {
            self.denied_audit.push(target.to_string());
            Err(Denied)
        }
    }

    /// The recorded denied attempts (audit trail).
    pub fn audit_log(&self) -> &[String] {
        &self.denied_audit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_by_default_and_audits() {
        let mut gate = EgressGate::new();
        assert_eq!(gate.check("api.example.com"), Err(Denied));
        assert_eq!(gate.audit_log(), ["api.example.com"]);
    }

    #[test]
    fn allows_authorized_target() {
        let mut gate = EgressGate::new();
        gate.allow("api.anthropic.com");
        assert_eq!(gate.check("api.anthropic.com"), Ok(()));
        assert!(
            gate.audit_log().is_empty(),
            "authorized sends are not audited as denials"
        );
    }
}
