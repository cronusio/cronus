# Multi-User Authentication

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-security.md

## Overview

Concrete multi-user authentication: bcrypt password storage, 7-day session tokens persisted atomically, TOTP two-factor authentication with single-use backup codes, per-user privilege maps, and admin promotion/demotion with privilege stashing. Reserved sentinel usernames prevent impersonation of internal service identities. All config writes are atomic (fsync + rename) to survive crash or kill during a write.

## Related Specifications

- [l1-security.md](l1-security.md) - SEC-1/SEC-2 invariants this implements.
- [l2-security.md](l2-security.md) - Secret storage, session files in `<state>/.env` / data tier.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - `<state>/auth.json` and `<state>/sessions.json` locations.
- [l2-role-catalog.md](l2-role-catalog.md) - Agents have roles; users have privileges — these are separate concepts.
- [l2-budget-engine.md](l2-budget-engine.md) - `max_messages_per_day` in privilege map is enforced by the budget engine.
- [l1-capability-reachability.md](l1-capability-reachability.md) - REA-1: user administration is a human-only surface the agent cannot reach.

## 1. Motivation

A desktop and mobile application serving multiple household members or team collaborators needs isolated user contexts. Admins need shell and file access; regular users need only chat and research. A single-user mode (auth disabled) preserves zero-friction for solo operators. Reserved sentinel usernames prevent an attacker from creating a real account that the middleware treats as a privileged internal service.

## 2. Constraints & Assumptions

- Passwords stored as bcrypt hashes; never logged or included in exports.
- Session tokens are random 32-byte hex strings with a 7-day TTL, persisted atomically to `sessions.json`.
- TOTP uses RFC 6238; 8 single-use backup codes are generated at enrollment.
- The sentinel username `internal-tool` is the most dangerous reserved name: it is the identity of the internal loopback channel (`l2-security` §4.5), and a human account carrying it could pass as that channel.
- All mutations to `auth.json` and `sessions.json` use the atomic-write protocol (see §4.7).
- A deleted user's sessions are revoked immediately (not at cookie expiry).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| SEC-1 Secret isolation | Password hashes and TOTP secrets in `auth.json` (mode 0600); never logged. |
| SEC-2 Safe defaults | `DEFAULT_PRIVILEGES` disables shell, file access, MCP, email, calendar for new users. |
| SEC-7 Auditable | Every user create/delete/rename/promote/demote action is logged with requester. |
| SEC-10 Authority self-containment | User administration — create, delete, set-priv, set-admin, revoke-sessions — writes the authority plane and is a human-only surface (`l1-capability-reachability` REA-1): no model-produced call reaches these verbs, and the `internal-tool` channel never acts above the session that issued its call (§4.3). |

## 4. Detailed Design

### 4.1 User record

```text
[REFERENCE]
UserRecord {
  password_hash: String,          // bcrypt
  created: f64,                   // UNIX timestamp
  is_admin: bool,
  privileges: PrivilegeMap,
  totp_secret?: String,           // active TOTP secret (Base32)
  totp_secret_pending?: String,   // generated but not yet confirmed
  totp_backup_codes?: String[],   // 8 single-use hex codes
  totp_enabled: bool,
  privileges_before_admin?: PrivilegeMap  // stash for promote/demote
}
```

### 4.2 Privilege map

```text
[REFERENCE]
PrivilegeMap {
  can_use_agent: bool,            // default true
  can_use_bash: bool,             // default false (admin only)
  can_use_documents: bool,        // default true
  can_use_research: bool,         // default true
  can_manage_memory: bool,        // default true
  max_messages_per_day: u32,      // default 0 = unlimited
  allowed_models: String[],       // default [] = no restriction
  allowed_models_restricted: bool,// default false
  block_all_models: bool          // explicit "no models" sentinel (distinct from [])
}
```

Admins always receive `ADMIN_PRIVILEGES` regardless of the stored map; `get_privileges(admin)` short-circuits to the full set. `block_all_models` is a separate flag because an empty `allowed_models` is ambiguous (it also means "all models allowed" in the unconstrained default).

### 4.3 Reserved sentinel usernames

The following usernames may never be created or renamed into:

```text
[REFERENCE]
RESERVED_USERNAMES = { "internal-tool", "api", "demo", "system" }
```

`internal-tool` is the most critical: it names the internal loopback channel, and a real account with this name could present itself as that channel. The channel authenticates *where a call came from*, not *who may do what*: a call arriving through it carries the privileges of the session that issued it and never more — it never satisfies `require_admin` on its own, and it can never write the authority plane (`l1-security` SEC-10; `l2-security` §4.5). A design that granted admin to the channel name would hand every model-issued internal call administrator rights. The other three names collide with synthetic owner sentinels used in task scheduling, research routing, and bearer-token attribution.

Any `auth.json` rows with reserved names are dropped at startup (fail-closed); each drop is logged with the row's name.

### 4.4 Session lifecycle

```text
[REFERENCE]
SessionToken {
  username: String,
  expiry: f64    // UNIX timestamp; TOKEN_TTL = 7 days
}
```

- **Issue:** password verified → TOTP verified (if enabled) → `secrets.token_hex(32)` returned to the client; `sessions.json` stores only `SHA-256(token)` with the record, never the token itself, so a copy of the file yields no usable session.
- **Validate:** hash the presented token and look it up; check expiry; re-check that `username` still exists in `auth.json` (orphan guard — deleted users are kicked immediately).
- **Revoke on delete:** when a user is deleted, all their session tokens are purged before the auth row is removed.
- **Revoke on scope:** `revoke_user_sessions(username, except_token?)` allows a "sign out all other devices" flow.

### 4.5 TOTP two-factor authentication

Enrollment flow:

1. Generate random Base32 secret → store as `totp_secret_pending`.
2. Display `otpauth://` QR URI to user.
3. User submits a code; verify against `totp_secret_pending` with `valid_window=1`.
4. On success: move pending to `totp_secret`, set `totp_enabled = true`, generate 8 backup codes (`secrets.token_hex(4)` each).

Backup codes are single-use: consuming one removes it from the stored list.

**Fail-closed rule:** if `totp_enabled = true` but no `totp_secret` is found (corrupt record), TOTP verification returns `false` — the second factor is not bypassed.

### 4.6 Promote/demote with privilege stashing

Promoting a user to admin:

1. Stash current `privileges` into `privileges_before_admin`.
2. Set `is_admin = true` and `privileges = ADMIN_PRIVILEGES`.

Demoting:

1. Restore `privileges` from `privileges_before_admin` (fallback: `DEFAULT_PRIVILEGES`).
2. Set `is_admin = false`.

**Last-admin guard:** demotion is refused if it would remove the only remaining admin. Counting and flipping happen in one critical section so concurrent demotions cannot race the count to zero.

### 4.7 Atomic file writes

All `auth.json` and `sessions.json` mutations use the atomic-write protocol:

```text
[REFERENCE]
atomic_write_json(path, data):
  tmp = "{path}.tmp.{pid}"
  write JSON to tmp
  fsync(tmp)
  os.replace(tmp, path)   // atomic on POSIX, best-effort on Windows
```

The PID suffix prevents two concurrent processes from colliding on the rename target. Plain `write-then-rename` without fsync is not safe: a kill/OOM between truncate and fill produces an empty or partial config file.

### 4.8 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| create user | `cronus user create <name> [--admin]` | `/user create …` | `auth.createUser(name, password, isAdmin) -> User` |
| delete user | `cronus user delete <name>` | `/user delete <name>` | `auth.deleteUser(name, requestingUser) -> bool` |
| list users | `cronus user list` | `/user list` | `auth.listUsers() -> User[]` |
| show privileges | `cronus user show <name>` | `/user show <name>` | `auth.getPrivileges(name) -> PrivilegeMap` |
| set privileges | `cronus user set-priv <name> --key <k> --value <v>` | `/user set-priv …` | `auth.setPrivileges(name, patch) -> bool` |
| promote/demote | `cronus user set-admin <name> [--admin\|--demote]` | `/user set-admin …` | `auth.setAdmin(name, isAdmin, requester) -> SetAdminResult` |
| change password | `cronus user passwd <name>` | `/user passwd <name>` | `auth.changePassword(name, current, new) -> bool` |
| enable 2FA | `cronus user 2fa enable` | `/user 2fa enable` | `auth.totpConfirmEnable(name, code) -> bool` |
| disable 2FA | `cronus user 2fa disable` | `/user 2fa disable` | `auth.totpDisable(name, password) -> bool` |
| revoke sessions | `cronus user revoke-sessions <name>` | `/user revoke-sessions …` | `auth.revokeUserSessions(name) -> u32` |

## 5. Drawbacks & Alternatives

- **Flat privilege model:** there is no RBAC with custom roles; privileges are per-user booleans and limits. Sufficient for the family/small-team use case; a capability-scoped token system can extend it later.
- **7-day TTL is not revocable without explicit call:** a stolen cookie persists until expiry unless the owner or an admin calls `revoke_user_sessions`. Mitigated by TOTP and the user-facing "sign out all" flow.
- **TOTP backup codes are not HOTP:** they are random hex tokens, not counter-based TOTP. This is simpler and sufficient for recovery.
- **Alternative — OAuth/OIDC:** rejected for v0.1.0; desktop-first self-hosted deployments rarely have an IdP. Can be added as an extension.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | SEC-1/SEC-2 invariants |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | auth.json / sessions.json location |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): The `internal-tool` channel no longer "grants admin unconditionally": that made every model-issued internal call pass `require_admin` (SEC-10 hole, cf. `l2-security` §4.5); the channel carries the issuing session's privileges. SEC-10 compliance row: user administration is a human-only surface (REA-1). `sessions.json` stores token hashes, never tokens (the persisted store is not yet built, so no drift). Dropped reserved-name rows are logged. |
| 1.0.0 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
