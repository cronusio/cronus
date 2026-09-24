//! Multi-user authentication: bcrypt password storage, 7-day
//! session tokens, RFC 6238 TOTP two-factor authentication with single-use
//! backup codes, per-user privilege maps, and admin promotion/demotion with
//! privilege stashing. Reserved sentinel usernames are refused at creation so
//! a real account can never impersonate an internal service identity — most
//! critically `internal-tool`, which middleware elsewhere grants admin to
//! unconditionally.
//!
//! Password hashes and TOTP secrets never leave this module except through
//! the narrow verify/enroll operations; nothing here is ever logged.
//!
//! The on-device default for the auth and identity provider planes.
//! `PrivilegeMap` and `RESERVED_USERNAMES` have no consumer
//! outside `AuthStore` (nothing in the domain tier references them), so the
//! whole module travels together rather than being split across a
//! tier boundary with nothing on the other side.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

pub mod developer_admission;
pub use developer_admission::{DeveloperAdmissionStore, HumanPrincipal};

type HmacSha1 = Hmac<Sha1>;

#[cfg(not(test))]
const BCRYPT_COST: u32 = bcrypt::DEFAULT_COST;
/// Lower cost in tests: still exercises the real bcrypt algorithm, just with
/// fewer rounds, so the suite stays fast.
#[cfg(test)]
const BCRYPT_COST: u32 = 4;

/// Usernames that may never be created or renamed into. Any row with
/// a reserved name found on load is dropped fail-closed.
pub const RESERVED_USERNAMES: &[&str] = &["internal-tool", "api", "demo", "system"];

const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const TOTP_STEP_SECONDS: u64 = 30;
const TOTP_VALID_WINDOW: i64 = 1;
const TOTP_BACKUP_CODE_COUNT: usize = 8;

/// Per-user capability flags and limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeMap {
    pub can_use_agent: bool,
    pub can_use_bash: bool,
    pub can_use_documents: bool,
    pub can_use_research: bool,
    pub can_manage_memory: bool,
    /// `0` means unlimited.
    pub max_messages_per_day: u32,
    /// Empty means "no restriction" unless `block_all_models` is set.
    pub allowed_models: Vec<String>,
    pub allowed_models_restricted: bool,
    /// Explicit "no models" sentinel — distinct from an empty allowlist,
    /// which otherwise means unrestricted.
    pub block_all_models: bool,
}

impl Default for PrivilegeMap {
    /// `DEFAULT_PRIVILEGES` (safe defaults): shell access is off for a
    /// new user; the rest of normal chat/research use is on.
    fn default() -> Self {
        Self {
            can_use_agent: true,
            can_use_bash: false,
            can_use_documents: true,
            can_use_research: true,
            can_manage_memory: true,
            max_messages_per_day: 0,
            allowed_models: Vec::new(),
            allowed_models_restricted: false,
            block_all_models: false,
        }
    }
}

impl PrivilegeMap {
    /// `ADMIN_PRIVILEGES`: the full set every admin receives regardless of
    /// what is stored for them.
    pub fn admin() -> Self {
        Self {
            can_use_bash: true,
            ..Self::default()
        }
    }
}

/// One stored user.
#[derive(Debug, Clone)]
pub struct UserRecord {
    password_hash: String,
    pub created: f64,
    pub is_admin: bool,
    pub privileges: PrivilegeMap,
    totp_secret: Option<Vec<u8>>,
    totp_secret_pending: Option<Vec<u8>>,
    totp_backup_codes: Vec<String>,
    pub totp_enabled: bool,
    privileges_before_admin: Option<PrivilegeMap>,
}

/// Failures raised by [`AuthStore`] operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    ReservedUsername,
    UserExists,
    UserNotFound,
    WrongPassword,
    HashingFailed,
    /// Demotion refused — it would leave the office with zero admins.
    LastAdmin,
    NoPendingEnrollment,
    InvalidTotpCode,
}

/// In-memory user directory. Persistence to `auth.json` (atomic-write)
/// is a host-side concern layered on top of this store.
#[derive(Debug, Default)]
pub struct AuthStore {
    users: BTreeMap<String, UserRecord>,
}

impl AuthStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn exists(&self, username: &str) -> bool {
        self.users.contains_key(username)
    }

    /// Create a user with a bcrypt-hashed password. Reserved names and
    /// duplicates are rejected before any hashing work happens.
    pub fn create_user(
        &mut self,
        username: &str,
        password: &str,
        is_admin: bool,
        created: f64,
    ) -> Result<(), AuthError> {
        if RESERVED_USERNAMES.contains(&username) {
            return Err(AuthError::ReservedUsername);
        }
        if self.users.contains_key(username) {
            return Err(AuthError::UserExists);
        }
        let password_hash =
            bcrypt::hash(password, BCRYPT_COST).map_err(|_| AuthError::HashingFailed)?;
        let privileges = if is_admin {
            PrivilegeMap::admin()
        } else {
            PrivilegeMap::default()
        };
        self.users.insert(
            username.to_string(),
            UserRecord {
                password_hash,
                created,
                is_admin,
                privileges,
                totp_secret: None,
                totp_secret_pending: None,
                totp_backup_codes: Vec::new(),
                totp_enabled: false,
                privileges_before_admin: None,
            },
        );
        Ok(())
    }

    pub fn delete_user(&mut self, username: &str) -> Result<(), AuthError> {
        self.users
            .remove(username)
            .map(|_| ())
            .ok_or(AuthError::UserNotFound)
    }

    /// Bcrypt verify. The stored hash is never returned or compared as
    /// plaintext — only `bcrypt::verify`'s own comparison is used.
    pub fn verify_password(&self, username: &str, password: &str) -> Result<bool, AuthError> {
        let user = self.users.get(username).ok_or(AuthError::UserNotFound)?;
        Ok(bcrypt::verify(password, &user.password_hash).unwrap_or(false))
    }

    pub fn change_password(
        &mut self,
        username: &str,
        current: &str,
        new_password: &str,
    ) -> Result<(), AuthError> {
        if !self.verify_password(username, current)? {
            return Err(AuthError::WrongPassword);
        }
        let hash = bcrypt::hash(new_password, BCRYPT_COST).map_err(|_| AuthError::HashingFailed)?;
        let user = self
            .users
            .get_mut(username)
            .ok_or(AuthError::UserNotFound)?;
        user.password_hash = hash;
        Ok(())
    }

    /// Admins always receive the full privilege set regardless of the stored
    /// map — this short-circuits before returning it.
    pub fn get_privileges(&self, username: &str) -> Result<PrivilegeMap, AuthError> {
        let user = self.users.get(username).ok_or(AuthError::UserNotFound)?;
        Ok(if user.is_admin {
            PrivilegeMap::admin()
        } else {
            user.privileges.clone()
        })
    }

    pub fn set_privileges(
        &mut self,
        username: &str,
        privileges: PrivilegeMap,
    ) -> Result<(), AuthError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or(AuthError::UserNotFound)?;
        user.privileges = privileges;
        Ok(())
    }

    fn admin_count(&self) -> usize {
        self.users.values().filter(|user| user.is_admin).count()
    }

    /// Promote/demote with privilege stashing. Demotion is refused if
    /// it would remove the last remaining admin; the count check and the
    /// flip happen against the same `&mut self` borrow, so no concurrent
    /// caller can observe or race an intermediate state.
    pub fn set_admin(&mut self, username: &str, make_admin: bool) -> Result<(), AuthError> {
        let target_is_last_admin = !make_admin
            && self.admin_count() <= 1
            && self.users.get(username).is_some_and(|user| user.is_admin);
        if target_is_last_admin {
            return Err(AuthError::LastAdmin);
        }
        let user = self
            .users
            .get_mut(username)
            .ok_or(AuthError::UserNotFound)?;
        if make_admin && !user.is_admin {
            user.privileges_before_admin = Some(user.privileges.clone());
            user.privileges = PrivilegeMap::admin();
            user.is_admin = true;
        } else if !make_admin && user.is_admin {
            user.privileges = user.privileges_before_admin.take().unwrap_or_default();
            user.is_admin = false;
        }
        Ok(())
    }

    // --- TOTP two-factor authentication ---

    /// Begin enrollment: generate a pending secret and return it Base32
    /// encoded (for the `otpauth://` QR URI). Not active until confirmed.
    pub fn totp_enroll_begin(&mut self, username: &str) -> Result<String, AuthError> {
        if !self.users.contains_key(username) {
            return Err(AuthError::UserNotFound);
        }
        let secret = random_bytes::<20>();
        let encoded = base32::encode(&secret);
        let user = self
            .users
            .get_mut(username)
            .ok_or(AuthError::UserNotFound)?;
        user.totp_secret_pending = Some(secret.to_vec());
        Ok(encoded)
    }

    /// Confirm enrollment with a code from the pending secret; on success,
    /// activates TOTP and returns 8 single-use backup codes.
    pub fn totp_confirm_enable(
        &mut self,
        username: &str,
        code: &str,
        now_unix: u64,
    ) -> Result<Vec<String>, AuthError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or(AuthError::UserNotFound)?;
        let pending = user
            .totp_secret_pending
            .as_ref()
            .ok_or(AuthError::NoPendingEnrollment)?;
        if !totp_verify_at(pending, now_unix, code, TOTP_VALID_WINDOW) {
            return Err(AuthError::InvalidTotpCode);
        }
        user.totp_secret = user.totp_secret_pending.take();
        user.totp_enabled = true;
        let codes: Vec<String> = (0..TOTP_BACKUP_CODE_COUNT)
            .map(|_| hex_encode(&random_bytes::<4>()))
            .collect();
        user.totp_backup_codes = codes.clone();
        Ok(codes)
    }

    pub fn totp_disable(&mut self, username: &str) -> Result<(), AuthError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or(AuthError::UserNotFound)?;
        user.totp_secret = None;
        user.totp_secret_pending = None;
        user.totp_backup_codes.clear();
        user.totp_enabled = false;
        Ok(())
    }

    /// Check a code against the active TOTP secret, then against the backup
    /// codes (consuming one on match). Callers should only invoke this when
    /// `totp_enabled` is true (the login flow decides whether 2FA applies);
    /// on a disabled or corrupt-secret account there is nothing to match
    /// against, so this fails closed to `false` rather than special-casing.
    pub fn totp_verify(
        &mut self,
        username: &str,
        code: &str,
        now_unix: u64,
    ) -> Result<bool, AuthError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or(AuthError::UserNotFound)?;
        if let Some(secret) = &user.totp_secret
            && totp_verify_at(secret, now_unix, code, TOTP_VALID_WINDOW)
        {
            return Ok(true);
        }
        if let Some(pos) = user.totp_backup_codes.iter().position(|c| c == code) {
            user.totp_backup_codes.remove(pos);
            return Ok(true);
        }
        Ok(false)
    }

    pub fn totp_enabled(&self, username: &str) -> Result<bool, AuthError> {
        Ok(self
            .users
            .get(username)
            .ok_or(AuthError::UserNotFound)?
            .totp_enabled)
    }
}

/// Compose the login flow: verify the password, then verify
/// TOTP only if the account has it enabled, then hand back a fresh session.
pub fn login(
    auth: &mut AuthStore,
    sessions: &mut SessionStore,
    username: &str,
    password: &str,
    totp_code: Option<&str>,
    now_unix: u64,
) -> Result<String, AuthError> {
    if !auth.verify_password(username, password)? {
        return Err(AuthError::WrongPassword);
    }
    if auth.totp_enabled(username)? {
        let code = totp_code.ok_or(AuthError::InvalidTotpCode)?;
        if !auth.totp_verify(username, code, now_unix)? {
            return Err(AuthError::InvalidTotpCode);
        }
    }
    Ok(sessions.issue(username))
}

/// One issued session token.
#[derive(Debug, Clone)]
struct SessionRecord {
    username: String,
    issued_at: Instant,
}

/// Active session tokens. `validate` re-checks the token's owner still
/// exists in the auth store — a deleted user's sessions die immediately,
/// not at cookie expiry (the "orphan guard").
#[derive(Debug, Default)]
pub struct SessionStore {
    tokens: BTreeMap<String, SessionRecord>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue a fresh random 32-byte-hex token for `username`.
    pub fn issue(&mut self, username: &str) -> String {
        self.issue_at(username, Instant::now())
    }

    pub fn issue_at(&mut self, username: &str, now: Instant) -> String {
        let token = hex_encode(&random_bytes::<32>());
        self.tokens.insert(
            token.clone(),
            SessionRecord {
                username: username.to_string(),
                issued_at: now,
            },
        );
        token
    }

    pub fn validate(&self, token: &str, auth: &AuthStore) -> bool {
        self.validate_at(token, Instant::now(), auth)
    }

    pub fn validate_at(&self, token: &str, now: Instant, auth: &AuthStore) -> bool {
        match self.tokens.get(token) {
            Some(record) => {
                now.duration_since(record.issued_at) < SESSION_TTL && auth.exists(&record.username)
            }
            None => false,
        }
    }

    /// Revoke every session belonging to `username`; returns the count revoked.
    pub fn revoke_user_sessions(&mut self, username: &str) -> u32 {
        let before = self.tokens.len();
        self.tokens.retain(|_, record| record.username != username);
        (before - self.tokens.len()) as u32
    }

    pub fn active_count(&self) -> usize {
        self.tokens.len()
    }
}

// ── AuthProvider / IdentityProvider (ports tier) ─────────────────────────────
//
// `AuthStore` is the concrete, bcrypt-backed implementation of the auth
// plane. Unlike the SQLite-backed memory store, it is pure in-memory
// data (`BTreeMap`) with no non-`Sync` interior — the `Send + Sync` bound on
// `AuthProvider` holds here without needing to be relaxed.

impl cronus_contract::AuthProvider for AuthStore {
    fn authenticate(&self, principal: &str, credential: &str) -> std::result::Result<bool, String> {
        self.verify_password(principal, credential)
            .map_err(|e| format!("{e:?}"))
    }
}

/// The identity-plane on-device default: a single local
/// principal, no accounts, no multi-tenancy. The principal name is supplied
/// by the caller (e.g. an OS-username lookup elsewhere) — this type holds
/// it, it does not discover it, so it has no I/O of its own.
pub struct SingleUserIdentity {
    principal: Option<String>,
}

impl SingleUserIdentity {
    pub fn new(principal: impl Into<String>) -> Self {
        SingleUserIdentity {
            principal: Some(principal.into()),
        }
    }

    pub fn anonymous() -> Self {
        SingleUserIdentity { principal: None }
    }
}

impl cronus_contract::IdentityProvider for SingleUserIdentity {
    fn current_principal(&self) -> Option<String> {
        self.principal.clone()
    }
}

// --- RFC 6238 TOTP over HMAC-SHA1 ---

/// HOTP (RFC 4226) at a given counter value. Returns `None` only if the HMAC
/// key construction fails, which fails the caller closed (no code matches).
fn hotp(secret: &[u8], counter: u64) -> Option<u32> {
    let mut mac = <HmacSha1 as KeyInit>::new_from_slice(secret).ok()?;
    Mac::update(&mut mac, &counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = ((u32::from(digest[offset]) & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Some(binary % 1_000_000)
}

fn totp_at_step(secret: &[u8], step: u64) -> Option<String> {
    hotp(secret, step).map(|code| format!("{code:06}"))
}

/// Verify a 6-digit code within `valid_window` steps of `now_unix` (default
/// window is 1.5 enrollment confirmation) to tolerate clock drift.
fn totp_verify_at(secret: &[u8], now_unix: u64, code: &str, valid_window: i64) -> bool {
    let counter = (now_unix / TOTP_STEP_SECONDS) as i64;
    for delta in -valid_window..=valid_window {
        let step = counter + delta;
        if step < 0 {
            continue;
        }
        if totp_at_step(secret, step as u64).as_deref() == Some(code) {
            return true;
        }
    }
    false
}

/// RFC 4648 Base32 (no padding) — used only to encode/decode TOTP secrets
/// for display; not a cryptographic primitive.
mod base32 {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    pub fn encode(data: &[u8]) -> String {
        let mut output = String::new();
        let mut buffer: u32 = 0;
        let mut bits_in_buffer: u32 = 0;
        for &byte in data {
            buffer = (buffer << 8) | u32::from(byte);
            bits_in_buffer += 8;
            while bits_in_buffer >= 5 {
                bits_in_buffer -= 5;
                let index = (buffer >> bits_in_buffer) & 0x1f;
                output.push(ALPHABET[index as usize] as char);
            }
        }
        if bits_in_buffer > 0 {
            let index = (buffer << (5 - bits_in_buffer)) & 0x1f;
            output.push(ALPHABET[index as usize] as char);
        }
        output
    }

    #[cfg(test)]
    pub fn decode(input: &str) -> Option<Vec<u8>> {
        let mut buffer: u32 = 0;
        let mut bits_in_buffer: u32 = 0;
        let mut output = Vec::new();
        for ch in input.chars() {
            let ch = ch.to_ascii_uppercase();
            let value = ALPHABET.iter().position(|&c| c as char == ch)? as u32;
            buffer = (buffer << 5) | value;
            bits_in_buffer += 5;
            if bits_in_buffer >= 8 {
                bits_in_buffer -= 8;
                output.push(((buffer >> bits_in_buffer) & 0xff) as u8);
            }
        }
        Some(output)
    }
}

/// `N` random bytes from the OS CSPRNG, for session tokens, TOTP secrets,
/// and backup codes. A failure here means the OS randomness source itself is
/// broken — a truly unrecoverable invariant (per the project's panic policy):
/// silently substituting weak or all-zero bytes for a security token would
/// be strictly worse than stopping, so this is the one place in the module
/// that panics rather than propagating a `Result` no caller could act on.
fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("OS CSPRNG unavailable — cannot mint secrets safely");
    bytes
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trips_through_bcrypt_and_stores_no_plaintext() {
        let mut store = AuthStore::new();
        store.create_user("alice", "hunter2", false, 0.0).unwrap();
        assert!(store.verify_password("alice", "hunter2").unwrap());
        assert!(!store.verify_password("alice", "wrong").unwrap());
        let hash = &store.users.get("alice").unwrap().password_hash;
        assert_ne!(
            hash, "hunter2",
            "the stored value is a hash, not the password"
        );
        assert!(hash.starts_with("$2"), "bcrypt hashes carry the $2 prefix");
    }

    #[test]
    fn change_password_requires_the_current_password() {
        let mut store = AuthStore::new();
        store.create_user("alice", "old-pass", false, 0.0).unwrap();
        assert_eq!(
            store.change_password("alice", "wrong", "new-pass"),
            Err(AuthError::WrongPassword)
        );
        store
            .change_password("alice", "old-pass", "new-pass")
            .unwrap();
        assert!(store.verify_password("alice", "new-pass").unwrap());
        assert!(!store.verify_password("alice", "old-pass").unwrap());
    }

    #[test]
    fn reserved_sentinel_usernames_are_rejected_at_creation() {
        let mut store = AuthStore::new();
        for name in RESERVED_USERNAMES {
            assert_eq!(
                store.create_user(name, "whatever", false, 0.0),
                Err(AuthError::ReservedUsername)
            );
        }
        assert!(!store.exists("internal-tool"));
    }

    #[test]
    fn duplicate_username_is_rejected() {
        let mut store = AuthStore::new();
        store.create_user("alice", "p1", false, 0.0).unwrap();
        assert_eq!(
            store.create_user("alice", "p2", false, 0.0),
            Err(AuthError::UserExists)
        );
    }

    #[test]
    fn session_issues_validates_and_expires() {
        let mut auth = AuthStore::new();
        auth.create_user("alice", "hunter2", false, 0.0).unwrap();
        let mut sessions = SessionStore::new();

        let t0 = Instant::now();
        let token = sessions.issue_at("alice", t0);
        assert!(
            sessions.validate_at(&token, t0, &auth),
            "fresh token validates"
        );

        let just_before_expiry = t0 + SESSION_TTL - Duration::from_secs(1);
        assert!(sessions.validate_at(&token, just_before_expiry, &auth));

        let just_after_expiry = t0 + SESSION_TTL + Duration::from_secs(1);
        assert!(
            !sessions.validate_at(&token, just_after_expiry, &auth),
            "expired token no longer validates"
        );

        assert!(!sessions.validate_at("not-a-real-token", t0, &auth));
    }

    #[test]
    fn deleting_a_user_orphans_and_invalidates_their_sessions_immediately() {
        let mut auth = AuthStore::new();
        auth.create_user("alice", "hunter2", false, 0.0).unwrap();
        let mut sessions = SessionStore::new();
        let token = sessions.issue("alice");
        assert!(sessions.validate(&token, &auth));

        auth.delete_user("alice").unwrap();
        assert!(
            !sessions.validate(&token, &auth),
            "orphan guard: deleted user's session dies immediately, not at TTL"
        );
    }

    #[test]
    fn revoke_user_sessions_removes_only_that_users_tokens() {
        let mut sessions = SessionStore::new();
        sessions.issue("alice");
        sessions.issue("alice");
        sessions.issue("bob");
        assert_eq!(sessions.revoke_user_sessions("alice"), 2);
        assert_eq!(sessions.active_count(), 1);
    }

    #[test]
    fn totp_enrollment_confirms_with_a_valid_code_and_issues_backup_codes() {
        let mut store = AuthStore::new();
        store.create_user("alice", "hunter2", false, 0.0).unwrap();
        let encoded_secret = store.totp_enroll_begin("alice").unwrap();
        let secret = base32::decode(&encoded_secret).unwrap();

        let now: u64 = 1_800_000_000;
        let code = totp_at_step(&secret, now / TOTP_STEP_SECONDS).unwrap();

        let backup_codes = store.totp_confirm_enable("alice", &code, now).unwrap();
        assert_eq!(backup_codes.len(), TOTP_BACKUP_CODE_COUNT);
        assert!(store.totp_enabled("alice").unwrap());
    }

    #[test]
    fn totp_enrollment_rejects_a_wrong_code() {
        let mut store = AuthStore::new();
        store.create_user("alice", "hunter2", false, 0.0).unwrap();
        store.totp_enroll_begin("alice").unwrap();
        assert_eq!(
            store.totp_confirm_enable("alice", "000000", 1_800_000_000),
            Err(AuthError::InvalidTotpCode)
        );
        assert!(!store.totp_enabled("alice").unwrap());
    }

    #[test]
    fn totp_verify_accepts_the_current_code_and_rejects_a_stale_one() {
        let mut store = AuthStore::new();
        store.create_user("alice", "hunter2", false, 0.0).unwrap();
        let encoded_secret = store.totp_enroll_begin("alice").unwrap();
        let secret = base32::decode(&encoded_secret).unwrap();
        let now: u64 = 1_800_000_000;
        let code = totp_at_step(&secret, now / TOTP_STEP_SECONDS).unwrap();
        store.totp_confirm_enable("alice", &code, now).unwrap();

        assert!(store.totp_verify("alice", &code, now).unwrap());

        let far_future = now + 10 * TOTP_STEP_SECONDS;
        let stale_code = code;
        assert!(!store.totp_verify("alice", &stale_code, far_future).unwrap());
    }

    #[test]
    fn totp_backup_code_is_single_use() {
        let mut store = AuthStore::new();
        store.create_user("alice", "hunter2", false, 0.0).unwrap();
        let encoded_secret = store.totp_enroll_begin("alice").unwrap();
        let secret = base32::decode(&encoded_secret).unwrap();
        let now: u64 = 1_800_000_000;
        let code = totp_at_step(&secret, now / TOTP_STEP_SECONDS).unwrap();
        let backup_codes = store.totp_confirm_enable("alice", &code, now).unwrap();
        let backup = backup_codes[0].clone();

        assert!(store.totp_verify("alice", &backup, now).unwrap());
        assert!(
            !store.totp_verify("alice", &backup, now).unwrap(),
            "a consumed backup code cannot be reused"
        );
    }

    #[test]
    fn totp_fails_closed_when_enabled_but_secret_is_corrupt() {
        let mut store = AuthStore::new();
        store.create_user("alice", "hunter2", false, 0.0).unwrap();
        // Simulate a corrupt record: enabled with no secret and no codes.
        store.users.get_mut("alice").unwrap().totp_enabled = true;
        assert!(!store.totp_verify("alice", "123456", 1_800_000_000).unwrap());
    }

    #[test]
    fn login_composes_password_and_totp_when_enabled() {
        let mut auth = AuthStore::new();
        let mut sessions = SessionStore::new();
        auth.create_user("alice", "hunter2", false, 0.0).unwrap();
        let encoded_secret = auth.totp_enroll_begin("alice").unwrap();
        let secret = base32::decode(&encoded_secret).unwrap();
        let now: u64 = 1_800_000_000;
        let code = totp_at_step(&secret, now / TOTP_STEP_SECONDS).unwrap();
        auth.totp_confirm_enable("alice", &code, now).unwrap();

        assert_eq!(
            login(&mut auth, &mut sessions, "alice", "hunter2", None, now),
            Err(AuthError::InvalidTotpCode),
            "TOTP is required once enabled"
        );

        let fresh_code = totp_at_step(&secret, now / TOTP_STEP_SECONDS).unwrap();
        let token = login(
            &mut auth,
            &mut sessions,
            "alice",
            "hunter2",
            Some(&fresh_code),
            now,
        )
        .unwrap();
        assert!(sessions.validate(&token, &auth));
    }

    #[test]
    fn login_skips_totp_when_not_enabled() {
        let mut auth = AuthStore::new();
        let mut sessions = SessionStore::new();
        auth.create_user("alice", "hunter2", false, 0.0).unwrap();
        let token = login(&mut auth, &mut sessions, "alice", "hunter2", None, 0).unwrap();
        assert!(sessions.validate(&token, &auth));
    }

    #[test]
    fn admin_promote_stashes_privileges_and_demote_restores_them() {
        let mut store = AuthStore::new();
        store.create_user("alice", "p1", false, 0.0).unwrap();
        store.create_user("root", "p2", true, 0.0).unwrap();

        let custom = PrivilegeMap {
            max_messages_per_day: 42,
            ..PrivilegeMap::default()
        };
        store.set_privileges("alice", custom.clone()).unwrap();

        store.set_admin("alice", true).unwrap();
        assert_eq!(
            store.get_privileges("alice").unwrap(),
            PrivilegeMap::admin()
        );

        store.set_admin("alice", false).unwrap();
        assert_eq!(
            store.get_privileges("alice").unwrap(),
            custom,
            "stashed privileges restored"
        );
    }

    #[test]
    fn demoting_the_last_admin_is_refused() {
        let mut store = AuthStore::new();
        store.create_user("root", "p", true, 0.0).unwrap();
        assert_eq!(store.set_admin("root", false), Err(AuthError::LastAdmin));
    }

    #[test]
    fn demoting_one_of_two_admins_succeeds() {
        let mut store = AuthStore::new();
        store.create_user("root1", "p", true, 0.0).unwrap();
        store.create_user("root2", "p", true, 0.0).unwrap();
        store.set_admin("root1", false).unwrap();
        assert!(!store.get_privileges("root1").unwrap().can_use_bash);
        assert_eq!(
            store.get_privileges("root2").unwrap(),
            PrivilegeMap::admin(),
            "the sibling admin is unaffected by root1's demotion"
        );
    }

    #[test]
    fn base32_round_trips() {
        let bytes = random_bytes::<20>();
        let encoded = base32::encode(&bytes);
        assert_eq!(base32::decode(&encoded).unwrap(), bytes.to_vec());
    }

    #[test]
    fn auth_provider_authenticate_matches_verify_password() {
        use cronus_contract::AuthProvider;
        let mut store = AuthStore::new();
        store.create_user("alice", "hunter2", false, 0.0).unwrap();

        let provider: &dyn AuthProvider = &store;
        assert!(provider.authenticate("alice", "hunter2").unwrap());
        assert!(!provider.authenticate("alice", "wrong").unwrap());
    }

    #[test]
    fn single_user_identity_reports_the_configured_principal() {
        use cronus_contract::IdentityProvider;
        let id = SingleUserIdentity::new("alice");
        assert_eq!(id.current_principal(), Some("alice".to_string()));

        let anon = SingleUserIdentity::anonymous();
        assert_eq!(anon.current_principal(), None);
    }
}
