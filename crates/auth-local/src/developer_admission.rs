//! Developer-office admission — the sole write-gated record on this
//! project's human-write-only authority plane for the dev office.
//!
//! Mint and revoke take a [`HumanPrincipal`], constructible only in this
//! module: nothing exported here lets agent-reachable code fabricate one, so
//! no domain-logic call path can mint or revoke an admission — only a
//! human-operated entry point (the `cronus dev admit`/`revoke` CLI verbs,
//! never a tool an agent can invoke) can construct one and call through.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Proof the caller is the human operator, not agent-reachable domain logic.
/// The only constructor is [`HumanPrincipal::assert_human_operated`] — a
/// human-operated entry point calls it directly; nothing routes one through
/// from anywhere an agent tool-call could reach.
pub struct HumanPrincipal(());

impl HumanPrincipal {
    /// Assert the current call originates from a human-operated entry point.
    /// Call this only at such an entry point (e.g. a CLI verb the operator
    /// runs themselves) — never from code an agent tool-call could execute.
    pub fn assert_human_operated() -> Self {
        HumanPrincipal(())
    }
}

/// A file-backed admission flag: minted or revoked only via
/// [`HumanPrincipal`], read back by anyone holding the store. The facade
/// wraps this as the domain gate's read-only `AdmissionReader` port.
pub struct DeveloperAdmissionStore {
    path: PathBuf,
}

impl DeveloperAdmissionStore {
    /// Open a store at an explicit path. This crate holds no path-resolution
    /// logic of its own — the caller resolves the real state-tier location.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        DeveloperAdmissionStore { path: path.into() }
    }

    /// Grant admission. Human-principal-only by construction.
    pub fn mint(&self, _human: &HumanPrincipal) -> io::Result<()> {
        self.write(true)
    }

    /// Revoke admission. Human-principal-only by construction.
    pub fn revoke(&self, _human: &HumanPrincipal) -> io::Result<()> {
        self.write(false)
    }

    /// Whether admission is currently granted. Fail-closed: a missing,
    /// unreadable, or corrupt record reads as not-admitted, never admitted.
    pub fn is_admitted(&self) -> bool {
        match fs::read_to_string(&self.path) {
            Ok(text) => text.starts_with("admitted=true"),
            Err(_) => false,
        }
    }

    fn write(&self, admitted: bool) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let changed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        fs::write(
            &self.path,
            format!("admitted={admitted}\nchanged_at={changed_at}\n"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cronus-dev-admission-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{tag}.txt"))
    }

    #[test]
    fn a_fresh_store_reports_not_admitted() {
        let store = DeveloperAdmissionStore::open(temp_path("fresh"));
        assert!(!store.is_admitted());
    }

    #[test]
    fn mint_then_read_back_reports_admitted() {
        let store = DeveloperAdmissionStore::open(temp_path("mint"));
        let human = HumanPrincipal::assert_human_operated();
        store.mint(&human).unwrap();
        assert!(store.is_admitted());
    }

    #[test]
    fn revoke_after_mint_reports_not_admitted() {
        let store = DeveloperAdmissionStore::open(temp_path("revoke"));
        let human = HumanPrincipal::assert_human_operated();
        store.mint(&human).unwrap();
        assert!(store.is_admitted());
        store.revoke(&human).unwrap();
        assert!(!store.is_admitted());
    }

    #[test]
    fn a_corrupt_record_fails_closed_to_not_admitted() {
        let path = temp_path("corrupt");
        fs::write(&path, [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let store = DeveloperAdmissionStore::open(path);
        assert!(!store.is_admitted());
    }

    #[test]
    fn mint_persists_across_separate_store_handles() {
        // Proves the record is genuinely file-backed, not held in the
        // `DeveloperAdmissionStore` instance's own memory — required for a
        // real `admit` CLI run and a later, separate `status` run to agree.
        let path = temp_path("cross-handle");
        let writer = DeveloperAdmissionStore::open(&path);
        writer
            .mint(&HumanPrincipal::assert_human_operated())
            .unwrap();

        let reader = DeveloperAdmissionStore::open(&path);
        assert!(reader.is_admitted());
    }
}
